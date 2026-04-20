//! Admin Intelligence Layer — 0.7.0.
//!
//! Pure helpers that turn *schema + context* into *user-facing hints*:
//! the form-field label beside a `personnummer` input, the masked
//! display of a sensitive value on a list page, the filter dropdown
//! inferred from a `status` column, the "Interpreted as ID" badge on a
//! numeric search. Nothing in this module touches the filesystem, the
//! database, or produces HTML — it returns structured data that the
//! admin renderer consumes.
//!
//! ## Principles
//!
//! - **Inference, not configuration.** Rules are derived from
//!   `(field name, field type, nullability) + ContextConfig`. No
//!   per-project hooks.
//! - **Conservative sensitivity.** Under GDPR / country rules, the
//!   layer marks a field as sensitive *up*, never down. A project
//!   without context gets 0.6.x behaviour.
//! - **Deterministic.** Same inputs → same outputs. No ordering
//!   surprises, no random masking length.
//!
//! ## Public API
//!
//! - [`classify_field`] — labels a field by role (`Id`, `Email`,
//!   `Personnummer`, …). Every downstream renderer branches on
//!   this enum.
//! - [`field_ui_metadata`] — packages the label, placeholder, hint,
//!   and sensitivity marker a form needs to render one input.
//! - [`infer_filters`] — walks a model's fields and decides which
//!   filters make sense on its list page.
//! - [`classify_search`] — inspects a search query and tells the
//!   list handler what the user probably meant (`NumericId`, `Email`,
//!   `Personnummer`, `Text`).
//! - [`mask_pii`] — deterministic string masker used to hide
//!   personal data by default on list views.

use std::sync::OnceLock;

use crate::admin::{AdminField, FieldType};
use crate::ai::ContextConfig;

/// Process-global cache for the project's `rustio.context.json`.
///
/// Loaded lazily on first access and held for the life of the
/// process — the admin runs as a long-lived server and the context
/// file is static between restarts. `None` means either the file
/// isn't present or it couldn't be parsed.
///
/// Pattern mirrors [`crate::admin::design::Design::global`]; the two
/// artefacts are read once and shared across every render.
pub fn context_global() -> Option<&'static ContextConfig> {
    static INSTANCE: OnceLock<Option<ContextConfig>> = OnceLock::new();
    INSTANCE
        .get_or_init(|| {
            let raw = std::fs::read_to_string("rustio.context.json").ok()?;
            ContextConfig::parse(&raw).ok()
        })
        .as_ref()
}

/// The role a field plays in the admin UI. One field maps to exactly
/// one role; the ordering of branches in [`classify_field`] resolves
/// overlaps (e.g. an `email` column is `FieldRole::Email`, not
/// `FieldRole::PlainText`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldRole {
    /// Primary key. Rendered monospace, excluded from edit forms.
    Id,
    /// `DateTime<Utc>`-shaped columns.
    Timestamp,
    /// Booleans — rendered as a pill on the list page, a checkbox in
    /// forms.
    Bool,
    /// Numeric values that aren't identifiers — priorities, scores,
    /// counts. Rendered with tabular numerics on the list page.
    NumericCount,
    /// `<something>_id` column that points at another model. Rendered
    /// monospace, filter is a relation dropdown (deferred).
    ForeignKey,
    /// A `status` / `*_status` column. Renders as a coloured pill and
    /// becomes a dropdown filter.
    Status,
    /// A Swedish personal identity number under `country=SE`.
    Personnummer,
    /// An email address under GDPR. Masked by default on list views.
    Email,
    /// A phone number under GDPR. Masked by default.
    Phone,
    /// An opaque healthcare identifier (`patient_id`, `mrn`, ...)
    /// under `industry=healthcare`.
    OpaqueIdentifier,
    /// A monetary amount under `industry=banking`. Stored as integer
    /// minor units.
    Money,
    /// Everything else. Default role; triggers the plain-text input.
    PlainText,
}

impl FieldRole {
    /// `true` when the role carries personal / sensitive data and
    /// should be masked by default on list views.
    pub fn is_sensitive(self) -> bool {
        matches!(
            self,
            FieldRole::Personnummer
                | FieldRole::Email
                | FieldRole::Phone
                | FieldRole::OpaqueIdentifier
        )
    }
}

/// Everything a form / list renderer needs to present one field to a
/// human. All strings are plain text (no HTML) — the caller escapes
/// before emitting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldUI {
    pub role: FieldRole,
    pub label: String,
    pub placeholder: Option<String>,
    pub hint: Option<String>,
    /// `true` when the field should carry the lock marker and (for
    /// list views) be masked by default.
    pub sensitive: bool,
    /// One-line explanation of *why* the field is sensitive — shown
    /// next to the lock marker or in a tooltip.
    pub sensitivity_note: Option<String>,
    /// 0.8.0 — set when the field is a FK to a known model. Carries
    /// the *singular* display name of the target (e.g. `"Applicant"`)
    /// so list views can render "Applicant #42" and forms can hint
    /// "Foreign key to Applicant". `None` for every field that isn't
    /// a modelled relation — callers must not invent a label from the
    /// column name alone.
    pub relation_label: Option<String>,
}

/// What shape of filter the admin list page should render for a given
/// field. Each variant maps to a concrete HTML control.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FilterKind {
    /// `<select>` over distinct string values, filled at render time.
    DropdownText,
    /// Yes / No dropdown over a boolean column.
    BoolYesNo,
    /// Two `date` / `datetime-local` inputs bounding a range.
    DateRange,
    /// Numeric exact-match input (integer).
    NumericExact,
    /// Single-line input, compared exactly. Used for identity numbers
    /// where substring is the wrong semantics.
    ExactMatch,
    /// 0.8.0 — `<select>` populated by the admin runtime from rows of
    /// the target model. Rendered as "Applicant (42)" / "Applicant
    /// (43)" etc. The `target_model` carries the *singular* display
    /// name so the handler knows which table to read.
    RelationSelect { target_model: String },
}

/// One filter the list page should show for a model. Produced by
/// [`infer_filters`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilterDef {
    pub field: String,
    pub label: String,
    pub kind: FilterKind,
}

/// What the user *probably* typed into the list-page search box.
/// Letting the handler branch on this gives cleaner narrow-match
/// behaviour than "grep every String field".
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SearchIntent {
    /// Parsed as a non-negative integer — likely an ID lookup.
    NumericId(i64),
    /// Contains `@` and `.` in plausible positions — email search.
    Email(String),
    /// Matches the 12/13-character Swedish personnummer shape.
    Personnummer(String),
    /// 0.8.0 — an FK field is being searched by target id. Emitted
    /// only by [`classify_search_for_field`] when the caller supplies
    /// a relation target; plain `classify_search` never produces it.
    RelationId { model: String, id: i64 },
    /// Everything else, including empty string.
    Text(String),
}

impl SearchIntent {
    /// Stable short label for the CLI / UI badge.
    pub fn label(&self) -> &'static str {
        match self {
            SearchIntent::NumericId(_) => "ID",
            SearchIntent::Email(_) => "email",
            SearchIntent::Personnummer(_) => "personnummer",
            SearchIntent::RelationId { .. } => "relation",
            SearchIntent::Text(_) => "text",
        }
    }
}

// ---------------------------------------------------------------------------
// classify_field
// ---------------------------------------------------------------------------

/// Assign a [`FieldRole`] to one field, taking context into account.
///
/// Order of precedence (highest first):
///
/// 1. Country-scoped PII names (`personnummer` under `SE`).
/// 2. Industry-scoped opaque identifiers (`patient_id` under
///    `healthcare`, `balance` under `banking`).
/// 3. GDPR-scoped generics (`email`, `phone`).
/// 4. Shape: `id`, `*_id`, `status`, bool, datetime, numeric.
/// 5. Fallback: `PlainText`.
pub fn classify_field(f: &AdminField, context: Option<&ContextConfig>) -> FieldRole {
    let name = f.name;
    if let Some(ctx) = context {
        if matches!(ctx.country.as_deref(), Some(cc) if cc.eq_ignore_ascii_case("SE"))
            && matches!(
                name,
                "personnummer" | "personal_id" | "personal_number" | "pnr"
            )
        {
            return FieldRole::Personnummer;
        }
        if matches!(ctx.country.as_deref(), Some(cc) if cc.eq_ignore_ascii_case("NO"))
            && matches!(name, "fodselsnummer" | "personal_number")
        {
            return FieldRole::Personnummer;
        }
        if matches!(ctx.industry.as_deref(), Some(i) if i.eq_ignore_ascii_case("healthcare"))
            && matches!(name, "patient_id" | "mrn" | "medical_record_number")
        {
            return FieldRole::OpaqueIdentifier;
        }
        if matches!(ctx.industry.as_deref(), Some(i) if i.eq_ignore_ascii_case("banking"))
            && (name == "balance" || name == "amount" || name.ends_with("_amount"))
        {
            return FieldRole::Money;
        }
        if ctx.requires_gdpr() {
            if name == "email" {
                return FieldRole::Email;
            }
            if name == "phone" {
                return FieldRole::Phone;
            }
        }
    }

    // Shape-only fallbacks (no context needed).
    if name == "id" {
        return FieldRole::Id;
    }
    if name == "email" {
        return FieldRole::Email;
    }
    if name == "phone" {
        return FieldRole::Phone;
    }
    if matches!(f.ty, FieldType::Bool) {
        return FieldRole::Bool;
    }
    if matches!(f.ty, FieldType::DateTime) {
        return FieldRole::Timestamp;
    }
    if name == "status" || name.ends_with("_status") {
        return FieldRole::Status;
    }
    if name.ends_with("_id") {
        return FieldRole::ForeignKey;
    }
    if matches!(f.ty, FieldType::I32 | FieldType::I64) {
        return FieldRole::NumericCount;
    }
    FieldRole::PlainText
}

// ---------------------------------------------------------------------------
// field_ui_metadata
// ---------------------------------------------------------------------------

/// Package a field's display metadata for the admin form / list
/// renderers. All strings are plain text — escape before emitting.
pub fn field_ui_metadata(f: &AdminField, context: Option<&ContextConfig>) -> FieldUI {
    let role = classify_field(f, context);
    let label = humanise(f.name);
    let mut placeholder: Option<String> = None;
    let mut hint: Option<String> = None;
    let mut sensitive = false;
    let mut sensitivity_note: Option<String> = None;

    match role {
        FieldRole::Personnummer => {
            placeholder = Some("YYYYMMDD-XXXX".into());
            hint = Some("Swedish personal identity number.".into());
            sensitive = true;
            sensitivity_note = Some("Sensitive personal data (GDPR).".into());
        }
        FieldRole::Email => {
            placeholder = Some("name@example.com".into());
            if context.is_some_and(|c| c.requires_gdpr()) {
                sensitive = true;
                sensitivity_note = Some("Personal data (GDPR).".into());
            }
        }
        FieldRole::Phone => {
            placeholder = Some("+46 70 123 45 67".into());
            if context.is_some_and(|c| c.requires_gdpr()) {
                sensitive = true;
                sensitivity_note = Some("Personal data (GDPR).".into());
            }
        }
        FieldRole::OpaqueIdentifier => {
            hint = Some("Opaque identifier — do not expose publicly.".into());
            sensitive = true;
            sensitivity_note = Some("Clinical identifier.".into());
        }
        FieldRole::Money => {
            hint = Some("Integer minor units (öre, cents). Never use floats.".into());
        }
        FieldRole::Timestamp => {
            placeholder = Some("YYYY-MM-DDTHH:MM".into());
            hint = Some("Interpreted as UTC.".into());
        }
        FieldRole::Status => {
            hint = Some("Short status label (e.g. active, pending, resolved).".into());
        }
        FieldRole::ForeignKey => {
            hint = Some("Foreign-key id — must reference an existing row.".into());
        }
        FieldRole::Id | FieldRole::Bool | FieldRole::NumericCount | FieldRole::PlainText => {}
    }

    FieldUI {
        role,
        label,
        placeholder,
        hint,
        sensitive,
        sensitivity_note,
        relation_label: None,
    }
}

/// 0.8.0 — like [`field_ui_metadata`] but relation-aware. Pass the
/// singular display name of the target model (e.g. `"Applicant"`) when
/// the schema records a relation for this field; the returned
/// [`FieldUI`] then carries `relation_label` and a form hint of the
/// form "Foreign key to Applicant". Passing `None` is equivalent to
/// calling [`field_ui_metadata`].
///
/// The caller (admin renderer) looks the target up in
/// [`Schema::relation_for`](crate::schema::Schema::relation_for); this
/// helper intentionally doesn't take a `&Schema` so the intelligence
/// module stays schema-free for callers that don't need it.
pub fn field_ui_metadata_with_relation(
    f: &AdminField,
    context: Option<&ContextConfig>,
    relation_target: Option<&str>,
) -> FieldUI {
    let mut ui = field_ui_metadata(f, context);
    if let Some(target) = relation_target.filter(|t| !t.is_empty()) {
        // Escalate the role — a known relation always renders as
        // ForeignKey even if the column name wouldn't hit the `_id`
        // heuristic.
        ui.role = FieldRole::ForeignKey;
        ui.relation_label = Some(target.to_string());
        // Rewrite the generic ForeignKey hint to name the target.
        ui.hint = Some(format!("Foreign key to {target}."));
    }
    ui
}

/// Render "Target #42" for a foreign-key cell on a list view. Falls
/// back to the raw id when the caller doesn't have a target name.
/// Kept as a free function so the admin list renderer doesn't have to
/// reach into [`FieldUI`] directly for the common case.
pub fn format_relation_cell(id: i64, target: Option<&str>) -> String {
    match target {
        Some(t) if !t.is_empty() => format!("{t} #{id}"),
        _ => id.to_string(),
    }
}

// ---------------------------------------------------------------------------
// infer_filters
// ---------------------------------------------------------------------------

/// Infer the filter controls for a model's list page from its fields
/// plus active context. Order follows the order of `fields`; every
/// filter references a field that actually exists on the model.
pub fn infer_filters(fields: &[AdminField], context: Option<&ContextConfig>) -> Vec<FilterDef> {
    infer_filters_with_relations(fields, context, |_| None)
}

/// 0.8.0 — like [`infer_filters`] but invokes `relation_target_of` for
/// each field to detect relation columns. If the callback returns
/// `Some(target)`, the filter is emitted as
/// [`FilterKind::RelationSelect`] instead of the numeric-exact fallback.
///
/// The callback shape (rather than a `&Schema`) keeps this module
/// schema-agnostic; the admin renderer is free to wire it to
/// [`Schema::relation_for`](crate::schema::Schema::relation_for).
pub fn infer_filters_with_relations<F>(
    fields: &[AdminField],
    context: Option<&ContextConfig>,
    relation_target_of: F,
) -> Vec<FilterDef>
where
    F: Fn(&AdminField) -> Option<String>,
{
    let mut out: Vec<FilterDef> = Vec::new();
    for f in fields {
        if f.name == "id" {
            continue;
        }
        let role = classify_field(f, context);
        let kind = match role {
            FieldRole::Status => FilterKind::DropdownText,
            FieldRole::Bool => FilterKind::BoolYesNo,
            FieldRole::Timestamp => FilterKind::DateRange,
            FieldRole::NumericCount => FilterKind::NumericExact,
            FieldRole::Personnummer => FilterKind::ExactMatch,
            FieldRole::ForeignKey => match relation_target_of(f) {
                Some(target_model) if !target_model.is_empty() => {
                    FilterKind::RelationSelect { target_model }
                }
                _ => FilterKind::NumericExact,
            },
            // Plain text, email, phone, money, opaque-identifier —
            // no stock filter. Email/phone would deserve their own
            // filter UI, but live search already covers the common
            // case; adding a dedicated control is a 0.7.1 candidate.
            _ => continue,
        };
        out.push(FilterDef {
            field: f.name.to_string(),
            label: humanise(f.name),
            kind,
        });
    }
    out
}

// ---------------------------------------------------------------------------
// classify_search
// ---------------------------------------------------------------------------

/// 0.8.0 — variant of [`classify_search`] that knows the field is a
/// relation. When the query parses as a non-negative integer, emits
/// [`SearchIntent::RelationId`] carrying the target model; otherwise
/// falls through to [`classify_search`] for the usual shape-based
/// routing. Called by the admin search handler when the user is
/// searching a specific FK column.
pub fn classify_search_for_field(query: &str, relation_target: Option<&str>) -> SearchIntent {
    let t = query.trim();
    if let Some(model) = relation_target.filter(|m| !m.is_empty()) {
        if let Ok(id) = t.parse::<i64>() {
            if id >= 0 {
                return SearchIntent::RelationId {
                    model: model.to_string(),
                    id,
                };
            }
        }
    }
    classify_search(query)
}

/// Guess what the user meant by the text in the list-page search box.
/// Order of tries: numeric → email → personnummer → text.
pub fn classify_search(query: &str) -> SearchIntent {
    let t = query.trim();
    if t.is_empty() {
        return SearchIntent::Text(String::new());
    }
    // Personnummer first — a 12-digit string would otherwise look
    // like a numeric ID, and `42` would still reach the Id branch
    // below because only 12 digits match the shape.
    if looks_like_personnummer(t) {
        return SearchIntent::Personnummer(t.to_string());
    }
    if let Ok(n) = t.parse::<i64>() {
        if n >= 0 {
            return SearchIntent::NumericId(n);
        }
    }
    if looks_like_email(t) {
        return SearchIntent::Email(t.to_string());
    }
    SearchIntent::Text(t.to_string())
}

fn looks_like_email(s: &str) -> bool {
    if s.len() > 254 || s.len() < 3 {
        return false;
    }
    let at = match s.find('@') {
        Some(i) => i,
        None => return false,
    };
    if at == 0 || at == s.len() - 1 {
        return false;
    }
    let domain = &s[at + 1..];
    // Domain must contain a dot and neither start nor end with it.
    if !domain.contains('.') || domain.starts_with('.') || domain.ends_with('.') {
        return false;
    }
    // Local + domain must not contain whitespace.
    !s.chars().any(|c| c.is_whitespace())
}

fn looks_like_personnummer(s: &str) -> bool {
    // Accept 12 plain digits or 8-digit / 4-digit split by `-`.
    let digits: String = s.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.len() != 12 {
        return false;
    }
    // The non-digit chars we allow are '-' only.
    if s.chars().any(|c| !c.is_ascii_digit() && c != '-') {
        return false;
    }
    match s.len() {
        12 => true,
        13 => s.as_bytes().get(8) == Some(&b'-'),
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// mask_pii
// ---------------------------------------------------------------------------

/// Produce a masked display string for a sensitive value. Keeps the
/// first few characters so a reviewer can tell which row they're
/// looking at, replaces the rest with `•`. Length of the output
/// matches the input so the layout doesn't jump when a user toggles
/// visibility.
///
/// Deterministic, Unicode-safe. Empty input → empty output.
pub fn mask_pii(value: &str) -> String {
    if value.is_empty() {
        return String::new();
    }
    let chars: Vec<char> = value.chars().collect();
    let n = chars.len();
    // Keep ~⅓ of the string visible, clamped to [2, 4] so short
    // values still show some identifying prefix without fully
    // revealing the content.
    let keep = (n / 3).clamp(2, 4).min(n);
    let mut out = String::with_capacity(n);
    for (i, c) in chars.iter().enumerate() {
        if i < keep {
            out.push(*c);
        } else {
            out.push('•');
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// snake_case → Title Case. Mirrors `admin::humanise`; kept local so
/// the intelligence module doesn't reach into private admin helpers.
fn humanise(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut next_upper = true;
    for ch in s.chars() {
        if ch == '_' {
            out.push(' ');
            next_upper = true;
        } else if next_upper {
            out.push(ch.to_ascii_uppercase());
            next_upper = false;
        } else {
            out.push(ch);
        }
    }
    out
}
