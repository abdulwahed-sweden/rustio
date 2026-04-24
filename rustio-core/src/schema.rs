//! The machine-readable schema. This file's JSON shape is the only
//! stable contract external tools (the AI planner, downstream
//! generators, dashboards) are allowed to depend on.

use serde::{Deserialize, Serialize};

/// Bumped only when the schema shape changes in a breaking way.
pub const SCHEMA_VERSION: u32 = 2;

pub const VALID_TYPE_NAMES: &[&str] =
    &["i32", "i64", "String", "bool", "DateTime", "OptionalString", "OptionalI64"];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Schema {
    pub version: u32,
    pub rustio_version: String,
    pub models: Vec<SchemaModel>,
}

impl Schema {
    pub fn new(rustio_version: impl Into<String>) -> Self {
        Self {
            version: SCHEMA_VERSION,
            rustio_version: rustio_version.into(),
            models: Vec::new(),
        }
    }

    pub fn model(&self, name: &str) -> Option<&SchemaModel> {
        self.models.iter().find(|m| m.name == name)
    }

    pub fn relation_for(&self, model: &str, field: &str) -> Option<&SchemaRelation> {
        self.model(model)
            .and_then(|m| m.fields.iter().find(|f| f.name == field))
            .and_then(|f| f.relation.as_ref())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SchemaModel {
    pub name: String,
    pub table: String,
    pub admin_name: String,
    pub display_name: String,
    pub singular_name: String,
    #[serde(default)]
    pub core: bool,
    pub fields: Vec<SchemaField>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SchemaField {
    pub name: String,
    #[serde(rename = "type")]
    pub field_type: String,
    pub nullable: bool,
    pub editable: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relation: Option<SchemaRelation>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SchemaRelation {
    pub model: String,
    pub field: String,
    pub kind: RelationKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_field: Option<String>,
}

#[non_exhaustive]
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RelationKind {
    BelongsTo,
    HasMany,
}

/// Convert a Rust type name (as the derive macro writes it) into one of
/// our canonical schema type strings. The match is deliberately
/// exhaustive — a new field type means adding a variant here, and
/// forgetting produces a compile error.
pub fn field_type_name(kind: FieldKind) -> &'static str {
    match kind {
        FieldKind::I32 => "i32",
        FieldKind::I64 => "i64",
        FieldKind::Bool => "bool",
        FieldKind::String => "String",
        FieldKind::DateTime => "DateTime",
        FieldKind::OptionalI64 => "OptionalI64",
        FieldKind::OptionalString => "OptionalString",
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldKind {
    I32,
    I64,
    Bool,
    String,
    DateTime,
    OptionalI64,
    OptionalString,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_round_trips() {
        let s = Schema {
            version: SCHEMA_VERSION,
            rustio_version: "0.9.0".into(),
            models: vec![SchemaModel {
                name: "Post".into(),
                table: "posts".into(),
                admin_name: "posts".into(),
                display_name: "Posts".into(),
                singular_name: "Post".into(),
                core: false,
                fields: vec![SchemaField {
                    name: "id".into(),
                    field_type: "i64".into(),
                    nullable: false,
                    editable: false,
                    relation: None,
                }],
            }],
        };
        let json = serde_json::to_string(&s).unwrap();
        let back: Schema = serde_json::from_str(&json).unwrap();
        assert_eq!(s, back);
    }

    #[test]
    fn relation_lookup_works() {
        let s = Schema {
            version: SCHEMA_VERSION,
            rustio_version: "0.9.0".into(),
            models: vec![SchemaModel {
                name: "Post".into(),
                table: "posts".into(),
                admin_name: "posts".into(),
                display_name: "Posts".into(),
                singular_name: "Post".into(),
                core: false,
                fields: vec![SchemaField {
                    name: "author_id".into(),
                    field_type: "i64".into(),
                    nullable: false,
                    editable: true,
                    relation: Some(SchemaRelation {
                        model: "Author".into(),
                        field: "id".into(),
                        kind: RelationKind::BelongsTo,
                        display_field: Some("name".into()),
                    }),
                }],
            }],
        };
        assert!(s.relation_for("Post", "author_id").is_some());
        assert!(s.relation_for("Post", "title").is_none());
    }
}
