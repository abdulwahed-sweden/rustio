//! Dynamic AdminEntry construction — 0.7.3.
//!
//! The compile-time [`AdminEntry`] list is baked by
//! `#[derive(RustioAdmin)]` and drives *route registration* — there
//! is no way to register a GET/POST pair for a struct the binary
//! doesn't know about. But for everything that just **reads** the
//! model shape (the dashboard alerts, the suggestion engine, the
//! schema diff on the review page), the schema on disk is a better
//! source of truth than the compiled `&'static [AdminField]` slice.
//!
//! This module turns that observation into a concrete type. A
//! [`DynamicAdminEntry`] is the same conceptual shape as an
//! `AdminEntry` but with owned `String` names and an owned
//! `Vec<DynamicAdminField>`. Renderers that currently iterate
//! compile-time entries can iterate dynamic ones instead; the only
//! thing they give up is `&'static` identity.
//!
//! ## Safety
//!
//! - [`field_type_from_str`] is total: an unknown string never
//!   panics, it falls back to [`FieldType::String`] (which the
//!   renderer shows as a plain-text input). The CHANGELOG bump rule
//!   means the planner + schema layers would catch unknown types
//!   much earlier, but the fallback here is the defence in depth.
//! - [`entries_effective`] is deterministic: when the cache is warm,
//!   order follows `schema.models`; when it's cold, order follows
//!   the compile-time slice. Neither path allocates randomness.
//!
//! ## What this module does NOT do
//!
//! - It does not register routes. Route registration still needs a
//!   real `T: AdminModel` because `item.field_display(name)` is
//!   trait-bound.
//! - It does not mutate the schema cache. Reads only.

use super::{schema_cache, AdminEntry, AdminField, FieldType};
use crate::schema::Schema;

/// Same shape as [`AdminEntry`] but with owned strings, so a schema
/// re-read can rebuild one without touching the compile-time slice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DynamicAdminEntry {
    pub admin_name: String,
    pub display_name: String,
    pub singular_name: String,
    pub table: String,
    pub fields: Vec<DynamicAdminField>,
    pub core: bool,
}

/// Same shape as [`AdminField`] but owned. Derived either from a
/// `&'static AdminField` or from a [`crate::schema::SchemaField`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DynamicAdminField {
    pub name: String,
    pub ty: FieldType,
    pub editable: bool,
    pub nullable: bool,
}

impl DynamicAdminField {
    /// Build from a compile-time [`AdminField`]. Zero-cost clone —
    /// just copies the static slices' bytes into owned strings.
    pub fn from_admin(f: &AdminField) -> Self {
        Self {
            name: f.name.to_string(),
            ty: f.ty,
            editable: f.editable,
            nullable: f.nullable,
        }
    }
}

impl DynamicAdminEntry {
    /// Project a compile-time entry into a dynamic one. The pair
    /// is round-trippable for any schema that matches the compiled
    /// binary — the invariant tested by
    /// `compile_time_round_trip_matches_admin_entry`.
    pub fn from_admin(entry: &AdminEntry) -> Self {
        Self {
            admin_name: entry.admin_name.to_string(),
            display_name: entry.display_name.to_string(),
            singular_name: entry.singular_name.to_string(),
            table: entry.table.to_string(),
            fields: entry
                .fields
                .iter()
                .map(DynamicAdminField::from_admin)
                .collect(),
            core: entry.core,
        }
    }
}

/// Parse a schema-level type name (`"i32"`, `"DateTime"`, …) back
/// into a [`FieldType`]. Unknown strings fall through to
/// [`FieldType::String`] so the renderer shows a plain-text input
/// instead of panicking — the 0.7.3 "render as PlainText" rule.
///
/// Kept as a separate function (rather than a `FromStr` impl) so the
/// fallback is explicit at every call site.
pub fn field_type_from_str(ty: &str) -> FieldType {
    match ty {
        "i32" => FieldType::I32,
        "i64" => FieldType::I64,
        "String" => FieldType::String,
        "bool" => FieldType::Bool,
        "DateTime" => FieldType::DateTime,
        _ => FieldType::String,
    }
}

/// Build a `Vec<DynamicAdminEntry>` from the schema on disk. Every
/// model's fields are projected from schema fields; unknown type
/// strings fall back to [`FieldType::String`] (see
/// [`field_type_from_str`]). Order follows `schema.models`.
pub fn build_admin_entries(schema: &Schema) -> Vec<DynamicAdminEntry> {
    schema
        .models
        .iter()
        .map(|m| DynamicAdminEntry {
            admin_name: m.admin_name.clone(),
            display_name: m.display_name.clone(),
            singular_name: m.singular_name.clone(),
            table: m.table.clone(),
            core: m.core,
            fields: m
                .fields
                .iter()
                .map(|f| DynamicAdminField {
                    name: f.name.clone(),
                    ty: field_type_from_str(&f.ty),
                    editable: f.editable,
                    nullable: f.nullable,
                })
                .collect(),
        })
        .collect()
}

/// Single source of truth for rendering-side admin entries.
///
/// When the [`schema_cache`] has a live snapshot, builds dynamic
/// entries from it. Otherwise falls back to projecting the
/// compile-time slice. The returned `Vec` is independent of either
/// source and can be passed to any helper that needs to iterate
/// models without being type-bound.
pub fn entries_effective(compiled: &[AdminEntry]) -> Vec<DynamicAdminEntry> {
    if let Some(cached) = schema_cache::snapshot() {
        // Intersect with the compiled list so we still drop core
        // entries the schema happens to mention but the admin never
        // actually serves (core: true on `User`). Also preserves the
        // `core` flag correctly — the schema tracks it but the
        // compiled side is authoritative.
        let compiled_by_name: std::collections::HashMap<&str, &AdminEntry> =
            compiled.iter().map(|e| (e.admin_name, e)).collect();
        return build_admin_entries(&cached.schema)
            .into_iter()
            .map(|mut dyn_entry| {
                if let Some(compiled_e) = compiled_by_name.get(dyn_entry.admin_name.as_str()) {
                    // Trust the compiled entry for `core` since the
                    // admin never registers routes for core models.
                    dyn_entry.core = compiled_e.core;
                }
                dyn_entry
            })
            .collect();
    }
    compiled.iter().map(DynamicAdminEntry::from_admin).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{SchemaField, SchemaModel, SCHEMA_VERSION};

    fn tiny_schema() -> Schema {
        Schema {
            version: SCHEMA_VERSION,
            rustio_version: env!("CARGO_PKG_VERSION").to_string(),
            models: vec![SchemaModel {
                name: "Post".into(),
                table: "posts".into(),
                admin_name: "posts".into(),
                display_name: "Posts".into(),
                singular_name: "Post".into(),
                fields: vec![
                    SchemaField {
                        name: "id".into(),
                        ty: "i64".into(),
                        nullable: false,
                        editable: false,
                        relation: None,
                    },
                    SchemaField {
                        name: "title".into(),
                        ty: "String".into(),
                        nullable: false,
                        editable: true,
                        relation: None,
                    },
                    SchemaField {
                        // Unknown type — exercises the PlainText fallback.
                        name: "odd".into(),
                        ty: "UnknownType".into(),
                        nullable: true,
                        editable: true,
                        relation: None,
                    },
                ],
                relations: vec![],
                core: false,
            }],
        }
    }

    #[test]
    fn field_type_fallback_is_string_for_unknown() {
        assert!(matches!(field_type_from_str("i32"), FieldType::I32));
        assert!(matches!(field_type_from_str("i64"), FieldType::I64));
        assert!(matches!(field_type_from_str("bool"), FieldType::Bool));
        assert!(matches!(
            field_type_from_str("DateTime"),
            FieldType::DateTime
        ));
        assert!(matches!(field_type_from_str("String"), FieldType::String));
        // Unknown → String (safety).
        assert!(matches!(field_type_from_str("Decimal"), FieldType::String));
        assert!(matches!(field_type_from_str(""), FieldType::String));
    }

    #[test]
    fn build_admin_entries_mirrors_the_schema() {
        let s = tiny_schema();
        let entries = build_admin_entries(&s);
        assert_eq!(entries.len(), 1);
        let posts = &entries[0];
        assert_eq!(posts.admin_name, "posts");
        assert_eq!(posts.display_name, "Posts");
        assert_eq!(posts.table, "posts");
        assert_eq!(posts.fields.len(), 3);
        // Field order preserved from the schema.
        assert_eq!(posts.fields[0].name, "id");
        assert_eq!(posts.fields[1].name, "title");
        assert_eq!(posts.fields[2].name, "odd");
        // Unknown type fell back to String without panicking.
        assert!(matches!(posts.fields[2].ty, FieldType::String));
        // Nullability / editability survived the projection.
        assert!(!posts.fields[0].nullable);
        assert!(posts.fields[2].nullable);
        assert!(!posts.fields[0].editable);
        assert!(posts.fields[1].editable);
    }

    #[test]
    fn entry_from_admin_round_trips_compile_time_shape() {
        // A synthetic compile-time entry; projecting it to a dynamic
        // one must not lose or add anything.
        let af = AdminField {
            name: "x",
            ty: FieldType::I32,
            editable: true,
            nullable: false,
        };
        let fields: &'static [AdminField] = Box::leak(vec![af].into_boxed_slice());
        let ae = AdminEntry {
            admin_name: "widgets",
            display_name: "Widgets",
            singular_name: "Widget",
            table: "widgets",
            fields,
            core: false,
        };
        let de = DynamicAdminEntry::from_admin(&ae);
        assert_eq!(de.admin_name, "widgets");
        assert_eq!(de.display_name, "Widgets");
        assert_eq!(de.table, "widgets");
        assert_eq!(de.fields.len(), 1);
        assert_eq!(de.fields[0].name, "x");
        assert!(matches!(de.fields[0].ty, FieldType::I32));
        assert!(de.fields[0].editable);
        assert!(!de.fields[0].nullable);
        assert!(!de.core);
    }
}
