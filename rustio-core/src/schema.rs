//! Schema export: a deterministic, machine-readable description of every
//! model the admin knows about.
//!
//! The emitted `rustio.schema.json` file is **the** interface between a
//! RustIO project and external tooling — including the Phase 2 AI layer.
//! Its shape is versioned and expected to stay stable across patch
//! releases. Additions in minor releases are allowed; renames and removals
//! are breaking changes and must bump [`SCHEMA_VERSION`].
//!
//! The schema is produced by introspecting a built [`Admin`] registry,
//! not by parsing source code. This guarantees that whatever the admin
//! actually serves is what the schema describes.

use std::fs;
use std::path::Path;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::admin::{Admin, AdminField, FieldType};
use crate::error::Error;

/// Version of the `rustio.schema.json` format itself. Independent of the
/// rustio-core crate version — a single schema version can outlive many
/// rustio-core releases as long as the wire format doesn't change.
pub const SCHEMA_VERSION: u32 = 1;

/// Top-level schema document. Serialised as `rustio.schema.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Schema {
    pub version: u32,
    pub generated_at: DateTime<Utc>,
    pub rustio_version: String,
    pub models: Vec<SchemaModel>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchemaModel {
    pub name: String,
    pub table: String,
    pub admin_name: String,
    pub display_name: String,
    pub singular_name: String,
    pub fields: Vec<SchemaField>,
    /// Placeholder for Phase 2. Always empty in 0.4.0 — reserving the
    /// field now means 0.5.0 can add relations without a breaking change.
    pub relations: Vec<SchemaRelation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchemaField {
    pub name: String,
    #[serde(rename = "type")]
    pub ty: String,
    pub nullable: bool,
    pub editable: bool,
}

/// Placeholder relation shape. Present so consumers can depend on the
/// `relations` field existing in every model. Concrete variants land in
/// 0.5.0 (`belongs_to`, `has_many`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchemaRelation {
    pub kind: String,
    pub to: String,
    pub via: String,
}

impl Schema {
    /// Build a schema from an already-constructed [`Admin`]. This is the
    /// single supported path — we don't parse Rust sources or read the
    /// DB, so whatever the admin is serving is exactly what the schema
    /// describes.
    pub fn from_admin(admin: &Admin) -> Self {
        let models = admin
            .entries()
            .iter()
            .map(SchemaModel::from_entry)
            .collect();
        Self {
            version: SCHEMA_VERSION,
            generated_at: Utc::now(),
            rustio_version: env!("CARGO_PKG_VERSION").to_string(),
            models,
        }
    }

    /// Serialise to pretty JSON. We pretty-print on purpose: the file is
    /// meant to be read by humans during code review and by AI tools that
    /// benefit from stable line-level anchors.
    pub fn to_pretty_json(&self) -> Result<String, Error> {
        serde_json::to_string_pretty(self).map_err(|e| Error::Internal(e.to_string()))
    }

    /// Write the schema to a file, atomically. Uses a temp-file + rename
    /// so a concurrent reader can never observe a half-written JSON file.
    pub fn write_to(&self, path: &Path) -> Result<(), Error> {
        let json = self.to_pretty_json()?;
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, json).map_err(|e| Error::Internal(e.to_string()))?;
        fs::rename(&tmp, path).map_err(|e| Error::Internal(e.to_string()))?;
        Ok(())
    }
}

impl SchemaModel {
    fn from_entry(entry: &crate::admin::AdminEntry) -> Self {
        Self {
            name: entry.singular_name.to_string(),
            table: entry.table.to_string(),
            admin_name: entry.admin_name.to_string(),
            display_name: entry.display_name.to_string(),
            singular_name: entry.singular_name.to_string(),
            fields: entry
                .fields
                .iter()
                .map(SchemaField::from_admin_field)
                .collect(),
            relations: Vec::new(),
        }
    }
}

impl SchemaField {
    fn from_admin_field(f: &AdminField) -> Self {
        Self {
            name: f.name.to_string(),
            ty: field_type_name(f.ty).to_string(),
            nullable: f.nullable,
            editable: f.editable,
        }
    }
}

/// Stable string identifier for each [`FieldType`] variant. Used in the
/// exported schema and as the primary key external tools key off of.
/// **Changing a mapping here is a breaking change** — bump
/// [`SCHEMA_VERSION`] if you ever have to.
///
/// We deliberately do NOT include a wildcard arm. `FieldType` is
/// `#[non_exhaustive]` only to downstream crates; inside rustio-core any
/// added variant must be mapped here or the build breaks — exactly the
/// signal we want when extending the type system.
fn field_type_name(ty: FieldType) -> &'static str {
    match ty {
        FieldType::I32 => "i32",
        FieldType::I64 => "i64",
        FieldType::String => "String",
        FieldType::Bool => "bool",
        FieldType::DateTime => "DateTime",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::admin::{Admin, AdminField, AdminModel, FieldType, FormData};
    use crate::error::Error;
    use crate::orm::{Model, Row, Value};

    struct Post;

    impl Model for Post {
        const TABLE: &'static str = "posts";
        const COLUMNS: &'static [&'static str] = &["id", "title", "published_at"];
        const INSERT_COLUMNS: &'static [&'static str] = &["title", "published_at"];
        fn id(&self) -> i64 {
            0
        }
        fn from_row(_: Row<'_>) -> Result<Self, Error> {
            unimplemented!()
        }
        fn insert_values(&self) -> Vec<Value> {
            Vec::new()
        }
    }

    impl AdminModel for Post {
        const ADMIN_NAME: &'static str = "posts";
        const DISPLAY_NAME: &'static str = "Posts";
        const FIELDS: &'static [AdminField] = &[
            AdminField {
                name: "id",
                ty: FieldType::I64,
                editable: false,
                nullable: false,
            },
            AdminField {
                name: "title",
                ty: FieldType::String,
                editable: true,
                nullable: false,
            },
            AdminField {
                name: "published_at",
                ty: FieldType::DateTime,
                editable: true,
                nullable: true,
            },
        ];
        fn singular_name() -> &'static str {
            "Post"
        }
        fn field_display(&self, _: &str) -> Option<String> {
            None
        }
        fn from_form(_: &FormData, _: Option<i64>) -> Result<Self, Error> {
            unimplemented!()
        }
    }

    #[test]
    fn schema_reflects_admin_registry() {
        let admin = Admin::new().model::<Post>();
        let schema = Schema::from_admin(&admin);

        assert_eq!(schema.version, SCHEMA_VERSION);
        assert_eq!(schema.models.len(), 1);

        let m = &schema.models[0];
        assert_eq!(m.name, "Post");
        assert_eq!(m.table, "posts");
        assert_eq!(m.admin_name, "posts");
        assert_eq!(m.display_name, "Posts");
        assert_eq!(m.singular_name, "Post");
        assert_eq!(m.fields.len(), 3);
        assert!(m.relations.is_empty());

        let title = m.fields.iter().find(|f| f.name == "title").unwrap();
        assert_eq!(title.ty, "String");
        assert!(!title.nullable);
        assert!(title.editable);

        let pub_at = m.fields.iter().find(|f| f.name == "published_at").unwrap();
        assert_eq!(pub_at.ty, "DateTime");
        assert!(pub_at.nullable);
        assert!(pub_at.editable);

        let id = m.fields.iter().find(|f| f.name == "id").unwrap();
        assert_eq!(id.ty, "i64");
        assert!(!id.editable);
    }

    #[test]
    fn to_pretty_json_round_trips() {
        let admin = Admin::new().model::<Post>();
        let schema = Schema::from_admin(&admin);
        let json = schema.to_pretty_json().unwrap();
        let parsed: Schema = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.version, schema.version);
        assert_eq!(parsed.models.len(), schema.models.len());
        assert_eq!(parsed.models[0].fields.len(), 3);
    }
}
