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
    let api_key = std::env::var("ANTHROPIC_API_KEY")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .ok_or(GenerateError::MissingApiKey)?;

    let body = client::request(&api_key, prompt)
        .await
        .map_err(|e| GenerateError::Transport(e.to_string()))?;

    parse_response(&body)
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
