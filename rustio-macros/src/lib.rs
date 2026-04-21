use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{
    parse_macro_input, Data, DeriveInput, Field, Fields, GenericArgument, Lit, PathArguments, Type,
};

#[derive(Clone, Copy)]
enum FieldKind {
    I32,
    I64,
    String,
    Bool,
    DateTime,
}

/// Parsed `#[rustio(belongs_to = "Patient", display = "full_name")]`
/// declaration on a struct field. The macro emits both the runtime
/// `AdminRelation` metadata and a companion `const _` block that makes
/// the compiler verify the target type exists and the named display
/// column is real — both failures surface as ordinary compile errors.
#[derive(Clone)]
struct RelationAttr {
    /// Target model type name — must resolve as a type in scope (the
    /// `const` verification block references `<Target as Model>`).
    target: syn::Ident,
    /// Optional column on the target whose value will be rendered in
    /// the admin. `None` ⇒ the admin shows `#<id>`; no inference.
    display: Option<String>,
}

struct FieldInfo {
    ident: syn::Ident,
    name_str: String,
    kind: FieldKind,
    editable: bool,
    nullable: bool,
    relation: Option<RelationAttr>,
}

#[proc_macro_derive(RustioAdmin, attributes(rustio))]
pub fn derive_rustio_admin(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;

    let data = match &input.data {
        Data::Struct(d) => d,
        _ => {
            return syn::Error::new_spanned(
                &input.ident,
                "RustioAdmin only supports structs with named fields",
            )
            .to_compile_error()
            .into();
        }
    };

    let named = match &data.fields {
        Fields::Named(n) => n,
        _ => {
            return syn::Error::new_spanned(&input.ident, "RustioAdmin requires named fields")
                .to_compile_error()
                .into();
        }
    };

    let mut fields: Vec<FieldInfo> = Vec::new();
    for f in &named.named {
        let ident = f.ident.clone().expect("named field");
        let name_str = ident.to_string();
        let (kind, nullable) = match classify_type(&f.ty) {
            Some(r) => r,
            None => {
                return syn::Error::new_spanned(
                    &f.ty,
                    "RustioAdmin: unsupported field type (supported: i32, i64, \
                     String, bool, DateTime<Utc>, and Option<T> of any of those)",
                )
                .to_compile_error()
                .into();
            }
        };
        // `id` is always non-editable; `Option<i64>` ids are not supported
        // (the ORM contract requires an `i64` id).
        if name_str == "id" && nullable {
            return syn::Error::new_spanned(
                &f.ty,
                "RustioAdmin: `id` must be `i64`, not `Option<i64>`",
            )
            .to_compile_error()
            .into();
        }
        let editable = name_str != "id";

        let relation = match parse_relation_attr(f) {
            Ok(r) => r,
            Err(e) => return e.to_compile_error().into(),
        };

        // Relations only make sense on integer foreign-key columns; the
        // macro's job is to catch the nonsense case at compile time.
        if relation.is_some() && !matches!(kind, FieldKind::I64 | FieldKind::I32) {
            return syn::Error::new_spanned(
                &f.ty,
                "RustioAdmin: #[rustio(belongs_to = \"...\")] can only be applied to \
                 `i32` or `i64` fields (the foreign-key column)",
            )
            .to_compile_error()
            .into();
        }

        fields.push(FieldInfo {
            ident,
            name_str,
            kind,
            editable,
            nullable,
            relation,
        });
    }

    let admin_name = pluralize(&name.to_string().to_lowercase());
    let display_name = pluralize(&name.to_string());
    let singular_name = singularize(&name.to_string());

    let field_entries: Vec<TokenStream2> = fields
        .iter()
        .map(|f| {
            let n = &f.name_str;
            let kind_token = kind_token(f.kind);
            let editable = f.editable;
            let nullable = f.nullable;
            let relation_token = relation_token(f.relation.as_ref());
            quote! {
                ::rustio_core::admin::AdminField {
                    name: #n,
                    ty: #kind_token,
                    editable: #editable,
                    nullable: #nullable,
                    relation: #relation_token,
                }
            }
        })
        .collect();

    let display_arms: Vec<TokenStream2> = fields.iter().map(display_arm).collect();

    let from_form_assignments: Vec<TokenStream2> =
        fields.iter().map(from_form_assignment).collect();

    // Compile-time checks for every `#[rustio(belongs_to = "...")]`:
    // target type must exist and impl `Model`; if `display = "..."` is
    // set, the named column must appear in `Target::COLUMNS`. Both live
    // in a single `const _: ()` block per struct so bad declarations
    // fail the build with a readable message.
    let relation_checks: Vec<TokenStream2> = fields
        .iter()
        .filter_map(|f| f.relation.as_ref().map(|r| relation_check(&f.name_str, r)))
        .collect();

    let expanded = quote! {
        impl ::rustio_core::admin::AdminModel for #name {
            const ADMIN_NAME: &'static str = #admin_name;
            const DISPLAY_NAME: &'static str = #display_name;
            const FIELDS: &'static [::rustio_core::admin::AdminField] = &[
                #( #field_entries ),*
            ];

            fn singular_name() -> &'static str {
                #singular_name
            }

            fn field_display(&self, name: &str) -> Option<String> {
                match name {
                    #( #display_arms )*
                    _ => None,
                }
            }

            fn from_form(
                form: &::rustio_core::admin::FormData,
                id: Option<i64>,
            ) -> Result<Self, ::rustio_core::Error> {
                Ok(Self {
                    #( #from_form_assignments )*
                })
            }
        }

        #( #relation_checks )*
    };

    expanded.into()
}

fn pluralize(name: &str) -> String {
    if name.ends_with('s') {
        name.to_string()
    } else {
        format!("{name}s")
    }
}

fn singularize(name: &str) -> String {
    if let Some(stripped) = name.strip_suffix('s') {
        if !stripped.is_empty() {
            return stripped.to_string();
        }
    }
    name.to_string()
}

/// Parse `#[rustio(belongs_to = "Patient")]` or
/// `#[rustio(belongs_to = "Patient", display = "full_name")]`.
/// Returns `Ok(None)` if the field has no `#[rustio(...)]` attribute.
/// Returns `Err` if the attribute is malformed — unknown keys, wrong
/// value types, `display` without `belongs_to`, etc.
fn parse_relation_attr(field: &Field) -> syn::Result<Option<RelationAttr>> {
    let mut found: Option<RelationAttr> = None;
    for attr in &field.attrs {
        if !attr.path().is_ident("rustio") {
            continue;
        }

        let mut belongs_to: Option<syn::Ident> = None;
        let mut display: Option<String> = None;

        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("belongs_to") {
                let value: Lit = meta.value()?.parse()?;
                match value {
                    Lit::Str(s) => {
                        let ident = syn::parse_str::<syn::Ident>(&s.value()).map_err(|_| {
                            meta.error(format!(
                                "`belongs_to` value `{}` is not a valid Rust type name",
                                s.value()
                            ))
                        })?;
                        belongs_to = Some(ident);
                        Ok(())
                    }
                    _ => Err(meta.error("`belongs_to` must be a string literal")),
                }
            } else if meta.path.is_ident("display") {
                let value: Lit = meta.value()?.parse()?;
                match value {
                    Lit::Str(s) => {
                        display = Some(s.value());
                        Ok(())
                    }
                    _ => Err(meta.error("`display` must be a string literal")),
                }
            } else {
                Err(meta.error(format!(
                    "unknown #[rustio(...)] key: `{}` (expected `belongs_to` or `display`)",
                    meta.path
                        .get_ident()
                        .map(|i| i.to_string())
                        .unwrap_or_else(|| "<non-ident>".into())
                )))
            }
        })?;

        match (belongs_to, display) {
            (Some(target), display) => {
                if found.is_some() {
                    return Err(syn::Error::new_spanned(
                        attr,
                        "#[rustio(...)] may appear at most once per field",
                    ));
                }
                found = Some(RelationAttr { target, display });
            }
            (None, Some(_)) => {
                return Err(syn::Error::new_spanned(
                    attr,
                    "#[rustio(display = \"...\")] requires `belongs_to = \"...\"` on the same field",
                ));
            }
            (None, None) => {
                return Err(syn::Error::new_spanned(
                    attr,
                    "empty #[rustio()]: expected `belongs_to = \"ModelName\"`",
                ));
            }
        }
    }
    Ok(found)
}

fn relation_token(r: Option<&RelationAttr>) -> TokenStream2 {
    let Some(r) = r else {
        return quote! { None };
    };
    let target = r.target.to_string();
    let display_token = match &r.display {
        Some(s) => quote! { Some(#s) },
        None => quote! { None },
    };
    quote! {
        Some(::rustio_core::admin::AdminRelation {
            kind: ::rustio_core::schema::RelationKind::BelongsTo,
            model: #target,
            display_field: #display_token,
        })
    }
}

/// Emit a `const _: ()` block that forces the compiler to:
///   1. resolve `<Target as ::rustio_core::Model>` — fails if the type
///      doesn't exist or doesn't implement `Model`;
///   2. if `display = "col"` is set, verify that `col` appears in
///      `<Target as Model>::COLUMNS` — fails with a readable panic
///      message at const-eval time if it doesn't.
///
/// The field name is baked into the error message so the compiler's
/// output points at the author's declaration without needing a span
/// round-trip through the const block.
fn relation_check(field_name: &str, r: &RelationAttr) -> TokenStream2 {
    let target = &r.target;
    let target_str = target.to_string();
    match &r.display {
        None => quote! {
            const _: () = {
                // Forces the target to exist and impl `Model`.
                let _: &'static str = <#target as ::rustio_core::orm::Model>::TABLE;
            };
        },
        Some(display) => {
            let not_found_msg = format!(
                "#[rustio(belongs_to = \"{target_str}\", display = \"{display}\")] on field `{field_name}`: \
                 column `{display}` not found in `{target_str}::COLUMNS`. Declare the field on the target \
                 model or drop the `display = ...` key."
            );
            quote! {
                const _: () = {
                    let _: &'static str = <#target as ::rustio_core::orm::Model>::TABLE;

                    const fn __rustio_str_eq(a: &str, b: &str) -> bool {
                        let a = a.as_bytes();
                        let b = b.as_bytes();
                        if a.len() != b.len() {
                            return false;
                        }
                        let mut i = 0;
                        while i < a.len() {
                            if a[i] != b[i] {
                                return false;
                            }
                            i += 1;
                        }
                        true
                    }

                    let cols = <#target as ::rustio_core::orm::Model>::COLUMNS;
                    let mut i = 0;
                    let mut found = false;
                    while i < cols.len() {
                        if __rustio_str_eq(cols[i], #display) {
                            found = true;
                        }
                        i += 1;
                    }
                    if !found {
                        panic!(#not_found_msg);
                    }
                };
            }
        }
    }
}

/// Classify a struct field's type into `(FieldKind, nullable)`.
///
/// Peels a single layer of `Option<T>` and marks the field nullable;
/// rejects unknown types and nested optionals.
fn classify_type(ty: &Type) -> Option<(FieldKind, bool)> {
    let Type::Path(syn::TypePath { path, .. }) = ty else {
        return None;
    };
    let last = path.segments.last()?;

    if last.ident == "Option" {
        // Peel exactly one layer; `Option<Option<T>>` is not supported.
        let PathArguments::AngleBracketed(args) = &last.arguments else {
            return None;
        };
        let inner_ty = args.args.iter().find_map(|a| match a {
            GenericArgument::Type(t) => Some(t),
            _ => None,
        })?;
        let kind = base_kind(inner_ty)?;
        return Some((kind, true));
    }

    base_kind(ty).map(|k| (k, false))
}

/// Classify a non-`Option` type into a `FieldKind`.
fn base_kind(ty: &Type) -> Option<FieldKind> {
    let Type::Path(syn::TypePath { path, .. }) = ty else {
        return None;
    };
    let last = path.segments.last()?;
    match last.ident.to_string().as_str() {
        "i32" => Some(FieldKind::I32),
        "i64" => Some(FieldKind::I64),
        "String" => Some(FieldKind::String),
        "bool" => Some(FieldKind::Bool),
        // Accept both `DateTime` and the fully-qualified `DateTime<Utc>`.
        // We don't verify the type parameter; if it isn't `Utc`, the trait
        // bounds on `Row::get_datetime` will surface the error at the use
        // site with a much better message than anything we could produce
        // here.
        "DateTime" => Some(FieldKind::DateTime),
        _ => None,
    }
}

fn kind_token(kind: FieldKind) -> TokenStream2 {
    match kind {
        FieldKind::I32 => quote! { ::rustio_core::admin::FieldType::I32 },
        FieldKind::I64 => quote! { ::rustio_core::admin::FieldType::I64 },
        FieldKind::String => quote! { ::rustio_core::admin::FieldType::String },
        FieldKind::Bool => quote! { ::rustio_core::admin::FieldType::Bool },
        FieldKind::DateTime => quote! { ::rustio_core::admin::FieldType::DateTime },
    }
}

/// Format a `DateTime<Utc>` as `YYYY-MM-DDTHH:MM`, which is what the
/// browser's `<input type="datetime-local">` emits and accepts. Seconds
/// are dropped on purpose — admin forms don't need sub-minute precision
/// and including them trips the widget in some browsers.
const DATETIME_FORMAT: &str = "%Y-%m-%dT%H:%M";

/// Produce the `match` arm that renders one field as a form-ready string.
fn display_arm(f: &FieldInfo) -> TokenStream2 {
    let ident = &f.ident;
    let name_str = &f.name_str;

    // Nullable: empty string when None, formatted value when Some.
    if f.nullable {
        return match f.kind {
            FieldKind::DateTime => quote! {
                #name_str => Some(match &self.#ident {
                    Some(v) => v.format(#DATETIME_FORMAT).to_string(),
                    None => String::new(),
                }),
            },
            _ => quote! {
                #name_str => Some(match &self.#ident {
                    Some(v) => v.to_string(),
                    None => String::new(),
                }),
            },
        };
    }

    match f.kind {
        FieldKind::DateTime => quote! {
            #name_str => Some(self.#ident.format(#DATETIME_FORMAT).to_string()),
        },
        _ => quote! {
            #name_str => Some(self.#ident.to_string()),
        },
    }
}

/// Produce the struct-field assignment inside the generated `from_form`.
///
/// The `id` field is always filled from the `id: Option<i64>` argument.
/// Editable fields pull from the `form` by name and validate/parse.
/// Nullable fields accept an empty string and produce `None`.
fn from_form_assignment(f: &FieldInfo) -> TokenStream2 {
    let ident = &f.ident;
    let name_str = &f.name_str;
    if !f.editable {
        return quote! { #ident: id.unwrap_or(0), };
    }

    if f.nullable {
        return nullable_assignment(ident, name_str, f.kind);
    }

    match f.kind {
        FieldKind::String => quote! {
            #ident: {
                let v = form.get(#name_str).unwrap_or("").trim();
                if v.is_empty() {
                    return Err(::rustio_core::Error::BadRequest(
                        format!("field `{}` is required", #name_str)
                    ));
                }
                v.to_owned()
            },
        },
        FieldKind::Bool => quote! {
            #ident: matches!(form.get(#name_str), Some(v) if v == "on" || v == "true"),
        },
        FieldKind::I64 => quote! {
            #ident: {
                let raw = form.get(#name_str).unwrap_or("").trim();
                if raw.is_empty() {
                    return Err(::rustio_core::Error::BadRequest(
                        format!("field `{}` is required", #name_str)
                    ));
                }
                raw.parse::<i64>().map_err(|_| ::rustio_core::Error::BadRequest(
                    format!("field `{}` must be a valid integer", #name_str)
                ))?
            },
        },
        FieldKind::I32 => quote! {
            #ident: {
                let raw = form.get(#name_str).unwrap_or("").trim();
                if raw.is_empty() {
                    return Err(::rustio_core::Error::BadRequest(
                        format!("field `{}` is required", #name_str)
                    ));
                }
                raw.parse::<i32>().map_err(|_| ::rustio_core::Error::BadRequest(
                    format!("field `{}` must be a valid integer", #name_str)
                ))?
            },
        },
        FieldKind::DateTime => quote! {
            #ident: {
                let raw = form.get(#name_str).unwrap_or("").trim();
                if raw.is_empty() {
                    return Err(::rustio_core::Error::BadRequest(
                        format!("field `{}` is required", #name_str)
                    ));
                }
                ::rustio_core::admin::parse_datetime_local(raw).map_err(|e| {
                    ::rustio_core::Error::BadRequest(
                        format!("field `{}`: {}", #name_str, e)
                    )
                })?
            },
        },
    }
}

/// Build the assignment for an `Option<T>` field: empty input → `None`.
fn nullable_assignment(ident: &syn::Ident, name_str: &str, kind: FieldKind) -> TokenStream2 {
    match kind {
        FieldKind::String => quote! {
            #ident: {
                let v = form.get(#name_str).unwrap_or("").trim();
                if v.is_empty() { None } else { Some(v.to_owned()) }
            },
        },
        // Checkboxes don't support a tri-state "unset". For a nullable
        // bool we treat "absent" as `None` and any present value ("on",
        // "true") as `Some(true)`. A pair of radio buttons would be the
        // correct widget here; we ship the checkbox form for now and
        // will revisit when the admin gets a proper field-widget layer.
        FieldKind::Bool => quote! {
            #ident: match form.get(#name_str) {
                Some(v) if v == "on" || v == "true" => Some(true),
                Some(v) if v == "off" || v == "false" => Some(false),
                Some(_) | None => None,
            },
        },
        FieldKind::I64 => quote! {
            #ident: {
                let raw = form.get(#name_str).unwrap_or("").trim();
                if raw.is_empty() {
                    None
                } else {
                    Some(raw.parse::<i64>().map_err(|_| ::rustio_core::Error::BadRequest(
                        format!("field `{}` must be a valid integer", #name_str)
                    ))?)
                }
            },
        },
        FieldKind::I32 => quote! {
            #ident: {
                let raw = form.get(#name_str).unwrap_or("").trim();
                if raw.is_empty() {
                    None
                } else {
                    Some(raw.parse::<i32>().map_err(|_| ::rustio_core::Error::BadRequest(
                        format!("field `{}` must be a valid integer", #name_str)
                    ))?)
                }
            },
        },
        FieldKind::DateTime => quote! {
            #ident: {
                let raw = form.get(#name_str).unwrap_or("").trim();
                if raw.is_empty() {
                    None
                } else {
                    Some(::rustio_core::admin::parse_datetime_local(raw).map_err(|e| {
                        ::rustio_core::Error::BadRequest(
                            format!("field `{}`: {}", #name_str, e)
                        )
                    })?)
                }
            },
        },
    }
}
