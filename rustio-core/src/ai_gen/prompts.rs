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
   {{ \"name\": \"id\", \"type\": \"i64\", \"nullable\": false, \"editable\": false }}

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

/// Phase 8.1 — system prompt for the **update** path. Distinct from
/// the generator system prompt because the contract is different:
/// here we hand the model an existing schema and ask it to evolve it
/// minimally, NOT to redesign from scratch. The "preserve by default"
/// rule is the load-bearing one — without it the model tends to
/// rewrite naming or drop fields it doesn't see referenced in the
/// instruction.
pub fn system_prompt_update() -> String {
    let valid_types = VALID_TYPE_NAMES.join(", ");
    format!(
        "You are RustIO's schema editor. The user gives you an existing \
`Schema` JSON document and a free-form instruction. Your job is to \
return the FULL updated `Schema` JSON, applying the requested change \
and PRESERVING everything else byte-for-byte.

PRESERVE-BY-DEFAULT — this is the most important rule:

1. NEVER remove a model unless the instruction explicitly says to.
2. NEVER remove a field unless the instruction explicitly says to.
3. NEVER rename a model or field unless the instruction explicitly says to.
4. NEVER change a field's `type`, `nullable`, or `editable` unless \
the instruction explicitly says to.
5. NEVER reorder fields or models for cosmetic reasons.

When in doubt, leave the existing structure as-is.

OUTPUT CONTRACT — same as the generator path:

1. Reply with ONE valid JSON object and nothing else. No markdown \
fences, no prose, no comments, no leading or trailing text.

2. The top-level shape MUST stay:
   {{
     \"version\": {version},
     \"rustio_version\": \"1.0.0\",
     \"models\": [ ... ]
   }}

3. Each model in `models` MUST have:
   - name (PascalCase), table (snake_case plural),
   - admin_name, display_name, singular_name,
   - fields (array), relations (empty array []).

4. Every model's first field MUST be:
   {{ \"name\": \"id\", \"type\": \"i64\", \"nullable\": false, \"editable\": false }}

5. Field types MUST be one of [{valid_types}]. Do NOT invent types.

6. Audit fields convention: keep `created_at` / `updated_at` of type \
`DateTime` with `editable: false` on every model that already has them.

7. Foreign keys: declare an `<other>_id` field of type `i64`, then add \
a `relation` object on it:
   {{ \"name\": \"author_id\", \"type\": \"i64\", \"nullable\": false, \"editable\": true,
      \"relation\": {{ \"model\": \"User\", \"field\": \"id\", \"kind\": \"belongs_to\" }} }}

8. Do NOT emit `core: true` on any model.

The output MUST be parseable by `serde_json::from_str` AND pass \
RustIO's `Schema::validate()`. If the requested change would violate \
any of these constraints, apply the closest variant that does pass — \
do NOT refuse and do NOT explain.",
        version = SCHEMA_VERSION,
        valid_types = valid_types,
    )
}

/// Phase 8.1 — wraps the existing schema JSON + the operator's
/// instruction. Closing reminder mirrors the generator path so the
/// model produces the same shape regardless of which prompt path
/// fires.
pub fn build_user_update_prompt(existing_json: &str, instruction: &str) -> String {
    format!(
        "Here is an existing schema:\n\
<JSON>\n\
{existing_json}\n\
</JSON>\n\n\
Apply the following change:\n\
{instruction}\n\n\
Return the FULL updated schema JSON only. No fences, no prose, no commentary. \
Preserve every model and field that the change does not explicitly affect."
    )
}

/// Phase 8.2 — system prompt for the **analyze** path. Read-only: the
/// model audits a schema and returns issues + suggestions + score in
/// a structured-text format that the parser can split by section
/// headers. NOT JSON because LLMs are inconsistent at producing valid
/// JSON for free-text analyses (especially the score field — strict
/// JSON requires a number, but models occasionally write `7.5/10` as
/// a string and break the parse).
pub fn system_prompt_analyze() -> String {
    "You are RustIO's schema auditor. The user gives you an existing \
`Schema` JSON document. Your job is to read it and return a short \
written analysis. You DO NOT generate, modify, or rewrite the schema. \
You DO NOT propose code. You DO NOT invent models or fields the \
schema doesn't already declare.

OUTPUT CONTRACT — read carefully:

Reply with EXACTLY three sections, in this order, separated by blank \
lines. Each section header is on its own line, followed by content.

ISSUES:
- one issue per line, prefixed with \"- \"
- empty line OR \"(none)\" if no issues found

SUGGESTIONS:
- one suggestion per line, prefixed with \"- \"
- empty line OR \"(none)\" if nothing to suggest

SCORE: <number between 0 and 10, one decimal allowed, e.g. 7.5>

Rules:

- ISSUES are real problems: contradictions, missing relation targets, \
fields that violate RustIO conventions, broken patterns. Cite the \
exact `Model.field` or `Model` in each line so the developer can \
locate the problem.
- SCHEMA SHAPE NOTE — DO NOT flag the following as an issue: every \
`SchemaModel` has a top-level `relations: []` array that is \
intentionally empty. The actual foreign-key metadata lives at the \
field level (`field.relation`), and the per-field shape is the \
authoritative source of truth. The empty `model.relations` array is \
a reserved slot in the wire format, not a contradiction.
- SUGGESTIONS are best-practice improvements: missing audit fields, \
absent indexes, unclear naming, opportunities to introduce enums. \
Be concrete. Cite specific models / fields.
- SCORE reflects schema quality on the 0-10 scale: 10 = production-\
ready, 5 = workable but rough, 0 = unusable.
- DO NOT add a fourth section.
- DO NOT use markdown fences or headings other than the three \
section labels above.
- DO NOT write commentary outside the three sections.
- Be concise. Each issue / suggestion fits on one line; no \
multi-paragraph explanations.

If the schema looks empty or trivial, say so in ISSUES and give a \
score below 5. If the schema is excellent, ISSUES can be \"(none)\" \
and the score should be at or near 10.".to_string()
}

/// Phase 8.2 — wraps the schema JSON for the analyze path. Mirrors
/// the closing-reminder pattern used by the generate / update prompts.
pub fn build_user_analyze_prompt(existing_json: &str) -> String {
    format!(
        "Here is a schema:\n\
<JSON>\n\
{existing_json}\n\
</JSON>\n\n\
Analyze it and return:\n\
1. Issues (errors or inconsistencies)\n\
2. Suggestions (improvements or best practices)\n\
3. Score (0-10)\n\n\
Use the ISSUES / SUGGESTIONS / SCORE section format from the system \
instructions. Do NOT modify, regenerate, or quote the schema back."
    )
}

/// Phase 8.4 — system prompt for the **explain-diff** path. The
/// model receives the BEFORE + AFTER schemas and must explain ONLY
/// what changed: the rationale (WHY) and the consequences (IMPACT).
///
/// Strict by design: no further suggestions, no invented features,
/// no rewrites. The output is plain text in two sections so the
/// parser can split deterministically; this is the same trade-off
/// as `system_prompt_analyze` (LLMs are flaky at JSON for free-text
/// content).
pub fn system_prompt_explain() -> String {
    "You are RustIO's diff narrator. The user gives you two schemas: a \
BEFORE document and an AFTER document. Your job is to explain what \
changed between them — and ONLY what changed.

STRICT CONTRACT — read carefully:

1. DO NOT invent features, models, or fields that aren't in the diff.
2. DO NOT suggest further changes or improvements. Other commands \
(`ai analyze`) handle suggestions.
3. DO NOT echo or rewrite the schema. The user already has both.
4. DO NOT comment on parts of the schema that did NOT change.
5. Be concrete: cite the exact `Model` or `Model.field` each line \
talks about.
6. Be concise: one sentence per bullet, no multi-paragraph prose.

OUTPUT CONTRACT:

Reply with EXACTLY two sections, in this order, separated by a blank \
line. Each section header is on its own line, followed by bullet \
items.

WHY:
- one line per reason; explain why this change improves the schema
- empty section OR \"(none)\" is acceptable when nothing meaningful \
can be said

IMPACT:
- one line per consequence; data-model implications (new tables, \
new joins, FK additions, nullability shifts, etc.)
- empty section OR \"(none)\" is acceptable

Rules:

- Bullet lines start with \"- \" or \"* \".
- DO NOT use markdown fences or headings other than the two section \
labels above.
- DO NOT add a third section.
- DO NOT write commentary outside the two sections.

If the diff is empty (BEFORE == AFTER), say so in WHY with one line \
and leave IMPACT as \"(none)\".".to_string()
}

/// Phase 8.4 — wraps the BEFORE + AFTER schemas for the explain
/// path. The closing reminder repeats the no-suggestions rule —
/// load-bearing because models often slip into "and you might also
/// want..." territory unprompted.
pub fn build_user_explain_prompt(old_json: &str, new_json: &str) -> String {
    format!(
        "Here are the BEFORE and AFTER schemas. Explain ONLY what changed.\n\n\
BEFORE:\n\
<JSON>\n\
{old_json}\n\
</JSON>\n\n\
AFTER:\n\
<JSON>\n\
{new_json}\n\
</JSON>\n\n\
Use the WHY / IMPACT section format. Do NOT suggest further changes. \
Do NOT comment on unchanged parts. One sentence per bullet."
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

    /// Phase 8.1 — the update system prompt's load-bearing rule is
    /// "preserve by default". Without it the model rewrites or drops
    /// fields that weren't mentioned in the instruction. Snapshot
    /// the rule + the type list + the version pin.
    #[test]
    fn update_system_prompt_carries_preserve_contract() {
        let p = system_prompt_update();
        assert!(
            p.contains("PRESERVE-BY-DEFAULT"),
            "update prompt missing the preserve-by-default contract"
        );
        assert!(
            p.contains("NEVER remove a model"),
            "update prompt missing the no-remove-model rule"
        );
        assert!(
            p.contains("NEVER remove a field"),
            "update prompt missing the no-remove-field rule"
        );
        assert!(
            p.contains(&format!("\"version\": {SCHEMA_VERSION}")),
            "update prompt missing schema version pin"
        );
        for ty in VALID_TYPE_NAMES {
            assert!(p.contains(ty), "update prompt missing allowed type {ty:?}");
        }
    }

    /// Phase 8.1 — the user-update prompt must include the existing
    /// schema verbatim AND the instruction. The closing reminder
    /// re-states preserve-by-default at the last position the model
    /// reads.
    #[test]
    fn user_update_prompt_includes_schema_instruction_and_reminder() {
        let existing = r#"{"version":2,"models":[]}"#;
        let p = build_user_update_prompt(existing, "add tags");
        assert!(p.contains(existing), "existing schema must be embedded verbatim");
        assert!(p.contains("add tags"), "instruction must be embedded verbatim");
        assert!(
            p.contains("Preserve every model and field"),
            "closing reminder missing"
        );
    }

    /// Phase 8.2 — the analyze system prompt's load-bearing pieces:
    /// the read-only contract, the three-section output format, the
    /// score range. Drift in any of these breaks the parser.
    #[test]
    fn analyze_system_prompt_carries_section_contract() {
        let p = system_prompt_analyze();
        assert!(
            p.contains("DO NOT generate, modify, or rewrite the schema"),
            "analyze prompt missing the read-only contract"
        );
        assert!(p.contains("ISSUES:"), "analyze prompt missing ISSUES header");
        assert!(
            p.contains("SUGGESTIONS:"),
            "analyze prompt missing SUGGESTIONS header"
        );
        assert!(p.contains("SCORE:"), "analyze prompt missing SCORE header");
        assert!(
            p.contains("between 0 and 10"),
            "analyze prompt missing score range"
        );
    }

    /// Phase 8.2 — the user-analyze prompt embeds the schema verbatim
    /// and re-states the section format at the closing position so
    /// the model reads it last.
    #[test]
    fn user_analyze_prompt_includes_schema_and_format_reminder() {
        let existing = r#"{"version":2,"models":[]}"#;
        let p = build_user_analyze_prompt(existing);
        assert!(p.contains(existing), "schema must be embedded verbatim");
        assert!(
            p.contains("ISSUES / SUGGESTIONS / SCORE"),
            "analyze user prompt missing closing format reminder"
        );
    }

    /// Phase 8.4 — explain system prompt's load-bearing pieces:
    /// the explain-only-what-changed rule, the WHY/IMPACT format,
    /// and the no-suggestions rule (so the LLM doesn't drift into
    /// recommending further changes the user didn't ask for).
    #[test]
    fn explain_system_prompt_carries_strict_contract() {
        let p = system_prompt_explain();
        assert!(
            p.contains("ONLY what changed"),
            "explain prompt missing the only-what-changed rule"
        );
        assert!(
            p.contains("DO NOT suggest further changes"),
            "explain prompt missing no-suggestions rule"
        );
        assert!(
            p.contains("DO NOT invent features"),
            "explain prompt missing no-inventing rule"
        );
        assert!(p.contains("WHY:"), "explain prompt missing WHY header");
        assert!(p.contains("IMPACT:"), "explain prompt missing IMPACT header");
    }

    /// Phase 8.4 — the user-explain prompt embeds BOTH schemas with
    /// clear BEFORE / AFTER labels and re-states the no-suggestions
    /// rule at the closing position the model reads last.
    #[test]
    fn user_explain_prompt_includes_both_schemas_and_reminder() {
        let old = r#"{"models":[{"name":"Post"}]}"#;
        let new = r#"{"models":[{"name":"Post"},{"name":"Tag"}]}"#;
        let p = build_user_explain_prompt(old, new);
        assert!(p.contains("BEFORE:"), "must label the before schema");
        assert!(p.contains("AFTER:"), "must label the after schema");
        assert!(p.contains(old), "before schema embedded verbatim");
        assert!(p.contains(new), "after schema embedded verbatim");
        assert!(
            p.contains("Do NOT suggest further changes"),
            "closing reminder missing"
        );
    }
}
