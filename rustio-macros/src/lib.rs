use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{parse_macro_input, Data, DeriveInput, Fields, Type};

#[derive(Clone, Copy)]
enum FieldKind {
    I32,
    I64,
    String,
    Bool,
}

struct FieldInfo {
    ident: syn::Ident,
    name_str: String,
    kind: FieldKind,
    editable: bool,
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
        let kind = match classify_type(&f.ty) {
            Some(k) => k,
            None => {
                return syn::Error::new_spanned(
                    &f.ty,
                    "RustioAdmin: unsupported field type (supported: i32, i64, String, bool)",
                )
                .to_compile_error()
                .into();
            }
        };
        let editable = name_str != "id";
        fields.push(FieldInfo {
            ident,
            name_str,
            kind,
            editable,
        });
    }

    let admin_name = pluralize(&name.to_string().to_lowercase());
    let display_name = pluralize(&name.to_string());

    let field_entries: Vec<TokenStream2> = fields
        .iter()
        .map(|f| {
            let n = &f.name_str;
            let kind_token = kind_token(f.kind);
            let editable = f.editable;
            quote! {
                ::rustio_core::admin::AdminField {
                    name: #n,
                    ty: #kind_token,
                    editable: #editable,
                }
            }
        })
        .collect();

    let display_arms: Vec<TokenStream2> = fields
        .iter()
        .map(|f| {
            let ident = &f.ident;
            let name_str = &f.name_str;
            quote! {
                #name_str => Some(self.#ident.to_string()),
            }
        })
        .collect();

    let from_form_assignments: Vec<TokenStream2> =
        fields.iter().map(from_form_assignment).collect();

    let expanded = quote! {
        impl ::rustio_core::admin::AdminModel for #name {
            const ADMIN_NAME: &'static str = #admin_name;
            const DISPLAY_NAME: &'static str = #display_name;
            const FIELDS: &'static [::rustio_core::admin::AdminField] = &[
                #( #field_entries ),*
            ];

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

fn classify_type(ty: &Type) -> Option<FieldKind> {
    if let Type::Path(syn::TypePath { path, .. }) = ty {
        if let Some(last) = path.segments.last() {
            let ident = last.ident.to_string();
            return match ident.as_str() {
                "i32" => Some(FieldKind::I32),
                "i64" => Some(FieldKind::I64),
                "String" => Some(FieldKind::String),
                "bool" => Some(FieldKind::Bool),
                _ => None,
            };
        }
    }
    None
}

fn kind_token(kind: FieldKind) -> TokenStream2 {
    match kind {
        FieldKind::I32 => quote! { ::rustio_core::admin::FieldType::I32 },
        FieldKind::I64 => quote! { ::rustio_core::admin::FieldType::I64 },
        FieldKind::String => quote! { ::rustio_core::admin::FieldType::String },
        FieldKind::Bool => quote! { ::rustio_core::admin::FieldType::Bool },
    }
}

fn from_form_assignment(f: &FieldInfo) -> TokenStream2 {
    let ident = &f.ident;
    let name_str = &f.name_str;
    if !f.editable {
        return quote! { #ident: id.unwrap_or(0), };
    }
    match f.kind {
        FieldKind::String => quote! {
            #ident: form.get(#name_str).unwrap_or("").to_owned(),
        },
        FieldKind::Bool => quote! {
            #ident: matches!(form.get(#name_str), Some(v) if v == "on" || v == "true"),
        },
        FieldKind::I64 => quote! {
            #ident: form
                .get(#name_str)
                .unwrap_or("0")
                .parse::<i64>()
                .map_err(|_| ::rustio_core::Error::BadRequest(
                    format!("invalid integer for field `{}`", #name_str)
                ))?,
        },
        FieldKind::I32 => quote! {
            #ident: form
                .get(#name_str)
                .unwrap_or("0")
                .parse::<i32>()
                .map_err(|_| ::rustio_core::Error::BadRequest(
                    format!("invalid integer for field `{}`", #name_str)
                ))?,
        },
    }
}
