//! The AI boundary: a fixed vocabulary of primitives that the Phase 2
//! intelligence layer is allowed to emit.
//!
//! Shipped in 0.4.0 as **definitions only**. There is no executor. The
//! runtime does not read or apply these. The module exists now so:
//!
//! 1. The wire format is stable by the time 0.5.0 starts writing it.
//! 2. External tooling can begin shaping intents against a real type.
//! 3. The code review trail for 0.4.0 includes the exact shape 0.5.0
//!    will constrain itself to.
//!
//! **Core rule enforced at the boundary (0.5.0):** if a change cannot be
//! expressed as one of these primitives, it is **rejected** — no
//! free-form code generation, no partial writes, no "close enough"
//! fallback. A project whose shape cannot be described in this vocabulary
//! is a project the AI layer will refuse to touch.

use serde::{Deserialize, Serialize};

/// The complete set of operations the AI layer is allowed to perform on
/// a RustIO project.
///
/// Marked `#[non_exhaustive]` so new primitives can land in a minor
/// release without breaking external matchers. Consumers must include a
/// wildcard arm and treat unknown variants as "refuse" rather than
/// guess.
#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Primitive {
    AddModel(AddModel),
    RemoveModel(RemoveModel),
    AddField(AddField),
    RemoveField(RemoveField),
    AddRelation(AddRelation),
    RemoveRelation(RemoveRelation),
    UpdateAdmin(UpdateAdmin),
    CreateMigration(CreateMigration),
}

/// A single field on an add_model / add_field primitive.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldSpec {
    pub name: String,
    /// Stable type name from `rustio.schema.json` (`i32`, `i64`,
    /// `String`, `bool`, `DateTime`). Any value not in that set must be
    /// rejected by the executor.
    #[serde(rename = "type")]
    pub ty: String,
    #[serde(default)]
    pub nullable: bool,
    #[serde(default = "default_editable")]
    pub editable: bool,
}

fn default_editable() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddModel {
    /// Struct name in Rust (PascalCase), e.g. `Post`.
    pub name: String,
    /// Table name in SQLite (snake_case, pluralised), e.g. `posts`.
    pub table: String,
    pub fields: Vec<FieldSpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoveModel {
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddField {
    pub model: String,
    #[serde(flatten)]
    pub field: FieldSpec,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoveField {
    pub model: String,
    pub field: String,
}

/// The kind of relation an `AddRelation` primitive describes.
///
/// 0.4.0 reserves the variants but the executor won't be wired up until
/// 0.5.0. `#[non_exhaustive]` so later releases can extend this set.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RelationKind {
    BelongsTo,
    HasMany,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddRelation {
    pub from: String,
    pub kind: RelationKind,
    pub to: String,
    /// Column or accessor name. For `belongs_to`, the FK column
    /// (e.g. `user_id`). For `has_many`, the reverse accessor name on
    /// the parent side (e.g. `posts`).
    pub via: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoveRelation {
    pub from: String,
    pub via: String,
}

/// Mutate one admin-facing attribute of a field without changing its
/// type — for example flipping `searchable` on or off.
///
/// The attribute vocabulary is intentionally narrow; fields outside it
/// must be rejected at the 0.5.0 executor rather than silently ignored.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateAdmin {
    pub model: String,
    pub field: String,
    pub attr: String,
    pub value: serde_json::Value,
}

/// Attach a raw SQL migration alongside a schema-level change.
///
/// The 0.5.0 executor will require every primitive that alters persisted
/// shape (add_model, add_field, add_relation) to be accompanied by a
/// `CreateMigration` whose SQL matches the change. Primitives that only
/// touch admin metadata do not need one.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateMigration {
    pub name: String,
    pub sql: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_field_round_trips_through_json() {
        let p = Primitive::AddField(AddField {
            model: "Post".to_string(),
            field: FieldSpec {
                name: "published".to_string(),
                ty: "bool".to_string(),
                nullable: false,
                editable: true,
            },
        });
        let json = serde_json::to_string(&p).unwrap();
        assert!(json.contains(r#""op":"add_field""#));
        assert!(json.contains(r#""name":"published""#));

        let back: Primitive = serde_json::from_str(&json).unwrap();
        match back {
            Primitive::AddField(af) => {
                assert_eq!(af.model, "Post");
                assert_eq!(af.field.name, "published");
                assert_eq!(af.field.ty, "bool");
            }
            _ => panic!("expected AddField"),
        }
    }

    #[test]
    fn unknown_op_is_rejected_not_swallowed() {
        // Guard for the "reject unknown primitive" rule at the boundary.
        // An executor must see an error here, not silently drop the op.
        let bad = r#"{"op": "rewrite_universe", "world": "goodbye"}"#;
        let parsed: Result<Primitive, _> = serde_json::from_str(bad);
        assert!(parsed.is_err(), "unknown op must not parse");
    }

    #[test]
    fn add_relation_with_belongs_to_serialises_snake_case() {
        let p = Primitive::AddRelation(AddRelation {
            from: "Post".to_string(),
            kind: RelationKind::BelongsTo,
            to: "User".to_string(),
            via: "user_id".to_string(),
        });
        let json = serde_json::to_string(&p).unwrap();
        assert!(json.contains(r#""kind":"belongs_to""#));
    }
}
