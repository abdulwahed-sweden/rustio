//! Procedural macros for RustIO.
//!
//! The big one: `#[derive(RustioAdmin)]`. Given a user-written struct,
//! the derive emits:
//!
//!   - `impl AdminModel for TheStruct` with `ADMIN_NAME`, `DISPLAY_NAME`,
//!     `SINGULAR_NAME`, `FIELDS`, and the row/form/update helpers.
//!
//! The macro deliberately stays dumb: all runtime behaviour lives in
//! `rustio_core`. Keeping the macro small makes it easier to debug —
//! if something feels wrong, you can read the generated code with
//! `cargo expand`.

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::{parse_macro_input, Data, DeriveInput, Fields, Lit, Meta};

#[proc_macro_derive(RustioAdmin, attributes(rustio))]
pub fn derive_rustio_admin(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    expand(input)
        .unwrap_or_else(|e| e.to_compile_error())
        .into()
}

fn expand(input: DeriveInput) -> syn::Result<TokenStream2> {
    let struct_name = &input.ident;
    let fields = struct_fields(&input)?;

    let admin_name = plural_snake(&struct_name.to_string());
    let display_name = humanise(&plural_snake(&struct_name.to_string()));
    let singular = struct_name.to_string();

    let mut field_metas = Vec::new();
    let mut display_value_arms = Vec::new();
    let mut from_form_parses = Vec::new();
    let mut from_form_fields = Vec::new();
    let mut update_tuples = Vec::new();

    for f in fields {
        let fname = f.ident.as_ref().unwrap();
        let fname_str = fname.to_string();
        let kind = classify_type(&f.ty)?;
        // Phase 1/a — fields named `created_at` / `updated_at` are
        // managed by the framework: hidden from forms, defaulted to
        // `Utc::now()` in `from_form`. The macro already wires that
        // behaviour through `FieldKind::DateTimeAuto`; this promotion
        // is the missing trigger that makes the variant reachable for
        // the conventionally named timestamp columns.
        let kind = if matches!(kind, FieldKind::DateTime) && is_auto_timestamp_name(&fname_str) {
            FieldKind::DateTimeAuto
        } else {
            kind
        };
        let editable = fname_str != "id" && kind != FieldKind::DateTimeAuto;

        let type_variant = kind.field_type_ident();
        let relation = parse_relation_attr(&f.attrs, &fname_str)?;
        let relation_tokens = match &relation {
            Some((target, display)) => {
                let display_tok = match display {
                    Some(d) => quote! { ::std::option::Option::Some(#d) },
                    None => quote! { ::std::option::Option::None },
                };
                quote! {
                    ::std::option::Option::Some(::rustio_core::admin::AdminRelation {
                        target_model: #target,
                        display_field: #display_tok,
                        // Phase 5/d — single belongs_to relations default to
                        // single `<select>`. Many-to-many is opt-in via a
                        // future `#[rustio(many_to_many)]` attribute; the
                        // macro emits `false` for now so consumers that want
                        // multi-select must hand-set the field on the
                        // generated AdminRelation.
                        multi: false,
                    })
                }
            }
            None => quote! { ::std::option::Option::None },
        };

        field_metas.push(quote! {
            ::rustio_core::admin::AdminField {
                name: #fname_str,
                label: #fname_str,
                field_type: ::rustio_core::admin::FieldType::#type_variant,
                editable: #editable,
                relation: #relation_tokens,
                // Phase 5/d — derived models don't carry enum choices yet.
                // A future macro pass will accept `#[rustio(choices = [...])]`
                // and populate this; today consumers that want a `<select>`
                // backed by a static value list set this on the generated
                // AdminField directly.
                choices: ::std::option::Option::None,
            }
        });

        // `display_values`: stringify the field for the list page.
        let display_arm = match kind {
            FieldKind::String => quote! {
                out.push((#fname_str.to_string(), self.#fname.clone()));
            },
            FieldKind::OptionalString => quote! {
                // Stress-test fix (v1.4.x) — `Option<String>` does not
                // implement `Display`, so the previous shared
                // `String | OptionalString` arm that called
                // `self.#fname.clone().to_string()` would not compile
                // for any model that declared an `Option<String>`
                // field. Mirrors the `OptionalI64` arm: None →
                // empty string, Some(v) → v.
                out.push((#fname_str.to_string(), match &self.#fname {
                    Some(v) => v.clone(),
                    None => String::new(),
                }));
            },
            FieldKind::I32 | FieldKind::I64 => quote! {
                out.push((#fname_str.to_string(), self.#fname.to_string()));
            },
            FieldKind::OptionalI64 => quote! {
                out.push((#fname_str.to_string(), match &self.#fname {
                    Some(v) => v.to_string(),
                    None => String::new(),
                }));
            },
            FieldKind::Bool => quote! {
                out.push((#fname_str.to_string(), if self.#fname { "true".to_string() } else { "false".to_string() }));
            },
            FieldKind::DateTime | FieldKind::DateTimeAuto => quote! {
                // Phase v1.4.x — ISO-8601 form with `T` separator. This is the
                // exact wire format `<input type="datetime-local">` expects
                // (`%Y-%m-%dT%H:%M`); the form-render path puts this string
                // straight into the input's `value=` attribute. The list path
                // detects the same shape (16 chars, `T` at index 10) and
                // splits it into the two-line time-on-top / date-below cell
                // layout in admin/list.html.
                //
                // NOTE: `datetime-local` input cannot encode timezone. We
                // currently surface UTC values directly. v1.5.0 will add
                // user-locale conversion.
                out.push((#fname_str.to_string(), self.#fname.format("%Y-%m-%dT%H:%M").to_string()));
            },
        };
        display_value_arms.push(display_arm);

        // `from_form`: read the HTML form body into a struct field.
        if fname_str == "id" {
            from_form_fields.push(quote! { #fname: 0 });
            continue;
        }

        // Phase 1/b — precompute human-readable validation messages
        // at expansion time so the runtime error path doesn't repeat
        // the same `format!` work per request and so every model
        // emits identically-styled copy ("Title is required.").
        let humanised_label = humanise_field(&fname_str);
        let required_msg = format!("{humanised_label} is required.");
        let number_msg = format!("{humanised_label} must be a number.");
        let date_invalid_msg = format!("{humanised_label} is not a valid date.");

        match kind {
            FieldKind::String => {
                // Phase 7.6 — trim incoming whitespace so a `"   "`
                // submission is treated as empty (and triggers the
                // required-field error) instead of silently saving a
                // whitespace-only string.
                from_form_parses.push(quote! {
                    let #fname = match form.get(#fname_str).map(str::trim) {
                        Some(v) if !v.is_empty() => v.to_string(),
                        _ => { errors.push(#required_msg.to_string()); String::new() }
                    };
                });
                from_form_fields.push(quote! { #fname });
            }
            FieldKind::OptionalString => {
                // Phase 7.6 — trim, then collapse trimmed-empty to None
                // so the column stores NULL instead of `""`.
                from_form_parses.push(quote! {
                    let #fname: Option<String> = form
                        .get(#fname_str)
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty());
                });
                from_form_fields.push(quote! { #fname });
            }
            FieldKind::I32 => {
                from_form_parses.push(quote! {
                    let #fname: i32 = match form.get(#fname_str).and_then(|v| v.parse().ok()) {
                        Some(v) => v,
                        None => { errors.push(#number_msg.to_string()); 0 }
                    };
                });
                from_form_fields.push(quote! { #fname });
            }
            FieldKind::I64 => {
                from_form_parses.push(quote! {
                    let #fname: i64 = match form.get(#fname_str).and_then(|v| v.parse().ok()) {
                        Some(v) => v,
                        None => { errors.push(#number_msg.to_string()); 0 }
                    };
                });
                from_form_fields.push(quote! { #fname });
            }
            FieldKind::OptionalI64 => {
                // Phase 7.6 — distinguish "user left it blank" (None,
                // legitimate) from "user typed garbage" (validation
                // error, NOT silently dropped). Pre-7.6 used
                // `.and_then(|v| v.parse().ok())` which collapsed both
                // cases to None.
                from_form_parses.push(quote! {
                    let #fname: Option<i64> = match form.get(#fname_str).map(str::trim) {
                        None | Some("") => None,
                        Some(raw) => match raw.parse::<i64>() {
                            Ok(n) => Some(n),
                            Err(_) => {
                                errors.push(#number_msg.to_string());
                                None
                            }
                        },
                    };
                });
                from_form_fields.push(quote! { #fname });
            }
            FieldKind::Bool => {
                from_form_parses.push(quote! {
                    let #fname: bool = form.bool_flag(#fname_str);
                });
                from_form_fields.push(quote! { #fname });
            }
            FieldKind::DateTime => {
                from_form_parses.push(quote! {
                    let #fname = match form.get(#fname_str) {
                        Some(raw) if !raw.is_empty() => {
                            match ::chrono::NaiveDateTime::parse_from_str(raw, "%Y-%m-%dT%H:%M") {
                                Ok(dt) => ::chrono::DateTime::<::chrono::Utc>::from_naive_utc_and_offset(dt, ::chrono::Utc),
                                Err(_) => { errors.push(#date_invalid_msg.to_string()); ::chrono::Utc::now() }
                            }
                        }
                        _ => { errors.push(#required_msg.to_string()); ::chrono::Utc::now() }
                    };
                });
                from_form_fields.push(quote! { #fname });
            }
            FieldKind::DateTimeAuto => {
                // created_at-style fields default to now().
                from_form_parses.push(quote! {
                    let #fname = ::chrono::Utc::now();
                });
                from_form_fields.push(quote! { #fname });
            }
        }

        update_tuples.push(quote! {
            (#fname_str, self.#fname.clone().into())
        });
    }

    let object_label_expr = find_label_field(fields)
        .map(|n| {
            let id = format_ident!("{n}");
            quote! { self.#id.clone().to_string() }
        })
        .unwrap_or_else(|| quote! { format!("#{}", self.id) });

    Ok(quote! {
        impl ::rustio_core::admin::AdminModel for #struct_name {
            const ADMIN_NAME: &'static str = #admin_name;
            const DISPLAY_NAME: &'static str = #display_name;
            const SINGULAR_NAME: &'static str = #singular;
            const FIELDS: &'static [::rustio_core::admin::AdminField] = &[
                #(#field_metas),*
            ];

            fn display_values(&self) -> ::std::vec::Vec<(::std::string::String, ::std::string::String)> {
                let mut out = ::std::vec::Vec::new();
                #(#display_value_arms)*
                out
            }

            fn from_form(form: &::rustio_core::http::FormData) -> ::std::result::Result<Self, ::std::vec::Vec<::std::string::String>>
            where
                Self: Sized,
            {
                let mut errors: ::std::vec::Vec<::std::string::String> = ::std::vec::Vec::new();
                #(#from_form_parses)*
                if !errors.is_empty() {
                    return Err(errors);
                }
                Ok(Self { #(#from_form_fields),* })
            }

            fn object_label(&self) -> ::std::string::String {
                #object_label_expr
            }

            fn id(&self) -> i64 {
                self.id
            }

            fn values_to_update(&self) -> ::std::vec::Vec<(&'static str, ::rustio_core::orm::Value)> {
                ::std::vec![#(#update_tuples),*]
            }
        }
    })
}

fn struct_fields(input: &DeriveInput) -> syn::Result<&syn::punctuated::Punctuated<syn::Field, syn::Token![,]>> {
    let data = match &input.data {
        Data::Struct(s) => s,
        _ => {
            return Err(syn::Error::new_spanned(
                &input.ident,
                "RustioAdmin can only derive on structs",
            ))
        }
    };
    match &data.fields {
        Fields::Named(named) => Ok(&named.named),
        _ => Err(syn::Error::new_spanned(
            &input.ident,
            "RustioAdmin requires a struct with named fields",
        )),
    }
}

#[derive(Debug, PartialEq, Clone, Copy)]
enum FieldKind {
    I32,
    I64,
    Bool,
    String,
    DateTime,
    DateTimeAuto,
    OptionalString,
    OptionalI64,
}

impl FieldKind {
    fn field_type_ident(&self) -> proc_macro2::Ident {
        match self {
            FieldKind::I32 => format_ident!("I32"),
            FieldKind::I64 => format_ident!("I64"),
            FieldKind::Bool => format_ident!("Bool"),
            FieldKind::String => format_ident!("String"),
            FieldKind::DateTime | FieldKind::DateTimeAuto => format_ident!("DateTime"),
            FieldKind::OptionalString => format_ident!("OptionalString"),
            FieldKind::OptionalI64 => format_ident!("OptionalI64"),
        }
    }
}

/// Phase 1/a — names treated as framework-managed timestamps. These
/// fields are auto-promoted to `FieldKind::DateTimeAuto` regardless of
/// declared type so the admin UI doesn't render them and `from_form`
/// fills them with `Utc::now()`. Conservative list; expand only when a
/// real model needs another conventionally-named timestamp.
fn is_auto_timestamp_name(name: &str) -> bool {
    matches!(name, "created_at" | "updated_at")
}

/// Phase 1/b — turn a snake_case column name into a Title-Case label
/// for human-readable validation errors emitted by `from_form`. Mirrors
/// `rustio_core::admin::intelligence::humanise` so the error message
/// label and the rendered form label use identical capitalisation.
fn humanise_field(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut next_upper = true;
    for ch in s.chars() {
        if ch == '_' {
            out.push(' ');
            next_upper = true;
        } else if next_upper {
            out.push(ch.to_ascii_uppercase());
            next_upper = false;
        } else {
            out.push(ch);
        }
    }
    out
}

fn classify_type(ty: &syn::Type) -> syn::Result<FieldKind> {
    let as_string = quote! { #ty }.to_string().replace(' ', "");
    let kind = match as_string.as_str() {
        "i32" => FieldKind::I32,
        "i64" => FieldKind::I64,
        "bool" => FieldKind::Bool,
        "String" => FieldKind::String,
        "DateTime<Utc>" | "chrono::DateTime<chrono::Utc>" => FieldKind::DateTime,
        "Option<String>" => FieldKind::OptionalString,
        "Option<i64>" => FieldKind::OptionalI64,
        other => {
            return Err(syn::Error::new_spanned(
                ty,
                format!("unsupported field type for RustioAdmin: {other}"),
            ))
        }
    };
    Ok(kind)
}

fn parse_relation_attr(
    attrs: &[syn::Attribute],
    field_name: &str,
) -> syn::Result<Option<(String, Option<String>)>> {
    for attr in attrs {
        if !attr.path().is_ident("rustio") {
            continue;
        }
        let mut target: Option<String> = None;
        let mut display: Option<String> = None;
        attr.parse_nested_meta(|m| {
            if m.path.is_ident("belongs_to") {
                let value = m.value()?;
                let lit: Lit = value.parse()?;
                if let Lit::Str(s) = lit {
                    target = Some(s.value());
                }
                Ok(())
            } else if m.path.is_ident("display") {
                let value = m.value()?;
                let lit: Lit = value.parse()?;
                if let Lit::Str(s) = lit {
                    display = Some(s.value());
                }
                Ok(())
            } else {
                Err(m.error(format!("unknown rustio attribute for field `{field_name}`")))
            }
        })?;
        if let Some(t) = target {
            return Ok(Some((t, display)));
        }
        if display.is_some() {
            return Err(syn::Error::new_spanned(
                attr,
                "`display` requires `belongs_to` alongside it",
            ));
        }
    }
    // Just suppress the unused warning for `Meta`.
    let _ = std::marker::PhantomData::<Meta>;
    Ok(None)
}

fn plural_snake(camel: &str) -> String {
    let snake = camel_to_snake(camel);
    if snake.ends_with('s') {
        snake
    } else {
        format!("{snake}s")
    }
}

fn camel_to_snake(s: &str) -> String {
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if c.is_ascii_uppercase() && i > 0 {
            out.push('_');
        }
        out.push(c.to_ascii_lowercase());
    }
    out
}

fn humanise(snake: &str) -> String {
    // "blog_posts" → "Blog posts"
    let mut chars = snake.chars();
    let mut out = String::new();
    if let Some(first) = chars.next() {
        out.push(first.to_ascii_uppercase());
    }
    for c in chars {
        if c == '_' {
            out.push(' ');
        } else {
            out.push(c);
        }
    }
    out
}

fn find_label_field(fields: &syn::punctuated::Punctuated<syn::Field, syn::Token![,]>) -> Option<String> {
    // Heuristic: prefer `name`, then `title`, then `full_name`, then
    // fall through to #id. Keeps object_label() useful without forcing
    // users to implement anything.
    //
    // Future evolution — explicit `label_field` plug-in point:
    // when a struct-level attribute is added (likely
    // `#[rustio(label_field = "summary")]`), this function becomes
    // the override site: parse the attribute off `input.attrs` in
    // `expand`, pass the chosen ident in, and return it here before
    // running the heuristic. The trait layer (`AdminModel::object_label`
    // in `rustio-core/src/admin/types.rs`) and the FK rendering layer
    // (`AdminRelation::display_field` in the same file) already accept
    // a per-model label without further plumbing — this is the only
    // file the new attribute needs to touch.
    let names = ["name", "title", "full_name", "label", "email"];
    for candidate in names {
        if fields
            .iter()
            .any(|f| f.ident.as_ref().map(|i| i == candidate).unwrap_or(false))
        {
            return Some(candidate.to_string());
        }
    }
    None
}

// ============================================================================
// RustioModel — Phase 14, commit 2.
// ============================================================================
//
// Generates ONLY:
//
//     impl ::rustio_core::contract::HasSchema for T {
//         const SCHEMA: ::rustio_core::contract::ModelSchema = ...;
//     }
//
// Does NOT generate `impl Model`, `impl AdminModel`, or `impl Searchable` —
// those are the responsibility of separate derives / commits. This macro is
// pure schema metadata.
//
// Attribute surface (initial set):
//
//     #[rustio(table = "...")]                            on the struct
//     #[rustio(sql = "...", searchable, filterable,       on each field
//              sortable, readonly,
//              widget = "...", label = "...",
//              references = "table(column)")]
//
// Compile-time validations (errors):
//   1. field name `id` MUST be `i64` (Type Rule #1).
//   2. SQL containing NUMERIC/DECIMAL MUST pair with `Decimal` (Type Rule #3).
//   3. `NaiveDateTime` is forbidden — use `DateTime<Utc>` (Type Rule #2).
//   4. The `sql = "..."` attribute is required on every field.
//   5. The `#[rustio(table = "...")]` attribute is required on the struct.
//   6. At least one field's SQL must declare `PRIMARY KEY`.
//
// Warnings (deferred — proc-macro warnings on stable Rust are awkward):
//   - VARCHAR usage on a String column.
//   - JSON (without B) on a serde_json::Value column.
// These are listed in the contract layer's docs and will become validator
// warnings in commit 3 instead.

#[proc_macro_derive(RustioModel, attributes(rustio))]
pub fn derive_rustio_model(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    rustio_model::expand(input)
        .unwrap_or_else(|e| e.to_compile_error())
        .into()
}

mod rustio_model {
    use super::*;
    use syn::{
        parse::{Parse, ParseStream},
        Attribute, GenericArgument, LitStr, PathArguments, Token, Type,
    };

    /// Internal classification of a Rust field type. One variant per
    /// `RustType` enum the contract layer recognises. Computed by
    /// `classify` from a `syn::Type`; converted to its
    /// `::rustio_core::contract::RustType` token form for emission.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    enum RustTypeKind {
        I32,
        I64,
        F64,
        Bool,
        String,
        DateTimeUtc,
        JsonValue,
        Decimal,
        Uuid,
    }

    impl RustTypeKind {
        fn to_token(self) -> TokenStream2 {
            match self {
                RustTypeKind::I32 => quote! { ::rustio_core::contract::RustType::I32 },
                RustTypeKind::I64 => quote! { ::rustio_core::contract::RustType::I64 },
                RustTypeKind::F64 => quote! { ::rustio_core::contract::RustType::F64 },
                RustTypeKind::Bool => quote! { ::rustio_core::contract::RustType::Bool },
                RustTypeKind::String => quote! { ::rustio_core::contract::RustType::String },
                RustTypeKind::DateTimeUtc => {
                    quote! { ::rustio_core::contract::RustType::DateTimeUtc }
                }
                RustTypeKind::JsonValue => {
                    quote! { ::rustio_core::contract::RustType::JsonValue }
                }
                RustTypeKind::Decimal => quote! { ::rustio_core::contract::RustType::Decimal },
                RustTypeKind::Uuid => quote! { ::rustio_core::contract::RustType::Uuid },
            }
        }
    }

    /// Per-field attributes parsed from `#[rustio(...)]`.
    #[derive(Default)]
    struct FieldAttr {
        sql: String,
        searchable: bool,
        filterable: bool,
        sortable: bool,
        readonly: bool,
        widget: Option<String>,
        label: Option<String>,
        references: Option<String>,
    }

    /// Top-level entry point — emits the `impl HasSchema` block or
    /// a compile error.
    pub(super) fn expand(input: DeriveInput) -> syn::Result<TokenStream2> {
        // Reject anything that isn't a named struct. Generics, tuple
        // structs, and enums all want different expansions; this
        // commit handles the common case only.
        if !input.generics.params.is_empty() {
            return Err(syn::Error::new_spanned(
                &input.ident,
                "RustioModel does not support generic structs (yet)",
            ));
        }
        let struct_name = &input.ident;
        let fields = match &input.data {
            Data::Struct(ds) => match &ds.fields {
                Fields::Named(f) => &f.named,
                _ => {
                    return Err(syn::Error::new_spanned(
                        struct_name,
                        "RustioModel requires a named-field struct (no tuple structs)",
                    ));
                }
            },
            _ => {
                return Err(syn::Error::new_spanned(
                    struct_name,
                    "RustioModel can only be derived on structs",
                ));
            }
        };

        // Struct-level: `#[rustio(table = "...")]`
        let table = parse_table_attr(&input.attrs)?;

        // Per-field: build column expressions + locate primary key.
        let mut column_exprs = Vec::new();
        let mut primary_key: Option<String> = None;

        for field in fields {
            let field_name = field
                .ident
                .as_ref()
                .expect("named struct fields have idents")
                .to_string();
            let field_attr = parse_field_attr(&field.attrs)?;
            if field_attr.sql.is_empty() {
                return Err(syn::Error::new_spanned(
                    field,
                    format!("field `{field_name}` is missing the required `#[rustio(sql = \"...\")]` attribute"),
                ));
            }

            let (kind, nullable) = classify(&field.ty)?;
            validate_field_rules(&field_name, &field_attr.sql, kind, &field.ty)?;

            let sql_upper = field_attr.sql.to_uppercase();
            let is_pk = sql_upper.contains("PRIMARY KEY");
            if is_pk {
                if let Some(prev) = &primary_key {
                    return Err(syn::Error::new_spanned(
                        field,
                        format!(
                            "more than one field declares PRIMARY KEY: `{prev}` and `{field_name}`"
                        ),
                    ));
                }
                primary_key = Some(field_name.clone());
            }

            column_exprs.push(build_column_expr(&field_name, &field_attr, kind, nullable, is_pk));
        }

        let pk = primary_key.ok_or_else(|| {
            syn::Error::new_spanned(
                struct_name,
                "RustioModel requires at least one field whose `sql = \"...\"` declares PRIMARY KEY",
            )
        })?;

        // Trait impl. The contract is defined by the `HasSchema`
        // trait, not by an inherent const — this is what lets future
        // commits (validator, admin, search) constrain generics on
        // `<T: HasSchema>` without each consumer knowing the
        // emission shape.
        //
        // Nested `const __COLS: &[ModelColumn] = &[...]` is required
        // here, not a direct `&[...]` literal in the outer
        // ModelSchema::new(...) call. The inner column expressions
        // contain block expressions with mutable bindings (the
        // SchemaFlags construction pattern, see `build_column_expr`
        // above), and Rust's "const promotion" rules don't promote
        // array literals containing non-trivially-const expressions
        // to `'static`. Wrapping the array in its own `const`
        // item forces compile-time evaluation explicitly, after
        // which the resulting `&'static [ModelColumn]` flows cleanly
        // into `ModelSchema::new`'s `&'static`-bound parameter.
        Ok(quote! {
            impl ::rustio_core::contract::HasSchema for #struct_name {
                const SCHEMA: ::rustio_core::contract::ModelSchema = {
                    const __COLS: &[::rustio_core::contract::ModelColumn] = &[
                        #(#column_exprs),*
                    ];
                    ::rustio_core::contract::ModelSchema::new(#table, __COLS, #pk)
                };
            }
        })
    }

    /// Classify a `syn::Type` into a (RustTypeKind, nullable) pair.
    /// Errors on `NaiveDateTime` (Type Rule #2) and any unsupported
    /// type. Recurses on `Option<T>` to peel one layer.
    fn classify(ty: &Type) -> syn::Result<(RustTypeKind, bool)> {
        if let Some(inner) = unwrap_option(ty) {
            let (k, _) = classify(inner)?;
            return Ok((k, true));
        }

        let path = match ty {
            Type::Path(tp) => tp,
            _ => {
                return Err(syn::Error::new_spanned(
                    ty,
                    "RustioModel: unsupported type shape (need a simple path type)",
                ));
            }
        };

        let last = path
            .path
            .segments
            .last()
            .ok_or_else(|| syn::Error::new_spanned(ty, "RustioModel: empty type path"))?;
        let name = last.ident.to_string();

        // Handle DateTime<Utc> specifically — must have <Utc> arg.
        if name == "DateTime" {
            if let PathArguments::AngleBracketed(args) = &last.arguments {
                let mut got_utc = false;
                for arg in &args.args {
                    if let GenericArgument::Type(Type::Path(tp)) = arg {
                        if tp
                            .path
                            .segments
                            .last()
                            .map(|s| s.ident == "Utc")
                            .unwrap_or(false)
                        {
                            got_utc = true;
                        }
                    }
                }
                if got_utc {
                    return Ok((RustTypeKind::DateTimeUtc, false));
                }
            }
            return Err(syn::Error::new_spanned(
                ty,
                "RustioModel: only `DateTime<Utc>` is supported (Type Rule #2). Other timezone parameters are not accepted.",
            ));
        }

        // NaiveDateTime — explicitly rejected per Type Rule #2.
        if name == "NaiveDateTime" {
            return Err(syn::Error::new_spanned(
                ty,
                "RustioModel: `NaiveDateTime` is forbidden (Type Rule #2) — use `chrono::DateTime<chrono::Utc>` for all timestamp columns",
            ));
        }

        // Plain type names. Path prefixes are allowed
        // (`serde_json::Value`, `rust_decimal::Decimal`, `uuid::Uuid`)
        // because we only inspect the last path segment.
        let kind = match name.as_str() {
            "i32" => RustTypeKind::I32,
            "i64" => RustTypeKind::I64,
            "f64" => RustTypeKind::F64,
            "bool" => RustTypeKind::Bool,
            "String" => RustTypeKind::String,
            "Value" => RustTypeKind::JsonValue,    // serde_json::Value
            "Decimal" => RustTypeKind::Decimal,    // rust_decimal::Decimal
            "Uuid" => RustTypeKind::Uuid,          // uuid::Uuid
            other => {
                return Err(syn::Error::new_spanned(
                    ty,
                    format!(
                        "RustioModel: unsupported field type `{other}`. \
                         Supported: i32, i64, f64, bool, String, \
                         DateTime<Utc>, serde_json::Value, \
                         rust_decimal::Decimal, uuid::Uuid \
                         (and Option<T> for any of the above)."
                    ),
                ));
            }
        };
        Ok((kind, false))
    }

    /// Peel one `Option<T>` layer if present. Returns `None` for
    /// non-Option types.
    fn unwrap_option(ty: &Type) -> Option<&Type> {
        let path = match ty {
            Type::Path(tp) => &tp.path,
            _ => return None,
        };
        let last = path.segments.last()?;
        if last.ident != "Option" {
            return None;
        }
        let args = match &last.arguments {
            PathArguments::AngleBracketed(a) => a,
            _ => return None,
        };
        for arg in &args.args {
            if let GenericArgument::Type(t) = arg {
                return Some(t);
            }
        }
        None
    }

    /// Apply the compile-time type rules. Run after classification —
    /// every error message names the rule it enforces.
    fn validate_field_rules(
        name: &str,
        sql: &str,
        kind: RustTypeKind,
        ty: &Type,
    ) -> syn::Result<()> {
        let sql_upper = sql.to_uppercase();

        // Type Rule #1 — id must be i64.
        if name == "id" && kind != RustTypeKind::I64 {
            return Err(syn::Error::new_spanned(
                ty,
                "Type Rule #1: field `id` must be `i64` (mapped to BIGINT/BIGSERIAL). \
                 Using a smaller integer type for IDs silently truncates at 2.1B rows.",
            ));
        }

        // Type Rule #3 — NUMERIC requires Decimal.
        // Match whole tokens so a column called "numericality" doesn't
        // false-positive. We split on non-alphanumeric chars and look
        // for the exact tokens.
        let has_numeric_token = sql_upper
            .split(|c: char| !c.is_alphanumeric())
            .any(|t| t == "NUMERIC" || t == "DECIMAL");
        if has_numeric_token && kind != RustTypeKind::Decimal {
            return Err(syn::Error::new_spanned(
                ty,
                "Type Rule #3: NUMERIC/DECIMAL columns must pair with \
                 `rust_decimal::Decimal`. Using `f64` (or any other type) \
                 for money loses precision under arithmetic.",
            ));
        }

        Ok(())
    }

    /// Build the `ModelColumn::new(...).chain()...` expression for one
    /// field. Always emits `with_flags(...)` so the generated code is
    /// uniform regardless of which flags are set.
    fn build_column_expr(
        name: &str,
        attr: &FieldAttr,
        kind: RustTypeKind,
        nullable: bool,
        is_pk: bool,
    ) -> TokenStream2 {
        let name_lit = LitStr::new(name, proc_macro2::Span::call_site());
        let sql_lit = LitStr::new(&attr.sql, proc_macro2::Span::call_site());
        let kind_token = kind.to_token();

        let mut expr = quote! {
            ::rustio_core::contract::ModelColumn::new(#name_lit, #sql_lit, #kind_token)
        };
        if nullable {
            expr = quote! { #expr.nullable() };
        }
        if is_pk {
            expr = quote! { #expr.primary_key() };
        }

        // Flags — `.with_flags(...)` always emitted, even when no
        // flags are set. The shape inside depends on whether any
        // flag is on:
        //
        //   * No flags:  `SchemaFlags::empty()`
        //   * Any flags: `{ let mut __f = SchemaFlags::empty();
        //                   __f.searchable = true; ...; __f }`
        //
        // The block form is required because cc25125's `SchemaFlags`
        // is `#[non_exhaustive]` (blocking cross-crate struct
        // literals) and exposes only `empty()` + `searchable()` —
        // there's no per-flag setter. Field ASSIGNMENT is allowed on
        // a `#[non_exhaustive]` struct cross-crate even when struct
        // literals are blocked, so this `let mut … ; __f.x = true; __f`
        // pattern works in any consumer crate. It also evaluates in
        // const context (mut bindings + field mutation in const fn /
        // const initialisers have been stable since Rust 1.46), so
        // the whole `static SCHEMA: ModelSchema = …` initialiser
        // remains const-correct.
        let s = attr.searchable;
        let f = attr.filterable;
        let so = attr.sortable;
        let r = attr.readonly;
        let flags_expr = if !s && !f && !so && !r {
            quote! { ::rustio_core::contract::SchemaFlags::empty() }
        } else {
            let mut mutations = Vec::new();
            if s {
                mutations.push(quote! { __f.searchable = true; });
            }
            if f {
                mutations.push(quote! { __f.filterable = true; });
            }
            if so {
                mutations.push(quote! { __f.sortable = true; });
            }
            if r {
                mutations.push(quote! { __f.readonly = true; });
            }
            quote! {
                {
                    let mut __f = ::rustio_core::contract::SchemaFlags::empty();
                    #(#mutations)*
                    __f
                }
            }
        };
        expr = quote! { #expr.with_flags(#flags_expr) };

        if let Some(label) = &attr.label {
            let l = LitStr::new(label, proc_macro2::Span::call_site());
            expr = quote! { #expr.with_label(#l) };
        }
        if let Some(widget) = &attr.widget {
            let w = LitStr::new(widget, proc_macro2::Span::call_site());
            expr = quote! { #expr.with_widget(#w) };
        }
        // `references` is parsed (so the attribute doesn't error)
        // but NOT emitted as code: cc25125's `ModelColumn` has no
        // `references` field. Per the strict-isolation spec for
        // commit 2 ("DO NOT modify ModelColumn ... ignore or store
        // only if already supported"), we silently drop the value
        // here. When commit 3+ extends `ModelColumn` to carry FK
        // metadata, a one-line addition restores emission.
        let _ = &attr.references;

        expr
    }

    /// Parse the struct-level `#[rustio(table = "...")]`. Required.
    fn parse_table_attr(attrs: &[Attribute]) -> syn::Result<String> {
        for attr in attrs {
            if !attr.path().is_ident("rustio") {
                continue;
            }
            let parsed: TableAttr = attr.parse_args()?;
            return Ok(parsed.table);
        }
        Err(syn::Error::new(
            proc_macro2::Span::call_site(),
            "RustioModel requires a `#[rustio(table = \"...\")]` attribute on the struct",
        ))
    }

    /// Parse all `#[rustio(...)]` attributes on a single field,
    /// merging into one `FieldAttr`.
    fn parse_field_attr(attrs: &[Attribute]) -> syn::Result<FieldAttr> {
        let mut out = FieldAttr::default();
        for attr in attrs {
            if !attr.path().is_ident("rustio") {
                continue;
            }
            let parsed: FieldAttrTokens = attr.parse_args()?;
            // Last write wins per key — duplicates across multiple
            // `#[rustio(...)]` attributes overwrite. Rare enough we
            // don't bother detecting; loud parser errors would
            // produce noisier compiler output without helping anyone.
            for entry in parsed.entries {
                match entry {
                    AttrEntry::Sql(s) => out.sql = s,
                    AttrEntry::Searchable => out.searchable = true,
                    AttrEntry::Filterable => out.filterable = true,
                    AttrEntry::Sortable => out.sortable = true,
                    AttrEntry::Readonly => out.readonly = true,
                    AttrEntry::Widget(s) => out.widget = Some(s),
                    AttrEntry::Label(s) => out.label = Some(s),
                    AttrEntry::References(s) => out.references = Some(s),
                }
            }
        }
        Ok(out)
    }

    // ---- Attribute parsers (syn::parse plumbing) ---------------------------

    struct TableAttr {
        table: String,
    }
    impl Parse for TableAttr {
        fn parse(input: ParseStream) -> syn::Result<Self> {
            let key: syn::Ident = input.parse()?;
            if key != "table" {
                return Err(syn::Error::new(
                    key.span(),
                    "expected `table = \"...\"` on the struct",
                ));
            }
            input.parse::<Token![=]>()?;
            let value: LitStr = input.parse()?;
            // Tolerate trailing tokens (commas etc) silently — only
            // error on extras that aren't whitespace.
            Ok(Self { table: value.value() })
        }
    }

    enum AttrEntry {
        Sql(String),
        Searchable,
        Filterable,
        Sortable,
        Readonly,
        Widget(String),
        Label(String),
        References(String),
    }

    struct FieldAttrTokens {
        entries: Vec<AttrEntry>,
    }
    impl Parse for FieldAttrTokens {
        fn parse(input: ParseStream) -> syn::Result<Self> {
            let mut entries = Vec::new();
            loop {
                if input.is_empty() {
                    break;
                }
                let key: syn::Ident = input.parse()?;
                let key_str = key.to_string();
                let entry = match key_str.as_str() {
                    "sql" => {
                        input.parse::<Token![=]>()?;
                        AttrEntry::Sql(input.parse::<LitStr>()?.value())
                    }
                    "searchable" => AttrEntry::Searchable,
                    "filterable" => AttrEntry::Filterable,
                    "sortable" => AttrEntry::Sortable,
                    "readonly" => AttrEntry::Readonly,
                    "widget" => {
                        input.parse::<Token![=]>()?;
                        AttrEntry::Widget(input.parse::<LitStr>()?.value())
                    }
                    "label" => {
                        input.parse::<Token![=]>()?;
                        AttrEntry::Label(input.parse::<LitStr>()?.value())
                    }
                    "references" => {
                        input.parse::<Token![=]>()?;
                        AttrEntry::References(input.parse::<LitStr>()?.value())
                    }
                    other => {
                        return Err(syn::Error::new(
                            key.span(),
                            format!(
                                "unknown rustio attribute `{other}`. \
                                 Known: sql, searchable, filterable, sortable, \
                                 readonly, widget, label, references."
                            ),
                        ));
                    }
                };
                entries.push(entry);
                if input.is_empty() {
                    break;
                }
                input.parse::<Token![,]>()?;
            }
            Ok(Self { entries })
        }
    }
}
