//! Phase 8.0 — prompt construction for the AI schema generator.
//!
//! The prompt is split deliberately:
//!
//! - The **system** prompt locks down the output contract. It states
//!   the role, names the exact JSON shape (matching `schema::Schema`),
//!   enumerates the allowed field types, and forbids fencing /
//!   commentary. This is the deterministic part — same template every
//!   call.
//! - The **user** prompt carries the operator's prose request and the
//!   reminder that ONLY the JSON document should be returned.
//!
//! Tests snapshot the system prompt so a future drift (intentional or
//! not) is visible in a diff. Live API calls are out of scope.

use crate::schema::{SCHEMA_VERSION, VALID_TYPE_NAMES};

/// The system prompt sent on every `rustio ai generate` call. Built
/// from constants in `schema.rs` so the allowed-type list and version
/// number stay in sync with the rest of the codebase — drift between
/// the prompt and the validator is impossible.
pub fn system_prompt() -> String {
    let valid_types = VALID_TYPE_NAMES.join(", ");
    format!(
        "You are RustIO's schema generator. Your sole job is to translate a developer's \
prose description of a system into a single JSON document matching RustIO's \
`Schema` type.

OUTPUT CONTRACT — read carefully:

1. Reply with ONE valid JSON object and nothing else. No markdown \
fences, no prose, no comments, no leading or trailing text.

2. The top-level shape MUST be:
   {{
     \"version\": {version},
     \"rustio_version\": \"1.0.0\",
     \"models\": [ ... ]
   }}

3. Each model in `models` MUST have these keys:
   - name: PascalCase Rust type name (e.g. \"Post\")
   - table: snake_case plural SQL table name (e.g. \"posts\")
   - admin_name: snake_case plural admin slug, often == table
   - display_name: human-readable plural (e.g. \"Posts\")
   - singular_name: human-readable singular (e.g. \"Post\")
   - fields: array of SchemaField objects
   - relations: empty array []

4. Every model MUST start with a primary-key field:
   {{ \"name\": \"id\", \"type\": \"i64\", \"nullable\": false, \"editable\": true }}

5. Each field in `fields` MUST have:
   - name: snake_case identifier
   - type: ONE of [{valid_types}]
   - nullable: bool
   - editable: bool (false for `id`, `created_at`, `updated_at`; true otherwise)

6. Audit fields convention: include `created_at` and `updated_at` of \
type `DateTime` with `editable: false` on every model.

7. Foreign keys: declare an `<other>_id` field of type `i64`, then add \
a `relation` object on it:
   {{
     \"name\": \"author_id\",
     \"type\": \"i64\",
     \"nullable\": false,
     \"editable\": true,
     \"relation\": {{ \"model\": \"User\", \"field\": \"id\", \"kind\": \"belongs_to\" }}
   }}

8. Do NOT invent types outside [{valid_types}].

9. Do NOT emit `core: true` on any model — that flag is reserved for \
framework-internal models.

10. Output the JSON document on a single object, properly formatted, \
parseable by `serde_json::from_str`. Pretty-print is welcome but not \
required.",
        version = SCHEMA_VERSION,
        valid_types = valid_types,
    )
}

/// The user-side message wrapping the operator's prose. The reminder
/// suffix is the last thing the model reads and is intentionally
/// stricter than anything in the system prompt — Anthropic models in
/// particular respect a closing instruction.
pub fn build_user_prompt(prose: &str) -> String {
    format!(
        "Generate a RustIO `Schema` for the following description.\n\n\
DESCRIPTION:\n{prose}\n\n\
Reply with ONLY the JSON document. No fences, no prose, no commentary."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Phase 8.0 — snapshot the load-bearing pieces of the system
    /// prompt. We don't stringify the whole template (which would
    /// turn cosmetic edits into test failures); we lock the parts
    /// that determine output shape:
    ///
    ///   - the schema-version number must come from `SCHEMA_VERSION`,
    ///   - the allowed-type list must come from `VALID_TYPE_NAMES`,
    ///   - the no-fence / no-prose contract must be present.
    ///
    /// Drift in any of these is a real bug — the model would either
    /// produce schemas the validator rejects (wrong version / type)
    /// or wrap them in markdown the parser has to strip.
    #[test]
    fn system_prompt_carries_version_and_type_list() {
        let p = system_prompt();
        assert!(
            p.contains(&format!("\"version\": {SCHEMA_VERSION}")),
            "system prompt missing schema version literal"
        );
        for ty in VALID_TYPE_NAMES {
            assert!(
                p.contains(ty),
                "system prompt missing allowed type {ty:?}; full prompt:\n{p}"
            );
        }
        assert!(
            p.contains("No markdown fences"),
            "system prompt missing the no-fence contract"
        );
        assert!(
            p.contains("created_at") && p.contains("updated_at"),
            "system prompt missing audit-field convention"
        );
    }

    /// Phase 8.0 — the user prompt must echo the operator's prose
    /// verbatim and end with the reminder. The reminder is
    /// load-bearing: removing it lets the model add commentary that
    /// breaks `parse_response`.
    #[test]
    fn user_prompt_includes_prose_and_reminder() {
        let p = build_user_prompt("blog system with posts and users");
        assert!(p.contains("blog system with posts and users"));
        assert!(
            p.contains("ONLY the JSON document"),
            "user prompt missing the closing reminder"
        );
    }
}
