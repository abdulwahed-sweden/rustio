//! The AI boundary: a fixed vocabulary of primitives that the Phase 2
//! intelligence layer is allowed to emit.
//!
//! Shipped in 0.4.0 as **definitions + validation**. There is no runtime
//! executor — nothing in this module touches the filesystem or runs
//! migrations. What it does do:
//!
//! 1. Define the complete set of operations the AI layer can propose
//!    ([`Primitive`]).
//! 2. Enforce strict serde shape: unknown ops, unknown keys, and missing
//!    fields all fail to parse (`deny_unknown_fields` everywhere).
//! 3. Provide structural validation ([`validate_primitive`]) and
//!    plan-level simulation ([`Plan::validate`]) so a proposed change
//!    set is checked end-to-end before any hypothetical executor sees
//!    it.
//!
//! **Core rule enforced at the boundary (0.5.0):** if a change cannot be
//! expressed as one of these primitives, it is **rejected** — no
//! free-form code generation, no partial writes, no "close enough"
//! fallback. A project whose shape cannot be described in this vocabulary
//! is a project the AI layer will refuse to touch.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::schema::{
    Schema, SchemaField, SchemaModel, SchemaRelation, SCHEMA_VERSION, VALID_TYPE_NAMES,
};

pub mod executor;
pub mod planner;
pub mod review;

#[cfg(test)]
mod executor_tests;
#[cfg(test)]
mod executor_tests_advanced;
#[cfg(test)]
mod planner_tests;
#[cfg(test)]
mod review_tests;

pub use executor::{
    execute_plan_document, plan_execution, render_preview_human, ExecuteOptions, ExecutionError,
    ExecutionPreview, ExecutionResult, FileChangeKind, ParsedModelsFile, PlannedFileChange,
    ProjectView,
};
pub use planner::{generate_plan, ContextConfig, PlanError, PlanRequest, PlanResult};
pub use review::{
    build_plan_document, build_plan_document_with_timestamp, classify_risk, compute_impact,
    load_plan, render_plan_document_json, render_review_human, review_plan, warnings_for,
    LoadedPlan, PlanDocument, PlanImpact, PlanReview, ReviewError, RiskLevel, ValidationOutcome,
    PLAN_DOCUMENT_VERSION,
};

/// The complete set of operations the AI layer is allowed to perform on
/// a RustIO project.
///
/// Marked `#[non_exhaustive]` so new primitives can land in a minor
/// release without breaking external matchers. Consumers must include a
/// wildcard arm and treat unknown variants as "refuse" rather than
/// guess.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Primitive {
    AddModel(AddModel),
    RemoveModel(RemoveModel),
    RenameModel(RenameModel),
    AddField(AddField),
    RemoveField(RemoveField),
    RenameField(RenameField),
    ChangeFieldType(ChangeFieldType),
    ChangeFieldNullability(ChangeFieldNullability),
    AddRelation(AddRelation),
    RemoveRelation(RemoveRelation),
    UpdateAdmin(UpdateAdmin),
    /// Attach a raw SQL migration. **Developer-only** — this primitive
    /// bypasses the AI boundary's "no free-form code" rule and is
    /// rejected by [`Plan::validate`]. Project maintainers can still
    /// emit migrations through this type directly; the AI executor
    /// must not.
    CreateMigration(CreateMigration),
}

impl Primitive {
    /// `true` if this primitive is permitted only from developer /
    /// tooling code, not from any AI-emitted [`Plan`].
    ///
    /// Today, only [`Primitive::CreateMigration`] qualifies: it
    /// accepts arbitrary SQL, which violates the AI boundary rule
    /// that every change must be expressible as a structured
    /// primitive. [`Plan::validate`] rejects any step for which this
    /// returns `true`.
    ///
    /// Kept as a method (not a `const`) so future variants can opt
    /// in explicitly.
    pub fn is_developer_only(&self) -> bool {
        matches!(self, Primitive::CreateMigration(_))
    }

    /// Stable short name of this variant, suitable for error
    /// messages. Matches the serde tag so callers can cross-reference
    /// the wire format.
    pub fn op_name(&self) -> &'static str {
        match self {
            Primitive::AddModel(_) => "add_model",
            Primitive::RemoveModel(_) => "remove_model",
            Primitive::RenameModel(_) => "rename_model",
            Primitive::AddField(_) => "add_field",
            Primitive::RemoveField(_) => "remove_field",
            Primitive::RenameField(_) => "rename_field",
            Primitive::ChangeFieldType(_) => "change_field_type",
            Primitive::ChangeFieldNullability(_) => "change_field_nullability",
            Primitive::AddRelation(_) => "add_relation",
            Primitive::RemoveRelation(_) => "remove_relation",
            Primitive::UpdateAdmin(_) => "update_admin",
            Primitive::CreateMigration(_) => "create_migration",
        }
    }
}

/// A single field on an `add_model` / `add_field` primitive.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AddModel {
    /// Struct name in Rust (PascalCase), e.g. `Post`.
    pub name: String,
    /// Table name in SQLite (snake_case, pluralised), e.g. `posts`.
    pub table: String,
    pub fields: Vec<FieldSpec>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RemoveModel {
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AddField {
    pub model: String,
    #[serde(flatten)]
    pub field: FieldSpec,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RemoveField {
    pub model: String,
    pub field: String,
}

/// Rename a model (schema-level). Data-preserving: the AI executor
/// must translate this into a table rename, not a drop+recreate.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RenameModel {
    pub from: String,
    pub to: String,
}

/// Rename a single field of a model (schema-level). Data-preserving.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RenameField {
    pub model: String,
    pub from: String,
    pub to: String,
}

/// Change a field's Rust type. The executor is responsible for
/// translating the change into a migration (and refusing lossy
/// conversions); this primitive only records the intent at the
/// schema layer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChangeFieldType {
    pub model: String,
    pub field: String,
    /// Target type name from [`VALID_TYPE_NAMES`]. Anything else is
    /// rejected by [`validate_primitive`].
    pub new_type: String,
}

/// Flip a field's nullability (`Option<T>` ↔ `T`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChangeFieldNullability {
    pub model: String,
    pub field: String,
    pub nullable: bool,
}

/// The kind of relation an `AddRelation` primitive describes.
///
/// 0.4.0 reserves the variants but the executor won't be wired up until
/// 0.5.0. `#[non_exhaustive]` so later releases can extend this set.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RelationKind {
    BelongsTo,
    HasMany,
}

impl RelationKind {
    fn as_str(self) -> &'static str {
        match self {
            RelationKind::BelongsTo => "belongs_to",
            RelationKind::HasMany => "has_many",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AddRelation {
    pub from: String,
    pub kind: RelationKind,
    pub to: String,
    /// Column or accessor name. For `belongs_to`, the FK column
    /// (e.g. `user_id`). For `has_many`, the reverse accessor name on
    /// the parent side (e.g. `posts`).
    pub via: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RemoveRelation {
    pub from: String,
    pub via: String,
}

/// Mutate one admin-facing attribute of a field without changing its
/// type — for example flipping `searchable` on or off.
///
/// The attribute vocabulary is intentionally narrow; fields outside it
/// must be rejected at the 0.5.0 executor rather than silently ignored.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateAdmin {
    pub model: String,
    pub field: String,
    pub attr: String,
    pub value: serde_json::Value,
}

/// Attach a raw SQL migration alongside a schema-level change.
///
/// The 0.5.0 executor will require every primitive that alters persisted
/// shape (`add_model`, `add_field`, `add_relation`) to be accompanied by
/// a `CreateMigration` whose SQL matches the change. Primitives that
/// only touch admin metadata do not need one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateMigration {
    pub name: String,
    pub sql: String,
}

/// Reasons a primitive (or a plan composed of primitives) can be
/// rejected. The AI boundary converts these into a blunt refusal — the
/// executor never silently "fixes" a primitive or applies a partial plan.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub enum PrimitiveError {
    /// A required identifier is empty (`name`, `model`, `field`, …).
    EmptyIdentifier(&'static str),
    /// A field's declared type isn't in [`VALID_TYPE_NAMES`].
    UnknownType {
        model: String,
        field: String,
        ty: String,
    },
    /// Two fields with the same name inside an `add_model` payload.
    DuplicateFieldInAddModel { model: String, field: String },
    /// Target of an `add_*` already exists in the schema.
    AlreadyExists { what: &'static str, name: String },
    /// Target of a `remove_*` / `update_admin` doesn't exist.
    NotFound { what: &'static str, name: String },
    /// Relation target model doesn't exist in the (shadow-applied) schema.
    UnknownRelationTarget { from: String, to: String },
    /// `UpdateAdmin` referenced an attribute outside the accepted vocabulary.
    UnknownAdminAttribute { attr: String },
    /// A rename primitive was given identical `from` and `to`.
    /// Rejecting no-ops early keeps plans honest and diff-reviewable.
    NoOpRename { what: &'static str, name: String },
    /// A developer-only primitive appeared inside a [`Plan`]. Plans
    /// represent the AI boundary; anything with
    /// [`Primitive::is_developer_only`] set must be rejected before
    /// an executor touches it.
    DeveloperOnlyNotAllowedInPlan { op: &'static str },
    /// `validate_plan` annotates inner errors with the step index so a
    /// caller can point the user at "step 3 failed because …".
    InStep {
        step: usize,
        inner: Box<PrimitiveError>,
    },
}

impl std::fmt::Display for PrimitiveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyIdentifier(which) => write!(f, "empty {which}"),
            Self::UnknownType { model, field, ty } => write!(
                f,
                "field `{model}.{field}` has unknown type `{ty}` (valid: {valid})",
                valid = VALID_TYPE_NAMES.join(", "),
            ),
            Self::DuplicateFieldInAddModel { model, field } => write!(
                f,
                "add_model `{model}` lists field `{field}` more than once",
            ),
            Self::AlreadyExists { what, name } => write!(f, "{what} `{name}` already exists"),
            Self::NotFound { what, name } => write!(f, "{what} `{name}` does not exist"),
            Self::UnknownRelationTarget { from, to } => {
                write!(f, "relation from `{from}` targets unknown model `{to}`")
            }
            Self::UnknownAdminAttribute { attr } => {
                write!(f, "unknown admin attribute `{attr}`")
            }
            Self::NoOpRename { what, name } => {
                write!(f, "rename of {what} `{name}` is a no-op (from == to)")
            }
            Self::DeveloperOnlyNotAllowedInPlan { op } => write!(
                f,
                "`{op}` is developer-only and cannot appear in an AI plan"
            ),
            Self::InStep { step, inner } => write!(f, "step {step}: {inner}"),
        }
    }
}

impl std::error::Error for PrimitiveError {}

/// Admin attributes that `UpdateAdmin` is allowed to touch in 0.4.0.
/// Anything outside this set is rejected; extending requires a CHANGELOG
/// entry and a matching executor.
const ALLOWED_ADMIN_ATTRS: &[&str] = &["searchable", "editable", "nullable"];

/// Structural check: validates one primitive in isolation, without
/// comparing against a surrounding schema. Catches empty names, bad
/// types, and internally inconsistent payloads.
pub fn validate_primitive(p: &Primitive) -> Result<(), PrimitiveError> {
    match p {
        Primitive::AddModel(m) => {
            require_nonempty(&m.name, "model name")?;
            require_nonempty(&m.table, "table name")?;
            let mut seen: BTreeSet<&str> = BTreeSet::new();
            for field in &m.fields {
                validate_field_spec(&m.name, field)?;
                if !seen.insert(field.name.as_str()) {
                    return Err(PrimitiveError::DuplicateFieldInAddModel {
                        model: m.name.clone(),
                        field: field.name.clone(),
                    });
                }
            }
            Ok(())
        }
        Primitive::RemoveModel(m) => {
            require_nonempty(&m.name, "model name")?;
            Ok(())
        }
        Primitive::AddField(af) => {
            require_nonempty(&af.model, "model name")?;
            validate_field_spec(&af.model, &af.field)
        }
        Primitive::RemoveField(rf) => {
            require_nonempty(&rf.model, "model name")?;
            require_nonempty(&rf.field, "field name")?;
            Ok(())
        }
        Primitive::RenameModel(rm) => {
            require_nonempty(&rm.from, "from")?;
            require_nonempty(&rm.to, "to")?;
            if rm.from == rm.to {
                return Err(PrimitiveError::NoOpRename {
                    what: "model",
                    name: rm.from.clone(),
                });
            }
            Ok(())
        }
        Primitive::RenameField(rf) => {
            require_nonempty(&rf.model, "model name")?;
            require_nonempty(&rf.from, "from")?;
            require_nonempty(&rf.to, "to")?;
            if rf.from == rf.to {
                return Err(PrimitiveError::NoOpRename {
                    what: "field",
                    name: format!("{}.{}", rf.model, rf.from),
                });
            }
            Ok(())
        }
        Primitive::ChangeFieldType(c) => {
            require_nonempty(&c.model, "model name")?;
            require_nonempty(&c.field, "field name")?;
            if !VALID_TYPE_NAMES.contains(&c.new_type.as_str()) {
                return Err(PrimitiveError::UnknownType {
                    model: c.model.clone(),
                    field: c.field.clone(),
                    ty: c.new_type.clone(),
                });
            }
            Ok(())
        }
        Primitive::ChangeFieldNullability(c) => {
            require_nonempty(&c.model, "model name")?;
            require_nonempty(&c.field, "field name")?;
            Ok(())
        }
        Primitive::AddRelation(r) => {
            require_nonempty(&r.from, "from")?;
            require_nonempty(&r.to, "to")?;
            require_nonempty(&r.via, "via")?;
            // RelationKind is a typed enum; no further check needed.
            Ok(())
        }
        Primitive::RemoveRelation(r) => {
            require_nonempty(&r.from, "from")?;
            require_nonempty(&r.via, "via")?;
            Ok(())
        }
        Primitive::UpdateAdmin(u) => {
            require_nonempty(&u.model, "model name")?;
            require_nonempty(&u.field, "field name")?;
            require_nonempty(&u.attr, "attr")?;
            if !ALLOWED_ADMIN_ATTRS.contains(&u.attr.as_str()) {
                return Err(PrimitiveError::UnknownAdminAttribute {
                    attr: u.attr.clone(),
                });
            }
            Ok(())
        }
        Primitive::CreateMigration(m) => {
            require_nonempty(&m.name, "migration name")?;
            require_nonempty(&m.sql, "migration sql")?;
            Ok(())
        }
    }
}

fn require_nonempty(s: &str, which: &'static str) -> Result<(), PrimitiveError> {
    if s.trim().is_empty() {
        Err(PrimitiveError::EmptyIdentifier(which))
    } else {
        Ok(())
    }
}

fn validate_field_spec(model: &str, f: &FieldSpec) -> Result<(), PrimitiveError> {
    require_nonempty(&f.name, "field name")?;
    if !VALID_TYPE_NAMES.contains(&f.ty.as_str()) {
        return Err(PrimitiveError::UnknownType {
            model: model.to_string(),
            field: f.name.clone(),
            ty: f.ty.clone(),
        });
    }
    Ok(())
}

/// A proposed set of primitives to apply in order.
///
/// The plan is the *unit of validation* for the AI boundary. Individual
/// primitives can look sensible in isolation but fail as a sequence
/// (e.g. `add_field` twice, or `remove_model` followed by `add_field`
/// against the now-gone model). [`Plan::validate`] simulates the full
/// sequence against a shadow copy of the target schema and fails fast.
///
/// The struct is intentionally tiny. 0.4.0 does not execute plans; it
/// just defines the contract every 0.5.0 executor is built to.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    pub steps: Vec<Primitive>,
}

impl Plan {
    pub fn new(steps: Vec<Primitive>) -> Self {
        Self { steps }
    }

    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }

    pub fn len(&self) -> usize {
        self.steps.len()
    }

    /// Validate the entire plan against an initial schema state. Every
    /// step is first checked structurally, then checked against the
    /// shadow-applied schema, then applied to the shadow before the
    /// next step is considered.
    ///
    /// The shadow is pure in-memory data — no filesystem, no DB. This
    /// stays consistent with the 0.4.0 boundary rule: **no execution**.
    ///
    /// Additionally, any step whose primitive returns `true` from
    /// [`Primitive::is_developer_only`] is rejected up front. Plans
    /// represent the AI boundary; developer-only primitives
    /// (currently `CreateMigration`) are reserved for direct tooling
    /// use and must never appear in an AI-emitted plan.
    pub fn validate(&self, initial: &Schema) -> Result<(), PrimitiveError> {
        let mut state = initial.clone();
        for (idx, step) in self.steps.iter().enumerate() {
            if step.is_developer_only() {
                return Err(PrimitiveError::InStep {
                    step: idx,
                    inner: Box::new(PrimitiveError::DeveloperOnlyNotAllowedInPlan {
                        op: step.op_name(),
                    }),
                });
            }
            if let Err(inner) = validate_primitive(step) {
                return Err(PrimitiveError::InStep {
                    step: idx,
                    inner: Box::new(inner),
                });
            }
            if let Err(inner) = validate_against(step, &state) {
                return Err(PrimitiveError::InStep {
                    step: idx,
                    inner: Box::new(inner),
                });
            }
            apply_shadow(step, &mut state);
        }
        Ok(())
    }
}

/// Semantic check: a primitive is valid *against a given schema*.
/// Used both standalone and as the per-step check inside
/// [`Plan::validate`].
pub fn validate_against(p: &Primitive, schema: &Schema) -> Result<(), PrimitiveError> {
    match p {
        Primitive::AddModel(m) => {
            if schema.models.iter().any(|x| x.name == m.name) {
                return Err(PrimitiveError::AlreadyExists {
                    what: "model",
                    name: m.name.clone(),
                });
            }
            Ok(())
        }
        Primitive::RemoveModel(m) => {
            if !schema.models.iter().any(|x| x.name == m.name) {
                return Err(PrimitiveError::NotFound {
                    what: "model",
                    name: m.name.clone(),
                });
            }
            Ok(())
        }
        Primitive::AddField(af) => {
            let model = find_model(schema, &af.model)?;
            if model.fields.iter().any(|f| f.name == af.field.name) {
                return Err(PrimitiveError::AlreadyExists {
                    what: "field",
                    name: format!("{}.{}", af.model, af.field.name),
                });
            }
            Ok(())
        }
        Primitive::RemoveField(rf) => {
            let model = find_model(schema, &rf.model)?;
            if !model.fields.iter().any(|f| f.name == rf.field) {
                return Err(PrimitiveError::NotFound {
                    what: "field",
                    name: format!("{}.{}", rf.model, rf.field),
                });
            }
            Ok(())
        }
        Primitive::RenameModel(rm) => {
            let _ = find_model(schema, &rm.from)?;
            if schema.models.iter().any(|m| m.name == rm.to) {
                return Err(PrimitiveError::AlreadyExists {
                    what: "model",
                    name: rm.to.clone(),
                });
            }
            Ok(())
        }
        Primitive::RenameField(rf) => {
            let model = find_model(schema, &rf.model)?;
            if !model.fields.iter().any(|f| f.name == rf.from) {
                return Err(PrimitiveError::NotFound {
                    what: "field",
                    name: format!("{}.{}", rf.model, rf.from),
                });
            }
            if model.fields.iter().any(|f| f.name == rf.to) {
                return Err(PrimitiveError::AlreadyExists {
                    what: "field",
                    name: format!("{}.{}", rf.model, rf.to),
                });
            }
            Ok(())
        }
        Primitive::ChangeFieldType(c) => {
            let model = find_model(schema, &c.model)?;
            if !model.fields.iter().any(|f| f.name == c.field) {
                return Err(PrimitiveError::NotFound {
                    what: "field",
                    name: format!("{}.{}", c.model, c.field),
                });
            }
            Ok(())
        }
        Primitive::ChangeFieldNullability(c) => {
            let model = find_model(schema, &c.model)?;
            if !model.fields.iter().any(|f| f.name == c.field) {
                return Err(PrimitiveError::NotFound {
                    what: "field",
                    name: format!("{}.{}", c.model, c.field),
                });
            }
            Ok(())
        }
        Primitive::AddRelation(r) => {
            let from = find_model(schema, &r.from)?;
            if !schema.models.iter().any(|m| m.name == r.to) {
                return Err(PrimitiveError::UnknownRelationTarget {
                    from: r.from.clone(),
                    to: r.to.clone(),
                });
            }
            if from.relations.iter().any(|rel| rel.via == r.via) {
                return Err(PrimitiveError::AlreadyExists {
                    what: "relation",
                    name: format!("{}.{}", r.from, r.via),
                });
            }
            Ok(())
        }
        Primitive::RemoveRelation(r) => {
            let from = find_model(schema, &r.from)?;
            if !from.relations.iter().any(|rel| rel.via == r.via) {
                return Err(PrimitiveError::NotFound {
                    what: "relation",
                    name: format!("{}.{}", r.from, r.via),
                });
            }
            Ok(())
        }
        Primitive::UpdateAdmin(u) => {
            let model = find_model(schema, &u.model)?;
            if !model.fields.iter().any(|f| f.name == u.field) {
                return Err(PrimitiveError::NotFound {
                    what: "field",
                    name: format!("{}.{}", u.model, u.field),
                });
            }
            Ok(())
        }
        // A raw migration doesn't need a schema target; the structural
        // check already ensures name + sql are non-empty.
        Primitive::CreateMigration(_) => Ok(()),
    }
}

fn find_model<'a>(schema: &'a Schema, name: &str) -> Result<&'a SchemaModel, PrimitiveError> {
    schema
        .models
        .iter()
        .find(|m| m.name == name)
        .ok_or_else(|| PrimitiveError::NotFound {
            what: "model",
            name: name.to_string(),
        })
}

/// Apply a primitive to an in-memory schema *copy*. Used for plan
/// simulation only — never touches the filesystem or DB, by design.
///
/// Callers must invoke [`validate_against`] first; `apply_shadow` assumes
/// the step is legal and panics on contradiction rather than silently
/// diverging.
fn apply_shadow(p: &Primitive, schema: &mut Schema) {
    match p {
        Primitive::AddModel(m) => {
            let mut fields: Vec<SchemaField> = m
                .fields
                .iter()
                .map(|f| SchemaField {
                    name: f.name.clone(),
                    ty: f.ty.clone(),
                    nullable: f.nullable,
                    editable: f.editable,
                })
                .collect();
            fields.sort_by(|a, b| a.name.cmp(&b.name));
            schema.models.push(SchemaModel {
                name: m.name.clone(),
                table: m.table.clone(),
                admin_name: m.table.clone(),
                display_name: m.name.clone(),
                singular_name: m.name.clone(),
                fields,
                relations: Vec::new(),
                // New models added via AI primitives are never core —
                // core-ness is a property of built-in infrastructure
                // (the `User` entry seeded by `Admin::new()`), not
                // something the AI layer can mint.
                core: false,
            });
            schema.models.sort_by(|a, b| a.name.cmp(&b.name));
        }
        Primitive::RemoveModel(m) => {
            schema.models.retain(|x| x.name != m.name);
        }
        Primitive::AddField(af) => {
            if let Some(model) = schema.models.iter_mut().find(|m| m.name == af.model) {
                model.fields.push(SchemaField {
                    name: af.field.name.clone(),
                    ty: af.field.ty.clone(),
                    nullable: af.field.nullable,
                    editable: af.field.editable,
                });
                model.fields.sort_by(|a, b| a.name.cmp(&b.name));
            }
        }
        Primitive::RemoveField(rf) => {
            if let Some(model) = schema.models.iter_mut().find(|m| m.name == rf.model) {
                model.fields.retain(|f| f.name != rf.field);
            }
        }
        Primitive::RenameModel(rm) => {
            if let Some(model) = schema.models.iter_mut().find(|m| m.name == rm.from) {
                model.name = rm.to.clone();
                model.singular_name = rm.to.clone();
            }
            schema.models.sort_by(|a, b| a.name.cmp(&b.name));
        }
        Primitive::RenameField(rf) => {
            if let Some(model) = schema.models.iter_mut().find(|m| m.name == rf.model) {
                if let Some(field) = model.fields.iter_mut().find(|f| f.name == rf.from) {
                    field.name = rf.to.clone();
                }
                model.fields.sort_by(|a, b| a.name.cmp(&b.name));
            }
        }
        Primitive::ChangeFieldType(c) => {
            if let Some(model) = schema.models.iter_mut().find(|m| m.name == c.model) {
                if let Some(field) = model.fields.iter_mut().find(|f| f.name == c.field) {
                    field.ty = c.new_type.clone();
                }
            }
        }
        Primitive::ChangeFieldNullability(c) => {
            if let Some(model) = schema.models.iter_mut().find(|m| m.name == c.model) {
                if let Some(field) = model.fields.iter_mut().find(|f| f.name == c.field) {
                    field.nullable = c.nullable;
                }
            }
        }
        Primitive::AddRelation(r) => {
            if let Some(model) = schema.models.iter_mut().find(|m| m.name == r.from) {
                model.relations.push(SchemaRelation {
                    kind: r.kind.as_str().to_string(),
                    to: r.to.clone(),
                    via: r.via.clone(),
                });
            }
        }
        Primitive::RemoveRelation(r) => {
            if let Some(model) = schema.models.iter_mut().find(|m| m.name == r.from) {
                model.relations.retain(|rel| rel.via != r.via);
            }
        }
        // UpdateAdmin and CreateMigration don't alter the structural
        // shape reflected in `rustio.schema.json`; the executor will
        // rewrite files, not mutate the schema snapshot.
        Primitive::UpdateAdmin(_) | Primitive::CreateMigration(_) => {}
    }
}

/// Sanity hook for callers that want to assert they're looking at a
/// schema the current rustio-core understands before doing anything
/// else. Exported for executor code; this module uses it in tests.
pub fn assert_schema_version_supported(schema: &Schema) -> Result<(), PrimitiveError> {
    if schema.version != SCHEMA_VERSION {
        return Err(PrimitiveError::NotFound {
            what: "schema version",
            name: schema.version.to_string(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::admin::{Admin, AdminField, AdminModel, FieldType, FormData};
    use crate::error::Error as CoreError;
    use crate::orm::{Model, Row, Value};

    // Reuse a simple Post model for the "schema exists" fixtures.
    struct Post;
    impl Model for Post {
        const TABLE: &'static str = "posts";
        const COLUMNS: &'static [&'static str] = &["id", "title"];
        const INSERT_COLUMNS: &'static [&'static str] = &["title"];
        fn id(&self) -> i64 {
            0
        }
        fn from_row(_: Row<'_>) -> Result<Self, CoreError> {
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
        ];
        fn singular_name() -> &'static str {
            "Post"
        }
        fn field_display(&self, _: &str) -> Option<String> {
            None
        }
        fn from_form(_: &FormData, _: Option<i64>) -> Result<Self, CoreError> {
            unimplemented!()
        }
    }

    fn schema() -> Schema {
        Schema::from_admin(&Admin::new().model::<Post>())
    }

    // ---- structural validation --------------------------------------------

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
        let bad = r#"{"op":"rewrite_universe","world":"goodbye"}"#;
        let parsed: Result<Primitive, _> = serde_json::from_str(bad);
        assert!(parsed.is_err(), "unknown op must not parse");
    }

    #[test]
    fn unknown_field_on_known_op_is_rejected() {
        // `add_model` payload with a typo'd field must fail rather than
        // being silently dropped.
        let bad = r#"{"op":"add_model","name":"X","table":"xs","fields":[],"extra":true}"#;
        let parsed: Result<Primitive, _> = serde_json::from_str(bad);
        assert!(
            parsed.is_err(),
            "unknown keys on known ops must be rejected"
        );
    }

    #[test]
    fn missing_required_field_is_rejected() {
        // `add_model` requires `table`; dropping it must fail.
        let bad = r#"{"op":"add_model","name":"X","fields":[]}"#;
        let parsed: Result<Primitive, _> = serde_json::from_str(bad);
        assert!(parsed.is_err(), "missing required fields must be rejected");
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

    #[test]
    fn validate_primitive_rejects_unknown_type() {
        let p = Primitive::AddField(AddField {
            model: "Post".to_string(),
            field: FieldSpec {
                name: "flux".to_string(),
                ty: "HyperFloat128".to_string(),
                nullable: false,
                editable: true,
            },
        });
        assert!(matches!(
            validate_primitive(&p),
            Err(PrimitiveError::UnknownType { .. })
        ));
    }

    #[test]
    fn validate_primitive_rejects_empty_names() {
        let p = Primitive::AddField(AddField {
            model: "".to_string(),
            field: FieldSpec {
                name: "x".to_string(),
                ty: "i64".to_string(),
                nullable: false,
                editable: true,
            },
        });
        assert_eq!(
            validate_primitive(&p),
            Err(PrimitiveError::EmptyIdentifier("model name"))
        );
    }

    #[test]
    fn validate_primitive_rejects_duplicate_fields_in_add_model() {
        let p = Primitive::AddModel(AddModel {
            name: "Book".to_string(),
            table: "books".to_string(),
            fields: vec![
                FieldSpec {
                    name: "title".to_string(),
                    ty: "String".to_string(),
                    nullable: false,
                    editable: true,
                },
                FieldSpec {
                    name: "title".to_string(),
                    ty: "String".to_string(),
                    nullable: false,
                    editable: true,
                },
            ],
        });
        assert!(matches!(
            validate_primitive(&p),
            Err(PrimitiveError::DuplicateFieldInAddModel { .. })
        ));
    }

    #[test]
    fn update_admin_rejects_unknown_attribute() {
        let p = Primitive::UpdateAdmin(UpdateAdmin {
            model: "Post".to_string(),
            field: "title".to_string(),
            attr: "telepathy".to_string(),
            value: serde_json::Value::Bool(true),
        });
        assert!(matches!(
            validate_primitive(&p),
            Err(PrimitiveError::UnknownAdminAttribute { .. })
        ));
    }

    // ---- semantic validation ----------------------------------------------

    #[test]
    fn validate_against_rejects_remove_of_nonexistent_model() {
        let p = Primitive::RemoveModel(RemoveModel {
            name: "Ghost".to_string(),
        });
        let err = validate_against(&p, &schema()).unwrap_err();
        assert!(matches!(
            err,
            PrimitiveError::NotFound { what: "model", .. }
        ));
    }

    #[test]
    fn validate_against_rejects_add_field_to_missing_model() {
        let p = Primitive::AddField(AddField {
            model: "Ghost".to_string(),
            field: FieldSpec {
                name: "age".to_string(),
                ty: "i32".to_string(),
                nullable: false,
                editable: true,
            },
        });
        let err = validate_against(&p, &schema()).unwrap_err();
        assert!(matches!(
            err,
            PrimitiveError::NotFound { what: "model", .. }
        ));
    }

    #[test]
    fn validate_against_rejects_duplicate_field_add() {
        let p = Primitive::AddField(AddField {
            model: "Post".to_string(),
            field: FieldSpec {
                name: "title".to_string(),
                ty: "String".to_string(),
                nullable: false,
                editable: true,
            },
        });
        let err = validate_against(&p, &schema()).unwrap_err();
        assert!(matches!(
            err,
            PrimitiveError::AlreadyExists { what: "field", .. }
        ));
    }

    #[test]
    fn validate_against_rejects_relation_to_missing_model() {
        let p = Primitive::AddRelation(AddRelation {
            from: "Post".to_string(),
            kind: RelationKind::BelongsTo,
            to: "Ghost".to_string(),
            via: "ghost_id".to_string(),
        });
        let err = validate_against(&p, &schema()).unwrap_err();
        assert!(matches!(err, PrimitiveError::UnknownRelationTarget { .. }));
    }

    // ---- plan-level simulation --------------------------------------------

    #[test]
    fn plan_validates_sequential_additions() {
        let plan = Plan::new(vec![
            Primitive::AddModel(AddModel {
                name: "Book".to_string(),
                table: "books".to_string(),
                fields: vec![FieldSpec {
                    name: "title".to_string(),
                    ty: "String".to_string(),
                    nullable: false,
                    editable: true,
                }],
            }),
            // Plan-aware: this add_field is against the model the
            // previous step *just added* — the simulator must see it.
            Primitive::AddField(AddField {
                model: "Book".to_string(),
                field: FieldSpec {
                    name: "published".to_string(),
                    ty: "bool".to_string(),
                    nullable: false,
                    editable: true,
                },
            }),
        ]);
        assert_eq!(plan.validate(&schema()), Ok(()));
    }

    #[test]
    fn plan_rejects_second_add_of_same_model() {
        let add_book = Primitive::AddModel(AddModel {
            name: "Book".to_string(),
            table: "books".to_string(),
            fields: Vec::new(),
        });
        let plan = Plan::new(vec![add_book.clone(), add_book]);
        let err = plan.validate(&schema()).unwrap_err();
        assert!(
            matches!(
                &err,
                PrimitiveError::InStep { step: 1, inner } if matches!(**inner, PrimitiveError::AlreadyExists { what: "model", .. })
            ),
            "got: {err:?}"
        );
    }

    #[test]
    fn plan_rejects_field_add_after_model_removed() {
        let plan = Plan::new(vec![
            Primitive::RemoveModel(RemoveModel {
                name: "Post".to_string(),
            }),
            Primitive::AddField(AddField {
                model: "Post".to_string(),
                field: FieldSpec {
                    name: "subtitle".to_string(),
                    ty: "String".to_string(),
                    nullable: true,
                    editable: true,
                },
            }),
        ]);
        let err = plan.validate(&schema()).unwrap_err();
        assert!(
            matches!(
                err,
                PrimitiveError::InStep { step: 1, inner } if matches!(*inner, PrimitiveError::NotFound { what: "model", .. })
            ),
            "plan must fail on the second step, not the first"
        );
    }

    #[test]
    fn empty_plan_is_always_valid() {
        assert_eq!(Plan::new(Vec::new()).validate(&schema()), Ok(()));
    }

    #[test]
    fn create_migration_is_developer_only() {
        let m = Primitive::CreateMigration(CreateMigration {
            name: "add_books".to_string(),
            sql: "CREATE TABLE books (id INTEGER);".to_string(),
        });
        assert!(m.is_developer_only());
        assert!(!Primitive::RemoveModel(RemoveModel {
            name: "X".to_string()
        })
        .is_developer_only());
    }

    #[test]
    fn validate_primitive_still_accepts_create_migration_for_direct_use() {
        // The developer-only gate lives on `Plan::validate`, not on
        // `validate_primitive`. Tooling code calling the latter
        // directly must still accept CreateMigration.
        let m = Primitive::CreateMigration(CreateMigration {
            name: "add_books".to_string(),
            sql: "CREATE TABLE books (id INTEGER);".to_string(),
        });
        assert_eq!(validate_primitive(&m), Ok(()));
    }

    #[test]
    fn plan_rejects_create_migration_even_when_structurally_valid() {
        let plan = Plan::new(vec![Primitive::CreateMigration(CreateMigration {
            name: "add_books".to_string(),
            sql: "CREATE TABLE books (id INTEGER);".to_string(),
        })]);
        let err = plan.validate(&schema()).unwrap_err();
        assert!(
            matches!(
                &err,
                PrimitiveError::InStep { step: 0, inner }
                    if matches!(
                        **inner,
                        PrimitiveError::DeveloperOnlyNotAllowedInPlan { op: "create_migration" },
                    )
            ),
            "got: {err:?}"
        );
    }

    #[test]
    fn plan_rejects_create_migration_at_the_offending_step() {
        let plan = Plan::new(vec![
            Primitive::RemoveModel(RemoveModel {
                name: "Post".to_string(),
            }),
            Primitive::CreateMigration(CreateMigration {
                name: "tidy".to_string(),
                sql: "DROP TABLE posts;".to_string(),
            }),
        ]);
        let err = plan.validate(&schema()).unwrap_err();
        assert!(
            matches!(
                err,
                PrimitiveError::InStep { step: 1, inner }
                    if matches!(*inner, PrimitiveError::DeveloperOnlyNotAllowedInPlan { .. })
            ),
            "developer-only check must locate the offending step index"
        );
    }

    // --- RenameModel / RenameField / ChangeFieldType / ChangeFieldNullability

    fn rename_model(from: &str, to: &str) -> Primitive {
        Primitive::RenameModel(RenameModel {
            from: from.to_string(),
            to: to.to_string(),
        })
    }

    fn rename_field(model: &str, from: &str, to: &str) -> Primitive {
        Primitive::RenameField(RenameField {
            model: model.to_string(),
            from: from.to_string(),
            to: to.to_string(),
        })
    }

    fn change_type(model: &str, field: &str, new_type: &str) -> Primitive {
        Primitive::ChangeFieldType(ChangeFieldType {
            model: model.to_string(),
            field: field.to_string(),
            new_type: new_type.to_string(),
        })
    }

    fn change_nullable(model: &str, field: &str, nullable: bool) -> Primitive {
        Primitive::ChangeFieldNullability(ChangeFieldNullability {
            model: model.to_string(),
            field: field.to_string(),
            nullable,
        })
    }

    #[test]
    fn rename_primitives_round_trip_through_json() {
        for p in [
            rename_model("Post", "Article"),
            rename_field("Post", "title", "heading"),
            change_type("Post", "priority", "i64"),
            change_nullable("Post", "title", true),
        ] {
            let json = serde_json::to_string(&p).unwrap();
            let back: Primitive = serde_json::from_str(&json).unwrap();
            assert_eq!(back.op_name(), p.op_name());
        }
    }

    #[test]
    fn rename_model_rejects_noop() {
        let p = rename_model("Post", "Post");
        assert!(matches!(
            validate_primitive(&p),
            Err(PrimitiveError::NoOpRename { what: "model", .. })
        ));
    }

    #[test]
    fn rename_field_rejects_noop() {
        let p = rename_field("Post", "title", "title");
        assert!(matches!(
            validate_primitive(&p),
            Err(PrimitiveError::NoOpRename { what: "field", .. })
        ));
    }

    #[test]
    fn rename_model_rejects_empty_names() {
        let p = rename_model("", "X");
        assert!(matches!(
            validate_primitive(&p),
            Err(PrimitiveError::EmptyIdentifier(_))
        ));
    }

    #[test]
    fn change_field_type_rejects_unknown_type() {
        let p = change_type("Post", "priority", "HyperFloat128");
        assert!(matches!(
            validate_primitive(&p),
            Err(PrimitiveError::UnknownType { .. })
        ));
    }

    #[test]
    fn validate_against_rejects_rename_of_missing_model() {
        let err = validate_against(&rename_model("Ghost", "Wraith"), &schema()).unwrap_err();
        assert!(matches!(
            err,
            PrimitiveError::NotFound { what: "model", .. }
        ));
    }

    #[test]
    fn validate_against_rejects_rename_to_existing_model() {
        // schema() has one model "Post". Renaming something TO "Post"
        // must collide (here we have to synthesize a second model so
        // there's something to rename; use an AddModel step first in
        // a plan).
        let plan = Plan::new(vec![
            Primitive::AddModel(AddModel {
                name: "Draft".to_string(),
                table: "drafts".to_string(),
                fields: Vec::new(),
            }),
            rename_model("Draft", "Post"),
        ]);
        let err = plan.validate(&schema()).unwrap_err();
        assert!(
            matches!(
                err,
                PrimitiveError::InStep { step: 1, inner }
                    if matches!(*inner, PrimitiveError::AlreadyExists { what: "model", .. })
            ),
            "must reject rename-over-existing-name"
        );
    }

    #[test]
    fn validate_against_rejects_rename_field_to_existing_name() {
        // schema() has Post with fields [id, title]. Renaming id → title collides.
        let err = validate_against(&rename_field("Post", "id", "title"), &schema()).unwrap_err();
        assert!(matches!(
            err,
            PrimitiveError::AlreadyExists { what: "field", .. }
        ));
    }

    #[test]
    fn validate_against_rejects_change_type_on_missing_field() {
        let err = validate_against(&change_type("Post", "ghost", "i32"), &schema()).unwrap_err();
        assert!(matches!(
            err,
            PrimitiveError::NotFound { what: "field", .. }
        ));
    }

    #[test]
    fn validate_against_rejects_change_nullability_on_missing_field() {
        let err = validate_against(&change_nullable("Post", "ghost", true), &schema()).unwrap_err();
        assert!(matches!(
            err,
            PrimitiveError::NotFound { what: "field", .. }
        ));
    }

    #[test]
    fn plan_chains_rename_then_change_type_correctly() {
        // After renaming a model, subsequent steps must reference the
        // new name — proves that rename_model's apply_shadow actually
        // updates the schema copy.
        let plan = Plan::new(vec![
            rename_model("Post", "Article"),
            change_type("Article", "title", "String"),
        ]);
        assert_eq!(plan.validate(&schema()), Ok(()));
    }

    #[test]
    fn plan_chains_rename_field_then_change_nullability() {
        let plan = Plan::new(vec![
            rename_field("Post", "title", "heading"),
            change_nullable("Post", "heading", true),
        ]);
        assert_eq!(plan.validate(&schema()), Ok(()));
    }

    #[test]
    fn plan_json_round_trip() {
        let plan = Plan::new(vec![Primitive::CreateMigration(CreateMigration {
            name: "add_books".to_string(),
            sql: "CREATE TABLE books (id INTEGER);".to_string(),
        })]);
        let json = serde_json::to_string(&plan).unwrap();
        let back: Plan = serde_json::from_str(&json).unwrap();
        assert_eq!(back.steps.len(), 1);
    }
}
