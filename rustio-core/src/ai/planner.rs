//! Rule-based AI planner — the *brain* of the 0.5.0 intelligence layer.
//!
//! Reads a natural-language prompt, a project schema, and an optional
//! context file. Emits a structured [`Plan`] of [`Primitive`] operations
//! plus a human-readable explanation. **Never executes.**
//!
//! ## Boundaries
//!
//! - No filesystem writes. No database. No SQL. No network.
//! - No `CreateMigration` in emitted plans — that primitive is
//!   developer-only and [`Plan::validate`] rejects it.
//! - Returns [`PlanError`] for every case the planner cannot confidently
//!   resolve; the caller decides whether to retry with a clearer prompt.
//!
//! ## Inference strategy
//!
//! Pure rule-based pattern matching. No model calls. The grammar covers:
//!
//! - `add <field> to <model>`
//! - `add <field> as <type> to <model>`
//! - `add optional <field> to <model>`
//! - `rename <field> to <new> in <model>`
//! - `rename model <old> to <new>` / `rename <old> to <new>`
//! - `remove <field> from <model>` / `drop <field> from <model>`
//! - `change <field> in <model> to <type>`
//! - `make <field> in <model> optional|nullable|required`
//!
//! Anything outside this grammar returns [`PlanError::InvalidIntent`]
//! with a list of supported forms — never a guessed plan.
//!
//! ## Why rule-based
//!
//! The planner is a *safety surface*. A statistical model can hallucinate
//! a field the schema doesn't have; a rule can't. The output of this
//! module is the single input the 0.5.x executor will see, so every
//! ambiguity that lives here would live in production. We keep it
//! deterministic and auditable.

use serde::{Deserialize, Serialize};

use super::{
    validate_primitive, AddField, ChangeFieldNullability, ChangeFieldType, FieldSpec, Plan,
    Primitive, PrimitiveError, RemoveField, RenameField, RenameModel,
};
use crate::schema::{Schema, SchemaModel};

/// Optional per-project context loaded from `rustio.context.json`.
///
/// The 0.6.0 shape covers four axes:
///
/// - `country` — ISO-3166-1 alpha-2 (`"SE"`, `"NO"`, …). Drives
///   locale-aware naming (a Swedish project gets `personnummer` for
///   "personal id", not an `i32`).
/// - `region` — supra-national grouping (`"EU"`). Mostly inferred from
///   `country`; explicit setting is an override.
/// - `industry` — `"housing"`, `"healthcare"`, `"banking"`. Picked up
///   by the planner / review / executor so "patient id" under
///   `healthcare` becomes a `String` rather than an `i32`, and removing
///   a convention field raises a warning.
/// - `compliance` — explicit list (`["GDPR", "HIPAA"]`). Empty by
///   default; the helpers below treat `region=EU` as implying `GDPR`
///   even when the list is empty.
///
/// `#[serde(default, deny_unknown_fields)]` keeps the wire contract
/// tight: a typo is a loud error, not a silent miss.
///
/// **Breaking change vs 0.5.x:** the old `domain` key is gone. If your
/// `rustio.context.json` still reads `{"domain": "housing"}`, rename
/// the key to `industry`.
#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct ContextConfig {
    pub country: Option<String>,
    pub region: Option<String>,
    pub industry: Option<String>,
    #[serde(default)]
    pub compliance: Vec<String>,
}

impl ContextConfig {
    pub fn parse(json: &str) -> Result<Self, PlanError> {
        serde_json::from_str::<ContextConfig>(json)
            .map_err(|e| PlanError::ContextParse(e.to_string()))
    }

    /// Either the explicit `region`, or a best-effort inference from
    /// `country`. Today we only know the EU list; other regions
    /// (ASEAN, LATAM, MENA, …) fall through to `None` until projects
    /// ask for them.
    pub fn effective_region(&self) -> Option<String> {
        if let Some(r) = &self.region {
            if !r.trim().is_empty() {
                return Some(r.clone());
            }
        }
        const EU_MEMBER_STATES: &[&str] = &[
            "AT", "BE", "BG", "HR", "CY", "CZ", "DK", "EE", "FI", "FR", "DE", "GR", "HU", "IE",
            "IT", "LV", "LT", "LU", "MT", "NL", "PL", "PT", "RO", "SK", "SI", "ES", "SE",
        ];
        match self.country.as_deref() {
            Some(cc) if EU_MEMBER_STATES.iter().any(|m| m.eq_ignore_ascii_case(cc)) => {
                Some("EU".into())
            }
            _ => None,
        }
    }

    /// `true` if the project operates under the GDPR. Detected by
    /// either (a) `compliance` listing `"GDPR"` explicitly, or
    /// (b) the resolved region being `"EU"`.
    pub fn requires_gdpr(&self) -> bool {
        if self
            .compliance
            .iter()
            .any(|c| c.trim().eq_ignore_ascii_case("GDPR"))
        {
            return true;
        }
        matches!(self.effective_region().as_deref(), Some("EU"))
    }

    /// Look up the industry convention bundle for the selected industry
    /// (case-insensitive). `None` if the project didn't set one or the
    /// name isn't in the registry.
    pub fn industry_schema(&self) -> Option<super::industry::IndustrySchema> {
        self.industry
            .as_deref()
            .and_then(super::industry::industry_schema_for)
    }

    /// Field names considered personally-identifying under the current
    /// context. Returns a stable, deduplicated list. Used by the review
    /// layer to escalate risk on destructive primitives and by the
    /// executor to refuse them outright with
    /// `ExecutionError::PolicyViolation`.
    ///
    /// Conservative by design: the list grows only as each rule is
    /// justified. A project needing stricter enforcement can still
    /// layer its own checks on top.
    pub fn pii_fields(&self) -> Vec<&'static str> {
        let mut out: Vec<&'static str> = Vec::new();
        match self.country.as_deref() {
            Some(cc) if cc.eq_ignore_ascii_case("SE") => {
                out.push("personnummer");
            }
            Some(cc) if cc.eq_ignore_ascii_case("NO") => {
                out.push("fodselsnummer");
            }
            Some(cc) if cc.eq_ignore_ascii_case("US") => {
                out.push("ssn");
            }
            _ => {}
        }
        if self.requires_gdpr() {
            // Generic PII under GDPR. The list is deliberately short —
            // contact details that a reasonable reviewer would want to
            // flag. Wider lists (device IDs, IP addresses) need their
            // own review pass.
            for f in ["email", "phone", "address", "date_of_birth"] {
                if !out.contains(&f) {
                    out.push(f);
                }
            }
        }
        out
    }

    /// `true` when the context carries at least one useful signal. The
    /// CLI uses this to decide whether `rustio context show` has
    /// something to print.
    pub fn is_empty(&self) -> bool {
        self.country.is_none()
            && self.region.is_none()
            && self.industry.is_none()
            && self.compliance.is_empty()
    }
}

/// A natural-language planning request.
#[derive(Debug, Clone)]
pub struct PlanRequest {
    pub prompt: String,
}

impl PlanRequest {
    pub fn new<S: Into<String>>(prompt: S) -> Self {
        Self {
            prompt: prompt.into(),
        }
    }
}

/// Output of [`generate_plan`]: the structured steps plus a one-
/// paragraph rationale the CLI can display.
#[derive(Debug, Clone)]
pub struct PlanResult {
    pub plan: Plan,
    pub explanation: String,
}

/// Every reason the planner can refuse. Named variants so downstream
/// tooling can branch on kind instead of parsing strings.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub enum PlanError {
    /// Prompt was empty or whitespace only.
    EmptyPrompt,
    /// Prompt didn't match any supported grammar.
    InvalidIntent(String),
    /// Prompt referenced a model the schema doesn't know about.
    UnknownModel { hint: String },
    /// Prompt referenced a model name that matched more than one
    /// registered model (e.g. under both struct name and singular form).
    /// The candidates are surfaced so the caller can re-prompt with a
    /// specific name.
    AmbiguousModel {
        hint: String,
        candidates: Vec<String>,
    },
    /// `add_field` would collide with an existing field.
    FieldAlreadyExists { model: String, field: String },
    /// `remove_field` / `rename_field` / `change_*` referenced a field
    /// that doesn't exist on the resolved model.
    FieldDoesNotExist { model: String, field: String },
    /// User asked for something only a developer may do (e.g. raw
    /// SQL, `create_migration`). The planner refuses up front.
    DeveloperOnlyRequested(&'static str),
    /// Planner-proposed an operation that would modify a `core: true`
    /// model (e.g. `User`). Refused.
    CoreModelProtected(String),
    /// Unknown type hint the user supplied (`as foobar`).
    UnknownType(String),
    /// The composed plan failed [`Plan::validate`] — the schema-aware
    /// simulation disagreed with the proposed primitive. Wrapped error
    /// carries the step index + reason.
    Validation(PrimitiveError),
    /// `rustio.context.json` existed but failed to parse.
    ContextParse(String),
}

impl std::fmt::Display for PlanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyPrompt => write!(f, "empty prompt"),
            Self::InvalidIntent(msg) => write!(f, "invalid intent: {msg}"),
            Self::UnknownModel { hint } => write!(f, "unknown model `{hint}`"),
            Self::AmbiguousModel { hint, candidates } => write!(
                f,
                "ambiguous model `{hint}` (candidates: {})",
                candidates.join(", ")
            ),
            Self::FieldAlreadyExists { model, field } => {
                write!(f, "field `{model}.{field}` already exists")
            }
            Self::FieldDoesNotExist { model, field } => {
                write!(f, "field `{model}.{field}` does not exist")
            }
            Self::DeveloperOnlyRequested(op) => write!(
                f,
                "`{op}` is developer-only and the AI planner cannot emit it"
            ),
            Self::CoreModelProtected(name) => write!(
                f,
                "model `{name}` is a core model and cannot be modified by the AI planner"
            ),
            Self::UnknownType(t) => write!(
                f,
                "unknown type `{t}` (valid: i32, i64, String, bool, DateTime)"
            ),
            Self::Validation(e) => write!(f, "plan validation failed: {e}"),
            Self::ContextParse(msg) => write!(f, "rustio.context.json parse error: {msg}"),
        }
    }
}

impl std::error::Error for PlanError {}

/// Entry point. Interprets `request.prompt` against `schema` (with
/// optional `context`) and returns a validated [`Plan`] or a specific
/// [`PlanError`].
///
/// The function performs no I/O. The caller owns schema/context
/// loading, so this module stays trivially testable.
pub fn generate_plan(
    schema: &Schema,
    context: Option<&ContextConfig>,
    request: PlanRequest,
) -> Result<PlanResult, PlanError> {
    let raw = request.prompt.trim();
    if raw.is_empty() {
        return Err(PlanError::EmptyPrompt);
    }
    let lower = raw.to_lowercase();

    // Refuse anything that smells like a developer-only request before
    // we try to pattern-match it as a structured op.
    if lower.contains("create migration")
        || lower.contains("raw sql")
        || lower.contains("run sql")
        || lower.contains("execute sql")
        || lower.contains("add sql")
    {
        return Err(PlanError::DeveloperOnlyRequested("create_migration"));
    }

    // Order matters: more specific patterns first so a `rename model …`
    // isn't mistaken for a `rename <field> …` with a weird model hint.
    for parser in PARSERS {
        if let Some(result) = parser(raw, &lower, schema, context)? {
            // Every returned plan is validated against the schema; this
            // is the final safety gate the caller can rely on.
            result
                .plan
                .validate(schema)
                .map_err(PlanError::Validation)?;
            return Ok(result);
        }
    }

    Err(PlanError::InvalidIntent(supported_forms_message(raw)))
}

// ---------------------------------------------------------------------------
// Pattern parsers
// ---------------------------------------------------------------------------

type Parser = fn(
    raw: &str,
    lower: &str,
    schema: &Schema,
    ctx: Option<&ContextConfig>,
) -> Result<Option<PlanResult>, PlanError>;

const PARSERS: &[Parser] = &[
    try_rename_model,
    try_rename_field,
    try_remove_field,
    try_change_type,
    try_change_nullability,
    // Relation parsers have more specific prefixes than `add `,
    // so they run before `try_add_field` to avoid swallowing
    // "add relation from …" as a generic add-field.
    try_add_relation,
    try_add_field,
];

/// 0.8.0 — parse "add relation from <model> to <target>",
/// "link <model> to <target>", or "connect <model> to <target>".
/// The owning column name is inferred as `<target_admin_name>_id`
/// (singularised). Refuses when either model is unknown, when the
/// field already exists, or when a relation with the same `via`
/// is already recorded on the model.
fn try_add_relation(
    raw: &str,
    lower: &str,
    schema: &Schema,
    _context: Option<&ContextConfig>,
) -> Result<Option<PlanResult>, PlanError> {
    // Accept three prefixes. The longest wins — `add relation from`
    // must be tried before `add `, which belongs to `try_add_field`.
    let after = if let Some(rest) = lower.strip_prefix("add relation from ") {
        slice_original(raw, "add relation from ").unwrap_or(rest)
    } else if let Some(rest) = lower.strip_prefix("link ") {
        slice_original(raw, "link ").unwrap_or(rest)
    } else if let Some(rest) = lower.strip_prefix("connect ") {
        slice_original(raw, "connect ").unwrap_or(rest)
    } else {
        return Ok(None);
    };

    let Some((from_hint, to_hint)) = split_on_keyword(after, &[" to "]) else {
        return Err(PlanError::InvalidIntent(format!(
            "relation prompt expects `<model> to <target>`: got {raw:?}"
        )));
    };
    let from = resolve_model(schema, from_hint)?;
    let to = resolve_model(schema, to_hint)?;

    // Singularised admin slug of the target, suffixed with `_id`.
    // `applicants` → `applicant_id`; `posts` → `post_id`.
    let via = format!("{}_id", depluralise(&to.admin_name.to_lowercase()));

    // Refuse if the owning model already has this column (avoid
    // double-FK rows). The executor's idempotency gate catches this
    // later too, but catching it at plan time gives a clearer error.
    if from.fields.iter().any(|f| f.name == via) {
        return Err(PlanError::FieldAlreadyExists {
            model: from.name.clone(),
            field: via,
        });
    }

    // Refuse core models on either side — the AI boundary already
    // protects them against schema-shape changes.
    if from.core {
        return Err(PlanError::CoreModelProtected(from.name.clone()));
    }
    if to.core {
        return Err(PlanError::CoreModelProtected(to.name.clone()));
    }

    let primitive = Primitive::AddRelation(super::AddRelation {
        from: from.name.clone(),
        kind: crate::schema::RelationKind::BelongsTo,
        to: to.name.clone(),
        via: via.clone(),
    });
    validate_primitive(&primitive).map_err(PlanError::Validation)?;

    let explanation = format!(
        "Adds a `belongs_to` relation from `{}` to `{}` via column `{}` (i64). \
         The executor will add the column but not a SQL foreign-key \
         constraint — enforcement lands in 0.9.0.",
        from.name, to.name, via,
    );
    Ok(Some(PlanResult {
        plan: Plan::new(vec![primitive]),
        explanation,
    }))
}

fn try_add_field(
    raw: &str,
    lower: &str,
    schema: &Schema,
    context: Option<&ContextConfig>,
) -> Result<Option<PlanResult>, PlanError> {
    let Some(rest) = lower.strip_prefix("add ") else {
        return Ok(None);
    };
    // Reject things we already route elsewhere:
    if rest.starts_with("model ") {
        return Err(PlanError::InvalidIntent(
            "`add model …` is not supported yet by the planner (requires spec of every field). \
             Write the model by hand and the AI layer will read it from the schema."
                .to_string(),
        ));
    }
    let after = slice_original(raw, "add ").unwrap_or(raw);
    let Some((left, right)) = split_on_keyword(after, &[" to ", " on "]) else {
        return Err(PlanError::InvalidIntent(format!(
            "`add` requires `… to <model>`: got {raw:?}"
        )));
    };
    let model = resolve_model(schema, right)?;
    if model.core {
        return Err(PlanError::CoreModelProtected(model.name.clone()));
    }
    let (field_name, modifiers) = parse_field_phrase(left);
    if field_name.is_empty() {
        return Err(PlanError::InvalidIntent(
            "missing field name in `add` clause".to_string(),
        ));
    }
    if model.fields.iter().any(|f| f.name == field_name) {
        return Err(PlanError::FieldAlreadyExists {
            model: model.name.clone(),
            field: field_name,
        });
    }
    let (ty, nullable) = infer_field_type(&field_name, &modifiers, context)?;
    let nullable = nullable || phrase_is_optional(&modifiers);

    let primitive = Primitive::AddField(AddField {
        model: model.name.clone(),
        field: FieldSpec {
            name: field_name.clone(),
            ty: ty.clone(),
            nullable,
            editable: true,
        },
    });
    validate_primitive(&primitive).map_err(PlanError::Validation)?;
    let explanation = explain_add_field(&model.name, &field_name, &ty, nullable, context);
    Ok(Some(PlanResult {
        plan: Plan::new(vec![primitive]),
        explanation,
    }))
}

fn try_rename_field(
    raw: &str,
    lower: &str,
    schema: &Schema,
    _context: Option<&ContextConfig>,
) -> Result<Option<PlanResult>, PlanError> {
    let Some(rest) = lower.strip_prefix("rename ") else {
        return Ok(None);
    };
    // `rename model …` and `rename <X> to <Y>` (no "in <model>") fall through
    if rest.starts_with("model ") {
        return Ok(None);
    }
    if !rest.contains(" in ") {
        return Ok(None);
    }
    let after = slice_original(raw, "rename ").unwrap_or(raw);
    let Some((lhs, model_hint)) = split_on_keyword(after, &[" in "]) else {
        return Ok(None);
    };
    let Some((from, to)) = split_on_keyword(lhs, &[" to ", " -> "]) else {
        return Err(PlanError::InvalidIntent(format!(
            "`rename <field> to <new> in <model>` expected: got {raw:?}"
        )));
    };
    let model = resolve_model(schema, model_hint)?;
    if model.core {
        return Err(PlanError::CoreModelProtected(model.name.clone()));
    }
    let from_name = sanitise_identifier(from);
    let to_name = sanitise_identifier(to);
    if from_name.is_empty() || to_name.is_empty() {
        return Err(PlanError::InvalidIntent(
            "rename clause is missing a name on one side".to_string(),
        ));
    }
    if !model.fields.iter().any(|f| f.name == from_name) {
        return Err(PlanError::FieldDoesNotExist {
            model: model.name.clone(),
            field: from_name,
        });
    }
    if model.fields.iter().any(|f| f.name == to_name) {
        return Err(PlanError::FieldAlreadyExists {
            model: model.name.clone(),
            field: to_name,
        });
    }
    let primitive = Primitive::RenameField(RenameField {
        model: model.name.clone(),
        from: from_name.clone(),
        to: to_name.clone(),
    });
    validate_primitive(&primitive).map_err(PlanError::Validation)?;
    let explanation = format!(
        "Renames field `{from_name}` to `{to_name}` on model `{model}` \
         (data-preserving — the underlying column is renamed, not dropped).",
        model = model.name,
    );
    Ok(Some(PlanResult {
        plan: Plan::new(vec![primitive]),
        explanation,
    }))
}

fn try_rename_model(
    raw: &str,
    lower: &str,
    schema: &Schema,
    _context: Option<&ContextConfig>,
) -> Result<Option<PlanResult>, PlanError> {
    let prefix = if let Some(r) = lower.strip_prefix("rename model ") {
        r
    } else {
        return Ok(None);
    };
    let after = slice_original(raw, "rename model ").unwrap_or(prefix);
    let Some((from, to)) = split_on_keyword(after, &[" to ", " -> "]) else {
        return Err(PlanError::InvalidIntent(format!(
            "`rename model <from> to <to>` expected: got {raw:?}"
        )));
    };
    let model = resolve_model(schema, from)?;
    if model.core {
        return Err(PlanError::CoreModelProtected(model.name.clone()));
    }
    let to_name = pascalise(to.trim());
    if to_name.is_empty() {
        return Err(PlanError::InvalidIntent(
            "new model name is empty".to_string(),
        ));
    }
    if schema.models.iter().any(|m| m.name == to_name) {
        return Err(PlanError::InvalidIntent(format!(
            "a model named `{to_name}` already exists"
        )));
    }
    let from_name = model.name.clone();
    let primitive = Primitive::RenameModel(RenameModel {
        from: from_name.clone(),
        to: to_name.clone(),
    });
    validate_primitive(&primitive).map_err(PlanError::Validation)?;
    let explanation = format!(
        "Renames model `{from_name}` to `{to_name}`. Table is renamed in \
         place — existing rows are preserved."
    );
    Ok(Some(PlanResult {
        plan: Plan::new(vec![primitive]),
        explanation,
    }))
}

fn try_remove_field(
    raw: &str,
    lower: &str,
    schema: &Schema,
    _context: Option<&ContextConfig>,
) -> Result<Option<PlanResult>, PlanError> {
    let (prefix, original_prefix) = if lower.starts_with("remove ") {
        ("remove ", "remove ")
    } else if lower.starts_with("drop ") {
        ("drop ", "drop ")
    } else if lower.starts_with("delete ") {
        ("delete ", "delete ")
    } else {
        return Ok(None);
    };
    // `remove model …` is never AI-safe in 0.5.0 — refuse loudly.
    let body = &lower[prefix.len()..];
    if body.starts_with("model ") {
        return Err(PlanError::DeveloperOnlyRequested("remove_model"));
    }
    let after = slice_original(raw, original_prefix).unwrap_or(raw);
    let Some((field_phrase, model_hint)) = split_on_keyword(after, &[" from ", " on "]) else {
        return Err(PlanError::InvalidIntent(format!(
            "`{prefix}<field> from <model>` expected: got {raw:?}",
            prefix = prefix.trim_end(),
        )));
    };
    let model = resolve_model(schema, model_hint)?;
    if model.core {
        return Err(PlanError::CoreModelProtected(model.name.clone()));
    }
    let (field_name, _) = parse_field_phrase(field_phrase);
    if !model.fields.iter().any(|f| f.name == field_name) {
        return Err(PlanError::FieldDoesNotExist {
            model: model.name.clone(),
            field: field_name,
        });
    }
    let primitive = Primitive::RemoveField(RemoveField {
        model: model.name.clone(),
        field: field_name.clone(),
    });
    validate_primitive(&primitive).map_err(PlanError::Validation)?;
    let explanation = format!(
        "Removes field `{field_name}` from model `{model}`. The underlying column \
         is dropped; review data before applying.",
        model = model.name,
    );
    Ok(Some(PlanResult {
        plan: Plan::new(vec![primitive]),
        explanation,
    }))
}

fn try_change_type(
    raw: &str,
    lower: &str,
    schema: &Schema,
    _context: Option<&ContextConfig>,
) -> Result<Option<PlanResult>, PlanError> {
    let Some(rest) = lower.strip_prefix("change ") else {
        return Ok(None);
    };
    let after = slice_original(raw, "change ").unwrap_or(rest);
    // "change <field> in <model> to <type>"
    let Some((lhs, new_type_hint)) = split_on_keyword(after, &[" to "]) else {
        return Ok(None);
    };
    let Some((field_phrase, model_hint)) = split_on_keyword(lhs, &[" in ", " on "]) else {
        return Ok(None);
    };
    let model = resolve_model(schema, model_hint)?;
    if model.core {
        return Err(PlanError::CoreModelProtected(model.name.clone()));
    }
    let (field_name, _) = parse_field_phrase(field_phrase);
    if !model.fields.iter().any(|f| f.name == field_name) {
        return Err(PlanError::FieldDoesNotExist {
            model: model.name.clone(),
            field: field_name,
        });
    }
    let ty = normalise_type_hint(new_type_hint.trim())?;
    let primitive = Primitive::ChangeFieldType(ChangeFieldType {
        model: model.name.clone(),
        field: field_name.clone(),
        new_type: ty.clone(),
    });
    validate_primitive(&primitive).map_err(PlanError::Validation)?;
    let explanation = format!(
        "Changes type of `{model}.{field_name}` to `{ty}`. The executor (0.5.x) \
         will refuse lossy conversions at apply time.",
        model = model.name,
    );
    Ok(Some(PlanResult {
        plan: Plan::new(vec![primitive]),
        explanation,
    }))
}

fn try_change_nullability(
    raw: &str,
    lower: &str,
    schema: &Schema,
    _context: Option<&ContextConfig>,
) -> Result<Option<PlanResult>, PlanError> {
    let Some(rest) = lower.strip_prefix("make ") else {
        return Ok(None);
    };
    let after = slice_original(raw, "make ").unwrap_or(rest);
    let Some((field_phrase, rest_phrase)) = split_on_keyword(after, &[" in ", " on "]) else {
        return Ok(None);
    };
    let rest_lower = rest_phrase.to_lowercase();
    let mut nullable_hint: Option<bool> = None;
    for (needle, target) in [
        (" optional", true),
        (" nullable", true),
        (" required", false),
        (" not null", false),
        (" non-null", false),
    ] {
        if rest_lower.contains(needle) {
            nullable_hint = Some(target);
            break;
        }
    }
    // Trailing word like "optional" with no " in " — skip, falls back
    // elsewhere. We only proceed when we saw the directive.
    let Some(nullable) = nullable_hint else {
        return Ok(None);
    };
    let model_hint = strip_known_modifiers(rest_phrase);
    let model = resolve_model(schema, &model_hint)?;
    if model.core {
        return Err(PlanError::CoreModelProtected(model.name.clone()));
    }
    let (field_name, _) = parse_field_phrase(field_phrase);
    if !model.fields.iter().any(|f| f.name == field_name) {
        return Err(PlanError::FieldDoesNotExist {
            model: model.name.clone(),
            field: field_name,
        });
    }
    let primitive = Primitive::ChangeFieldNullability(ChangeFieldNullability {
        model: model.name.clone(),
        field: field_name.clone(),
        nullable,
    });
    validate_primitive(&primitive).map_err(PlanError::Validation)?;
    let explanation = format!(
        "Marks `{model}.{field_name}` as {state}.",
        model = model.name,
        state = if nullable { "nullable" } else { "required" },
    );
    Ok(Some(PlanResult {
        plan: Plan::new(vec![primitive]),
        explanation,
    }))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Case-insensitive model lookup. Matches against every user-visible
/// identifier on the model (`name`, `table`, `admin_name`,
/// `singular_name`) plus a tiny pluralise/depluralise pair so users
/// can say "tasks" or "task" interchangeably.
fn resolve_model<'a>(schema: &'a Schema, hint: &str) -> Result<&'a SchemaModel, PlanError> {
    let h = sanitise_identifier(hint).to_lowercase();
    if h.is_empty() {
        return Err(PlanError::UnknownModel {
            hint: hint.trim().to_string(),
        });
    }
    let h_singular = depluralise(&h);
    let h_plural = pluralise(&h);
    let mut matches: Vec<&SchemaModel> = schema
        .models
        .iter()
        .filter(|m| {
            let forms = [
                m.name.to_lowercase(),
                m.table.to_lowercase(),
                m.admin_name.to_lowercase(),
                m.singular_name.to_lowercase(),
            ];
            forms
                .iter()
                .any(|f| f == &h || f == &h_singular || f == &h_plural)
        })
        .collect();
    // Deduplicate in case the same model matched multiple aliases.
    matches.dedup_by(|a, b| a.name == b.name);
    match matches.len() {
        0 => Err(PlanError::UnknownModel {
            hint: hint.trim().to_string(),
        }),
        1 => Ok(matches[0]),
        _ => Err(PlanError::AmbiguousModel {
            hint: hint.trim().to_string(),
            candidates: matches.iter().map(|m| m.name.clone()).collect(),
        }),
    }
}

/// Extract a field name from a phrase like "a priority", "due date",
/// or "optional phone (as String)". Returns `(field_name, remainder)`
/// where the remainder is the original phrase — the caller can inspect
/// it for modifier words like "optional" or a trailing `as <type>`.
fn parse_field_phrase(phrase: &str) -> (String, String) {
    let remainder = phrase.to_string();
    let stripped = strip_known_modifiers(phrase);
    let no_as = match split_on_keyword(&stripped, &[" as ", ":"]) {
        Some((left, _)) => left.to_string(),
        None => stripped,
    };
    // "due date" → "due_date"; reject non-ident chars.
    let name = no_as
        .split_whitespace()
        .map(|w| {
            w.trim_matches(|c: char| !c.is_alphanumeric() && c != '_')
                .to_lowercase()
        })
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join("_");
    (name, remainder)
}

/// Drop articles and common modifier words that can't be part of an
/// identifier.
fn strip_known_modifiers(phrase: &str) -> String {
    let tokens = phrase.split_whitespace().filter(|t| {
        !matches!(
            t.to_lowercase().as_str(),
            "a" | "an"
                | "the"
                | "optional"
                | "nullable"
                | "required"
                | "new"
                | "field"
                | "column"
                | "to"
                | "add"
        )
    });
    tokens.collect::<Vec<_>>().join(" ")
}

fn phrase_is_optional(phrase: &str) -> bool {
    let l = phrase.to_lowercase();
    l.split_whitespace()
        .any(|w| w == "optional" || w == "nullable")
}

/// Infer `(type, nullable)` from a field name + surrounding phrase.
///
/// Resolution order:
///   1. Explicit `as <type>` in the phrase wins.
///   2. Context-aware names (Swedish personnummer under country=SE).
///   3. Heuristics from the identifier (prefix/suffix).
///   4. Fallback: `String`, non-nullable.
fn infer_field_type(
    name: &str,
    phrase: &str,
    context: Option<&ContextConfig>,
) -> Result<(String, bool), PlanError> {
    let lower = phrase.to_lowercase();
    // 1. Explicit type
    if let Some((_, after)) = split_on_keyword(phrase, &[" as ", ":"]) {
        let ty = normalise_type_hint(after.trim_end_matches('.').trim())?;
        return Ok((ty, phrase_is_optional(phrase)));
    }
    // Words like "datetime" or "number" anywhere in the phrase.
    for (needle, mapped) in [
        ("datetime", "DateTime"),
        ("timestamp", "DateTime"),
        ("boolean", "bool"),
        ("integer", "i32"),
        ("number", "i32"),
        ("string", "String"),
        ("text", "String"),
    ] {
        if lower.split_whitespace().any(|w| w == needle) {
            return Ok((mapped.to_string(), phrase_is_optional(phrase)));
        }
    }

    // 2. Context — country and industry rules. The earlier an arm
    // matches, the higher its priority. Order: country → industry →
    // fallbacks.
    if let Some(ctx) = context {
        let n = name.to_lowercase();
        // Country rules.
        if matches!(ctx.country.as_deref(), Some(cc) if cc.eq_ignore_ascii_case("SE"))
            && (n == "personnummer" || n == "personal_number" || n == "personal_id" || n == "pnr")
        {
            return Ok(("String".to_string(), false));
        }
        if matches!(ctx.country.as_deref(), Some(cc) if cc.eq_ignore_ascii_case("NO"))
            && (n == "fodselsnummer" || n == "personal_number" || n == "personal_id")
        {
            return Ok(("String".to_string(), false));
        }
        // Industry rules.
        match ctx.industry.as_deref() {
            Some(i) if i.eq_ignore_ascii_case("healthcare") => {
                // Patient IDs must be opaque strings — sequential
                // integers leak enrolment order and are refused by
                // the planner under this industry.
                if n == "patient_id"
                    || n == "patient"
                    || n.ends_with("_patient_id")
                    || n == "medical_record_number"
                    || n == "mrn"
                {
                    return Ok(("String".to_string(), false));
                }
            }
            Some(i) if i.eq_ignore_ascii_case("banking") => {
                // Account numbers must be String (international formats
                // overflow i32). Monetary amounts are stored as i64
                // minor units.
                if n == "account_number" || n == "iban" || n == "bic" {
                    return Ok(("String".to_string(), false));
                }
                if n == "balance"
                    || n == "amount"
                    || n.ends_with("_amount")
                    || n.ends_with("_balance")
                {
                    return Ok(("i64".to_string(), phrase_is_optional(phrase)));
                }
            }
            _ => {}
        }
    }

    // 3. Identifier heuristics
    let n = name.to_lowercase();
    let nullable = phrase_is_optional(phrase);
    if n.ends_with("_at")
        || n.ends_with("_on")
        || n.ends_with("_date")
        || n == "created_at"
        || n == "updated_at"
        || n == "deleted_at"
        || n.ends_with("_time")
        || n == "timestamp"
    {
        return Ok(("DateTime".to_string(), nullable));
    }
    if n.starts_with("is_")
        || n.starts_with("has_")
        || n == "active"
        || n == "enabled"
        || n == "archived"
    {
        return Ok(("bool".to_string(), nullable));
    }
    if n == "priority"
        || n == "count"
        || n == "score"
        || n == "rank"
        || n == "quantity"
        || n == "age"
        || n.ends_with("_count")
        || n.ends_with("_id")
    {
        return Ok(("i32".to_string(), nullable));
    }
    // Monetary names resolve to `i64` — we store amounts in minor
    // units (öre / cents) where `i32` can overflow for anything
    // above ~21 million units. This rule runs whether or not the
    // banking industry context is active; the banking context arm
    // already short-circuits for balance/amount above, so this is
    // the generic fallback for `annual_income`, `total_price`, etc.
    if n == "price"
        || n == "balance"
        || n == "amount"
        || n.ends_with("_income")
        || n.ends_with("_amount")
        || n.ends_with("_total")
        || n.ends_with("_price")
    {
        return Ok(("i64".to_string(), nullable));
    }
    // Fallback
    Ok(("String".to_string(), nullable))
}

/// Map a user-facing type word onto one of [`VALID_TYPE_NAMES`].
fn normalise_type_hint(raw: &str) -> Result<String, PlanError> {
    let r = raw.trim().trim_matches('`').to_lowercase();
    let r = r.trim_start_matches("type ").trim();
    match r {
        "i32" | "int" | "integer" | "number" | "int32" => Ok("i32".to_string()),
        "i64" | "long" | "bigint" | "int64" => Ok("i64".to_string()),
        "string" | "text" | "str" | "varchar" => Ok("String".to_string()),
        "bool" | "boolean" | "flag" => Ok("bool".to_string()),
        "datetime" | "timestamp" | "date" | "time" | "datetime<utc>" => Ok("DateTime".to_string()),
        _ => Err(PlanError::UnknownType(raw.to_string())),
    }
}

/// Find the *original-case* substring after `prefix` so we preserve
/// user casing on identifiers. Works because `prefix` was matched
/// case-insensitively and starts at byte 0 of `raw`.
fn slice_original<'a>(raw: &'a str, prefix_lower: &str) -> Option<&'a str> {
    if raw.len() < prefix_lower.len() {
        return None;
    }
    let head = &raw[..prefix_lower.len()];
    if head.to_lowercase() != prefix_lower {
        return None;
    }
    Some(&raw[prefix_lower.len()..])
}

/// Case-insensitive split on the first occurrence of any of the given
/// keywords. Returns the left/right halves in the **original** casing.
fn split_on_keyword<'a>(raw: &'a str, keywords: &[&str]) -> Option<(&'a str, &'a str)> {
    let lower = raw.to_lowercase();
    let mut best: Option<(usize, usize)> = None;
    for kw in keywords {
        if let Some(idx) = lower.find(kw) {
            match best {
                Some((best_idx, _)) if best_idx <= idx => {}
                _ => best = Some((idx, kw.len())),
            }
        }
    }
    let (idx, kw_len) = best?;
    let left = raw[..idx].trim();
    let right = raw[idx + kw_len..].trim();
    Some((left, right))
}

fn sanitise_identifier(raw: &str) -> String {
    raw.trim()
        .trim_matches(|c: char| c == '`' || c == '"' || c == '\'' || c == '.' || c == ',')
        .to_string()
}

fn pluralise(name: &str) -> String {
    if name.ends_with('s') {
        return name.to_string();
    }
    if name.ends_with('y') && name.len() > 1 {
        let mut out = String::from(&name[..name.len() - 1]);
        out.push_str("ies");
        return out;
    }
    format!("{name}s")
}

fn depluralise(name: &str) -> String {
    if let Some(stripped) = name.strip_suffix("ies") {
        let mut out = String::from(stripped);
        out.push('y');
        return out;
    }
    if let Some(stripped) = name.strip_suffix('s') {
        return stripped.to_string();
    }
    name.to_string()
}

/// Convert a model hint to PascalCase, e.g. "invoice_line" → `InvoiceLine`.
fn pascalise(raw: &str) -> String {
    let mut out = String::new();
    let mut next_upper = true;
    for ch in raw.chars() {
        if ch == '_' || ch == '-' || ch.is_whitespace() {
            next_upper = true;
            continue;
        }
        if !ch.is_alphanumeric() {
            continue;
        }
        if next_upper {
            out.extend(ch.to_uppercase());
            next_upper = false;
        } else {
            out.extend(ch.to_lowercase());
        }
    }
    out
}

fn explain_add_field(
    model: &str,
    field: &str,
    ty: &str,
    nullable: bool,
    context: Option<&ContextConfig>,
) -> String {
    let opt = if nullable { ", nullable" } else { "" };
    let head = format!("Adds field `{field}` ({ty}{opt}) to model `{model}`.");
    let rationale = match (ty, field) {
        ("DateTime", _) => {
            " Stored as ISO-8601 UTC; the admin renders it as a datetime-local input."
        }
        ("bool", _) => " Rendered as a checkbox in the admin and a pill on list pages.",
        ("i32", f) if f == "priority" || f == "score" || f == "rank" => {
            " Useful for sorting and filtering records by importance."
        }
        ("i32", _) => " Numeric — the list view shows it with tabular numerics.",
        ("String", "status") => " Status values get coloured pills in list views.",
        _ => "",
    };
    let mut tail = String::new();
    if let Some(ctx) = context {
        // Country-specific annotations. Matches any of the SE personal-
        // id aliases the planner maps to `String` so the explanation
        // lines up with what was actually inferred.
        if matches!(ctx.country.as_deref(), Some(cc) if cc.eq_ignore_ascii_case("SE"))
            && matches!(
                field,
                "personnummer" | "personal_id" | "personal_number" | "pnr"
            )
        {
            tail.push_str(
                " Swedish personnummer is stored as a 13-character string (YYYYMMDD-XXXX).",
            );
        }
        // Industry-specific annotations.
        match ctx.industry.as_deref() {
            Some(i)
                if i.eq_ignore_ascii_case("healthcare")
                    && (field == "patient_id"
                        || field == "mrn"
                        || field == "medical_record_number") =>
            {
                tail.push_str(
                    " Patient identifiers are opaque strings (UUID or hash); sequential integers would leak enrolment order.",
                );
            }
            Some(i)
                if i.eq_ignore_ascii_case("banking")
                    && (field == "balance" || field == "amount" || field.ends_with("_amount")) =>
            {
                tail.push_str(
                    " Monetary values are stored as integer minor units (öre, cents). Never use floats.",
                );
            }
            _ => {}
        }
        // GDPR guardrail.
        if ctx.requires_gdpr() && is_generic_pii_field(field) {
            tail.push_str(
                " Under GDPR this field is personal data — retention and right-to-erasure rules apply.",
            );
        }
    }
    format!("{head}{rationale}{tail}")
}

fn is_generic_pii_field(name: &str) -> bool {
    matches!(
        name,
        "email" | "phone" | "address" | "date_of_birth" | "ssn" | "personnummer" | "fodselsnummer"
    )
}

fn supported_forms_message(raw: &str) -> String {
    format!(
        "could not interpret prompt {raw:?}. Supported forms:\n  \
         - add <field> to <model>\n  \
         - add <field> as <type> to <model>\n  \
         - add optional <field> to <model>\n  \
         - rename <field> to <new> in <model>\n  \
         - rename model <from> to <to>\n  \
         - remove <field> from <model>\n  \
         - change <field> in <model> to <type>\n  \
         - make <field> in <model> optional|required"
    )
}

// ---------------------------------------------------------------------------
// CLI-facing JSON rendering
// ---------------------------------------------------------------------------

/// Render a plan as the strict JSON shape documented for
/// `rustio ai plan`: `[{ "op": "AddField", "model": "Task", "field":
/// "priority", "type": "i32", "nullable": false }, …]`.
///
/// This is **not** the internal `Plan` serde shape (which is tagged
/// `op` = snake_case and uses `name` for the field); the CLI wants a
/// PascalCase, flat-field shape that's explicitly stable across the
/// planner vocabulary. Keeping the renderer here means the internal
/// representation can evolve without breaking the documented output.
pub fn render_plan_json(plan: &Plan, explanation: &str) -> String {
    let steps: Vec<serde_json::Value> = plan.steps.iter().map(primitive_to_cli_json).collect();
    let out = serde_json::json!({
        "plan": steps,
        "explanation": explanation,
    });
    serde_json::to_string_pretty(&out).unwrap_or_else(|_| "{}".to_string())
}

/// Render a plan as a compact, Django-ish "Plan: …" summary for terminal
/// display alongside the JSON.
pub fn render_plan_human(plan: &Plan, explanation: &str) -> String {
    let mut out = String::from("Plan:\n");
    if plan.steps.is_empty() {
        out.push_str("  (no changes)\n");
    }
    for step in &plan.steps {
        out.push_str("  - ");
        out.push_str(&summarise_primitive(step));
        out.push('\n');
    }
    out.push_str("\nExplanation:\n");
    out.push_str(explanation);
    if !explanation.ends_with('\n') {
        out.push('\n');
    }
    out
}

fn primitive_to_cli_json(p: &Primitive) -> serde_json::Value {
    use serde_json::json;
    match p {
        Primitive::AddField(a) => json!({
            "op": "AddField",
            "model": a.model,
            "field": a.field.name,
            "type": a.field.ty,
            "nullable": a.field.nullable,
        }),
        Primitive::RemoveField(r) => json!({
            "op": "RemoveField",
            "model": r.model,
            "field": r.field,
        }),
        Primitive::RenameField(r) => json!({
            "op": "RenameField",
            "model": r.model,
            "from": r.from,
            "to": r.to,
        }),
        Primitive::RenameModel(r) => json!({
            "op": "RenameModel",
            "from": r.from,
            "to": r.to,
        }),
        Primitive::ChangeFieldType(c) => json!({
            "op": "ChangeFieldType",
            "model": c.model,
            "field": c.field,
            "type": c.new_type,
        }),
        Primitive::ChangeFieldNullability(c) => json!({
            "op": "ChangeFieldNullability",
            "model": c.model,
            "field": c.field,
            "nullable": c.nullable,
        }),
        Primitive::AddModel(m) => json!({
            "op": "AddModel",
            "name": m.name,
            "table": m.table,
            "fields": m.fields.iter().map(|f| json!({
                "name": f.name,
                "type": f.ty,
                "nullable": f.nullable,
            })).collect::<Vec<_>>(),
        }),
        Primitive::RemoveModel(m) => json!({
            "op": "RemoveModel",
            "name": m.name,
        }),
        Primitive::AddRelation(r) => json!({
            "op": "AddRelation",
            "from": r.from,
            "kind": format!("{:?}", r.kind).to_lowercase(),
            "to": r.to,
            "via": r.via,
        }),
        Primitive::RemoveRelation(r) => json!({
            "op": "RemoveRelation",
            "from": r.from,
            "via": r.via,
        }),
        Primitive::UpdateAdmin(u) => json!({
            "op": "UpdateAdmin",
            "model": u.model,
            "field": u.field,
            "attr": u.attr,
            "value": u.value,
        }),
        Primitive::CreateMigration(_) => {
            // Developer-only; Plan::validate rejects it before we get
            // here, but render the shape for symmetry.
            json!({"op": "CreateMigration", "note": "developer-only"})
        }
    }
}

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
            "Add model \"{}\" with {} field{}",
            m.name,
            m.fields.len(),
            if m.fields.len() == 1 { "" } else { "s" }
        ),
        Primitive::RemoveModel(m) => format!("Remove model \"{}\"", m.name),
        Primitive::AddRelation(r) => {
            format!("Add relation {:?}: {}.{} → {}", r.kind, r.from, r.via, r.to)
        }
        Primitive::RemoveRelation(r) => format!("Remove relation {}.{}", r.from, r.via),
        Primitive::UpdateAdmin(u) => {
            format!("Update admin attr \"{}.{}\".{}", u.model, u.field, u.attr)
        }
        Primitive::CreateMigration(m) => format!("[dev-only] create_migration \"{}\"", m.name),
    }
}
