//! Plan Review Layer — 0.5.1.
//!
//! The reviewable, risk-scored boundary between the AI planner (0.5.0)
//! and the (future) executor. Its single responsibility is to let a
//! human operator answer the question:
//!
//! > "I understand exactly what the AI wants to do, how risky it is,
//! >  and whether I should allow it."
//!
//! Nothing in this module touches the filesystem, the database, the
//! schema on disk, or emits SQL. It inspects an in-memory [`Plan`]
//! against an in-memory [`Schema`] and returns a structured report.
//!
//! ## What the layer provides
//!
//! - [`PlanDocument`] — a reviewable, serialisable envelope around a
//!   [`Plan`], carrying the prompt, the explanation the planner gave,
//!   the computed risk + impact, and a timestamp. Versioned (see
//!   [`PLAN_DOCUMENT_VERSION`]) so older-format documents are rejected
//!   rather than silently misread.
//! - [`review_plan`] — takes a plan and the current schema and
//!   produces a [`PlanReview`]: validation outcome, risk level, impact
//!   counts, and a deterministic list of warnings.
//! - [`load_plan`] — accepts either a full [`PlanDocument`] or a raw
//!   [`Plan`] JSON and tells the caller which it read, so CLI tools
//!   can normalise both shapes without guessing.
//! - Renderers for both a stable JSON output and an operator-friendly
//!   human summary.
//!
//! ## Determinism
//!
//! Risk classification, impact counting, and warning generation are
//! **deterministic**: the same `(Plan, Schema)` always yields the
//! same review. The only non-deterministic field anywhere in the
//! layer is [`PlanDocument::created_at`]; for tests, use
//! [`build_plan_document_with_timestamp`] to pin it.
//!
//! ## Safety posture
//!
//! Risk classification is *conservative*. When in doubt the layer
//! bumps up, never down. `Critical` is reserved for situations a
//! reviewer must refuse by default: plans that touch core models,
//! plans that fail validation, plans containing developer-only ops.
//!
//! ## What this module does NOT do
//!
//! - It does not parse user prompts (that is the planner's job).
//! - It does not modify the plan to "fix" it.
//! - It does not emit SQL, write files, or open databases.
//! - It does not call external services.

use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};

use super::planner::{ContextConfig, PlanResult};
use super::{validate_against, Plan, Primitive, PrimitiveError};
use crate::schema::{Schema, SchemaModel};

/// Version tag written into every [`PlanDocument`]. Bumped **only** on
/// a breaking change to the document shape — adding an optional field
/// is a minor change that the current reader handles via serde's
/// defaults. Parsers reject any document whose `version` doesn't match
/// this constant, so a future executor can trust the shape it reads.
pub const PLAN_DOCUMENT_VERSION: u32 = 1;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Severity class used by the review engine. Ordered so callers can
/// compare (`risk >= RiskLevel::High`) and take the max across steps.
///
/// The variants are a small, deliberately-closed enum; adding one is
/// a breaking change because reviewers rely on the four tiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RiskLevel {
    /// Reversible, non-destructive. e.g. an `AddField` for a nullable
    /// column, or flipping a field to nullable.
    Low,
    /// Data-preserving but disruptive. e.g. a rename, or a type change
    /// the executor will verify against data.
    Medium,
    /// Destructive or disruptive enough that a reviewer should pause.
    /// e.g. `RemoveField`, or a plan that mixes destructive and
    /// constructive steps.
    High,
    /// Must not execute without a reviewer overriding deliberately.
    /// Reached by: touching a core model, failing validation, or
    /// encountering a developer-only primitive in a plan.
    Critical,
}

impl RiskLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            RiskLevel::Low => "Low",
            RiskLevel::Medium => "Medium",
            RiskLevel::High => "High",
            RiskLevel::Critical => "Critical",
        }
    }
}

/// Aggregate counts of what a plan changes. Used by the CLI summary
/// and as an input to the risk classifier. Every field is non-negative
/// and derived mechanically from the plan — no fuzzy heuristics.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanImpact {
    pub adds_fields: usize,
    pub removes_fields: usize,
    pub renames: usize,
    pub type_changes: usize,
    pub nullability_changes: usize,
    /// `true` if any step's `model` points at a model flagged `core`
    /// in the supplied schema. Core models (e.g. `User`) are
    /// infrastructure — modifying them from an AI plan is never
    /// acceptable without a reviewer's explicit override.
    pub touches_core_models: bool,
    /// `true` if the plan contains any destructive primitive
    /// (`RemoveField`, `RemoveModel`, `RemoveRelation`). Distinct
    /// from the per-primitive counts so consumers can branch on
    /// "is anything destructive" without summing four fields.
    pub destructive: bool,
}

/// Serialisable, reviewable envelope around a validated [`Plan`].
///
/// The document carries every piece of context a reviewer needs to
/// decide yes/no without re-running the planner:
/// what the user asked for, the planner's one-paragraph explanation,
/// the computed impact + risk, and the plan itself.
///
/// `#[serde(deny_unknown_fields)]` is load-bearing: it prevents a
/// future executor from silently reading a field we never meant to
/// populate, and catches copy-paste errors in reviewer-authored
/// documents.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanDocument {
    pub version: u32,
    /// RFC 3339 UTC timestamp. Informational only — not used by the
    /// review engine. Stored as a string so the format is locked down
    /// regardless of chrono version.
    pub created_at: String,
    pub prompt: String,
    pub explanation: String,
    pub risk: RiskLevel,
    pub impact: PlanImpact,
    pub plan: Plan,
}

/// Output of a `load_plan` call. Lets the CLI distinguish "user
/// handed me a raw plan" from "user handed me a reviewed document"
/// and print a matching status line.
#[derive(Debug, Clone, PartialEq)]
pub enum LoadedPlan {
    Document(PlanDocument),
    RawPlan(Plan),
}

impl LoadedPlan {
    pub fn plan(&self) -> &Plan {
        match self {
            LoadedPlan::Document(d) => &d.plan,
            LoadedPlan::RawPlan(p) => p,
        }
    }
}

/// Result of [`review_plan`]. A review is always produced, even for
/// invalid plans — callers need to *see* the invalidity reason
/// without a separate error path.
#[derive(Debug, Clone, PartialEq)]
pub struct PlanReview {
    pub plan: Plan,
    pub impact: PlanImpact,
    pub risk: RiskLevel,
    pub warnings: Vec<String>,
    pub validation: ValidationOutcome,
}

/// Did the plan survive validation against the supplied schema?
///
/// Invalid ≠ malformed: a plan may be structurally fine but stale
/// (the schema it targeted has moved on). The variant carries the
/// step index + reason so operators can pinpoint which primitive is
/// now invalid.
#[derive(Debug, Clone, PartialEq)]
pub enum ValidationOutcome {
    Valid,
    Invalid { step: usize, reason: PrimitiveError },
}

impl ValidationOutcome {
    pub fn is_valid(&self) -> bool {
        matches!(self, ValidationOutcome::Valid)
    }
}

/// Reasons a review layer operation can fail. Parse errors and
/// version mismatches are the common cases; structural failures
/// (e.g. `build_plan_document` handed a plan that somehow fails
/// its own validation) are included for defence in depth.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub enum ReviewError {
    /// JSON didn't match any known shape (document or raw plan), or
    /// one of the shapes failed serde parsing.
    Parse(String),
    /// The document's `version` field didn't match
    /// [`PLAN_DOCUMENT_VERSION`]. Loud refusal rather than silent
    /// upgrade — the document shape is part of the API surface.
    UnknownVersion { found: u32, expected: u32 },
    /// A plan supplied to `build_plan_document` failed its own
    /// internal validation. This should only happen if the planner
    /// contract was violated upstream.
    InvalidPlan(PrimitiveError),
}

impl std::fmt::Display for ReviewError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Parse(msg) => write!(f, "plan review: parse error: {msg}"),
            Self::UnknownVersion { found, expected } => write!(
                f,
                "plan review: unsupported document version {found} (this build reads version {expected})"
            ),
            Self::InvalidPlan(e) => write!(f, "plan review: invalid plan: {e}"),
        }
    }
}

impl std::error::Error for ReviewError {}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Build a [`PlanDocument`] from a fresh planner result. Uses the
/// current UTC wall-clock for `created_at`.
///
/// The document is validated against `schema` before being returned:
/// an invalid plan surfaces as [`ReviewError::InvalidPlan`] rather
/// than producing a document the reviewer might trust.
pub fn build_plan_document(
    schema: &Schema,
    prompt: &str,
    result: &PlanResult,
    context: Option<&ContextConfig>,
) -> Result<PlanDocument, ReviewError> {
    build_plan_document_with_timestamp(schema, prompt, result, Utc::now(), context)
}

/// Same as [`build_plan_document`] but accepts an explicit timestamp.
/// Tests use a pinned value so snapshot comparisons stay stable;
/// callers with their own clock abstraction (e.g. a CI runner that
/// freezes time) can plumb it through here.
pub fn build_plan_document_with_timestamp(
    schema: &Schema,
    prompt: &str,
    result: &PlanResult,
    timestamp: DateTime<Utc>,
    context: Option<&ContextConfig>,
) -> Result<PlanDocument, ReviewError> {
    // Defence in depth: the planner already calls Plan::validate, but
    // an invalid plan sneaking into a saved document would be a
    // catastrophic review-layer failure. Re-check here.
    result
        .plan
        .validate(schema)
        .map_err(ReviewError::InvalidPlan)?;

    let impact = compute_impact(&result.plan, schema);
    let risk = classify_risk(&result.plan, &impact, &ValidationOutcome::Valid, context);
    Ok(PlanDocument {
        version: PLAN_DOCUMENT_VERSION,
        created_at: timestamp.to_rfc3339_opts(SecondsFormat::Secs, true),
        prompt: prompt.to_string(),
        explanation: result.explanation.clone(),
        risk,
        impact,
        plan: result.plan.clone(),
    })
}

/// Review a plan against a schema without executing anything. Returns
/// the full report even if the plan is invalid — the caller decides
/// how to present that to the user.
///
/// `context`, when present, escalates risk and adds context-aware
/// warnings (e.g. removing a personnummer field under `country=SE`
/// becomes Critical with a GDPR notice). Callers who don't have a
/// project context pass `None` — the review then behaves exactly
/// as in 0.5.x.
pub fn review_plan(
    schema: &Schema,
    plan: &Plan,
    context: Option<&ContextConfig>,
) -> Result<PlanReview, ReviewError> {
    let validation = match simulate_plan(plan, schema) {
        Ok(()) => ValidationOutcome::Valid,
        Err((step, reason)) => ValidationOutcome::Invalid { step, reason },
    };
    let impact = compute_impact(plan, schema);
    let risk = classify_risk(plan, &impact, &validation, context);
    let warnings = warnings_for(plan, context);
    Ok(PlanReview {
        plan: plan.clone(),
        impact,
        risk,
        warnings,
        validation,
    })
}

/// Parse JSON into either a [`PlanDocument`] or a raw [`Plan`].
///
/// The reader tries the richer [`PlanDocument`] first (because that's
/// what `rustio ai plan --save` emits). On failure it tries the raw
/// [`Plan`] shape. Only if *both* attempts fail do we surface an
/// error, so a simple `Plan` JSON piped in from another tool is
/// accepted transparently.
pub fn load_plan(json: &str) -> Result<LoadedPlan, ReviewError> {
    // Try the document shape first. `deny_unknown_fields` on
    // `PlanDocument` means this only succeeds for a real document.
    if let Ok(doc) = serde_json::from_str::<PlanDocument>(json) {
        if doc.version != PLAN_DOCUMENT_VERSION {
            return Err(ReviewError::UnknownVersion {
                found: doc.version,
                expected: PLAN_DOCUMENT_VERSION,
            });
        }
        return Ok(LoadedPlan::Document(doc));
    }
    // Then try a raw Plan.
    match serde_json::from_str::<Plan>(json) {
        Ok(plan) => Ok(LoadedPlan::RawPlan(plan)),
        Err(e) => Err(ReviewError::Parse(e.to_string())),
    }
}

/// Compute the impact counts for a plan against a given schema. Pure
/// and cheap — no allocation beyond the returned struct.
pub fn compute_impact(plan: &Plan, schema: &Schema) -> PlanImpact {
    let mut out = PlanImpact::default();
    for step in &plan.steps {
        match step {
            Primitive::AddField(_) => out.adds_fields += 1,
            Primitive::RemoveField(_) => {
                out.removes_fields += 1;
                out.destructive = true;
            }
            Primitive::RenameField(_) | Primitive::RenameModel(_) => out.renames += 1,
            Primitive::ChangeFieldType(_) => out.type_changes += 1,
            Primitive::ChangeFieldNullability(_) => out.nullability_changes += 1,
            Primitive::RemoveModel(_) | Primitive::RemoveRelation(_) => {
                out.destructive = true;
            }
            _ => {}
        }
        if touches_core_model(step, schema) {
            out.touches_core_models = true;
        }
    }
    out
}

/// Classify a plan's overall risk.
///
/// Rules (conservative — bump up, never down):
///
/// - If the plan fails validation → [`RiskLevel::Critical`].
/// - If any step targets a core model → [`RiskLevel::Critical`].
/// - If any step is developer-only (shouldn't happen from the planner
///   but is possible from a hand-edited document) → [`RiskLevel::Critical`].
/// - Otherwise take the max per-step risk, with one combinator: a
///   plan that mixes destructive and constructive steps bumps to at
///   least [`RiskLevel::High`].
pub fn classify_risk(
    plan: &Plan,
    impact: &PlanImpact,
    validation: &ValidationOutcome,
    context: Option<&ContextConfig>,
) -> RiskLevel {
    if !validation.is_valid() {
        return RiskLevel::Critical;
    }
    if impact.touches_core_models {
        return RiskLevel::Critical;
    }
    if plan.steps.iter().any(|s| s.is_developer_only()) {
        return RiskLevel::Critical;
    }

    // Context-aware escalation: destructive primitives on a field
    // flagged as PII under the current project context are Critical,
    // regardless of what the structural rules would score.
    if let Some(ctx) = context {
        let pii = ctx.pii_fields();
        for step in &plan.steps {
            match step {
                Primitive::RemoveField(r) if pii.iter().any(|p| *p == r.field) => {
                    return RiskLevel::Critical;
                }
                Primitive::RenameField(r) if pii.iter().any(|p| *p == r.from) => {
                    return RiskLevel::Critical;
                }
                Primitive::ChangeFieldType(c) if pii.iter().any(|p| *p == c.field) => {
                    return RiskLevel::Critical;
                }
                _ => {}
            }
        }
    }

    let mut max = RiskLevel::Low;
    for step in &plan.steps {
        let r = per_step_risk(step);
        if r > max {
            max = r;
        }
    }
    // Mixing add + remove in one plan is its own footgun — bump.
    let mixes_add_and_remove = impact.adds_fields > 0 && impact.removes_fields > 0;
    if mixes_add_and_remove && max < RiskLevel::High {
        max = RiskLevel::High;
    }
    max
}

/// Deterministic warnings derived strictly from the plan. Never
/// speculative — every bullet in the output has a concrete trigger.
///
/// `context`, when present, surfaces the extra warnings the review
/// layer owes an operator under real-world constraints: GDPR,
/// industry conventions, country-specific PII. Without context the
/// output is the 0.5.x set — nothing changes for projects that
/// haven't opted in.
pub fn warnings_for(plan: &Plan, context: Option<&ContextConfig>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut has_remove = false;
    let mut has_rename_model = false;
    let mut has_rename_field = false;
    let mut has_type_change = false;
    let mut has_require = false;
    let mut has_remove_model = false;
    let mut has_dev_only = false;

    for step in &plan.steps {
        match step {
            Primitive::RemoveField(_) => has_remove = true,
            Primitive::RenameModel(_) => has_rename_model = true,
            Primitive::RenameField(_) => has_rename_field = true,
            Primitive::ChangeFieldType(_) => has_type_change = true,
            Primitive::ChangeFieldNullability(c) if !c.nullable => has_require = true,
            Primitive::RemoveModel(_) => has_remove_model = true,
            _ => {}
        }
        if step.is_developer_only() {
            has_dev_only = true;
        }
    }
    if has_remove {
        out.push("This plan removes a field. Existing data in that column may become inaccessible after execution.".into());
    }
    if has_remove_model {
        out.push("This plan removes a model. Every row, foreign-key reference, and admin route for that model will be dropped.".into());
    }
    if has_rename_model {
        out.push("This plan renames a model. Downstream code, admin URLs, and external integrations that hard-code the old name will break.".into());
    }
    if has_rename_field {
        out.push("This plan renames a field. Queries, serialised payloads, and UI references using the old name will break.".into());
    }
    if has_require {
        out.push("This plan changes a field from nullable to required. Rows with a NULL in that column will fail to load after execution.".into());
    }
    if has_type_change {
        out.push("This plan changes a field's type. The executor may refuse conversions it considers lossy.".into());
    }
    if has_type_change || has_require {
        // Both triggers force a SQLite recreate-table migration.
        out.push("This operation rewrites the entire table. Large tables may cause downtime during execution.".into());
    }
    if plan.steps.len() > 1 {
        out.push(format!(
            "This plan performs {n} operations. Review each step individually.",
            n = plan.steps.len(),
        ));
    }
    if has_dev_only {
        out.push("This plan contains a developer-only primitive. It must never be executed from an AI pipeline.".into());
    }

    // Context-aware warnings. Each bullet is justified by the
    // combination of (plan, context); nothing speculative.
    if let Some(ctx) = context {
        let pii = ctx.pii_fields();
        for step in &plan.steps {
            match step {
                Primitive::RemoveField(r) if pii.iter().any(|p| *p == r.field) => {
                    out.push(format!(
                        "Field `{}.{}` is considered sensitive personal data under the project's context{}. Removing it is irreversible — review retention obligations first.",
                        r.model,
                        r.field,
                        describe_context(ctx),
                    ));
                }
                Primitive::RenameField(r) if pii.iter().any(|p| *p == r.from) => {
                    out.push(format!(
                        "Field `{}.{}` is sensitive personal data; renaming it invalidates any existing access-log / audit trail keyed on the old name.",
                        r.model, r.from,
                    ));
                }
                Primitive::ChangeFieldType(c) if pii.iter().any(|p| *p == c.field) => {
                    out.push(format!(
                        "Field `{}.{}` is sensitive personal data; type changes may affect hashing, masking, or retention pipelines keyed on its storage shape.",
                        c.model, c.field,
                    ));
                }
                _ => {}
            }
        }
        // Industry-convention removals: warn if a plan removes a field
        // the industry schema flags as a standard convention.
        if let Some(schema) = ctx.industry_schema() {
            for step in &plan.steps {
                if let Primitive::RemoveField(r) = step {
                    if schema.required_fields.iter().any(|f| f == &r.field) {
                        out.push(format!(
                            "Field `{}.{}` is a standard convention for the `{}` industry. Removing it will break downstream integrations that assume it exists.",
                            r.model,
                            r.field,
                            ctx.industry.as_deref().unwrap_or(""),
                        ));
                    }
                }
            }
        }
    }

    out
}

/// One-line description of the active context pieces. Used by warning
/// messages that want to cite the reason they fired.
fn describe_context(ctx: &ContextConfig) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(c) = &ctx.country {
        parts.push(format!("country={c}"));
    }
    if let Some(i) = &ctx.industry {
        parts.push(format!("industry={i}"));
    }
    if ctx.requires_gdpr() {
        parts.push("GDPR".to_string());
    }
    if parts.is_empty() {
        String::new()
    } else {
        format!(" ({})", parts.join(", "))
    }
}

/// Render a [`PlanReview`] as an operator-friendly summary.
///
/// Output is a single text block — no colour, no fancy formatting,
/// no `Debug` dumps. Designed to fit in a terminal window, a code
/// review comment, or a Slack message.
pub fn render_review_human(review: &PlanReview, header: Option<&ReviewHeader>) -> String {
    let mut out = String::new();
    out.push_str("Plan review\n");
    if let Some(h) = header {
        if let Some(p) = &h.prompt {
            out.push_str(&format!("\nPrompt:\n  {p}\n"));
        }
        if let Some(e) = &h.explanation {
            out.push_str(&format!("\nExplanation:\n  {e}\n"));
        }
        if let Some(src) = &h.source {
            out.push_str(&format!("\nSource:\n  {src}\n"));
        }
    }
    out.push_str(&format!("\nRisk:\n  {}\n", review.risk.as_str()));
    out.push_str("\nImpact:\n");
    for line in render_impact_lines(&review.impact) {
        out.push_str("  - ");
        out.push_str(&line);
        out.push('\n');
    }
    out.push_str("\nPlanned changes:\n");
    if review.plan.steps.is_empty() {
        out.push_str("  - (none)\n");
    } else {
        for step in &review.plan.steps {
            out.push_str("  - ");
            out.push_str(&summarise_primitive(step));
            out.push('\n');
        }
    }
    out.push_str("\nValidation:\n");
    match &review.validation {
        ValidationOutcome::Valid => out.push_str("  - Passes against the current schema.\n"),
        ValidationOutcome::Invalid { step, reason } => {
            out.push_str(&format!(
                "  - FAILS at step {step}: {reason}\n",
                step = step,
                reason = reason,
            ));
            out.push_str("  - The plan is stale or invalid for the current schema. Regenerate it before executing.\n");
        }
    }
    out.push_str("\nWarnings:\n");
    if review.warnings.is_empty() {
        out.push_str("  - None\n");
    } else {
        for w in &review.warnings {
            out.push_str("  - ");
            out.push_str(w);
            out.push('\n');
        }
    }
    out
}

/// Optional context the CLI passes to the human renderer (the review
/// engine itself doesn't see prompt / explanation — they live on the
/// enclosing document).
#[derive(Debug, Default, Clone)]
pub struct ReviewHeader {
    pub prompt: Option<String>,
    pub explanation: Option<String>,
    pub source: Option<String>,
}

/// Serialise a [`PlanDocument`] to deterministic, pretty-printed JSON
/// with a trailing newline. Matches the convention
/// `Schema::to_pretty_json` uses so both artefacts look uniform under
/// review.
pub fn render_plan_document_json(doc: &PlanDocument) -> Result<String, ReviewError> {
    let mut out =
        serde_json::to_string_pretty(doc).map_err(|e| ReviewError::Parse(e.to_string()))?;
    out.push('\n');
    Ok(out)
}

// ---------------------------------------------------------------------------
// Internals
// ---------------------------------------------------------------------------

/// Simulate a plan against a schema copy (same logic `Plan::validate`
/// uses internally). Returns the step index + error if validation
/// stops, so the review can point at exactly which step is stale.
fn simulate_plan(plan: &Plan, schema: &Schema) -> Result<(), (usize, PrimitiveError)> {
    let mut state = schema.clone();
    for (idx, step) in plan.steps.iter().enumerate() {
        if step.is_developer_only() {
            return Err((
                idx,
                PrimitiveError::DeveloperOnlyNotAllowedInPlan { op: step.op_name() },
            ));
        }
        if let Err(e) = super::validate_primitive(step) {
            return Err((idx, e));
        }
        if let Err(e) = validate_against(step, &state) {
            return Err((idx, e));
        }
        apply_shadow_for_review(step, &mut state);
    }
    Ok(())
}

/// Shadow-apply a primitive to an in-memory schema copy. Mirrors the
/// logic in `ai.rs::apply_shadow` but isn't re-exported, so we keep a
/// tiny local copy rather than widening the crate's private surface.
/// Safe to diverge? No — the point of the review is to model the
/// same transitions the executor will. Keep this list in sync.
fn apply_shadow_for_review(p: &Primitive, schema: &mut Schema) {
    use crate::schema::{SchemaField, SchemaRelation};
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
                    relation: None,
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
                core: false,
            });
            schema.models.sort_by(|a, b| a.name.cmp(&b.name));
        }
        Primitive::RemoveModel(m) => schema.models.retain(|x| x.name != m.name),
        Primitive::AddField(af) => {
            if let Some(model) = schema.models.iter_mut().find(|m| m.name == af.model) {
                model.fields.push(SchemaField {
                    name: af.field.name.clone(),
                    ty: af.field.ty.clone(),
                    nullable: af.field.nullable,
                    editable: af.field.editable,
                    relation: None,
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
                    kind: format!("{:?}", r.kind).to_lowercase(),
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
        Primitive::UpdateAdmin(_) | Primitive::CreateMigration(_) => {}
    }
}

/// Does this primitive target a model that is flagged `core` in the
/// schema? Used by [`compute_impact`] to set `touches_core_models`
/// and by [`classify_risk`] to bump to Critical.
fn touches_core_model(p: &Primitive, schema: &Schema) -> bool {
    let target = match p {
        Primitive::AddField(a) => Some(a.model.as_str()),
        Primitive::RemoveField(r) => Some(r.model.as_str()),
        Primitive::RenameField(r) => Some(r.model.as_str()),
        Primitive::ChangeFieldType(c) => Some(c.model.as_str()),
        Primitive::ChangeFieldNullability(c) => Some(c.model.as_str()),
        Primitive::UpdateAdmin(u) => Some(u.model.as_str()),
        Primitive::RenameModel(r) => Some(r.from.as_str()),
        Primitive::RemoveModel(m) => Some(m.name.as_str()),
        Primitive::AddRelation(r) => Some(r.from.as_str()),
        Primitive::RemoveRelation(r) => Some(r.from.as_str()),
        // AddModel creates a new (necessarily non-core) model.
        Primitive::AddModel(_) | Primitive::CreateMigration(_) => None,
    };
    let Some(name) = target else { return false };
    schema.models.iter().any(|m| m.name == name && m.core)
}

fn per_step_risk(p: &Primitive) -> RiskLevel {
    match p {
        // Safe additions
        Primitive::AddField(a) if a.field.nullable => RiskLevel::Low,
        Primitive::AddField(_) => RiskLevel::Low,
        Primitive::AddRelation(_) => RiskLevel::Low,
        Primitive::AddModel(_) => RiskLevel::Low,
        Primitive::UpdateAdmin(_) => RiskLevel::Low,
        // Flipping to nullable is reversible and safe; to required is not.
        Primitive::ChangeFieldNullability(c) if c.nullable => RiskLevel::Low,
        // Tightening nullable → required is High (0.5.3): the executor
        // will COALESCE existing NULLs with a type default at write
        // time, which is acceptable data loss *if* the reviewer has
        // consented to it. Conservative bump.
        Primitive::ChangeFieldNullability(_) => RiskLevel::High,
        // Data-preserving but noisy
        Primitive::RenameField(_) | Primitive::RenameModel(_) | Primitive::ChangeFieldType(_) => {
            RiskLevel::Medium
        }
        // Destructive
        Primitive::RemoveField(_) | Primitive::RemoveModel(_) | Primitive::RemoveRelation(_) => {
            RiskLevel::High
        }
        // Shouldn't be in a plan at all — reviewer must refuse.
        Primitive::CreateMigration(_) => RiskLevel::Critical,
    }
}

/// One-line description for the human review. Matches the style of
/// `planner::render_plan_human` but in past-tense-summary form.
fn summarise_primitive(p: &Primitive) -> String {
    match p {
        Primitive::AddField(a) => format!(
            "Add field \"{}\" ({}{}) to model \"{}\"",
            a.field.name,
            a.field.ty,
            if a.field.nullable { ", nullable" } else { "" },
            a.model,
        ),
        Primitive::RemoveField(r) => {
            format!("Remove field \"{}\" from model \"{}\"", r.field, r.model)
        }
        Primitive::RenameField(r) => {
            format!("Rename field \"{}.{}\" to \"{}\"", r.model, r.from, r.to)
        }
        Primitive::RenameModel(r) => {
            format!("Rename model \"{}\" to \"{}\"", r.from, r.to)
        }
        Primitive::ChangeFieldType(c) => format!(
            "Change type of \"{}.{}\" to {}",
            c.model, c.field, c.new_type
        ),
        Primitive::ChangeFieldNullability(c) => format!(
            "Mark \"{}.{}\" as {}",
            c.model,
            c.field,
            if c.nullable { "nullable" } else { "required" },
        ),
        Primitive::AddModel(m) => format!(
            "Add model \"{}\" ({} field{})",
            m.name,
            m.fields.len(),
            if m.fields.len() == 1 { "" } else { "s" }
        ),
        Primitive::RemoveModel(m) => format!("Remove model \"{}\"", m.name),
        Primitive::AddRelation(r) => format!(
            "Add relation {:?}: {}.{} -> {}",
            r.kind, r.from, r.via, r.to
        ),
        Primitive::RemoveRelation(r) => {
            format!("Remove relation \"{}.{}\"", r.from, r.via)
        }
        Primitive::UpdateAdmin(u) => format!(
            "Update admin attribute \"{}.{}\".{}",
            u.model, u.field, u.attr
        ),
        Primitive::CreateMigration(m) => format!("[dev-only] create_migration \"{}\"", m.name),
    }
}

/// Expand an impact struct into the bullet list the human renderer
/// uses. Each bullet is deterministic and self-describing.
fn render_impact_lines(i: &PlanImpact) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    push_count_line(&mut lines, "Add", i.adds_fields, "field");
    push_count_line(&mut lines, "Remove", i.removes_fields, "field");
    push_count_line(&mut lines, "Rename", i.renames, "item");
    push_count_line(&mut lines, "Type change", i.type_changes, "field");
    push_count_line(
        &mut lines,
        "Nullability change",
        i.nullability_changes,
        "field",
    );
    if i.destructive {
        lines.push("Includes at least one destructive step".into());
    } else {
        lines.push("No destructive changes".into());
    }
    if i.touches_core_models {
        lines.push("Touches a core model — review carefully".into());
    } else {
        lines.push("Does not touch core models".into());
    }
    lines
}

fn push_count_line(out: &mut Vec<String>, verb: &str, n: usize, unit: &str) {
    if n == 0 {
        return;
    }
    out.push(format!(
        "{verb} {n} {unit}{s}",
        s = if n == 1 { "" } else { "s" }
    ));
}
