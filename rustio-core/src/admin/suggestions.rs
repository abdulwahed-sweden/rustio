//! Actionable suggestion engine — 0.7.1.
//!
//! Derives concrete *add-this-field* suggestions from
//! `(schema, context)`. Every suggestion is a thin descriptor the
//! admin UI can turn into a button; clicking the button runs the
//! planner, the review layer, and finally the executor — the
//! standard chain. Nothing here bypasses safety gates.
//!
//! ## Scope (0.7.1)
//!
//! Only `AddField` suggestions for industry-required fields that a
//! model is missing. Destructive / renaming / type-changing
//! suggestions are explicitly out of scope — they need their own
//! review pass and are deferred.
//!
//! ## What this module does NOT do
//!
//! - It does not call the planner or executor itself. It only
//!   produces structured data (`Suggestion`) that describes what the
//!   user could opt into. Wiring lives in `admin.rs`.
//! - It does not touch the filesystem or database.

use crate::admin::entry_builder::DynamicAdminEntry;
use crate::admin::AdminEntry;
use crate::ai::ContextConfig;

/// How sure the suggestion engine is that this is the right action.
///
/// - [`Confidence::High`] — the field is explicitly listed in an
///   industry convention. We know the name; the type comes from
///   the planner's deterministic rules.
/// - [`Confidence::Medium`] — the suggestion was inferred from a
///   looser signal (heuristic, pattern match). Reserved for
///   0.7.x+ when we start producing suggestions from data shape
///   rather than explicit convention lists.
///
/// Rendered as a small badge next to the action button so the
/// operator sees, before clicking, how trustworthy the proposal is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Confidence {
    High,
    Medium,
}

impl Confidence {
    pub fn as_str(self) -> &'static str {
        match self {
            Confidence::High => "High",
            Confidence::Medium => "Medium",
        }
    }
    /// CSS pill class reusing the existing status palette.
    pub fn pill_class(self) -> &'static str {
        match self {
            Confidence::High => "rio-pill rio-pill-emerald",
            Confidence::Medium => "rio-pill rio-pill-amber",
        }
    }
}

/// One proposed action shown next to a dashboard alert.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Suggestion {
    /// The model's display name (e.g. `"Applicants"`). Used for the
    /// button label.
    pub model_display: String,
    /// The model's singular form (e.g. `"Applicant"`). Used in the
    /// planner prompt.
    pub model_singular: String,
    /// The URL slug under `/admin/<admin_name>` — also used as the
    /// routing key under `/admin/suggestions/<admin_name>/<field>`.
    pub admin_name: String,
    /// Field name the suggestion would add.
    pub field: String,
    /// Natural-language prompt handed to the planner when the user
    /// accepts. Example: `"add annual_income to applicants"`.
    pub prompt: String,
    /// One-line human rationale shown beside the button ("Housing
    /// industry convention", "GDPR retention required", …).
    pub reason: String,
    /// Short verb tag for the action type. Today always
    /// `"add_field"`; reserved so future variants (`"make_required"`
    /// etc.) can land without changing this struct.
    pub action: &'static str,
    /// How confident the engine is that this is the right move.
    pub confidence: Confidence,
}

impl Suggestion {
    /// Stable URL key under `/admin/suggestions/<admin_name>/<field>`.
    /// Used by the dashboard to render the href and by the route
    /// handler to re-derive + validate on both GET and POST.
    pub fn url_path(&self) -> String {
        format!(
            "/admin/suggestions/{admin}/{field}",
            admin = self.admin_name,
            field = self.field,
        )
    }
}

/// Enumerate every suggestion for the current project. Empty when
/// no context is loaded or when no model overlaps the industry's
/// convention list. Deterministic: iteration follows the order of
/// `entries` then `industry_schema.required_fields`.
pub fn derive_suggestions(
    entries: &[AdminEntry],
    context: Option<&ContextConfig>,
) -> Vec<Suggestion> {
    let Some(ctx) = context else {
        return Vec::new();
    };
    let Some(schema) = ctx.industry_schema() else {
        return Vec::new();
    };
    let industry = ctx.industry.as_deref().unwrap_or("").to_string();

    let mut out: Vec<Suggestion> = Vec::new();
    for entry in entries.iter().filter(|e| !e.core) {
        let field_names: Vec<&str> = entry.fields.iter().map(|f| f.name).collect();

        // Same gate the dashboard uses: only surface suggestions on a
        // model that already adopts *some* convention. Otherwise a
        // `Widget` model under `industry=housing` would nag about
        // personnummer, which is noise.
        let covers_any = schema
            .required_fields
            .iter()
            .any(|req| field_names.contains(&req.as_str()));
        if !covers_any {
            continue;
        }

        for req in &schema.required_fields {
            if field_names.contains(&req.as_str()) {
                continue;
            }
            let prompt = format!("add {req} to {admin}", admin = entry.admin_name);
            out.push(Suggestion {
                model_display: entry.display_name.to_string(),
                model_singular: entry.singular_name.to_string(),
                admin_name: entry.admin_name.to_string(),
                field: req.clone(),
                prompt,
                reason: format!("{industry} industry convention"),
                action: "add_field",
                // Industry-required fields are explicit, named
                // conventions — the engine isn't guessing. That's
                // High confidence.
                confidence: Confidence::High,
            });
        }
    }
    out
}

/// Look up a specific suggestion by `(admin_name, field)`. Returns
/// `None` if the pair isn't in the current derived set — this is how
/// the route handlers reject crafted URLs. An operator can only
/// click through suggestions the engine actually produced.
pub fn find_suggestion(
    entries: &[AdminEntry],
    context: Option<&ContextConfig>,
    admin_name: &str,
    field: &str,
) -> Option<Suggestion> {
    derive_suggestions(entries, context)
        .into_iter()
        .find(|s| s.admin_name == admin_name && s.field == field)
}

/// 0.7.3 schema-backed variant. Same rules as [`derive_suggestions`]
/// but reads field names from [`DynamicAdminEntry`], which the admin
/// builds fresh from [`crate::admin::schema_cache`] on every
/// dashboard render. When the cache sees an updated
/// `rustio.schema.json` — e.g. after `rustio ai apply` +
/// `rustio schema` + `[Reload schema]` — the suggestion for the
/// just-added field disappears on the next response, without
/// restarting the process.
pub fn derive_suggestions_from_entries(
    entries: &[DynamicAdminEntry],
    context: Option<&ContextConfig>,
) -> Vec<Suggestion> {
    let Some(ctx) = context else {
        return Vec::new();
    };
    let Some(schema) = ctx.industry_schema() else {
        return Vec::new();
    };
    let industry = ctx.industry.as_deref().unwrap_or("").to_string();

    let mut out: Vec<Suggestion> = Vec::new();
    for entry in entries.iter().filter(|e| !e.core) {
        let field_names: Vec<&str> = entry.fields.iter().map(|f| f.name.as_str()).collect();
        let covers_any = schema
            .required_fields
            .iter()
            .any(|req| field_names.contains(&req.as_str()));
        if !covers_any {
            continue;
        }
        for req in &schema.required_fields {
            if field_names.contains(&req.as_str()) {
                continue;
            }
            let prompt = format!("add {req} to {admin}", admin = entry.admin_name);
            out.push(Suggestion {
                model_display: entry.display_name.clone(),
                model_singular: entry.singular_name.clone(),
                admin_name: entry.admin_name.clone(),
                field: req.clone(),
                prompt,
                reason: format!("{industry} industry convention"),
                action: "add_field",
                confidence: Confidence::High,
            });
        }
    }
    out
}

/// Schema-backed counterpart to [`find_suggestion`].
pub fn find_suggestion_from_entries(
    entries: &[DynamicAdminEntry],
    context: Option<&ContextConfig>,
    admin_name: &str,
    field: &str,
) -> Option<Suggestion> {
    derive_suggestions_from_entries(entries, context)
        .into_iter()
        .find(|s| s.admin_name == admin_name && s.field == field)
}

/// 0.8.0 — propose linking an orphan `<thing>_id` column to a known
/// model when the schema has no [`Relation`](crate::schema::Relation)
/// recorded for it. Fired from `&Schema` directly because relation
/// metadata lives there (not on admin entries).
///
/// Matching rule: take the column name, strip the trailing `_id`, and
/// look for a model whose `singular_name` (case-insensitively) matches
/// the singularised remainder. If multiple models match — or none do —
/// skip: refusing is safer than guessing.
///
/// Deterministic: iteration follows `schema.models` then
/// `model.fields` order. No I/O, no allocation beyond the returned vec.
pub fn derive_relation_suggestions(schema: &crate::schema::Schema) -> Vec<Suggestion> {
    let mut out: Vec<Suggestion> = Vec::new();
    for model in schema.models.iter().filter(|m| !m.core) {
        for field in &model.fields {
            if field.name == "id" || !field.name.ends_with("_id") {
                continue;
            }
            if field.relation.is_some() {
                continue;
            }
            let stem = &field.name[..field.name.len() - 3];
            if stem.is_empty() {
                continue;
            }
            // Find the target: prefer a singular_name match; fall back
            // to the model name. Refuse on ambiguity or no match.
            let mut candidates: Vec<&crate::schema::SchemaModel> = schema
                .models
                .iter()
                .filter(|m| {
                    m.singular_name.eq_ignore_ascii_case(stem) || m.name.eq_ignore_ascii_case(stem)
                })
                .collect();
            candidates.dedup_by(|a, b| a.name == b.name);
            if candidates.len() != 1 {
                continue;
            }
            let target = candidates[0];
            if target.name == model.name {
                // Self-joins ("parent_id" on Category → Category) are
                // legitimate but easy to propose by accident. Skip in
                // 0.8.0 — the user can still type the prompt by hand.
                continue;
            }
            out.push(Suggestion {
                model_display: model.display_name.clone(),
                model_singular: model.singular_name.clone(),
                admin_name: model.admin_name.clone(),
                field: field.name.clone(),
                prompt: format!(
                    "link {from} to {to}",
                    from = model.singular_name,
                    to = target.singular_name,
                ),
                reason: format!(
                    "`{}` looks like a foreign key to `{}` but no relation is recorded.",
                    field.name, target.singular_name,
                ),
                action: "add_relation",
                // We inferred the target from a naming convention,
                // not from an explicit schema link. That's Medium —
                // the operator sees the confidence pill and decides.
                confidence: Confidence::Medium,
            });
        }
    }
    out
}

/// Companion to [`derive_relation_suggestions`] — locate one by
/// `(admin_name, field)`. Same rejection-of-crafted-URLs story as
/// [`find_suggestion`].
pub fn find_relation_suggestion(
    schema: &crate::schema::Schema,
    admin_name: &str,
    field: &str,
) -> Option<Suggestion> {
    derive_relation_suggestions(schema)
        .into_iter()
        .find(|s| s.admin_name == admin_name && s.field == field)
}
