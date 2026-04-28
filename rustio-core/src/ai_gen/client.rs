//! Phase 8.0 — Anthropic Messages API client. Used ONLY by
//! `rustio ai generate` (developer CLI). Not invoked at runtime.
//!
//! The client is intentionally minimal: one POST to /v1/messages,
//! returning the assistant's `content[0].text`. No streaming, no
//! retries, no caching — the operator runs `rustio ai generate` once
//! per schema, sees the result, decides what to do.

use serde::{Deserialize, Serialize};

use super::prompts;

/// Anthropic API base. Configurable via env so tests / staging
/// environments can point at a local mock without code changes.
const DEFAULT_BASE_URL: &str = "https://api.anthropic.com";

/// Anthropic API version pin. Bump together with the model id when
/// upgrading. Documented at https://docs.anthropic.com/en/api/versioning.
const ANTHROPIC_VERSION: &str = "2023-06-01";

/// Default model. The schema generator is a structured-output task,
/// so the trade-off is clarity over latency — Sonnet is a sensible
/// floor; tools / claude_4_x will pick something cheaper if needed.
const DEFAULT_MODEL: &str = "claude-sonnet-4-5";

/// Cap on tokens per response. Schemas are small; 4 000 is comfortable
/// for a 10-model design and keeps cost bounded if the model rambles.
const MAX_TOKENS: u32 = 4096;

/// Request body shape for POST /v1/messages.
#[derive(Serialize)]
struct MessagesRequest<'a> {
    model: &'a str,
    max_tokens: u32,
    system: String,
    messages: Vec<Message>,
}

#[derive(Serialize)]
struct Message {
    role: &'static str,
    content: String,
}

/// Response body shape (only the bits we consume). Anthropic returns a
/// `content` array of typed blocks; for our prompt we only ever expect
/// a single `{type: "text", text: ...}`.
#[derive(Deserialize)]
struct MessagesResponse {
    content: Vec<ContentBlock>,
}

#[derive(Deserialize)]
struct ContentBlock {
    #[serde(default)]
    text: String,
    #[serde(rename = "type", default)]
    block_type: String,
}

/// Make one Anthropic Messages API call and return the assistant's
/// text reply. Caller is `ai_gen::generate`, which feeds the result
/// into `parse_response`. Errors are stringified at this boundary —
/// the upper layer wraps them in `GenerateError::Transport`.
pub async fn request(api_key: &str, prose: &str) -> Result<String, String> {
    let base = std::env::var("ANTHROPIC_API_BASE")
        .unwrap_or_else(|_| DEFAULT_BASE_URL.to_string());
    let model = std::env::var("RUSTIO_AI_MODEL").unwrap_or_else(|_| DEFAULT_MODEL.to_string());

    let body = MessagesRequest {
        model: &model,
        max_tokens: MAX_TOKENS,
        system: prompts::system_prompt(),
        messages: vec![Message {
            role: "user",
            content: prompts::build_user_prompt(prose),
        }],
    };

    let url = format!("{base}/v1/messages");
    let client = reqwest::Client::new();
    let resp = client
        .post(&url)
        .header("x-api-key", api_key)
        .header("anthropic-version", ANTHROPIC_VERSION)
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("request to {url} failed: {e}"))?;

    let status = resp.status();
    let raw = resp
        .text()
        .await
        .map_err(|e| format!("read response body: {e}"))?;
    if !status.is_success() {
        return Err(format!("HTTP {status}: {raw}"));
    }

    let parsed: MessagesResponse = serde_json::from_str(&raw)
        .map_err(|e| format!("invalid Anthropic response (HTTP {status}): {e}; body: {raw}"))?;

    let text = parsed
        .content
        .into_iter()
        .find(|b| b.block_type == "text" && !b.text.is_empty())
        .ok_or_else(|| "Anthropic response had no text content block".to_string())?
        .text;

    Ok(text)
}
