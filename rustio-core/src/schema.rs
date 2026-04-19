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
//!
//! ## Determinism contract
//!
//! For a given registered model set, `Schema::from_admin` produces
//! **byte-for-byte identical JSON** on every invocation:
//!
//! - Models are emitted sorted by name.
//! - Fields within a model are emitted sorted by name.
//! - No timestamps, hashes, or environment-derived values are written
//!   to the file.
//!
//! This is what makes the schema usable as a diff target in CI and as a
//! stable anchor for AI-layer tooling.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::admin::{Admin, AdminField, FieldType};
use crate::error::Error;

/// Version of the `rustio.schema.json` format itself. Independent of the
/// rustio-core crate version — a single schema version can outlive many
/// rustio-core releases as long as the wire format doesn't change.
///
/// Bumping this value is a **breaking** change: every consumer of the
/// schema (including the AI layer) will refuse to load older or newer
/// documents until they are explicitly migrated.
pub const SCHEMA_VERSION: u32 = 1;

/// The complete set of type names that may appear in
/// `SchemaField.ty`. Anything outside this set is a schema error and the
/// AI boundary rejects it. Kept as a `const` so tests and validators
/// share a single source of truth.
pub const VALID_TYPE_NAMES: &[&str] = &["i32", "i64", "String", "bool", "DateTime"];

/// Top-level schema document. Serialised as `rustio.schema.json`.
///
/// `#[serde(deny_unknown_fields)]` locks the wire format: a future
/// schema version adding a field will fail to load under the older
/// rustio-core unless the version number is bumped in lockstep. Combined
/// with [`SCHEMA_VERSION`], this catches accidental silent drift.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Schema {
    pub version: u32,
    pub rustio_version: String,
    pub models: Vec<SchemaModel>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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
    /// `true` for built-in infrastructure models (e.g. `User`). The AI
    /// layer uses this to refuse destructive primitives (remove_model,
    /// remove_field) against core models.
    #[serde(default)]
    pub core: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SchemaField {
    pub name: String,
    #[serde(rename = "type")]
    pub ty: String,
    pub nullable: bool,
    pub editable: bool,
    /// 0.8.0: optional typed relation descriptor when the field is a
    /// foreign key. Old schema files without this key parse cleanly
    /// (defaults to `None`) and serialise back without it (skipped on
    /// `None`), so projects that don't use relations are byte-identical
    /// to the 0.7.x format.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relation: Option<Relation>,
}

/// 0.8.0 — first-class foreign-key annotation on a field.
///
/// Only `belongs_to` is stored explicitly. The inverse direction
/// (`has_many`) is *inferred* at runtime by
/// [`Schema::incoming_relations`]; adding it as a stored variant
/// would double-book the same information and drift over time.
///
/// Conservative by design: if any of these fields is missing, old
/// consumers ignore the whole `relation` key because the parent
/// field is `Option<Relation>`. The executor never writes this shape
/// yet — projects opt in by adding the block to `rustio.schema.json`
/// (the 0.9.0 foreign-key enforcement pass will own the write path).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Relation {
    /// Target model name (e.g. `"Applicant"`).
    pub model: String,
    /// Target field name — conventionally `"id"`, but explicit so a
    /// future release can support multi-column keys without another
    /// schema bump.
    pub field: String,
    /// Direction marker. 0.8.0 only accepts `BelongsTo` as stored;
    /// `HasMany` is reserved for inferred results.
    pub kind: RelationKind,
}

/// Typed relation direction. Kept `#[non_exhaustive]` so a later
/// pass can add variants (`OneToOne`, `ManyToMany`) without breaking
/// downstream matchers. Callers must include a wildcard arm.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RelationKind {
    BelongsTo,
    HasMany,
}

impl RelationKind {
    pub fn as_str(self) -> &'static str {
        match self {
            RelationKind::BelongsTo => "belongs_to",
            RelationKind::HasMany => "has_many",
        }
    }
}

/// Placeholder relation shape left from 0.4.0. Still serialised in
/// `SchemaModel.relations` for backward compatibility (reserved slot
/// for future per-model metadata). The 0.8.0 flow uses
/// [`SchemaField::relation`] instead — the per-field location is the
/// source of truth.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SchemaRelation {
    pub kind: String,
    pub to: String,
    pub via: String,
}

/// Reasons a schema can be rejected. Named variants (never raw strings)
/// so tooling can branch on the failure kind.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub enum SchemaError {
    /// The document's `version` field doesn't match [`SCHEMA_VERSION`].
    VersionMismatch { found: u32, expected: u32 },
    /// Two models share the same `name`.
    DuplicateModel(String),
    /// Two fields in the same model share the same `name`.
    DuplicateField { model: String, field: String },
    /// A field's `type` is not in [`VALID_TYPE_NAMES`].
    InvalidType {
        model: String,
        field: String,
        ty: String,
    },
    /// A relation's `to` doesn't name any model in the schema.
    UnknownRelationTarget { from: String, to: String },
    /// An identifier-shaped string is empty. Guards against callers that
    /// forget to fill in `name`, `table`, etc.
    EmptyIdentifier(&'static str),
    /// Failed to parse a schema document from its on-disk bytes.
    Parse(String),
}

impl std::fmt::Display for SchemaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::VersionMismatch { found, expected } => write!(
                f,
                "schema version mismatch: found {found}, expected {expected}"
            ),
            Self::DuplicateModel(name) => write!(f, "duplicate model `{name}`"),
            Self::DuplicateField { model, field } => {
                write!(f, "duplicate field `{field}` in model `{model}`")
            }
            Self::InvalidType { model, field, ty } => write!(
                f,
                "field `{model}.{field}` has invalid type `{ty}` (valid: {valid})",
                valid = VALID_TYPE_NAMES.join(", "),
            ),
            Self::UnknownRelationTarget { from, to } => {
                write!(f, "relation from `{from}` targets unknown model `{to}`")
            }
            Self::EmptyIdentifier(which) => write!(f, "empty {which}"),
            Self::Parse(msg) => write!(f, "schema parse error: {msg}"),
        }
    }
}

impl std::error::Error for SchemaError {}

impl From<SchemaError> for Error {
    fn from(e: SchemaError) -> Self {
        Error::Internal(e.to_string())
    }
}

/// 0.8.0 — one inferred incoming relation. Produced by
/// [`Schema::incoming_relations`]; not stored on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncomingRelation {
    /// The child model name (holder of the foreign-key field).
    pub from_model: String,
    /// The child field that points at the target.
    pub from_field: String,
    /// Always the target's own name — supplied for symmetry.
    pub to_model: String,
    /// Always `RelationKind::BelongsTo` in the stored direction;
    /// `HasMany` is the implicit inverse a caller is asking about.
    pub kind: RelationKind,
}

impl Schema {
    /// Look up the `Relation` descriptor attached to `(model, field)`,
    /// if the field carries one. Returns `None` if the model or field
    /// doesn't exist, or if the field has no relation metadata. A
    /// schema without relations behaves identically to pre-0.8.0 —
    /// this accessor just returns `None` everywhere.
    pub fn relation_for(&self, model: &str, field: &str) -> Option<&Relation> {
        self.models
            .iter()
            .find(|m| m.name == model)?
            .fields
            .iter()
            .find(|f| f.name == field)?
            .relation
            .as_ref()
    }

    /// Enumerate every `belongs_to` relation in the schema that points
    /// *at* `model` — i.e. the `has_many` view. Order follows model
    /// order, then field order inside the model. Empty when no field
    /// in the schema references `model`. Deterministic.
    pub fn incoming_relations(&self, model: &str) -> Vec<IncomingRelation> {
        let mut out: Vec<IncomingRelation> = Vec::new();
        for m in &self.models {
            for f in &m.fields {
                if let Some(rel) = &f.relation {
                    if rel.model == model && matches!(rel.kind, RelationKind::BelongsTo) {
                        out.push(IncomingRelation {
                            from_model: m.name.clone(),
                            from_field: f.name.clone(),
                            to_model: model.to_string(),
                            kind: RelationKind::HasMany,
                        });
                    }
                }
            }
        }
        out
    }

    /// Build a schema from an already-constructed [`Admin`]. This is the
    /// single supported path — we don't parse Rust sources or read the
    /// DB, so whatever the admin is serving is exactly what the schema
    /// describes.
    ///
    /// Output is deterministic: models and fields are emitted in sorted
    /// order so two invocations on the same registry produce identical
    /// JSON bytes.
    pub fn from_admin(admin: &Admin) -> Self {
        let mut models: Vec<SchemaModel> = admin
            .entries()
            .iter()
            .map(SchemaModel::from_entry)
            .collect();
        models.sort_by(|a, b| a.name.cmp(&b.name));
        Self {
            version: SCHEMA_VERSION,
            rustio_version: env!("CARGO_PKG_VERSION").to_string(),
            models,
        }
    }

    /// Check the schema for internal consistency. Every production
    /// writer should call this before persisting and every consumer
    /// (including the AI layer) should call it after loading. The error
    /// is the first problem found; fix and revalidate.
    pub fn validate(&self) -> Result<(), SchemaError> {
        if self.version != SCHEMA_VERSION {
            return Err(SchemaError::VersionMismatch {
                found: self.version,
                expected: SCHEMA_VERSION,
            });
        }

        let mut model_names: BTreeSet<&str> = BTreeSet::new();
        for model in &self.models {
            if model.name.is_empty() {
                return Err(SchemaError::EmptyIdentifier("model name"));
            }
            if model.table.is_empty() {
                return Err(SchemaError::EmptyIdentifier("model table"));
            }
            if !model_names.insert(model.name.as_str()) {
                return Err(SchemaError::DuplicateModel(model.name.clone()));
            }
        }

        let valid_types: BTreeSet<&str> = VALID_TYPE_NAMES.iter().copied().collect();

        for model in &self.models {
            let mut field_names: BTreeSet<&str> = BTreeSet::new();
            for field in &model.fields {
                if field.name.is_empty() {
                    return Err(SchemaError::EmptyIdentifier("field name"));
                }
                if !field_names.insert(field.name.as_str()) {
                    return Err(SchemaError::DuplicateField {
                        model: model.name.clone(),
                        field: field.name.clone(),
                    });
                }
                if !valid_types.contains(field.ty.as_str()) {
                    return Err(SchemaError::InvalidType {
                        model: model.name.clone(),
                        field: field.name.clone(),
                        ty: field.ty.clone(),
                    });
                }
            }

            for relation in &model.relations {
                if !model_names.contains(relation.to.as_str()) {
                    return Err(SchemaError::UnknownRelationTarget {
                        from: model.name.clone(),
                        to: relation.to.clone(),
                    });
                }
            }
        }

        Ok(())
    }

    /// Parse + validate a schema document. Both deserialization failure
    /// (unknown fields, wrong types, missing keys) and any semantic
    /// problem surface as [`SchemaError`]. Safe default for anything
    /// reading a `rustio.schema.json` off disk.
    pub fn parse(json: &str) -> Result<Self, SchemaError> {
        let schema: Schema =
            serde_json::from_str(json).map_err(|e| SchemaError::Parse(e.to_string()))?;
        schema.validate()?;
        Ok(schema)
    }

    /// Serialise to pretty JSON with a trailing newline. We pretty-print
    /// on purpose: the file is meant to be read by humans during code
    /// review and by AI tools that benefit from stable line-level
    /// anchors.
    pub fn to_pretty_json(&self) -> Result<String, Error> {
        let mut out =
            serde_json::to_string_pretty(self).map_err(|e| Error::Internal(e.to_string()))?;
        out.push('\n');
        Ok(out)
    }

    /// Write the schema to a file, atomically. Validates first so a
    /// broken schema never lands on disk. Uses a temp-file + rename so
    /// a concurrent reader can never observe a half-written JSON file.
    pub fn write_to(&self, path: &Path) -> Result<(), Error> {
        self.validate()?;
        let json = self.to_pretty_json()?;
        let tmp = path.with_extension("json.tmp");
        // Best-effort cleanup if a previous aborted run left the tmp
        // behind; we ignore errors because `write` will surface any
        // real permission problem.
        let _ = fs::remove_file(&tmp);
        fs::write(&tmp, json).map_err(|e| Error::Internal(e.to_string()))?;
        if let Err(e) = fs::rename(&tmp, path) {
            // Rename failed — clean up the tmp so we don't leave a
            // stale `.json.tmp` next to the target on retry.
            let _ = fs::remove_file(&tmp);
            return Err(Error::Internal(e.to_string()));
        }
        Ok(())
    }
}

impl SchemaModel {
    fn from_entry(entry: &crate::admin::AdminEntry) -> Self {
        let mut fields: Vec<SchemaField> = entry
            .fields
            .iter()
            .map(SchemaField::from_admin_field)
            .collect();
        fields.sort_by(|a, b| a.name.cmp(&b.name));
        Self {
            name: entry.singular_name.to_string(),
            table: entry.table.to_string(),
            admin_name: entry.admin_name.to_string(),
            display_name: entry.display_name.to_string(),
            singular_name: entry.singular_name.to_string(),
            fields,
            relations: Vec::new(),
            core: entry.core,
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
            // Compile-time fields don't yet carry a relation
            // annotation — that lives in `rustio.schema.json` in 0.8.0
            // and will move to a macro attribute in 0.8.x.
            relation: None,
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
pub(crate) fn field_type_name(ty: FieldType) -> &'static str {
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

    // A second non-core model for tests that need two entries side by
    // side. Not called `User` because the built-in core `User` entry is
    // already seeded by `Admin::new()`.
    struct Book;

    impl Model for Book {
        const TABLE: &'static str = "books";
        const COLUMNS: &'static [&'static str] = &["id", "title"];
        const INSERT_COLUMNS: &'static [&'static str] = &["title"];
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

    impl AdminModel for Book {
        const ADMIN_NAME: &'static str = "books";
        const DISPLAY_NAME: &'static str = "Books";
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
        ];
        fn singular_name() -> &'static str {
            "Book"
        }
        fn field_display(&self, _: &str) -> Option<String> {
            None
        }
        fn from_form(_: &FormData, _: Option<i64>) -> Result<Self, Error> {
            unimplemented!()
        }
    }

    /// Find a model by name. Used through the tests because `Admin::new()`
    /// seeds the built-in core `User`, so `schema.models[0]` isn't a
    /// stable reference to "the first user-registered model".
    fn find<'a>(schema: &'a Schema, name: &str) -> &'a SchemaModel {
        schema
            .models
            .iter()
            .find(|m| m.name == name)
            .unwrap_or_else(|| panic!("no model named `{name}` in schema"))
    }

    #[test]
    fn schema_reflects_admin_registry() {
        let admin = Admin::new().model::<Post>();
        let schema = Schema::from_admin(&admin);

        assert_eq!(schema.version, SCHEMA_VERSION);
        // Core `User` + registered `Post`.
        assert_eq!(schema.models.len(), 2);

        let m = find(&schema, "Post");
        assert_eq!(m.table, "posts");
        assert_eq!(m.admin_name, "posts");
        assert_eq!(m.display_name, "Posts");
        assert_eq!(m.singular_name, "Post");
        assert_eq!(m.fields.len(), 3);
        assert!(m.relations.is_empty());
        assert!(!m.core, "user models must not be marked core");

        let title = m.fields.iter().find(|f| f.name == "title").unwrap();
        assert_eq!(title.ty, "String");
        assert!(!title.nullable);
        assert!(title.editable);

        let pub_at = m.fields.iter().find(|f| f.name == "published_at").unwrap();
        assert_eq!(pub_at.ty, "DateTime");
        assert!(pub_at.nullable);
        assert!(pub_at.editable);
    }

    #[test]
    fn core_user_model_is_always_present() {
        // The spec requires User in every project's schema. This is the
        // test that fails if someone accidentally removes the seeding
        // from `Admin::new()`.
        let schema = Schema::from_admin(&Admin::new());
        let user = find(&schema, "User");
        assert!(user.core, "User must be flagged as a core model");
        assert_eq!(user.table, "rustio_users");
        let pw = user
            .fields
            .iter()
            .find(|f| f.name == "password_hash")
            .unwrap();
        assert!(
            !pw.editable,
            "password_hash must never be exposed as editable via admin"
        );
        // created_at mirrors the real DB column — guards against the
        // schema under-describing the actual table shape.
        let created_at = user.fields.iter().find(|f| f.name == "created_at").unwrap();
        assert_eq!(created_at.ty, "DateTime");
        assert!(!created_at.editable);
    }

    #[test]
    fn schema_fields_are_sorted_by_name() {
        // Admin declares id, title, published_at in that order. The
        // schema must re-emit them alphabetically so the file is a
        // diffable source-of-truth.
        let schema = Schema::from_admin(&Admin::new().model::<Post>());
        let post = find(&schema, "Post");
        let names: Vec<&str> = post.fields.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, vec!["id", "published_at", "title"]);
    }

    #[test]
    fn schema_models_are_sorted_by_name() {
        // Register Post + Book (not User — that name collides with the
        // core model). Expect alphabetical output: Book, Post, User.
        let schema = Schema::from_admin(&Admin::new().model::<Post>().model::<Book>());
        let names: Vec<&str> = schema.models.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, vec!["Book", "Post", "User"]);
    }

    #[test]
    fn to_pretty_json_round_trips() {
        let schema = Schema::from_admin(&Admin::new().model::<Post>());
        let json = schema.to_pretty_json().unwrap();
        let parsed = Schema::parse(&json).unwrap();
        assert_eq!(parsed, schema);
    }

    #[test]
    fn to_pretty_json_ends_with_newline() {
        let schema = Schema::from_admin(&Admin::new().model::<Post>());
        let json = schema.to_pretty_json().unwrap();
        assert!(json.ends_with('\n'), "schema JSON must end with newline");
    }

    #[test]
    fn same_registry_produces_identical_bytes() {
        // The determinism contract: identical inputs → identical bytes.
        // If this ever fails, someone added a clock, hash, or env read
        // to the serialisation path.
        let a = Schema::from_admin(&Admin::new().model::<Post>().model::<Book>())
            .to_pretty_json()
            .unwrap();
        let b = Schema::from_admin(&Admin::new().model::<Post>().model::<Book>())
            .to_pretty_json()
            .unwrap();
        assert_eq!(a, b);
    }

    /// Byte-for-byte snapshot.
    ///
    /// Locks the wire format of `rustio.schema.json`. Any diff in field
    /// ordering, type-name mapping, or surrounding JSON punctuation
    /// fails this test. If an intentional shape change is landing, bump
    /// [`SCHEMA_VERSION`] and update both the expected string and every
    /// consumer in the same PR.
    #[test]
    fn schema_snapshot_is_byte_for_byte_stable() {
        // Register only `Post`; the core `User` is seeded automatically
        // by `Admin::new()`. The expected JSON below is the *complete*
        // wire format: locking both the test model and the core User
        // fields in place.
        let schema = Schema::from_admin(&Admin::new().model::<Post>());
        let actual = schema.to_pretty_json().unwrap();

        let expected = format!(
            r#"{{
  "version": 1,
  "rustio_version": "{rv}",
  "models": [
    {{
      "name": "Post",
      "table": "posts",
      "admin_name": "posts",
      "display_name": "Posts",
      "singular_name": "Post",
      "fields": [
        {{
          "name": "id",
          "type": "i64",
          "nullable": false,
          "editable": false
        }},
        {{
          "name": "published_at",
          "type": "DateTime",
          "nullable": true,
          "editable": true
        }},
        {{
          "name": "title",
          "type": "String",
          "nullable": false,
          "editable": true
        }}
      ],
      "relations": [],
      "core": false
    }},
    {{
      "name": "User",
      "table": "rustio_users",
      "admin_name": "users",
      "display_name": "Users",
      "singular_name": "User",
      "fields": [
        {{
          "name": "created_at",
          "type": "DateTime",
          "nullable": false,
          "editable": false
        }},
        {{
          "name": "email",
          "type": "String",
          "nullable": false,
          "editable": true
        }},
        {{
          "name": "id",
          "type": "i64",
          "nullable": false,
          "editable": false
        }},
        {{
          "name": "is_active",
          "type": "bool",
          "nullable": false,
          "editable": true
        }},
        {{
          "name": "password_hash",
          "type": "String",
          "nullable": false,
          "editable": false
        }},
        {{
          "name": "role",
          "type": "String",
          "nullable": false,
          "editable": true
        }}
      ],
      "relations": [],
      "core": true
    }}
  ]
}}
"#,
            rv = env!("CARGO_PKG_VERSION"),
        );

        assert_eq!(actual, expected);
    }

    #[test]
    fn validate_accepts_clean_schema() {
        let schema = Schema::from_admin(&Admin::new().model::<Post>().model::<Book>());
        assert_eq!(schema.validate(), Ok(()));
    }

    #[test]
    fn validate_rejects_version_mismatch() {
        let mut schema = Schema::from_admin(&Admin::new().model::<Post>());
        schema.version = 999;
        assert_eq!(
            schema.validate(),
            Err(SchemaError::VersionMismatch {
                found: 999,
                expected: SCHEMA_VERSION
            })
        );
    }

    #[test]
    fn validate_rejects_duplicate_models() {
        let mut schema = Schema::from_admin(&Admin::new().model::<Post>());
        let post = find(&schema, "Post").clone();
        schema.models.push(post);
        assert_eq!(
            schema.validate(),
            Err(SchemaError::DuplicateModel("Post".to_string()))
        );
    }

    #[test]
    fn validate_rejects_duplicate_fields() {
        let mut schema = Schema::from_admin(&Admin::new().model::<Post>());
        let post_idx = schema.models.iter().position(|m| m.name == "Post").unwrap();
        let dup = schema.models[post_idx].fields[0].clone();
        schema.models[post_idx].fields.push(dup);
        assert_eq!(
            schema.validate(),
            Err(SchemaError::DuplicateField {
                model: "Post".to_string(),
                field: "id".to_string(),
            })
        );
    }

    #[test]
    fn validate_rejects_unknown_type() {
        let mut schema = Schema::from_admin(&Admin::new().model::<Post>());
        let post_idx = schema.models.iter().position(|m| m.name == "Post").unwrap();
        schema.models[post_idx].fields[0].ty = "HyperFloat128".to_string();
        assert_eq!(
            schema.validate(),
            Err(SchemaError::InvalidType {
                model: "Post".to_string(),
                field: "id".to_string(),
                ty: "HyperFloat128".to_string(),
            })
        );
    }

    #[test]
    fn validate_rejects_dangling_relation() {
        let mut schema = Schema::from_admin(&Admin::new().model::<Post>());
        let post_idx = schema.models.iter().position(|m| m.name == "Post").unwrap();
        schema.models[post_idx].relations.push(SchemaRelation {
            kind: "belongs_to".to_string(),
            to: "Ghost".to_string(),
            via: "ghost_id".to_string(),
        });
        assert_eq!(
            schema.validate(),
            Err(SchemaError::UnknownRelationTarget {
                from: "Post".to_string(),
                to: "Ghost".to_string(),
            })
        );
    }

    #[test]
    fn validate_accepts_self_referencing_relation() {
        // A model may reference itself — common for tree-shaped data
        // (parent/child). Reject only *dangling* targets, not recursion.
        let mut schema = Schema::from_admin(&Admin::new().model::<Post>());
        let post_idx = schema.models.iter().position(|m| m.name == "Post").unwrap();
        schema.models[post_idx].relations.push(SchemaRelation {
            kind: "belongs_to".to_string(),
            to: "Post".to_string(),
            via: "parent_id".to_string(),
        });
        assert_eq!(schema.validate(), Ok(()));
    }

    #[test]
    fn parse_rejects_unknown_top_level_field() {
        let bad = r#"{
            "version": 1,
            "rustio_version": "0.4.0",
            "models": [],
            "something_extra": true
        }"#;
        let result = Schema::parse(bad);
        assert!(
            matches!(result, Err(SchemaError::Parse(_))),
            "unknown fields must be rejected, got: {:?}",
            result
        );
    }

    #[test]
    fn parse_rejects_missing_required_field() {
        // `rustio_version` is required; dropping it must fail.
        let bad = r#"{
            "version": 1,
            "models": []
        }"#;
        let result = Schema::parse(bad);
        assert!(
            matches!(result, Err(SchemaError::Parse(_))),
            "missing fields must be rejected"
        );
    }

    #[test]
    fn parse_rejects_version_mismatch() {
        let bad = r#"{
            "version": 999,
            "rustio_version": "0.4.0",
            "models": []
        }"#;
        let err = Schema::parse(bad).unwrap_err();
        assert!(matches!(err, SchemaError::VersionMismatch { .. }));
    }

    #[test]
    fn write_to_is_atomic_no_tmp_left_behind() {
        let tmp_dir = std::env::temp_dir().join(format!(
            "rustio-schema-write-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&tmp_dir).unwrap();
        let target = tmp_dir.join("rustio.schema.json");

        let schema = Schema::from_admin(&Admin::new().model::<Post>());
        schema.write_to(&target).unwrap();

        // File exists and parses back identically.
        let bytes = std::fs::read_to_string(&target).unwrap();
        let parsed = Schema::parse(&bytes).unwrap();
        assert_eq!(parsed, schema);

        // No leftover temp file — the `.json.tmp` sibling should not
        // exist after a successful rename.
        assert!(!tmp_dir.join("rustio.schema.tmp").exists());
        assert!(!tmp_dir.join("rustio.schema.json.tmp").exists());

        std::fs::remove_dir_all(&tmp_dir).ok();
    }
}
