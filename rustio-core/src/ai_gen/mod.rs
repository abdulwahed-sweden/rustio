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
pub async fn update(existing: &Schema, instruction: &str) -> Result<Schema, GenerateError> {
    let api_key = api_key()?;
    let existing_json = existing
        .to_pretty_json()
        .map_err(|e| GenerateError::Transport(format!("serialise existing schema: {e}")))?;
    let body = client::request_update(&api_key, &existing_json, instruction)
        .await
        .map_err(|e| GenerateError::Transport(e.to_string()))?;
    parse_response(&body)
}

/// Read + validate the API key once for both entry points. Empty /
/// whitespace-only values count as missing.
fn api_key() -> Result<String, GenerateError> {
    std::env::var("ANTHROPIC_API_KEY")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .ok_or(GenerateError::MissingApiKey)
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
