//! ViewSpec: a deterministic, declarative description of **how** a single
//! model renders — never **what** it is.
//!
//! RustIO keeps two strictly separate sources of truth. The [`Schema`]
//! (`rustio.schema.json`, see [`crate::schema`]) owns the *data structure*:
//! models, fields, types, relations. A [`ViewSpec`] owns *presentation*:
//! the order fields appear in, the role each field plays in a row, which
//! fields become list filters, and the default layout. This is the same
//! split a database schema has from a Django `ModelAdmin` — the structure
//! does not know how it is shown, and the view does not redefine the
//! structure.
//!
//! A ViewSpec is **pure declarative data**. It contains no logic, no HTML,
//! and no rendering. It names schema fields by string and annotates them;
//! turning a ViewSpec into pixels is a later phase's job. Like [`Schema`],
//! it is versioned and round-trips through JSON.
//!
//! ## Determinism contract
//!
//! A ViewSpec serialises to **byte-for-byte identical JSON** on every
//! invocation for a given value:
//!
//! - `fields` order is *meaningful* — it is the display order — and is
//!   preserved exactly as authored, never sorted or reordered.
//! - No timestamps, hashes, or environment-derived values are written.
//! - No `HashMap` appears in the serialised form; ordered `Vec`s only.
//!
//! This makes a saved ViewSpec a stable diff target in CI and a stable
//! anchor for AI-layer tooling, exactly as the schema is.
//!
//! ## File convention
//!
//! A saved ViewSpec lives next to the schema under the filename
//! `<model_snake_case>.view.json` — e.g. a [`ViewSpec`] targeting the
//! `Customer` model is written as `customer.view.json`. Deriving that path
//! from a model name is **not** done in this phase; [`ViewSpec::write_to`]
//! takes an explicit path and performs no name-to-path logic.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::Error;
use crate::schema::{SchemaField, SchemaModel};

/// Deterministic renderer: turns a [`ViewSpec`] + data rows into a
/// structured [`render::RenderedView`]. No HTML, no AI — the web layer
/// owns markup. See the [`render`] module.
pub mod render;

/// Version of the ViewSpec format itself. Independent of the rustio-core
/// crate version — a single ViewSpec version can outlive many releases as
/// long as the wire format doesn't change.
///
/// Bumping this value is a **breaking** change: every consumer refuses to
/// load older or newer documents until they are explicitly migrated.
pub const VIEWSPEC_VERSION: u32 = 1;

/// Top-level presentation document for exactly one model. Serialised as
/// `<model_snake_case>.view.json`.
///
/// `#[serde(deny_unknown_fields)]` locks the wire format: a future version
/// adding a field will fail to load under the older rustio-core unless the
/// version number is bumped in lockstep. Combined with [`VIEWSPEC_VERSION`],
/// this catches accidental silent drift — the same guarantee [`Schema`]
/// makes for data structure.
///
/// [`Schema`]: crate::schema::Schema
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ViewSpec {
    /// Format version of this document. Starts at `1`; must equal
    /// [`VIEWSPEC_VERSION`] to load.
    pub version: u32,
    /// The schema model this view targets, by name — e.g. `"Customer"`.
    /// A ViewSpec describes exactly one model.
    pub model: String,
    /// The default rendering layout for this view.
    pub layout: ViewLayout,
    /// The fields to render, **in display order**. The order of this `Vec`
    /// is meaningful: it *is* the order fields appear in the rendered view.
    pub fields: Vec<FieldSpec>,
    /// Source field names exposed as list filters. Every entry must name
    /// the `source` of a [`FieldSpec`] whose `filterable` is `true`.
    pub filters: Vec<String>,
}

/// The four rendering layouts a view may default to. Presentation only —
/// the layout never changes which fields exist or what they mean.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ViewLayout {
    /// Dense tabular rows with one column per field.
    Table,
    /// Vertical list of rows, one logical record per line.
    List,
    /// Card grid — each record rendered as a self-contained card.
    Cards,
    /// Minimal single-line-per-record layout for tight spaces.
    Compact,
}

/// The role a field plays **in a view**. This governs presentation
/// emphasis, not data semantics.
///
/// IMPORTANT: a separate `FieldRole` enum exists in
/// [`crate::admin::intelligence`] for *forms*. That one and this one are
/// deliberately distinct types in distinct modules — this enum is for
/// views. Do not import, reuse, or unify them.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldRole {
    /// The row's headline (e.g. a customer name). One per view,
    /// conventionally.
    Title,
    /// Supporting line shown under the title (e.g. an email address).
    Subtitle,
    /// A short status pill (e.g. `active` / `suspended`).
    Badge,
    /// A date/time value, formatted for humans.
    Timestamp,
    /// Secondary info shown in detail/expanded contexts, not the compact
    /// list.
    Meta,
    /// Never rendered in any context (e.g. `password_hash`, `internal_id`).
    Hidden,
}

/// One field's presentation annotation. Names a schema field by string and
/// declares how it renders; it never redefines the field's type or
/// existence — that is the schema's job.
///
/// `#[serde(deny_unknown_fields)]` mirrors the schema's wire-format lock.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FieldSpec {
    /// The schema field name this spec renders (e.g. `"email"`).
    pub source: String,
    /// The role this field plays in the view.
    pub role: FieldRole,
    /// When set, this spec merges multiple source fields into one rendered
    /// cell (e.g. `["name", "email"]` shown together under one [`Title`]).
    /// The `source` field stays the primary anchor of the merged cell. A
    /// present `merge` must list at least two entries.
    ///
    /// [`Title`]: FieldRole::Title
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub merge: Option<Vec<String>>,
    /// Whether this field may be used as a list filter. Defaults to
    /// `false`. A name may appear in [`ViewSpec::filters`] only if the
    /// corresponding `FieldSpec` has this set to `true`.
    #[serde(default)]
    pub filterable: bool,
}

/// Reasons a ViewSpec can be rejected. Named variants (never raw strings)
/// so tooling can branch on the failure kind — the same discipline
/// [`SchemaError`] follows.
///
/// [`SchemaError`]: crate::schema::SchemaError
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub enum ViewSpecError {
    /// The document's `version` field doesn't match [`VIEWSPEC_VERSION`].
    VersionMismatch { found: u32, expected: u32 },
    /// An identifier-shaped string is empty (e.g. an empty `model`).
    EmptyIdentifier(&'static str),
    /// The view declares no fields. A view must render something.
    NoFields,
    /// Two [`FieldSpec`] entries share the same `source` value.
    DuplicateSource(String),
    /// A name in `filters` doesn't match the `source` of any field, or
    /// matches a field whose `filterable` is `false`.
    NonFilterableFilter(String),
    /// A `merge` vector is present but lists fewer than two entries.
    MergeTooShort { source: String, len: usize },
    /// Failed to parse a ViewSpec document from its on-disk bytes.
    Parse(String),
}

impl std::fmt::Display for ViewSpecError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::VersionMismatch { found, expected } => write!(
                f,
                "viewspec version mismatch: found {found}, expected {expected}"
            ),
            Self::EmptyIdentifier(which) => write!(f, "empty {which}"),
            Self::NoFields => write!(f, "viewspec declares no fields"),
            Self::DuplicateSource(source) => write!(f, "duplicate field source `{source}`"),
            Self::NonFilterableFilter(name) => {
                write!(f, "filter `{name}` names no filterable field source")
            }
            Self::MergeTooShort { source, len } => write!(
                f,
                "field `{source}` has a merge of {len} entr{plural} (need at least 2)",
                plural = if *len == 1 { "y" } else { "ies" },
            ),
            Self::Parse(msg) => write!(f, "viewspec parse error: {msg}"),
        }
    }
}

impl std::error::Error for ViewSpecError {}

impl From<ViewSpecError> for Error {
    fn from(e: ViewSpecError) -> Self {
        Error::Internal(e.to_string())
    }
}

impl ViewSpec {
    /// Check the ViewSpec for internal consistency. Every production writer
    /// should call this before persisting and every consumer should call it
    /// after loading. The error is the first problem found; fix and
    /// revalidate.
    ///
    /// Rules:
    /// - `version` must be `>= 1`.
    /// - `model` must be non-empty.
    /// - `fields` must be non-empty.
    /// - No two [`FieldSpec`] entries may share a `source`.
    /// - Every name in `filters` must equal the `source` of some
    ///   [`FieldSpec`] whose `filterable` is `true`.
    /// - A `merge`, when present, must list at least two entries.
    pub fn validate(&self) -> Result<(), ViewSpecError> {
        if self.version < 1 {
            return Err(ViewSpecError::VersionMismatch {
                found: self.version,
                expected: VIEWSPEC_VERSION,
            });
        }
        if self.model.is_empty() {
            return Err(ViewSpecError::EmptyIdentifier("model"));
        }
        if self.fields.is_empty() {
            return Err(ViewSpecError::NoFields);
        }

        // Collect sources while rejecting duplicates, and remember which
        // are filterable so the `filters` pass can be a pure lookup.
        let mut filterable_sources: BTreeSet<&str> = BTreeSet::new();
        let mut seen: BTreeSet<&str> = BTreeSet::new();
        for field in &self.fields {
            if field.source.is_empty() {
                return Err(ViewSpecError::EmptyIdentifier("field source"));
            }
            if !seen.insert(field.source.as_str()) {
                return Err(ViewSpecError::DuplicateSource(field.source.clone()));
            }
            if let Some(merge) = &field.merge {
                if merge.len() < 2 {
                    return Err(ViewSpecError::MergeTooShort {
                        source: field.source.clone(),
                        len: merge.len(),
                    });
                }
            }
            if field.filterable {
                filterable_sources.insert(field.source.as_str());
            }
        }

        for name in &self.filters {
            if !filterable_sources.contains(name.as_str()) {
                return Err(ViewSpecError::NonFilterableFilter(name.clone()));
            }
        }

        Ok(())
    }

    /// Parse + validate a ViewSpec document. Both deserialization failure
    /// (unknown fields, wrong types, missing keys) and any semantic problem
    /// surface as [`ViewSpecError`]. Safe default for anything reading a
    /// `<model>.view.json` off disk.
    pub fn parse(json: &str) -> Result<Self, ViewSpecError> {
        let spec: ViewSpec =
            serde_json::from_str(json).map_err(|e| ViewSpecError::Parse(e.to_string()))?;
        spec.validate()?;
        Ok(spec)
    }

    /// Serialise to pretty JSON with a trailing newline. We pretty-print on
    /// purpose: the file is meant to be read by humans during code review
    /// and by AI tools that benefit from stable line-level anchors.
    pub fn to_pretty_json(&self) -> Result<String, Error> {
        let mut out =
            serde_json::to_string_pretty(self).map_err(|e| Error::Internal(e.to_string()))?;
        out.push('\n');
        Ok(out)
    }

    /// Write the ViewSpec to a file, atomically. Validates first so a broken
    /// view never lands on disk. Uses a temp-file + rename so a concurrent
    /// reader can never observe a half-written JSON file.
    ///
    /// The caller supplies the full path. By convention that path is named
    /// `<model_snake_case>.view.json` (e.g. `customer.view.json`), but
    /// deriving it from [`ViewSpec::model`] is intentionally **not** done
    /// here — this method performs no name-to-path logic.
    pub fn write_to(&self, path: &Path) -> Result<(), Error> {
        self.validate()?;
        let json = self.to_pretty_json()?;
        let tmp = path.with_extension("json.tmp");
        // Best-effort cleanup if a previous aborted run left the tmp
        // behind; we ignore errors because `write` will surface any real
        // permission problem.
        let _ = fs::remove_file(&tmp);
        fs::write(&tmp, json).map_err(|e| Error::Internal(e.to_string()))?;
        if let Err(e) = fs::rename(&tmp, path) {
            // Rename failed — clean up the tmp so we don't leave a stale
            // `.json.tmp` next to the target on retry.
            let _ = fs::remove_file(&tmp);
            return Err(Error::Internal(e.to_string()));
        }
        Ok(())
    }
}

impl ViewSpec {
    /// Derive a default, domain-shaped view from a single model's schema.
    /// Deterministic, no AI, no I/O — a pure function of `model`. The
    /// result is a sensible starting point a developer can hand-edit; it
    /// is **not** authoritative the way the schema is.
    ///
    /// This is the "raw schema dump → domain view" transform: it walks
    /// `model.fields` in declared order, assigns each a view [`FieldRole`]
    /// using the deterministic rules in [`classify_view_field`], and
    /// returns a [`ViewSpec`] that is guaranteed to pass
    /// [`ViewSpec::validate`].
    ///
    /// Defaults:
    /// - `layout` is [`ViewLayout::List`].
    /// - `filters` lists the source of every field marked filterable. By
    ///   the rules below that is **only** a `status` / `*_status` field, if
    ///   the model has one — booleans are rendered as badges but are *not*
    ///   auto-filtered, to keep the filter bar quiet.
    /// - No field is ever merged automatically; `merge` is always `None`.
    pub fn from_schema_model(model: &SchemaModel) -> Self {
        // Pass 1: choose which field is the row's Title, by name-like
        // preference then first-plain-text fallback. Done up front so the
        // role pass can simply ask "is this the title source?".
        let title_source = pick_title_source(&model.fields);

        // Pass 2: assign a role (and filterability) to every field, in the
        // schema's declared order — which becomes the view's display order.
        let mut fields: Vec<FieldSpec> = Vec::with_capacity(model.fields.len());
        let mut filters: Vec<String> = Vec::new();
        for f in &model.fields {
            let (role, filterable) = classify_view_field(&f.name, &f.ty, title_source.as_deref());
            if filterable {
                filters.push(f.name.clone());
            }
            fields.push(FieldSpec {
                source: f.name.clone(),
                role,
                merge: None,
                filterable,
            });
        }

        Self {
            version: VIEWSPEC_VERSION,
            model: model.name.clone(),
            layout: ViewLayout::List,
            fields,
            filters,
        }
    }
}

/// Choose the field that should play [`FieldRole::Title`] in a default
/// view, or `None` if the model has no text field eligible to be a
/// headline. Two ordered passes, both deterministic:
///
/// 1. The first `String` field whose name is a conventional headline name
///    (`name`, `title`, `full_name`, `display_name`, `label`, `username`).
/// 2. Failing that, the first "plain" `String` field — one that no
///    higher-precedence rule in [`classify_view_field`] would claim
///    (not a secret, opaque id, `email`/`phone`, `status`, or `id`).
///
/// Both passes walk `fields` in declared order, so the choice is stable.
fn pick_title_source(fields: &[SchemaField]) -> Option<String> {
    const NAME_LIKE: &[&str] = &[
        "name",
        "title",
        "full_name",
        "display_name",
        "label",
        "username",
    ];
    for f in fields {
        if f.ty == "String" && NAME_LIKE.contains(&f.name.as_str()) && is_plain_text_name(&f.name) {
            return Some(f.name.clone());
        }
    }
    for f in fields {
        if f.ty == "String" && is_plain_text_name(&f.name) {
            return Some(f.name.clone());
        }
    }
    None
}

/// Assign a view [`FieldRole`] and a default `filterable` flag to one
/// field, from its `name`, schema type string (`ty`), and the
/// pre-selected `title_source`. Pure and deterministic; mirrors the
/// shape signals of [`crate::admin::intelligence::classify_field`] but
/// targets *presentation*, not form rendering — see the module header for
/// the data-vs-presentation split.
///
/// Precedence (highest first):
/// 1. Secret-shaped name → `Hidden` (see [`is_secret_name`]).
/// 2. Opaque PII name → `Hidden` (see [`is_opaque_pii_name`]).
/// 3. `id` → `Hidden` — raw primary keys aren't shown by default.
/// 4. `DateTime` type → `Timestamp`.
/// 5. `status` / `*_status` → `Badge`, **filterable**.
/// 6. `bool` type → `Badge`, *not* filterable (auto-filtering every flag
///    makes a noisy filter bar; only `status` earns an auto-filter).
/// 7. `email` / `phone` → `Subtitle` (sensitive means masked at render,
///    not omitted — the view still needs a contact line).
/// 8. integer `*_id` → `Meta` (foreign key, until relations render).
/// 9. the chosen `title_source` → `Title`.
/// 10. everything else → `Meta`.
fn classify_view_field(name: &str, ty: &str, title_source: Option<&str>) -> (FieldRole, bool) {
    if is_secret_name(name) {
        return (FieldRole::Hidden, false);
    }
    if is_opaque_pii_name(name) {
        return (FieldRole::Hidden, false);
    }
    if name == "id" {
        return (FieldRole::Hidden, false);
    }
    if ty == "DateTime" {
        return (FieldRole::Timestamp, false);
    }
    if is_status_name(name) {
        return (FieldRole::Badge, true);
    }
    if ty == "bool" {
        return (FieldRole::Badge, false);
    }
    if name == "email" || name == "phone" {
        return (FieldRole::Subtitle, false);
    }
    if name.ends_with("_id") && (ty == "i32" || ty == "i64") {
        return (FieldRole::Meta, false);
    }
    if Some(name) == title_source {
        return (FieldRole::Title, false);
    }
    (FieldRole::Meta, false)
}

/// `true` for credential / secret-shaped column names.
///
/// **This is a VIEW-SIDE rule and intentionally goes BEYOND
/// [`crate::admin::intelligence::FieldRole::is_sensitive`].** The two
/// layers have different jobs: `intelligence` decides what to *mask* on a
/// rendered list (it covers `email` / `phone` / `personnummer` / opaque
/// clinical ids) and notably **misses `password_hash`**, which it
/// classifies as plain text. A view, by contrast, can *fully hide* a
/// field — so for a default view we hide credentials outright rather than
/// merely masking them. Keep this list as the view's own concern; do not
/// "fix" it by deferring to `intelligence`, which would re-expose
/// `password_hash`.
fn is_secret_name(name: &str) -> bool {
    name.ends_with("_hash")
        || name.contains("password")
        || name.contains("secret")
        || name.contains("token")
}

/// `true` for opaque personal / identity numbers that should never appear
/// in a default view. These mirror the context-gated PII names
/// `intelligence` recognises (`personnummer` family, healthcare opaque
/// ids), applied here context-free: a default view errs toward hiding,
/// matching `intelligence`'s "mark sensitive up, never down" principle.
fn is_opaque_pii_name(name: &str) -> bool {
    matches!(
        name,
        "personnummer"
            | "personal_id"
            | "personal_number"
            | "pnr"
            | "fodselsnummer"
            | "patient_id"
            | "mrn"
            | "medical_record_number"
            | "ssn"
    )
}

/// `true` for `status` and `*_status` columns — the one shape that earns
/// an automatic [`Badge`] + list filter in a default view.
///
/// [`Badge`]: FieldRole::Badge
fn is_status_name(name: &str) -> bool {
    name == "status" || name.ends_with("_status")
}

/// `true` when a column name is "plain text" — i.e. no higher-precedence
/// rule in [`classify_view_field`] would claim it, so it is eligible to
/// be the [`FieldRole::Title`]. Type is checked separately by the caller.
fn is_plain_text_name(name: &str) -> bool {
    !is_secret_name(name)
        && !is_opaque_pii_name(name)
        && !is_status_name(name)
        && name != "id"
        && name != "email"
        && name != "phone"
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{SchemaField, SchemaModel};

    /// A representative, valid ViewSpec exercising every role, a merge, and
    /// a filter. Reused across tests so the round-trip and validation
    /// checks all agree on one well-formed shape.
    fn sample() -> ViewSpec {
        ViewSpec {
            version: VIEWSPEC_VERSION,
            model: "Customer".to_string(),
            layout: ViewLayout::Table,
            fields: vec![
                FieldSpec {
                    source: "name".to_string(),
                    role: FieldRole::Title,
                    merge: Some(vec!["name".to_string(), "email".to_string()]),
                    filterable: false,
                },
                FieldSpec {
                    source: "email".to_string(),
                    role: FieldRole::Subtitle,
                    merge: None,
                    filterable: false,
                },
                FieldSpec {
                    source: "status".to_string(),
                    role: FieldRole::Badge,
                    merge: None,
                    filterable: true,
                },
                FieldSpec {
                    source: "created_at".to_string(),
                    role: FieldRole::Timestamp,
                    merge: None,
                    filterable: false,
                },
                FieldSpec {
                    source: "notes".to_string(),
                    role: FieldRole::Meta,
                    merge: None,
                    filterable: false,
                },
                FieldSpec {
                    source: "password_hash".to_string(),
                    role: FieldRole::Hidden,
                    merge: None,
                    filterable: false,
                },
            ],
            filters: vec!["status".to_string()],
        }
    }

    #[test]
    fn round_trips_through_pretty_json() {
        let spec = sample();
        let json = spec.to_pretty_json().unwrap();
        let parsed = ViewSpec::parse(&json).unwrap();
        assert_eq!(parsed, spec);
    }

    #[test]
    fn to_pretty_json_ends_with_newline() {
        let json = sample().to_pretty_json().unwrap();
        assert!(json.ends_with('\n'), "viewspec JSON must end with newline");
    }

    #[test]
    fn validate_accepts_clean_spec() {
        assert_eq!(sample().validate(), Ok(()));
    }

    #[test]
    fn validate_rejects_empty_fields() {
        let mut spec = sample();
        spec.fields.clear();
        spec.filters.clear();
        assert_eq!(spec.validate(), Err(ViewSpecError::NoFields));
    }

    #[test]
    fn validate_rejects_duplicate_source() {
        let mut spec = sample();
        let dup = spec.fields[1].clone();
        spec.fields.push(dup);
        assert_eq!(
            spec.validate(),
            Err(ViewSpecError::DuplicateSource("email".to_string()))
        );
    }

    #[test]
    fn validate_rejects_filter_on_non_filterable_field() {
        let mut spec = sample();
        // `email` exists but is not flagged filterable.
        spec.filters.push("email".to_string());
        assert_eq!(
            spec.validate(),
            Err(ViewSpecError::NonFilterableFilter("email".to_string()))
        );
    }

    #[test]
    fn validate_rejects_filter_naming_unknown_field() {
        let mut spec = sample();
        spec.filters.push("ghost".to_string());
        assert_eq!(
            spec.validate(),
            Err(ViewSpecError::NonFilterableFilter("ghost".to_string()))
        );
    }

    #[test]
    fn validate_rejects_merge_with_one_entry() {
        let mut spec = sample();
        spec.fields[0].merge = Some(vec!["name".to_string()]);
        assert_eq!(
            spec.validate(),
            Err(ViewSpecError::MergeTooShort {
                source: "name".to_string(),
                len: 1,
            })
        );
    }

    #[test]
    fn serialisation_is_byte_stable() {
        // The determinism contract: identical inputs → identical bytes. If
        // this ever fails, someone added a clock, hash, or unordered map to
        // the serialisation path.
        let spec = sample();
        let a = spec.to_pretty_json().unwrap();
        let b = spec.to_pretty_json().unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn hidden_role_round_trips() {
        // A sensitive field must be expressible as never-rendered and must
        // survive a JSON round-trip unchanged.
        let spec = ViewSpec {
            version: VIEWSPEC_VERSION,
            model: "User".to_string(),
            layout: ViewLayout::List,
            fields: vec![
                FieldSpec {
                    source: "email".to_string(),
                    role: FieldRole::Title,
                    merge: None,
                    filterable: false,
                },
                FieldSpec {
                    source: "password_hash".to_string(),
                    role: FieldRole::Hidden,
                    merge: None,
                    filterable: false,
                },
            ],
            filters: vec![],
        };
        let parsed = ViewSpec::parse(&spec.to_pretty_json().unwrap()).unwrap();
        assert_eq!(parsed, spec);
        let hidden = parsed
            .fields
            .iter()
            .find(|f| f.source == "password_hash")
            .unwrap();
        assert_eq!(hidden.role, FieldRole::Hidden);
    }

    #[test]
    fn filterable_defaults_to_false_when_absent() {
        // `#[serde(default)]` on `filterable` and `skip_serializing_if` on
        // `merge` mean a minimal field object parses cleanly.
        let json = r#"{
            "version": 1,
            "model": "Customer",
            "layout": "table",
            "fields": [
                { "source": "name", "role": "title" }
            ],
            "filters": []
        }"#;
        let spec = ViewSpec::parse(json).unwrap();
        assert!(!spec.fields[0].filterable);
        assert_eq!(spec.fields[0].merge, None);
    }

    #[test]
    fn parse_rejects_unknown_field() {
        let bad = r#"{
            "version": 1,
            "model": "Customer",
            "layout": "table",
            "fields": [ { "source": "name", "role": "title" } ],
            "filters": [],
            "something_extra": true
        }"#;
        assert!(matches!(ViewSpec::parse(bad), Err(ViewSpecError::Parse(_))));
    }

    #[test]
    fn layout_serialises_lowercase() {
        let spec = sample();
        let json = spec.to_pretty_json().unwrap();
        assert!(json.contains("\"layout\": \"table\""));
        assert!(json.contains("\"role\": \"title\""));
        assert!(json.contains("\"role\": \"hidden\""));
    }

    // -- Phase 2: from_schema_model -----------------------------------------

    /// Build a bare `SchemaField` with the given name and schema type
    /// string. Nullability / editability don't affect view derivation, so
    /// they're fixed.
    fn field(name: &str, ty: &str) -> SchemaField {
        SchemaField {
            name: name.to_string(),
            ty: ty.to_string(),
            nullable: false,
            editable: true,
            relation: None,
        }
    }

    /// A "Customer"-shaped model in declared order, matching the agreed
    /// mapping table.
    fn customer_model() -> SchemaModel {
        SchemaModel {
            name: "Customer".to_string(),
            table: "customers".to_string(),
            admin_name: "customers".to_string(),
            display_name: "Customers".to_string(),
            singular_name: "Customer".to_string(),
            fields: vec![
                field("id", "i64"),
                field("name", "String"),
                field("email", "String"),
                field("status", "String"),
                field("created_at", "DateTime"),
                field("password_hash", "String"),
                field("notes", "String"),
            ],
            relations: Vec::new(),
            core: false,
        }
    }

    /// Look up the derived spec's role for a given source field.
    fn role_of(spec: &ViewSpec, source: &str) -> FieldRole {
        spec.fields
            .iter()
            .find(|f| f.source == source)
            .unwrap_or_else(|| panic!("no field `{source}` in derived spec"))
            .role
    }

    #[test]
    fn derived_roles_match_the_agreed_mapping() {
        let spec = ViewSpec::from_schema_model(&customer_model());

        assert_eq!(spec.version, VIEWSPEC_VERSION);
        assert_eq!(spec.model, "Customer");
        assert_eq!(spec.layout, ViewLayout::List);

        // Declared order is preserved as display order.
        let order: Vec<&str> = spec.fields.iter().map(|f| f.source.as_str()).collect();
        assert_eq!(
            order,
            vec![
                "id",
                "name",
                "email",
                "status",
                "created_at",
                "password_hash",
                "notes"
            ]
        );

        assert_eq!(role_of(&spec, "id"), FieldRole::Hidden);
        assert_eq!(role_of(&spec, "name"), FieldRole::Title);
        assert_eq!(role_of(&spec, "email"), FieldRole::Subtitle);
        assert_eq!(role_of(&spec, "status"), FieldRole::Badge);
        assert_eq!(role_of(&spec, "created_at"), FieldRole::Timestamp);
        assert_eq!(role_of(&spec, "password_hash"), FieldRole::Hidden);
        assert_eq!(role_of(&spec, "notes"), FieldRole::Meta);

        // Only `status` is filterable; it is the sole entry in `filters`.
        for f in &spec.fields {
            let expect_filterable = f.source == "status";
            assert_eq!(
                f.filterable, expect_filterable,
                "field `{}` filterable mismatch",
                f.source
            );
        }
        assert_eq!(spec.filters, vec!["status".to_string()]);
    }

    #[test]
    fn password_hash_is_hidden() {
        // Sensitive credentials must never leak into a default view — the
        // view-side secret-name rule covers what intelligence's
        // `is_sensitive` misses.
        let spec = ViewSpec::from_schema_model(&customer_model());
        assert_eq!(role_of(&spec, "password_hash"), FieldRole::Hidden);
    }

    #[test]
    fn derived_spec_passes_validate() {
        let spec = ViewSpec::from_schema_model(&customer_model());
        assert_eq!(spec.validate(), Ok(()));
    }

    #[test]
    fn derivation_is_deterministic() {
        let model = customer_model();
        let a = ViewSpec::from_schema_model(&model);
        let b = ViewSpec::from_schema_model(&model);
        assert_eq!(a, b);
    }
}
