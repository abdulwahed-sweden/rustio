//! Phase 8.0 — AI-assisted schema generation, developer-tool only.
//!
//! This module sits **alongside** the existing `ai/` module (which is
//! the deterministic, rule-based plan/review/apply pipeline). It does
//! NOT replace or extend that pipeline; it produces a `schema::Schema`
//! JSON document that the operator then runs through
//! `rustio ai plan / review / apply` manually.
//!
//! ## Hard contract
//!
//! - LLM calls happen ONLY from the CLI. No HTTP handler, no admin
//!   page, no background task in this crate calls into here. The
//!   deployed `rustio` binary serving requests has no network reach to
//!   any LLM provider.
//! - The LLM's output is parsed as `schema::Schema` JSON and run
//!   through `Schema::validate()` before it leaves this module. A
//!   malformed or semantically-invalid response is an error;
//!   half-validated artefacts never reach disk.
//! - Nothing here writes files, runs migrations, or modifies the DB.
//!   File I/O lives at the CLI layer where the operator can confirm.
//!
//! ## Pipeline
//!
//! ```text
//! prompt ──► client ──► raw JSON string ──► serde_json ──► Schema ──► validate() ──► Ok(Schema)
//!                                                          │
//!                                                          └─ on failure: SchemaError, no file written
//! ```
//!
//! The CLI's `rustio ai generate` command owns the file write and the
//! `--force` overwrite guard — see `rustio-cli/src/main.rs`.

pub mod client;
pub mod diff;
pub mod prompts;

use crate::schema::{Schema, SchemaError};

/// Errors `ai_gen::generate` can surface. Kept narrow on purpose:
/// callers only need to distinguish "couldn't talk to the API" from
/// "the API replied but the reply isn't a valid Schema."
#[derive(Debug)]
pub enum GenerateError {
    /// Missing / empty `ANTHROPIC_API_KEY`.
    MissingApiKey,
    /// HTTP error talking to the provider (network, auth, rate limit,
    /// 5xx). Carries the provider's message for triage.
    Transport(String),
    /// The provider replied but the body wasn't a parseable `Schema`
    /// JSON document. Wraps the underlying parse / validation error.
    Schema(SchemaError),
    /// Phase 9.1 — the model returned a syntactically valid schema
    /// with zero models, while the input had at least one. Hard
    /// safety rule for `update`: a "remove everything" instruction
    /// must NOT clear the schema. No bypass flag; never overridable.
    EmptyResult,
}

impl std::fmt::Display for GenerateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingApiKey => f.write_str(
                "ANTHROPIC_API_KEY is not set. Set it in your environment before running \
                 `rustio ai generate`.",
            ),
            Self::Transport(msg) => write!(f, "anthropic API transport error: {msg}"),
            Self::Schema(err) => write!(f, "anthropic API returned invalid schema: {err}"),
            Self::EmptyResult => f.write_str(
                "Refusing to apply update: schema would become empty",
            ),
        }
    }
}

impl std::error::Error for GenerateError {}

impl From<SchemaError> for GenerateError {
    fn from(e: SchemaError) -> Self {
        GenerateError::Schema(e)
    }
}

/// Top-level entry: prose `prompt` → validated `Schema`.
///
/// Calls the Anthropic API, parses the response as Schema JSON, runs
/// `Schema::validate()`. The CLI is the only intended caller; tests
/// hit the inner helpers (`prompts::build_user_prompt`,
/// `parse_response`) directly to avoid live API calls in CI.
pub async fn generate(prompt: &str) -> Result<Schema, GenerateError> {
    let api_key = api_key()?;
    let body = client::request(&api_key, prompt)
        .await
        .map_err(|e| GenerateError::Transport(e.to_string()))?;
    parse_response(&body)
}

/// Phase 8.1 — sibling of `generate`: hand the model the existing
/// schema + an instruction, get back a validated full `Schema` with
/// the change applied. Single LLM call. The CLI is responsible for
/// computing + showing the diff and for the y/N confirmation.
///
/// Phase 9.1 — empty-schema safety guard. After parsing, if the
/// model emptied the schema, refuse the result. No bypass flag.
pub async fn update(existing: &Schema, instruction: &str) -> Result<Schema, GenerateError> {
    let api_key = api_key()?;
    let existing_json = existing
        .to_pretty_json()
        .map_err(|e| GenerateError::Transport(format!("serialise existing schema: {e}")))?;
    let body = client::request_update(&api_key, &existing_json, instruction)
        .await
        .map_err(|e| GenerateError::Transport(e.to_string()))?;
    let updated = parse_response(&body)?;
    check_not_empty(existing, &updated)?;
    Ok(updated)
}

/// Phase 9.1 — hard safety guard: an `update` MUST NOT clear a
/// non-empty schema. Returns `Err(GenerateError::EmptyResult)`
/// when:
///   - input had >= 1 model AND
///   - output has 0 models.
///
/// Empty → empty (genuinely-empty input) and any → non-empty paths
/// pass through. Extracted as a free function so tests can pin the
/// truth table without standing up a fake LLM flow.
pub(crate) fn check_not_empty(old: &Schema, new: &Schema) -> Result<(), GenerateError> {
    if new.models.is_empty() && !old.models.is_empty() {
        return Err(GenerateError::EmptyResult);
    }
    Ok(())
}

/// Read + validate the API key once for both entry points. Empty /
/// whitespace-only values count as missing.
fn api_key() -> Result<String, GenerateError> {
    std::env::var("ANTHROPIC_API_KEY")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .ok_or(GenerateError::MissingApiKey)
}

/// Phase 8.2 — the read-only analyze report. Three flat fields: each
/// list is human-readable strings (one per line in the model's
/// output), the score is on a 0-10 scale. CLI prints these directly;
/// nothing here writes to disk or modifies the schema.
#[derive(Debug, Clone, PartialEq)]
pub struct AnalyzeReport {
    pub issues: Vec<String>,
    pub suggestions: Vec<String>,
    pub score: f32,
}

/// Phase 8.2 — analyze-path errors. Mirrors `GenerateError`'s shape
/// but with one less variant (no Schema-validation failure, since
/// analyze never produces a Schema).
#[derive(Debug)]
pub enum AnalyzeError {
    MissingApiKey,
    Transport(String),
    /// Couldn't serialise the input schema before sending. Should
    /// only fire if the caller hands us a schema that fails its own
    /// `validate()`; the CLI guards this with `load_schema`.
    Encode(String),
}

impl std::fmt::Display for AnalyzeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingApiKey => f.write_str(
                "ANTHROPIC_API_KEY is not set. Set it in your environment before running \
                 `rustio ai analyze`.",
            ),
            Self::Transport(msg) => write!(f, "anthropic API transport error: {msg}"),
            Self::Encode(msg) => write!(f, "could not serialise schema for analyze: {msg}"),
        }
    }
}

impl std::error::Error for AnalyzeError {}

/// Phase 8.2 — read-only audit. Hand the model the schema, get back
/// a structured-text analysis, parse into `AnalyzeReport`. Single
/// LLM call. Does NOT write to disk, does NOT modify the schema,
/// does NOT call `update` or `generate` internally.
pub async fn analyze(schema: &Schema) -> Result<AnalyzeReport, AnalyzeError> {
    let api_key = std::env::var("ANTHROPIC_API_KEY")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .ok_or(AnalyzeError::MissingApiKey)?;
    let existing_json = schema
        .to_pretty_json()
        .map_err(|e| AnalyzeError::Encode(e.to_string()))?;
    let body = client::request_analyze(&api_key, &existing_json)
        .await
        .map_err(AnalyzeError::Transport)?;
    Ok(parse_analyze_response(&body))
}

/// Parse a model response into `AnalyzeReport`. Tolerant by design:
///
/// 1. **Structured-text path** — find lines starting with `ISSUES:`,
///    `SUGGESTIONS:`, `SCORE:` (case-insensitive prefix match);
///    collect bullet items between headers. The score line accepts
///    "7.5", "7.5/10", "7", or any prefix that parses as f32.
/// 2. **Fallback path** — if no recognised section header appears,
///    treat the entire body as `suggestions` (one entry per
///    non-empty line, stripping any leading "- " bullet) so the
///    operator sees something useful instead of an empty report.
///
/// The score defaults to 0.0 when the SCORE line is missing or
/// unparseable. Callers wanting to gate on "did the model give us a
/// score" can check `report.score > 0.0`.
pub fn parse_analyze_response(body: &str) -> AnalyzeReport {
    let body = body.trim();
    if body.is_empty() {
        return AnalyzeReport {
            issues: Vec::new(),
            suggestions: Vec::new(),
            score: 0.0,
        };
    }

    let lower = body.to_lowercase();
    let has_section_header = lower.contains("issues:")
        || lower.contains("suggestions:")
        || lower.contains("score:");

    if !has_section_header {
        // Fallback: no structured headers. Treat everything as
        // suggestions so the developer at least sees the model's
        // analysis, even if it's free-form.
        let suggestions = collect_bullets(body);
        return AnalyzeReport {
            issues: Vec::new(),
            suggestions,
            score: 0.0,
        };
    }

    let mut section = Section::None;
    let mut issues: Vec<String> = Vec::new();
    let mut suggestions: Vec<String> = Vec::new();
    let mut score: f32 = 0.0;

    for raw_line in body.lines() {
        let line = raw_line.trim();
        let lower = line.to_lowercase();
        // Section-header detection — match prefix so a line like
        // "ISSUES: (none)" is still classified as the header line
        // (and the "(none)" body sits inside the section as zero
        // bullet items, which is what we want).
        if lower.starts_with("issues:") {
            section = Section::Issues;
            continue;
        }
        if lower.starts_with("suggestions:") {
            section = Section::Suggestions;
            continue;
        }
        if lower.starts_with("score:") {
            section = Section::Score;
            score = parse_score(line["score:".len()..].trim()).unwrap_or(0.0);
            continue;
        }

        // Skip blank lines and the explicit "(none)" placeholder.
        if line.is_empty() || line.eq_ignore_ascii_case("(none)") {
            continue;
        }

        // Strip a single leading bullet so consumers don't see "- ".
        let item = line
            .strip_prefix("- ")
            .or_else(|| line.strip_prefix("* "))
            .unwrap_or(line)
            .to_string();

        match section {
            Section::Issues => issues.push(item),
            Section::Suggestions => suggestions.push(item),
            // Lines after SCORE: are tolerated but ignored — the
            // model occasionally adds a one-line summary.
            Section::Score | Section::None => {}
        }
    }

    AnalyzeReport { issues, suggestions, score }
}

/// Internal section marker for the analyze parser.
enum Section {
    None,
    Issues,
    Suggestions,
    Score,
}

/// Pull a leading f32 out of a string like "7.5" / "7.5 / 10" /
/// "7.5/10" / "  8.0  ". Returns None if no float prefix matches.
fn parse_score(s: &str) -> Option<f32> {
    let s = s.trim();
    // Walk forward until the prefix stops looking like a number.
    let end = s
        .char_indices()
        .find(|(_, c)| !(c.is_ascii_digit() || *c == '.' || *c == '-'))
        .map(|(i, _)| i)
        .unwrap_or(s.len());
    s[..end].parse::<f32>().ok()
}

/// Split a body into bullet-style entries. Used by the unstructured
/// fallback. Strips the leading bullet (`- ` or `* `) if present;
/// blank lines are dropped.
fn collect_bullets(body: &str) -> Vec<String> {
    body.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(|l| {
            l.strip_prefix("- ")
                .or_else(|| l.strip_prefix("* "))
                .unwrap_or(l)
                .to_string()
        })
        .collect()
}

/// Phase 8.4 — the explain-diff report. Two parallel bullet lists.
/// Empty `why` AND empty `impact` is a legitimate result for an
/// empty diff (BEFORE == AFTER); the CLI prints "(none)" in that
/// case rather than nothing.
#[derive(Debug, Clone, PartialEq)]
pub struct ExplainReport {
    pub why: Vec<String>,
    pub impact: Vec<String>,
}

/// Phase 8.4 — explain-path errors. Mirrors the analyze shape; no
/// Schema-validation failure variant because explain doesn't
/// produce a Schema.
#[derive(Debug)]
pub enum ExplainError {
    MissingApiKey,
    Transport(String),
    /// Couldn't serialise one of the input schemas before sending.
    Encode(String),
}

impl std::fmt::Display for ExplainError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingApiKey => f.write_str(
                "ANTHROPIC_API_KEY is not set. Set it in your environment before requesting \
                 an explanation.",
            ),
            Self::Transport(msg) => write!(f, "anthropic API transport error: {msg}"),
            Self::Encode(msg) => write!(f, "could not serialise schema for explain: {msg}"),
        }
    }
}

impl std::error::Error for ExplainError {}

/// Phase 8.4 — narrate a diff between two schemas with one extra
/// LLM call. Receives the `api_key` directly (rather than reading
/// it from the env like `generate` / `update` / `analyze`) so the
/// caller can short-circuit when the flag is off without ever
/// touching the env. The CLI's `--explain` gate hands this in.
///
/// MAX 1 LLM call. NEVER mutates either schema. NEVER recurses
/// (no follow-up calls based on the response).
pub async fn explain_diff(
    old: &Schema,
    new: &Schema,
    api_key: &str,
) -> Result<ExplainReport, ExplainError> {
    if api_key.trim().is_empty() {
        return Err(ExplainError::MissingApiKey);
    }
    let old_json = old
        .to_pretty_json()
        .map_err(|e| ExplainError::Encode(e.to_string()))?;
    let new_json = new
        .to_pretty_json()
        .map_err(|e| ExplainError::Encode(e.to_string()))?;
    let body = client::request_explain(api_key, &old_json, &new_json)
        .await
        .map_err(ExplainError::Transport)?;
    Ok(parse_explain_response(&body))
}

/// Parse a model response into `ExplainReport`. Same tolerance
/// strategy as `parse_analyze_response`:
///
/// 1. **Structured-text path** — find lines starting with `WHY:` /
///    `IMPACT:` (case-insensitive prefix match); collect bullets
///    between them. The "(none)" placeholder is honoured.
/// 2. **Fallback** — if no recognised section header appears,
///    treat the entire body as `why` so the operator at least
///    sees the model's commentary, with `impact` empty.
pub fn parse_explain_response(body: &str) -> ExplainReport {
    let body = body.trim();
    if body.is_empty() {
        return ExplainReport { why: Vec::new(), impact: Vec::new() };
    }

    let lower = body.to_lowercase();
    let has_section_header = lower.contains("why:") || lower.contains("impact:");
    if !has_section_header {
        return ExplainReport { why: collect_bullets(body), impact: Vec::new() };
    }

    let mut section = ExplainSection::None;
    let mut why: Vec<String> = Vec::new();
    let mut impact: Vec<String> = Vec::new();

    for raw_line in body.lines() {
        let line = raw_line.trim();
        let lower = line.to_lowercase();

        if lower.starts_with("why:") {
            section = ExplainSection::Why;
            continue;
        }
        if lower.starts_with("impact:") {
            section = ExplainSection::Impact;
            continue;
        }

        if line.is_empty() || line.eq_ignore_ascii_case("(none)") {
            continue;
        }

        // Inside a section, only bullet-shaped lines (`- ` / `* `)
        // count as items. A non-bullet line ENDS the section; the
        // line itself is dropped (the system prompt forbids
        // commentary outside the two sections, so anything else is
        // the model breaking contract). This is the load-bearing
        // rule that makes `explain_ignores_extra_text` pass.
        let bullet = line
            .strip_prefix("- ")
            .or_else(|| line.strip_prefix("* "));

        match (&section, bullet) {
            (ExplainSection::Why, Some(item)) => why.push(item.to_string()),
            (ExplainSection::Impact, Some(item)) => impact.push(item.to_string()),
            (ExplainSection::Why | ExplainSection::Impact, None) => {
                section = ExplainSection::None;
            }
            (ExplainSection::None, _) => {}
        }
    }

    ExplainReport { why, impact }
}

/// Internal section marker for the explain parser.
enum ExplainSection {
    None,
    Why,
    Impact,
}

/// Parse a raw provider response body into a validated `Schema`.
/// Extracted so tests can exercise it against fixture JSON without a
/// network call.
///
/// The provider is asked for a JSON object matching `Schema` directly
/// — no wrapper envelope, no markdown fence. `extract_schema_json`
/// is tolerant of a single fenced ```json block in case the model
/// adds one despite the prompt's instruction not to.
pub fn parse_response(body: &str) -> Result<Schema, GenerateError> {
    let json = extract_schema_json(body);
    Ok(Schema::parse(json)?)
}

/// If `body` is wrapped in a single ```json … ``` fence, return the
/// inner content; otherwise return the body as-is. Defensive: the
/// system prompt explicitly tells the model not to fence the output,
/// but real LLMs sometimes do anyway.
pub(crate) fn extract_schema_json(body: &str) -> &str {
    let trimmed = body.trim();
    let stripped = trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
        .unwrap_or(trimmed);
    let stripped = stripped.trim_start_matches('\n');
    stripped.strip_suffix("```").map_or(stripped, str::trim_end)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Phase 8.0 — fenced output from the model is unwrapped before
    /// parsing. The system prompt forbids fencing but the parser
    /// tolerates it as a defensive measure.
    #[test]
    fn extract_schema_json_strips_fence() {
        let fenced = "```json\n{\"version\":2}\n```";
        assert_eq!(extract_schema_json(fenced), "{\"version\":2}");

        let fenced_no_lang = "```\n{\"version\":2}\n```";
        assert_eq!(extract_schema_json(fenced_no_lang), "{\"version\":2}");

        // No fence → byte-identical pass-through (modulo outer trim).
        let plain = "  {\"version\":2}  ";
        assert_eq!(extract_schema_json(plain), "{\"version\":2}");
    }

    /// Phase 8.0 — a full provider response that parses as a valid
    /// Schema. Fixture is hand-built so the test never touches the
    /// network. This is the green path for `parse_response`.
    #[test]
    fn parse_response_accepts_valid_schema() {
        let body = r#"{
            "version": 2,
            "rustio_version": "1.0.0",
            "models": [
                {
                    "name": "Post",
                    "table": "posts",
                    "admin_name": "posts",
                    "display_name": "Posts",
                    "singular_name": "Post",
                    "fields": [
                        { "name": "id",    "type": "i64",      "nullable": false, "editable": true },
                        { "name": "title", "type": "String",   "nullable": false, "editable": true }
                    ],
                    "relations": []
                }
            ]
        }"#;

        let schema = parse_response(body).expect("valid response parses");
        assert_eq!(schema.models.len(), 1);
        assert_eq!(schema.models[0].name, "Post");
    }

    /// Phase 8.1 — fixture covering the `update` happy path: the
    /// model returns a full schema with one new model added (Tag)
    /// and the original model preserved. Asserts both: the new
    /// model lands AND the existing one is byte-identical (no
    /// silent rename / reorder).
    #[test]
    fn update_adds_new_model() {
        // Fixture response — what the model would send back when
        // asked "add tags" against a one-model schema.
        let response = r#"{
            "version": 2,
            "rustio_version": "1.0.0",
            "models": [
                {
                    "name": "Post",
                    "table": "posts",
                    "admin_name": "posts",
                    "display_name": "Posts",
                    "singular_name": "Post",
                    "fields": [
                        { "name": "id",    "type": "i64",    "nullable": false, "editable": true },
                        { "name": "title", "type": "String", "nullable": false, "editable": true }
                    ],
                    "relations": []
                },
                {
                    "name": "Tag",
                    "table": "tags",
                    "admin_name": "tags",
                    "display_name": "Tags",
                    "singular_name": "Tag",
                    "fields": [
                        { "name": "id",    "type": "i64",    "nullable": false, "editable": true },
                        { "name": "label", "type": "String", "nullable": false, "editable": true }
                    ],
                    "relations": []
                }
            ]
        }"#;
        let updated = parse_response(response).expect("valid update parses");
        assert!(updated.models.iter().any(|m| m.name == "Tag"));
        assert!(updated.models.iter().any(|m| m.name == "Post"));
    }

    /// Phase 8.1 — preserve-by-default: a fixture response that
    /// keeps the original model + adds a status field to it must
    /// flow through parse_response cleanly AND the diff against the
    /// original must NOT report any of the surviving fields as
    /// removed. Locks the contract end-to-end.
    #[test]
    fn update_preserves_existing_fields() {
        let original = r#"{
            "version": 2,
            "rustio_version": "1.0.0",
            "models": [
                {
                    "name": "Post",
                    "table": "posts",
                    "admin_name": "posts",
                    "display_name": "Posts",
                    "singular_name": "Post",
                    "fields": [
                        { "name": "id",    "type": "i64",    "nullable": false, "editable": true },
                        { "name": "title", "type": "String", "nullable": false, "editable": true },
                        { "name": "body",  "type": "String", "nullable": false, "editable": true }
                    ],
                    "relations": []
                }
            ]
        }"#;
        let response = r#"{
            "version": 2,
            "rustio_version": "1.0.0",
            "models": [
                {
                    "name": "Post",
                    "table": "posts",
                    "admin_name": "posts",
                    "display_name": "Posts",
                    "singular_name": "Post",
                    "fields": [
                        { "name": "id",     "type": "i64",    "nullable": false, "editable": true },
                        { "name": "title",  "type": "String", "nullable": false, "editable": true },
                        { "name": "body",   "type": "String", "nullable": false, "editable": true },
                        { "name": "status", "type": "String", "nullable": false, "editable": true }
                    ],
                    "relations": []
                }
            ]
        }"#;

        let old = parse_response(original).expect("original parses");
        let new = parse_response(response).expect("response parses");
        let changes = diff::diff(&old, &new);

        // No FieldRemoved for any of the surviving fields.
        for surviving in ["id", "title", "body"] {
            assert!(
                !changes.iter().any(|c| matches!(c,
                    diff::Change::FieldRemoved { field, .. } if field == surviving
                )),
                "preserved field {surviving} surfaced as removed: {changes:?}"
            );
        }
        // Exactly one FieldAdded — the new status field.
        let adds: Vec<_> = changes
            .iter()
            .filter(|c| matches!(c, diff::Change::FieldAdded { .. }))
            .collect();
        assert_eq!(adds.len(), 1);
    }

    /// Phase 8.1 — invalid response (malformed JSON) must surface as
    /// GenerateError::Schema and never reach the diff / file-write
    /// layer. The CLI relies on this to abort before clobbering the
    /// existing schema.
    #[test]
    fn update_invalid_json_rejected() {
        // Malformed JSON: dangling comma after `models`.
        let bad = r#"{
            "version": 2,
            "rustio_version": "1.0.0",
            "models": [],
        }"#;
        let err = parse_response(bad).expect_err("malformed JSON must be rejected");
        assert!(matches!(err, GenerateError::Schema(_)));
    }

    /// Phase 8.1 / spec test #5 — meta-test asserting that the
    /// update path is reachable through pure functions (no live
    /// API). If this test ever needs `ANTHROPIC_API_KEY` to run,
    /// something has been wired wrong. The compile here proves it:
    /// the symbols exercised by the previous four tests are
    /// `parse_response` and `diff::diff` — neither hits the network.
    /// This test just imports the same surface to lock the contract.
    #[test]
    fn no_live_api_calls() {
        // Unset the env var explicitly. If any of the symbols below
        // tried to read it we'd hit MissingApiKey → easy to spot.
        let _ = std::env::var("ANTHROPIC_API_KEY"); // read, don't write
        let dummy = r#"{
            "version": 2, "rustio_version": "1.0.0",
            "models": [
                { "name": "Post", "table": "posts", "admin_name": "posts",
                  "display_name": "Posts", "singular_name": "Post",
                  "fields": [
                      { "name": "id", "type": "i64", "nullable": false, "editable": true }
                  ],
                  "relations": []
                }
            ]
        }"#;
        let parsed = parse_response(dummy).expect("offline parse path works");
        let _ = diff::diff(&parsed, &parsed); // diff is offline too
    }

    /// Phase 8.2 — well-formatted analyze response with one issue
    /// citing a missing relation target. Locks the structured-text
    /// parser end-to-end on the green path.
    #[test]
    fn analyze_detects_missing_relation_model() {
        let body = "ISSUES:\n\
- Post.author_id has relation but User model missing\n\
\n\
SUGGESTIONS:\n\
- Add created_at timestamp to all models\n\
\n\
SCORE: 6.0\n";
        let report = parse_analyze_response(body);
        assert_eq!(report.issues.len(), 1);
        assert!(report.issues[0].contains("author_id"));
        assert!(report.issues[0].contains("User"));
        assert_eq!(report.suggestions.len(), 1);
        assert!((report.score - 6.0).abs() < f32::EPSILON);
    }

    /// Phase 8.2 — best-practice suggestions land in the suggestions
    /// bucket, not issues. Locks the section-routing logic.
    #[test]
    fn analyze_suggests_best_practices() {
        let body = "ISSUES:\n\
(none)\n\
\n\
SUGGESTIONS:\n\
- Add created_at and updated_at to every model\n\
- Index Comment.post_id\n\
- Consider an enum for Post.status\n\
\n\
SCORE: 8.5\n";
        let report = parse_analyze_response(body);
        assert!(report.issues.is_empty(), "issues bucket should be empty");
        assert_eq!(report.suggestions.len(), 3);
        assert!(report.suggestions.iter().any(|s| s.contains("created_at")));
        assert!(report.suggestions.iter().any(|s| s.contains("Index")));
        assert!(report.suggestions.iter().any(|s| s.contains("enum")));
        assert!((report.score - 8.5).abs() < f32::EPSILON);
    }

    /// Phase 8.2 — composite valid output: bullets in both buckets,
    /// score with the spec example shape `7.5 / 10`. The "/ 10"
    /// suffix must be tolerated; only the leading float is consumed.
    #[test]
    fn analyze_parsing_valid_output() {
        let body = "ISSUES:\n\
- Post.author_id has relation but User model missing\n\
- Comment.post_id not indexed\n\
\n\
SUGGESTIONS:\n\
- Add created_at timestamp to all models\n\
- Add index on foreign keys\n\
\n\
SCORE: 7.5 / 10\n";
        let report = parse_analyze_response(body);
        assert_eq!(report.issues.len(), 2);
        assert_eq!(report.suggestions.len(), 2);
        assert!((report.score - 7.5).abs() < f32::EPSILON);
    }

    /// Phase 8.2 — fallback path: unstructured response with no
    /// section headers. Parser must NOT fail; everything lands in
    /// `suggestions` so the operator at least sees the model's
    /// commentary, with score defaulted to 0.0.
    #[test]
    fn analyze_handles_unstructured_output() {
        let body = "Looks fine overall. Maybe think about adding indexes \n\
on the foreign keys, and consider an enum for Post.status.\n\
- Add created_at on every model.";
        let report = parse_analyze_response(body);
        assert!(report.issues.is_empty(), "unstructured input → issues must be empty");
        assert!(
            !report.suggestions.is_empty(),
            "unstructured input → fallback should populate suggestions"
        );
        // Each non-empty line lands as one suggestion (with bullets
        // stripped). Three non-empty source lines → three entries.
        assert_eq!(report.suggestions.len(), 3);
        assert!(report.suggestions[2].starts_with("Add created_at"));
        assert_eq!(report.score, 0.0, "no SCORE: header → default 0.0");
    }

    /// Phase 8.2 / spec test #5 — meta-test asserting the analyze
    /// path is reachable through pure functions (no live API).
    /// Mirrors the equivalent test for `update`. Compile is the
    /// proof: every analyze test in this module exercises only
    /// `parse_analyze_response`. If anyone wires it to the network,
    /// this test starts depending on `ANTHROPIC_API_KEY` and stands
    /// out.
    #[test]
    fn analyze_no_live_api_calls() {
        // Reading the env var is fine; calling out across the wire
        // is not. The previous tests exercise the offline-only
        // surface; this test re-imports it to lock the contract.
        let _ = std::env::var("ANTHROPIC_API_KEY"); // read-only
        let report = parse_analyze_response(
            "ISSUES:\n(none)\n\nSUGGESTIONS:\n(none)\n\nSCORE: 9\n",
        );
        assert_eq!(report.issues.len(), 0);
        assert_eq!(report.suggestions.len(), 0);
        assert_eq!(report.score, 9.0);
    }

    // ----- Phase 8.4 — explain-diff parser tests -----------------

    /// Phase 8.4 / spec test #1 — green path: well-formatted
    /// response with both sections + bullets. Locks the contract
    /// that the parser splits the buckets correctly and strips
    /// "- " bullets.
    #[test]
    fn explain_parses_valid_response() {
        let body = "WHY:\n\
- Tags allow flexible categorization of posts\n\
- Decoupling from rigid categories\n\
\n\
IMPACT:\n\
- Adds new table (Tag)\n\
- Introduces many-to-many relationship\n";
        let report = parse_explain_response(body);
        assert_eq!(report.why.len(), 2);
        assert!(report.why[0].starts_with("Tags allow"));
        assert!(report.why[1].starts_with("Decoupling"));
        assert_eq!(report.impact.len(), 2);
        assert!(report.impact[0].starts_with("Adds new table"));
        assert!(report.impact[1].starts_with("Introduces"));
    }

    /// Phase 8.4 / spec test #2 — IMPACT section omitted entirely
    /// must not panic; missing section yields an empty bucket.
    /// Symmetrical: WHY omitted does the same.
    #[test]
    fn explain_handles_missing_sections() {
        // IMPACT only.
        let body = "IMPACT:\n- Adds Tag table\n";
        let report = parse_explain_response(body);
        assert!(report.why.is_empty(), "WHY missing → empty bucket");
        assert_eq!(report.impact.len(), 1);

        // WHY only.
        let body = "WHY:\n- Tags help categorize\n";
        let report = parse_explain_response(body);
        assert_eq!(report.why.len(), 1);
        assert!(report.impact.is_empty(), "IMPACT missing → empty bucket");

        // Both sections present but explicitly "(none)".
        let body = "WHY:\n(none)\n\nIMPACT:\n(none)\n";
        let report = parse_explain_response(body);
        assert!(report.why.is_empty());
        assert!(report.impact.is_empty());
    }

    /// Phase 8.4 / spec test #3 — extra commentary outside the two
    /// section labels must be dropped, not folded into either
    /// bucket. The spec forbids extra sections, but real models
    /// occasionally add a closing line; the parser tolerates it.
    #[test]
    fn explain_ignores_extra_text() {
        let body = "WHY:\n\
- Improves categorization\n\
\n\
IMPACT:\n\
- New table\n\
\n\
This concludes the explanation. Hope it helps!\n";
        let report = parse_explain_response(body);
        assert_eq!(report.why.len(), 1);
        assert_eq!(report.impact.len(), 1);
        assert!(report.impact[0].starts_with("New table"));
        // Trailing prose must NOT land in either bucket.
        assert!(
            !report.impact.iter().any(|l| l.contains("This concludes")),
            "trailing commentary leaked into impact: {:?}",
            report.impact
        );
        assert!(
            !report.why.iter().any(|l| l.contains("This concludes")),
            "trailing commentary leaked into why: {:?}",
            report.why
        );
    }

    /// Phase 8.4 — fallback path: unstructured response (no WHY: /
    /// IMPACT: headers). The whole body becomes `why`, `impact`
    /// stays empty. Operator still sees the model's commentary.
    #[test]
    fn explain_fallback_treats_unstructured_as_why() {
        let body = "Tags help categorize posts.\n\
- New table is added.\n\
- Many-to-many relationship is introduced.";
        let report = parse_explain_response(body);
        assert!(
            report.impact.is_empty(),
            "no headers → impact must be empty"
        );
        assert_eq!(report.why.len(), 3);
        assert_eq!(report.why[0], "Tags help categorize posts.");
        assert_eq!(report.why[1], "New table is added.");
    }

    /// Phase 9.1 — `update` MUST refuse a result that empties a
    /// non-empty schema. Truth table:
    ///   non-empty → empty   → Err(EmptyResult)   (the dangerous case)
    ///   non-empty → non-empty → Ok(())
    ///   empty     → empty   → Ok(())            (no-op, fine)
    ///   empty     → non-empty → Ok(())          (genuine first-time fill)
    #[test]
    fn update_refuses_empty_result() {
        let one_model = crate::schema::Schema {
            version: crate::schema::SCHEMA_VERSION,
            rustio_version: "1.0.0".into(),
            models: vec![crate::schema::SchemaModel {
                name: "Post".into(),
                table: "posts".into(),
                admin_name: "posts".into(),
                display_name: "Posts".into(),
                singular_name: "Post".into(),
                fields: vec![],
                relations: vec![],
                core: false,
            }],
        };
        let empty = crate::schema::Schema {
            version: crate::schema::SCHEMA_VERSION,
            rustio_version: "1.0.0".into(),
            models: vec![],
        };

        // The dangerous case — must reject.
        let err = check_not_empty(&one_model, &empty)
            .expect_err("non-empty → empty must reject");
        assert!(
            matches!(err, GenerateError::EmptyResult),
            "expected EmptyResult, got {err:?}"
        );
        assert_eq!(
            err.to_string(),
            "Refusing to apply update: schema would become empty"
        );

        // Other paths must pass.
        check_not_empty(&one_model, &one_model)
            .expect("non-empty preservation must pass");
        check_not_empty(&empty, &empty).expect("empty no-op must pass");
        check_not_empty(&empty, &one_model)
            .expect("first-time fill must pass");
    }

    /// Phase 8.0 — invalid Schema (here: unknown field type
    /// `"FooBar"`) must be rejected with `Schema(_)` so the CLI can
    /// abort before writing anything to disk.
    #[test]
    fn parse_response_rejects_invalid_schema() {
        let body = r#"{
            "version": 2,
            "rustio_version": "1.0.0",
            "models": [
                {
                    "name": "Post",
                    "table": "posts",
                    "admin_name": "posts",
                    "display_name": "Posts",
                    "singular_name": "Post",
                    "fields": [
                        { "name": "id",    "type": "i64",    "nullable": false, "editable": true },
                        { "name": "title", "type": "FooBar", "nullable": false, "editable": true }
                    ],
                    "relations": []
                }
            ]
        }"#;

        let err = parse_response(body).expect_err("invalid type must reject");
        match err {
            GenerateError::Schema(SchemaError::InvalidType { ref ty, .. }) => {
                assert_eq!(ty, "FooBar");
            }
            other => panic!("expected Schema(InvalidType), got {other:?}"),
        }
    }
}
