use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{parse_macro_input, Data, DeriveInput, Fields, GenericArgument, PathArguments, Type};

#[derive(Clone, Copy)]
enum FieldKind {
    I32,
    I64,
    String,
    Bool,
    DateTime,
}

struct FieldInfo {
    ident: syn::Ident,
    name_str: String,
    kind: FieldKind,
    editable: bool,
    nullable: bool,
}

#[proc_macro_derive(RustioAdmin)]
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
        fields.push(FieldInfo {
            ident,
            name_str,
            kind,
            editable,
            nullable,
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
            quote! {
                ::rustio_core::admin::AdminField {
                    name: #n,
                    ty: #kind_token,
                    editable: #editable,
                    nullable: #nullable,
                }
            }
        })
        .collect();

    let display_arms: Vec<TokenStream2> = fields.iter().map(display_arm).collect();

    let from_form_assignments: Vec<TokenStream2> =
        fields.iter().map(from_form_assignment).collect();

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
