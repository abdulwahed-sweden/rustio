# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added — 0.7.0 Admin Intelligence Layer

The admin UI now adapts to *(schema + context)* instead of treating
every model as a generic form with a data table. Same project, same
code, different context → different admin behaviour.

- **`rustio-core::admin::intelligence`** — new module with five
  pure, deterministic helpers:
  - `classify_field(field, context) -> FieldRole` — labels a field
    (`Id`, `Timestamp`, `Bool`, `NumericCount`, `ForeignKey`,
    `Status`, `Personnummer`, `Email`, `Phone`,
    `OpaqueIdentifier`, `Money`, `PlainText`).
  - `field_ui_metadata(field, context) -> FieldUI` — packages
    label, placeholder, hint, sensitivity marker + note.
  - `infer_filters(fields, context) -> Vec<FilterDef>` —
    determines the right filter shape per column
    (`DropdownText`, `BoolYesNo`, `DateRange`, `NumericExact`,
    `ExactMatch`).
  - `classify_search(query) -> SearchIntent` — routes a query to
    one of `NumericId`, `Email`, `Personnummer`, `Text` (in that
    precedence: a 12-digit string is never an ID).
  - `mask_pii(value) -> String` — deterministic masker preserving
    length + first few chars (`"19870512-4521"` → `"1987•••••••••"`).
  - Plus `context_global()` — lazy `OnceLock` cache for
    `rustio.context.json`, mirroring the design-config pattern.
- **Form rendering** — `render_field_block` now uses
  `field_ui_metadata`. Personnummer under SE gets placeholder
  `YYYYMMDD-XXXX` and a 🔒 PII marker; patient IDs under healthcare
  get the "opaque — do not expose publicly" hint; money fields under
  banking carry the "integer minor units" hint; datetimes show
  `YYYY-MM-DDTHH:MM` + UTC. Email / phone under GDPR are flagged
  sensitive.
- **List-page masking** — `render_cell` wraps sensitive values in
  a `.rio-pii` span with `data-value` / `data-mask` attributes and
  a `.rio-pii-toggle` button. Tiny inline JS (shipped once in the
  admin shell) flips the display on click — no framework, no
  external file.
- **Delete confirmation** — when the record carries any sensitive
  field, a `rio-alert-error` banner appears above the standard
  warning: *"This record contains personal data (GDPR). Deletion
  is typically irreversible — verify you have the right to erase."*
- **Dashboard alerts** — new `rio-dashboard-alerts` section under
  the model grid. Two sources of alerts:
  - *Industry conventions:* any model that covers at least one
    required field but is missing others is flagged (e.g.
    `Applicants` missing `annual_income` under `housing`).
  - *GDPR inventory:* every model carrying PII is listed with its
    sensitive fields so operators know where retention obligations
    apply.
- **Search intent badge** — a small `.rio-search-intent` chip next
  to the search box reads "Interpreted as: ID / email /
  personnummer" so operators see the classification the list
  handler made.
- **Context-aware empty state** — empty list pages now say
  *"Start by adding your first Applicant"* and, when the project
  has an industry context, append *"In Sweden, housing applicants
  usually include personnummer, queue_start_date,
  annual_income."* The hint is silent when the model's fields
  don't intersect the industry's required list.
- **Tests** — 33 admin-intelligence tests in
  `rustio-core/src/admin/admin_intelligence_tests.rs`: every
  classifier branch (country / industry / GDPR / shape / fallback),
  sensitivity roll-up, `field_ui_metadata` per role, filter
  inference (order-preserving, id excluded), search intent (ID /
  email / personnummer precedence, negative numbers, whitespace),
  PII masking (Unicode-safe, deterministic, length-preserving).
- **Design** — additions to `assets/admin.css` only, no redesign:
  `.rio-pii`, `.rio-pii-toggle`, `.rio-field-sensitive`,
  `.rio-dashboard-alerts`, `.rio-dashboard-alert`,
  `.rio-search-intent`, `.rio-empty-hint`. The shell gained one
  30-line inline `<script>` block for the PII toggle — the first
  JS in the admin.

Smoke-tested end-to-end against `~/Desktop/sveahousing` with
`{"country":"SE","industry":"housing"}`:
- Dashboard shows GDPR + convention alerts on the Applicants
  model.
- Applicants list masks every personnummer
  (`1987•••••••••`) with a per-row *show / hide* toggle.
- `?q=42` displays "Interpreted as: ID".
- Applicant edit form shows 🔒 PII marker, placeholder
  `YYYYMMDD-XXXX`, and the Swedish-format hint.
- Delete page carries the red "Sensitive data (GDPR)" banner
  above the standard warning.

### Added — 0.6.0 Intelligence Phase, Pass 5 (Context-Aware Execution)

Makes every layer of the AI pipeline aware of *who the project is*:
country, region, industry, compliance. A prompt that resolves to `i32`
for a generic project resolves to `String` under `country=SE`; a
destructive op on a personnummer field becomes `Critical` risk and
is refused by the executor; and the CLI gains `rustio context show` /
`rustio context validate`.

#### Context shape

- **Breaking.** `ContextConfig::domain` is removed. The equivalent is
  `industry`. Old `rustio.context.json` files with `{"domain": …}`
  parse-fail loudly (deny_unknown_fields) — rename the key to
  `industry`.
- Added fields `region` (e.g. `"EU"`, explicit or inferred from
  `country`), `industry` (`"housing"`, `"healthcare"`, `"banking"`),
  and `compliance: Vec<String>` (e.g. `["GDPR"]`).
- Helper methods: `effective_region()` (infers EU from the
  27 member-state country codes), `requires_gdpr()` (explicit list
  or EU region), `pii_fields()` (country-specific + generic GDPR
  list), `industry_schema()`, `is_empty()`.

#### Industry registry

- New `rustio_core::ai::industry` module with `IndustrySchema`
  and `industry_schema_for(name)`. 0.6.0 ships three entries:
  **housing** (personnummer, queue_start_date, annual_income),
  **healthcare** (patient_id, created_at; patient IDs must be
  opaque strings), **banking** (account_number, currency,
  balance; monetary amounts as integer minor units).

#### Context threaded through every layer

- `generate_plan` already took `Option<&ContextConfig>`. Logic
  extended: SE / NO personal id aliases → `String`, healthcare
  patient id → `String`, banking account_number → `String`,
  banking balance/amount → `i64`. Explanations now cite the
  reason ("opaque identifier", "integer minor units", Swedish
  personnummer format).
- **Breaking.** `review_plan`, `classify_risk`, `warnings_for`,
  `build_plan_document`, `build_plan_document_with_timestamp`,
  `plan_execution`, `execute_plan_document` all gained a trailing
  `Option<&ContextConfig>` parameter. Tests and downstream code
  pass `None` to keep 0.5.x behaviour byte-identical.
- **Review risk escalation**: destructive / rename / retype ops on
  a context-declared PII field become `Critical` regardless of
  structural rules.
- **Review warnings**: GDPR-aware line cites the active context
  (`country=SE, industry=housing, GDPR`); industry-convention
  removals add a warning pointing at the affected convention.
- **Executor policy gate**: new
  `ExecutionError::PolicyViolation { reason: String }` fires when
  a plan targets a PII field under context — refused up-front,
  before the dry-run. The existing critical-risk gate also
  catches these (review escalates first); the policy gate is a
  dedicated refusal shape so operators diagnose the real cause,
  not "risk Critical".

#### CLI

- `rustio context show` — pretty-prints the parsed context, the
  inferred region / GDPR, every PII field the review layer
  watches, and the industry conventions (if any).
- `rustio context validate` — exit 0 if the file parses (or if
  it's absent), exit 1 with the exact `serde` error on typos.
- `rustio ai review` / `rustio ai validate` / `rustio ai apply`
  now auto-load `rustio.context.json` and thread it through the
  pipeline. No flag needed; the file's presence is the opt-in.

#### Tests

- New `rustio-core/src/ai/context_tests.rs` with 15 scenarios:
  country → EU inference, explicit GDPR override, country-scoped
  PII list, deny_unknown_fields rejection, the old `domain` key
  rejection canary, SE personnummer planning, healthcare
  patient_id planning, banking account_number planning, Critical
  escalation on PII removal / rename, industry-convention
  warning, executor policy refusal (PII remove + PII rename
  under SE), executor allows non-PII changes under SE, industry
  registry coverage, and a None-context canary that confirms
  0.5.x behaviour survives.
- Existing planner / review / executor tests updated to pass
  `None` for the new context arg — no regressions.

#### Smoke-tested end-to-end

Ran against `~/Desktop/sveahousing` with
`{"country":"SE","industry":"housing"}`:
- `rustio context show` — reports SE, EU (inferred), GDPR
  (inferred), housing conventions, required field list.
- `rustio context validate` — three scenarios (missing, valid,
  typo) all respond correctly.
- `rustio ai review` on a hand-crafted `remove_field
  personnummer` plan — Risk: Critical, warnings cite
  `(country=SE, industry=housing, GDPR)` and the
  housing-convention removal.

### Added — 0.5.3 Intelligence Phase, Pass 4 (Advanced Schema Mutations)

Extends the Safe Executor with the three primitives that require a
SQLite table-recreation migration: `change_field_type`,
`change_field_nullability`, and `rename_model`. Everything remains
refusal-first — if the shape of the plan violates the safe subset the
executor stops and reports a named `ExecutionError`.

- **SQLite recreate-table engine** — `generate_sqlite_recreate_table_migration`
  emits the canonical four-step pattern: `CREATE TABLE <t>__new (…)`,
  `INSERT INTO <t>__new (cols) SELECT exprs FROM <t>`, `DROP TABLE <t>`,
  `ALTER TABLE <t>__new RENAME TO <t>`. Column DDL preserves
  `INTEGER PRIMARY KEY AUTOINCREMENT` for `id` and applies safe type
  defaults (`0`, `''`, `CURRENT_TIMESTAMP`) to every `NOT NULL` field.
- **Foreign-key guard** — `ProjectView` now carries the contents of
  every migration file. The executor refuses recreate-table on any
  table that participates in a FK (incoming *or* outgoing); FK rewriting
  is deferred to 0.6.0 rather than silently cascading-deleting dependent
  rows.
- **`change_field_type`** — supported safe casts:
  - `i32 ↔ i64`, `bool ↔ i32/i64`: same SQLite storage, no CAST.
  - `DateTime ↔ String`: same TEXT storage, no CAST.
  - `i32/i64/bool → String`: `CAST(col AS TEXT)` — widens safely.
  - `String → i32/i64/bool`: `CAST(col AS INTEGER)` — warned but
    allowed; review flags "may truncate or fail".
  - Anything else: `UnsupportedPrimitive`.
  The Rust side updates the struct field type, the `from_row`
  accessor, and (for `String`) the `.clone()` call in `insert_values`.
  `chrono::{DateTime, Utc}` is auto-imported when introducing
  `DateTime`.
- **`change_field_nullability`**:
  - Required → nullable (relaxing): safe. Migration is a straight
    recreate-table; the Rust struct wraps the field in `Option<T>` and
    the `from_row` accessor swaps to `get_optional_*`.
  - Nullable → required (tightening): the `INSERT SELECT` substitutes
    existing NULLs with the type default via
    `COALESCE(col, <default>)`. Risk bumped to **High**. A dedicated
    warning surfaces the NULL substitution so no reviewer can miss it.
  - No-op (same state requested): refused with `FileConflict`.
- **`rename_model`** (full) — updates, in the owning app:
  - `models.rs` — struct name, `impl Model for …` header, and the
    `TABLE` constant (pluralised from the new name).
  - `admin.rs` — `use super::models::Old;` and
    `admin.model::<Old>()`.
  - `views.rs` — bounded, identifier-boundary-safe rename (no
    substring clobbers, no string-literal rewrites).
  - Migration — `ALTER TABLE old_table RENAME TO new_table;`.
  Emits a summary warning that references outside the app dir must
  be updated manually. Refuses if the target struct name already
  exists or the owning table participates in FKs.
- **Review risk upgrade** — `ChangeFieldNullability` tightening moved
  from Medium → High (reflects the NULL-substitution). Any
  table-rewriting primitive now adds the warning *"This operation
  rewrites the entire table. Large tables may cause downtime during
  execution."*
- **CLI preview glyphs** — additive operations show as `+`, mutating
  operations as `~`; recreate-table steps emit a `⚠ This rewrites …`
  indented line directly in the "Applying:" block so the operator
  sees the cost before confirming.
- **Shadow-schema simulation** — multi-step plans now see each
  other's mutations (rename field → change type on the new name works
  in a single `Plan`).
- **Tests** — 12 new advanced tests in `executor_tests_advanced.rs`:
  type-cast happy-path, unsafe cast refusal, idempotent no-op,
  FK-participating table refusal, nullability relax (no COALESCE),
  nullability tighten (COALESCE), no-op nullability, rename-model
  happy path (models + admin + views + migration), rename-model
  target-collision, recreate-table determinism, and a wide-schema
  simulation (21 columns) asserting one CAST + straight copies.

Smoke-tested end-to-end: built a scratch project, ran
`rustio ai plan "change score in notes to String" --save adv.json` →
`rustio ai apply adv.json --yes` → `rustio migrate apply` → SQLite
`PRAGMA table_info('notes')` confirms `score` is now `TEXT`,
project re-compiles clean.

### Added — 0.5.2 Intelligence Phase, Pass 3 (Safe Executor)

The first layer that turns a reviewed `PlanDocument` into real on-disk
changes. Conservative by construction — if anything is uncertain, it
refuses. Never runs migrations itself; the user runs
`rustio migrate apply` as a separate step.

- **`rustio-core::ai::executor`** — new module with
  `plan_execution` (pure), `execute_plan_document` (impure wrapper),
  `render_preview_human`, and builder `ProjectView::from_dir`.
- **`ExecutionPreview`** / **`PlannedFileChange`** — the dry-run shape.
  Every file the executor will write is listed with its target kind
  (`Create` | `Update`) + the full new contents, so the CLI can print
  a real preview before asking the operator to confirm.
- **`ExecutionResult`** — post-apply summary: step count, generated
  file paths (relative to project root), one-line summary per step.
- **`ExecutionError`** — named refusals (`ValidationFailed`,
  `CriticalRiskNotAllowed`, `DeveloperOnlyForbidden`,
  `SchemaMismatch`, `FileConflict`, `UnsupportedPrimitive`,
  `DestructiveWithoutConfirmation`, `ProjectStructure`, `IoError`).
  No silent fallbacks anywhere.
- **Supported primitives (0.5.2):**
  - `AddField` — patches `struct`, `COLUMNS`, `INSERT_COLUMNS`,
    `from_row`, `insert_values` in the owning `apps/<app>/models.rs`;
    emits `ALTER TABLE … ADD COLUMN …` with a safe `NOT NULL
    DEFAULT` for required fields (`''`, `0`, `CURRENT_TIMESTAMP` by
    type). Adds `use chrono::{DateTime, Utc};` automatically when a
    `DateTime` field is introduced and the import is missing.
  - `RenameField` — scoped rename across the same five sections plus
    `ALTER TABLE … RENAME COLUMN`.
- **Refused primitives (0.5.2):** `RenameModel`, `ChangeFieldType`,
  `ChangeFieldNullability`, `AddModel`, `AddRelation`, `UpdateAdmin`
  with explicit `UnsupportedPrimitive { op, reason }`. `RemoveField`,
  `RemoveModel`, `RemoveRelation` return
  `DestructiveWithoutConfirmation`. `CreateMigration` hits
  `DeveloperOnlyForbidden`.
- **Safety pipeline on every apply:**
  1. Re-run `review_plan(&current_schema, &plan)` — stale plans are
     rejected with the exact failing step index.
  2. Re-run the risk classifier — `Critical` is refused.
  3. Developer-only gate — belt and suspenders on top of the
     review layer's own check.
  4. Dry-run the full change set against an in-memory project
     shadow (so two steps on the same file see each other's edits).
  5. Precondition pass against the live filesystem — refuse to
     overwrite changed files or duplicate existing ones.
  6. Atomic commit — write every target to a sibling `.rustio_tmp`
     file first, then rename each into place. A mid-flight failure
     restores already-renamed targets from in-memory snapshots of
     the pre-apply contents.
- **Idempotency:** `struct_declares_field` / column-list / accessor
  checks catch "the plan was already applied" and surface a precise
  `FileConflict` with the colliding name.
- **Deterministic migration naming:** `NNNN_<slug>.sql` where `NNNN`
  is `max(existing) + 1` and the slug is primitive-specific
  (`add_<field>_to_<table>`, `rename_<from>_to_<to>_on_<table>`).
  Every migration file carries a
  `-- Generated by rustio ai apply (0.5.2). DO NOT EDIT.` header.
- **CLI:** `rustio ai apply <path> [--yes] [--dry-run]`
  - Prints a "Plan to apply" preview with the exact list of files.
  - Refuses to run on a non-TTY stdin without `--yes`.
  - With `--dry-run`, stops after the preview.
  - On success, prints the "applied / wrote" summary and the
    `rustio migrate apply` hint; never runs migrations itself.
- **Tests:** 18 executor tests covering simple and datetime
  `AddField`, migration numbering with gaps, `RenameField` across
  all five patched sections, validation / risk / developer-only /
  destructive / unsupported gates, stale-plan detection,
  idempotency, deterministic previews, human rendering, and three
  temp-dir integration tests for the atomic commit path.

Smoke-tested end-to-end against `~/Desktop/sveahousing`:
`rustio ai plan "add phone to applicants" --save plan.json` →
`rustio ai apply plan.json --yes` → `rustio migrate apply` →
`cargo build` clean, `applicants.phone` column present in the
live SQLite DB.

### Added — 0.5.1 Intelligence Phase, Pass 2 (Plan Review Layer)

The reviewable, risk-scored boundary between the AI planner and the
(future) executor. Pure inspection — no filesystem, no database, no
SQL, no execution.

- **`rustio-core::ai::review`** — new module with
  `build_plan_document`, `build_plan_document_with_timestamp`,
  `review_plan`, `load_plan`, `compute_impact`, `classify_risk`,
  `warnings_for`, `render_review_human`, `render_plan_document_json`.
- **`PlanDocument`** (`version = 1`, `#[serde(deny_unknown_fields)]`)
  — the saved on-disk shape. Carries prompt, explanation, risk,
  impact, plan, and an RFC 3339 timestamp. Unknown fields are
  rejected; document version mismatches fail loudly with
  `ReviewError::UnknownVersion`.
- **`RiskLevel`** — four-tier closed enum (`Low`, `Medium`, `High`,
  `Critical`) with `Ord` so risks can be combined. Conservative by
  design: every edge case bumps *up*, never down.
- **`PlanImpact`** — mechanical counts (`adds_fields`,
  `removes_fields`, `renames`, `type_changes`,
  `nullability_changes`, `touches_core_models`, `destructive`).
- **`PlanReview`** / `ValidationOutcome` — always-populated report,
  even for invalid plans. Invalid plans carry the failing step
  index + the exact `PrimitiveError` so stale-plan detection can
  point at the right primitive.
- **Risk rules** — `AddField`, `AddModel`, `AddRelation`, flipping
  nullable ON, `UpdateAdmin` → Low. `RenameField`, `RenameModel`,
  `ChangeFieldType`, flipping nullable OFF → Medium. `RemoveField`,
  `RemoveModel`, `RemoveRelation` → High. Core-model touching,
  failed validation, `CreateMigration` in a plan → Critical.
  Mixing add+remove in one plan forces at least High.
- **Deterministic warnings**: removing a field, renaming a model,
  renaming a field, flipping to required, changing a type,
  multi-step plans, developer-only primitives — each triggered by
  a concrete plan shape, never speculative.
- **Stale-plan detection** — `review_plan` re-validates against the
  current schema and reports exactly which step broke and why.
- **CLI:**
  - `rustio ai plan "<prompt>" --save <path>` writes a
    `PlanDocument` atomically (tmp + rename) and prints a review.
  - `rustio ai review <path>` loads a saved document **or** a raw
    `Plan`, validates it against the current schema, prints an
    operator-friendly review, and exits non-zero if stale.
  - `rustio ai validate <path>` — terse CI gate: one-line output,
    exit 0 on valid, exit 1 with the failing step on invalid.
- **Tests:** 29 review-layer tests covering each risk tier,
  stale detection, multi-step plans, core-model protection,
  developer-only primitives, round-tripping a saved document,
  loading raw plans, refusing unknown document versions, refusing
  `deny_unknown_fields` violations, deterministic JSON rendering,
  and deterministic warnings.
- **Type polish:** `Primitive`, `Plan`, and every primitive struct
  now derive `PartialEq` so review and executor code can compare
  plans (and so tests can use `assert_eq!` on them).

### Added — 0.5.0 Intelligence Phase, Pass 1 (AI planning layer)

A read-only, rule-based AI planner. Reads a natural-language prompt,
the project's `rustio.schema.json`, and an optional
`rustio.context.json`; emits a structured `Plan` + one-paragraph
explanation. **Does not execute anything** — no file writes, no DB,
no migrations, no SQL. The planner is the brain; the executor that
turns plans into code lands in 0.5.x.

- **`rustio-core::ai::planner`** — new module with `generate_plan()`,
  `PlanRequest`, `PlanResult`, `ContextConfig`, `PlanError`.
- **Grammar (rule-based, deterministic):**
  - `add <field> to <model>` / `add <field> as <type> to <model>` /
    `add optional <field> to <model>`
  - `rename <field> to <new> in <model>`
  - `rename model <from> to <to>`
  - `remove <field> from <model>` (also `drop` / `delete`)
  - `change <field> in <model> to <type>`
  - `make <field> in <model> optional|nullable|required`
- **Type inference** from identifier shape (`*_at`/`_date` → DateTime,
  `is_*`/`has_*` → bool, `priority`/`score`/`*_count` → i32, else
  String), with `as <type>` as explicit override.
- **Context-aware:** `rustio.context.json` with `country: "SE"`
  makes `personnummer` resolve to `String` and adds a Swedish
  explanation to the plan.
- **Refusals** (never a guessed plan): unknown model, ambiguous
  model, field already exists, field missing, unknown type, empty
  prompt, unrecognised grammar, developer-only request (any mention
  of `create migration` / raw SQL), attempts to modify a `core: true`
  model.
- **Plan safety:** every returned plan is run through
  `Plan::validate(&schema)` before it leaves the planner, and the
  planner never emits `CreateMigration`.
- **CLI:** `rustio ai plan "<prompt>"` — prints the strict documented
  JSON shape to stdout (`{"plan": [...], "explanation": "..."}`)
  followed by a human-readable `Plan:` summary. On refusal it still
  prints a JSON skeleton with an `error_kind` tag, then exits non-zero
  with a friendly `error:` line on stderr.
- **Tests:** 23 planner-specific tests covering add/rename/remove/
  change-type/change-nullability/rename-model, context-aware SE
  upgrade, core-model protection, developer-only refusal, plan-
  validation invariants, deterministic output, and chaining (rename →
  change-type across two sequential calls).

### Hardened — Foundation Phase, Pass D (pre-Intelligence hardening)

Final security pass before the 0.5.0 Intelligence phase. Closes the
structural gaps the Pass-C audit flagged, without expanding feature
surface.

#### CSRF protection

- **Per-session CSRF tokens.** Every new session row carries its own
  256-bit random token in `rustio_sessions.csrf_token`, independent
  of the session id. Older databases get the column back-filled
  idempotently by `ensure_core_tables` (`pragma_table_info` check +
  conditional `ALTER TABLE ADD COLUMN`).
- **`auth::csrf::generate_token` / `verify_token`** — the latter is
  constant-time (length check + XOR accumulator). Empty strings on
  either side fail.
- **`auth::CsrfToken`** context item, attached by `authenticate`
  alongside `Identity` via the new
  `resolve_identity_with_session(db, token)` helper.
- **Admin forms render `<input type="hidden" name="_csrf" value=…>`**
  everywhere a state-changing POST originates: the header logout
  form, per-row delete buttons, create and edit forms, and the
  forbidden page's sign-out button.
- **`require_csrf` check at the top of every admin POST handler** —
  create, edit, delete, logout. Missing or mismatched token → 403.
  Login is deliberately left unprotected (no session exists yet);
  its defence is `SameSite=Strict` on the session cookie.

#### Request peer address

- **`Request::peer_addr() -> Option<SocketAddr>`** — socket address
  the TCP connection came from. Populated by `Server::serve` and
  `Server::serve_router_on` from the `TcpListener::accept` result.
  `None` when the request is constructed outside the server (tests
  that bypass the pipeline).
- **Used by the login handler** for multi-axis rate limiting (see
  below). The `X-Forwarded-For` header is **not** parsed here —
  projects behind reverse proxies must do that themselves to avoid
  spoofable trust.

#### Global body-size limit

- **`http::MAX_REQUEST_BODY_BYTES = 2 MB`** — framework-wide ceiling.
  `admin::MAX_FORM_BODY_BYTES` is now a re-export of the same
  constant.
- **`defaults::body_limit` middleware** wired by `with_defaults`
  checks `Content-Length` upfront and rejects oversized requests
  with 413 before any handler runs. Applies to admin, user, and
  default routes uniformly — no per-handler opt-in needed. Chunked
  / under-reported bodies still pay the ceiling at the body reader
  (`admin::read_form`), which wraps the body in
  `http_body_util::Limited`.

#### Rate limiter extension point

- **`LoginRateLimiter::compose_key(email, ip)`** — the documented
  extension point for multi-axis limiting. Email-only yields
  `"email:X"`; with an IP yields `"email:X|ip:Y"`. The login handler
  now passes the peer IP when available, so one attacker hammering
  many emails is also throttled per-IP. Three independent compose_key
  tests lock the format.

#### Admin security headers

- **`with_admin_headers`** wraps every admin response with:
  `X-Frame-Options: DENY`, `X-Content-Type-Options: nosniff`,
  `Referrer-Policy: no-referrer`. In production only
  (`RUSTIO_ENV=production`), also `Strict-Transport-Security:
  max-age=31536000; includeSubDomains`. Dev mode is deliberately
  HSTS-free so `http://localhost` flows stay usable.
- Applied at every admin response site: index page, per-model
  list/create/edit, login redirect, logout redirect, login page,
  forbidden page.

#### Session struct

- **`Session` is now `#[non_exhaustive]`** and carries
  `csrf_token: String`. Internal-facing struct; downstream code that
  constructs it directly (none known in the wild) must switch to
  `session::create` or pattern-match with `..`.

#### Tests

20 new tests:

**Integration (`tests/login_flow.rs`):**
`logout_without_csrf_returns_403`,
`anonymous_post_admin_logout_is_rejected`,
`global_body_limit_rejects_large_non_admin_post`,
`admin_response_headers_are_present`. The existing
`full_login_flow_admin_cookie_auth_logout` test was updated to
scrape the `_csrf` token from the admin page and include it on
logout, plus assert the full header set on the authenticated
render.

**Unit (auth.rs):** `compose_key_email_only_is_stable`,
`compose_key_with_ip_is_distinct_from_email_only`,
`compose_key_distinct_ips_produce_distinct_keys`,
`csrf_generate_returns_hex_of_expected_length`,
`csrf_generate_produces_unique_tokens`,
`csrf_verify_matching_returns_true`,
`csrf_verify_mismatched_returns_false`,
`csrf_verify_empty_either_side_returns_false`,
`csrf_verify_rejects_different_lengths`,
`csrf_verify_rejects_single_byte_difference`,
`session_create_generates_unique_csrf_per_session`,
`session_find_valid_returns_csrf_token`,
`resolve_identity_with_session_exposes_csrf`.

**Unit (defaults.rs):**
`content_length_at_limit_is_accepted`,
`content_length_over_limit_is_rejected`,
`content_length_way_over_limit_is_rejected`.

Test count: **230 → 250** (+20).

#### Trade-offs

- **Logout now requires CSRF.** Projects that scripted logout via
  plain `curl -X POST /admin/logout` without scraping the token will
  get 403. Documented migration: GET `/admin`, scrape `_csrf` hidden
  input, include in the logout body.
- **`Session { id, user_id, expires_at }` destructuring breaks** if
  any project did that directly. `#[non_exhaustive]` forces `..` or
  named access. No known caller in the wild.
- **CSRF token is process-stable but not rotated on privilege
  change.** Today a role change (user → admin) keeps the same CSRF
  token for the active session. Acceptable because the token is
  bound to the session, and the session is the authoritative state.

#### Deferred

- **Per-IP rate limiting for non-login routes** — the infrastructure
  (`peer_addr`, `compose_key`) is in place, but the login handler is
  the only call site in 0.4.0. Extending to API endpoints is a
  project-level concern.
- **Content-Security-Policy header** — listed as optional in the
  Pass-D spec; skipped because a default CSP tight enough to matter
  would block the inline `<style>` tag used by admin pages. A
  follow-up pass should externalise the CSS and then add a strict
  CSP.
- **`X-Forwarded-For` parsing** — when a project runs behind a
  reverse proxy, `peer_addr()` returns the proxy's IP. Parsing
  `X-Forwarded-For` / `Forwarded` safely is project-specific (whose
  proxies do you trust?) and belongs in user middleware, not the
  framework.

### Hardened — Foundation Phase, Pass C (security + integrity)

Post-audit hardening. No new surface beyond the stated scope; every
change closes a specific issue identified in the Pass-B review.

#### Critical fixes

- **SQLite foreign keys are now on.** `Db::connect` and `Db::memory`
  use `SqliteConnectOptions::foreign_keys(true)` so every connection
  runs with `PRAGMA foreign_keys = ON`. The `ON DELETE CASCADE` on
  `rustio_sessions.user_id` now actually fires — verified by a
  delete-user-cascades-to-sessions test.
- **Login is constant-time against user existence.**
  `auth::dummy_password_hash()` returns a cached argon2id hash that
  the login handler verifies against on the "user not found" branch,
  matching the ~50 ms cost of the "user found, wrong password"
  branch. Email enumeration via response time is closed.
- **AI plans reject `CreateMigration`.** `Primitive::is_developer_only()`
  marks the raw-SQL primitive as developer-only; `Plan::validate`
  refuses any step where that's true, emitting
  `PrimitiveError::DeveloperOnlyNotAllowedInPlan`. The variant stays
  in the enum for direct project/tooling use — only the AI boundary
  is tightened. Project maintainers can still emit migrations;
  `rustio ai` cannot.
- **Request bodies are capped at 2 MB.** Form parsing wraps the hyper
  body with `http_body_util::Limited`; overflow surfaces as the new
  `Error::PayloadTooLarge` → HTTP 413. Stops unauthenticated DoS via
  single large POST. `admin::MAX_FORM_BODY_BYTES` is the public
  constant projects can compare against.
- **Production cookies are `Secure`.** `build_session_cookie` appends
  `Secure` whenever `auth::in_production()` is true. Dev mode is
  unchanged so `http://localhost` flows still work.

#### High-priority security

- **Per-email login rate limit.** `auth::LoginRateLimiter` (in-memory,
  process-wide singleton) blocks further attempts for 60 s after 5
  failed logins on the same email, and clears the counter on
  successful login. Returns `Error::TooManyRequests` (HTTP 429) with
  a retry-after hint in the response body. **Per-IP is deferred** —
  adding the client address to `Request` requires a server-pipeline
  change outside Pass C scope; per-email still defeats targeted
  brute force against a single account.
- **Password change invalidates every session.**
  `auth::user::set_password` now runs the UPDATE and a
  `DELETE FROM rustio_sessions WHERE user_id = ?` in one transaction.
  Stolen cookies do not survive a password rotation.
- **Expired sessions self-clean on lookup.** `auth::session::find_valid`
  deletes the offending row inline when it sees an expiry in the past;
  `handle_login` also calls `sweep_expired` after a successful login.
  No background worker required.
- **Schema reflects `User.created_at`.** Added to `USER_FIELDS` so
  `rustio.schema.json` no longer under-describes the real
  `rustio_users` shape. Schema determinism preserved; snapshot test
  updated.

#### AI primitive vocabulary

Four new structured primitives land as **definitions + validation
only** (no executor):

- **`RenameModel`** and **`RenameField`** — data-preserving renames
  the AI boundary can actually express.
- **`ChangeFieldType`** — validates the target type name against
  `VALID_TYPE_NAMES`; a lossy-conversion check lives in the future
  0.5.0 executor.
- **`ChangeFieldNullability`** — flip `Option<T>` ↔ `T` at the schema
  layer.

All four have `#[serde(deny_unknown_fields)]`, round-trip through
JSON, and update `apply_shadow` so multi-step plans that rename then
mutate the renamed entity validate correctly. New
`PrimitiveError::NoOpRename` catches `from == to` early.

#### Testing

- `full_login_flow_admin_cookie_auth_logout` — end-to-end HTTP test
  (raw TCP client, `Server::serve_router_on` on a kernel-assigned
  port). Covers anonymous 401, wrong password / unknown email
  symmetric 401, successful 303 + HttpOnly/SameSite=Strict cookie,
  authenticated 200, logout 303 + Max-Age=0, and replay-after-logout
  401.
- `oversized_form_body_returns_413` — 3 MB POST to `/admin/login`
  must produce 413.
- `login_rate_limiter_triggers_lockout` — 6th failed attempt returns
  429.
- Unit coverage added for: FK cascade on user delete, inline cleanup
  of expired sessions on lookup, password-change invalidates all
  sessions, rate limiter (threshold, reset, lockout expiry,
  independent keys), dummy-hash shape + safety, the 4 new primitives
  (structural and plan-chained validation), cookie builder in dev
  and prod modes, `Plan` rejection of `CreateMigration`.

Test count: **197 → 230** (+33).

#### Public API additions (all additive)

- `Error::PayloadTooLarge` (413), `Error::TooManyRequests` (429).
- `auth::dummy_password_hash()` — precomputed filler hash.
- `auth::LoginRateLimiter` — struct + `global()` singleton.
- `auth::resolve_identity` already existed; no change.
- `admin::MAX_FORM_BODY_BYTES` — the 2 MB constant.
- `Primitive::is_developer_only()`, `Primitive::op_name()`.
- `Primitive::{RenameModel, RenameField, ChangeFieldType,
  ChangeFieldNullability}` variants + their payload structs.
- `PrimitiveError::{DeveloperOnlyNotAllowedInPlan, NoOpRename}`.
- `Server::serve_router_on(listener, router)` — serve on a
  pre-bound `TcpListener`. Required for the integration test; also
  useful for privilege-drop hosts.

#### Unresolved / deferred

- **Per-IP rate limiting** requires `Request` to carry the client
  address, which means changing `http::Request::new` and threading
  the peer addr through `server::Server::serve` → `Router::dispatch`.
  Not in Pass C scope. Per-email limit is the interim defence.
- **CSRF tokens** still absent. `SameSite=Strict` remains the only
  barrier. Revisit before 0.5.0.
- **Body size limit** applies to admin form parsing only. Custom
  handlers that do their own body collection are on their own;
  `MAX_FORM_BODY_BYTES` is exported so projects can adopt the same
  ceiling.

### Added — Foundation Phase, Pass B (authentication)

Real auth replaces the development token flow. Every RustIO project
now has a `User` table, argon2id-hashed passwords, DB-backed sessions,
and a session-cookie middleware. **Breaking** for generated projects
(see "Upgrading" below).

#### User

- **`User` model in `rustio-core`** — id, email, password_hash,
  is_active, role. Deliberately minimal; extend user data via a
  separate `Profile` model in user code rather than widening this one.
- Emails are **normalised** (trimmed + lowercased) on create and
  lookup so `Alice@Example.com` and `alice@example.com` are the same
  account.
- Roles are a closed set in 0.4.0: `admin` or `user`. Anything else is
  rejected at `user::create`.

#### Passwords

- **`auth::password::hash` / `auth::password::verify`** using argon2id
  with RFC 9106 default parameters (m_cost=19456 KiB, t_cost=2, p=1)
  and a 16-byte OS-entropy salt per password.
- Verification is **constant-time** (via argon2's own comparator) and
  **never panics** on malformed hash strings — returns `false` instead.
- Empty passwords are refused at `hash` boundary.

#### Sessions

- **`rustio_sessions` table**, keyed by a 256-bit OS-random hex token.
- **`auth::session::create` / `find_valid` / `delete` / `sweep_expired`**
  — `find_valid` enforces expiry on every lookup; the DB is the source
  of truth, no in-memory caching.
- 7-day TTL (`SESSION_TTL_DAYS` const; not configurable in 0.4.0).
- Cookie: `rustio_session=...; HttpOnly; SameSite=Strict; Max-Age=…`.
  `Secure` is documented at the deployment boundary — see
  `SECURITY.md`.

#### Middleware

- **`auth::authenticate(db)`** is now a factory returning a DB-capturing
  closure (was a free function). The old dev-token path is gone.
- Decision path: read `rustio_session` cookie → `session::find_valid`
  → `user::find_by_id` → `user.is_active` check → attach `Identity`.
  Failure at any step is silent; downstream `require_auth` /
  `require_admin` produce 401 / 403 from the missing identity.
- **`auth::resolve_identity(db, token)`** is the pure core of the
  middleware, extracted so every decision branch has a direct unit
  test (no hyper `Request` required).

#### Login + logout

- `POST /admin/login` — takes `email` + `password` form fields.
  Generic error ("Invalid email or password") for both unknown email
  and wrong password; explicit error for inactive accounts; 400 for
  missing fields. Email is prefilled on failed submissions; the
  password field never is.
- `POST /admin/logout` — deletes the server-side session row and
  expires the cookie. Idempotent.

#### Schema integration

- **`SchemaModel.core`** — new boolean flag. `true` for built-in
  infrastructure models (currently just `User`). The AI layer should
  refuse destructive primitives against core models.
- **`User` is seeded in every `Admin::new()`** and consequently in
  every project's `rustio.schema.json`. It does **not** get routed as
  an admin CRUD page in 0.4.0 — the entry exists for schema fidelity.
  The `len()` / `is_empty()` methods on `Admin` count user-registered
  models only, so the "no models registered yet" placeholder behaves
  as before.

#### CLI

- **`rustio user create`** — interactive command with masked password
  + role picker. Non-interactive form:
  `rustio user create --email E --password P --role admin`.

#### Test coverage

25 new tests in `auth::`:
- password hashing / verification / salt uniqueness / invalid-hash
  panic-safety / empty-password refusal;
- user create / duplicate email / unknown role / set_password /
  set_active;
- session create / lookup / expiration / delete / sweep;
- middleware decision path: no cookie / unknown token / expired
  session / inactive user / deleted user / valid admin / valid user /
  logout-invalidates-session.

Plus an updated schema snapshot test that locks the User core entry
into the wire format.

#### Upgrading from Pass A projects

1. Run `rustio migrate apply` — bootstraps `rustio_users` and
   `rustio_sessions` automatically.
2. Update generated `main.rs`: `authenticate` is now `authenticate(db)`
   (factory). The CLI-regenerated template shows the exact shape.
3. Create an admin user: `rustio user create`.
4. `Identity.user_id` changed from `String` to `i64` and gained an
   `email` field. If you read it in custom middleware or handlers,
   update accordingly.
5. Bearer-token dev auth (`dev-admin`, `dev-user`) is gone. Custom
   middleware using `auth::bearer_token` still compiles; implement
   your own token → identity mapping if you need Bearer auth.

### Hardened — Foundation Phase, Pass A.5

Pass A landed the shape; Pass A.5 locks it down. No new features — every
change here tightens an existing invariant.

#### Schema

- **Byte-for-byte determinism.** `Schema::from_admin` now sorts models
  by name and fields within each model by name. Two calls on the same
  registry produce identical JSON. The admin UI's display order is
  unchanged — only the exported file is sorted.
- **No clocks in the file.** Removed `generated_at` from the schema
  document entirely. The filesystem's mtime records when it was
  written; the JSON content is now purely structural.
- **`Schema::validate()`** — fail-fast checks for duplicate model names,
  duplicate field names, invalid type names, dangling relation targets,
  and version mismatches. `SchemaError` is a named enum; tooling can
  branch on the failure kind.
- **Version lock.** `Schema::parse` rejects documents whose `version`
  field doesn't match `SCHEMA_VERSION`. Consumers of `rustio.schema.json`
  (including the future AI layer) refuse to load anything they weren't
  built to understand.
- **Strict deserialization.** `#[serde(deny_unknown_fields)]` on every
  schema struct. Extra keys fail to load.
- **Atomic writes.** `Schema::write_to` validates before persisting, and
  cleans up the temp file on rename failure so no `.json.tmp` is left
  next to the target on retry.
- Trailing newline on the emitted JSON so `git diff` stops warning
  about "no newline at end of file".

#### AI primitives

- **`validate_primitive`** — structural check: non-empty identifiers,
  type names in `VALID_TYPE_NAMES`, no duplicate fields inside
  `add_model`, `update_admin.attr` in the allow-list.
- **`validate_against(&Primitive, &Schema)`** — semantic check: target
  models and fields exist, `add_*` doesn't collide with existing
  entries, relations resolve to real models.
- **`Plan { steps: Vec<Primitive> }`** with **`Plan::validate(&Schema)`**
  — shadow-applies each primitive to an in-memory schema copy so later
  steps validate against the expected post-state. All-or-nothing: the
  first invalid step rejects the plan. No filesystem, no DB — pure
  simulation, consistent with the 0.4.0 "definitions only" rule.
- **Strict deserialization.** `#[serde(deny_unknown_fields)]` on every
  primitive payload and `Plan`. Unknown ops, unknown keys, and missing
  required fields all fail to parse.
- **`PrimitiveError::InStep`** annotates plan failures with the step
  index so callers can report "step 3 failed because …".

#### DateTime

- `parse_datetime_local` now explicitly rejects empty strings, leading
  or trailing whitespace, timezone suffixes (`Z`, `+HH:MM`), out-of-range
  calendar values, and partial dates. UTC enforcement verified for
  every valid input via `to_rfc3339().ends_with("+00:00")`.
- Input-side contract pinned in tests: the macro trims before calling;
  `parse_datetime_local` itself does not.

#### Option<T>

- ORM round-trip coverage for `Option<String>`, `Option<i32>`, and
  `Option<DateTime<Utc>>`: `None` writes as SQL NULL (verified via
  `IS NULL` on the raw row), `Some` reads back identical to input,
  and the update path flips both directions without data loss.

#### Admin rendering

- Unit tests pin the `required` attribute rules:
  - nullable → never required,
  - non-nullable non-bool → required,
  - bool → never required (no "unset" UI for checkboxes).
- DateTime fields render as `<input type="datetime-local">` with the
  stored value round-tripped into the `value=` attribute.
- `field_display` returning `None` or `Some(String::new())` renders an
  empty value without panicking.

#### Tests

~50 new tests across `schema::`, `ai::`, `admin::`, and `orm::`,
including a **byte-for-byte schema snapshot** that will fail on any
future change to ordering, type-name mapping, or JSON punctuation.

### Added — Foundation Phase, Pass A (schema + typed core)

- **`rustio.schema.json`** — a deterministic, machine-readable description
  of every model, field, and admin behavior in a RustIO project. This is
  **the** interface the Phase 2 AI layer will consume. Shape is versioned
  (`SCHEMA_VERSION = 1`) and stable across patch releases.
- **`rustio schema`** — new CLI command. Compiles the project with
  `--dump-schema`, introspects the live `Admin` registry, and writes
  `rustio.schema.json` at the project root. Not generated on every
  `cargo build` — explicit, fast, and on demand.
- **Auto-dump on `rustio migrate apply`.** After a successful apply, the
  CLI regenerates `rustio.schema.json` best-effort (skipped with a hint
  if the project doesn't compile yet).
- **`DateTime<Utc>` field type.** Supported end-to-end: admin rendering
  (`<input type="datetime-local">`), form parse, SQLite storage, schema
  export. Re-exported as `rustio_core::DateTime` / `rustio_core::Utc`
  so models don't need to depend on chrono directly.
- **`Option<T>` field support.** Any supported scalar wrapped in
  `Option` becomes a nullable column — NULL in DB, `None` in Rust,
  empty input in admin. `nullable: true` in the exported schema.
- **Row readers for optional types**: `get_optional_i32`,
  `get_optional_i64`, `get_optional_string`, `get_optional_bool`,
  `get_optional_datetime`.
- **`Value::DateTime` + `Value::Null`** plus a blanket
  `From<Option<T>>` so `None` binds as NULL automatically.
- **`AdminField.nullable`** metadata, surfaced in schema and used to
  relax form-level `required` for nullable fields.
- **`rustio_core::ai`** — *definitions only*. The `Primitive` enum fixes
  the vocabulary the 0.5.0 AI layer will be allowed to emit
  (`add_model`, `remove_model`, `add_field`, `remove_field`,
  `add_relation`, `remove_relation`, `update_admin`,
  `create_migration`). No executor ships in 0.4.0 — the hard rule for
  Phase 2 is that anything not expressible as a primitive is rejected.
- **`rustio ai`** — CLI stub. Prints the primitive vocabulary and
  explains the refusal rule. Accepts an intent string which is logged
  but not acted on until 0.5.0.

### Changed

- `FieldType` is now `#[non_exhaustive]`. Downstream matchers must add a
  wildcard arm; inside rustio-core the compiler checks exhaustiveness so
  new variants can't silently miss the schema mapping.
- `AdminEntry` grew `table` and `fields` so the schema exporter can
  introspect it without a second trait-object round trip.
- Generated `apps/mod.rs` now defines a `build_admin()` helper so
  `main.rs --dump-schema` can introspect the admin without connecting
  to the DB or binding a port. `register_all` delegates to it.

### Upgrading from 0.3.x

Projects scaffolded under 0.3.x will keep working at runtime but can't
emit `rustio.schema.json` until their `main.rs` and `apps/mod.rs` learn
the `--dump-schema` and `build_admin` shape. Either:

1. Re-scaffold with `rustio init <name> --preset <kind>` and copy your
   apps across, or
2. Hand-merge the two snippets from the generated templates — they are
   ~10 lines each.

## [0.3.1]

### Added

- **Browser-friendly admin login.** Visiting `/admin` without auth now
  renders a proper sign-in form instead of a dead-end "paste this curl
  command" hint. Submit the token and the admin sets an HttpOnly
  `rustio_token` cookie so subsequent requests authenticate
  automatically.
- `POST /admin/login` — validates the submitted token, sets the cookie,
  redirects to `/admin`. Empty → 400. Unknown → 401. Both render the
  form with an inline error.
- `POST /admin/logout` — expires the cookie, redirects back to
  `/admin` (which re-renders the login form).
- **Sign-out button in the admin header** — every admin page now has a
  visible way out.
- `rustio_core::http::Request::cookie(name)` — read a single cookie by
  name from the request. Returns `None` for missing / malformed.
- `rustio_core::http::set_cookie(&mut resp, value)` — append a
  `Set-Cookie` header (user supplies the attribute string).
- `authenticate` middleware now checks `Authorization: Bearer` **and**
  the `rustio_token` cookie. Bearer auth for API callers remains
  unchanged; cookie auth serves browsers.

### Security

- Login cookie is set with `HttpOnly; SameSite=Strict; Path=/`. JS can't
  read it; cross-site navigations don't send it. `Secure` is not set
  automatically (the server can't reliably tell whether the request
  came via HTTPS); add it at your TLS terminator or reverse proxy for
  production deployments.
- Login is fully disabled under `RUSTIO_ENV=production` — the form
  rejects all submissions until a real auth middleware is installed.
  This keeps the 0.2.2 production guard intact.

### Notes

- 403 responses (authenticated but not admin) now render a small
  "Forbidden" page with a sign-out button, instead of the generic auth
  error page.
- No breaking changes. Existing Bearer-based integrations and
  programmatic callers work untouched.

## [0.3.0]

Theme: close the "now what?" gap between scaffolding and actually using
the framework.

### Added

- **Custom app name in the wizard.** After picking a preset, the wizard
  asks *"What should your first model track?"* — type `books` and get
  `pub struct Book`, table `books`, and `/admin/books` end-to-end. The
  wizard's preset default still populates (`posts` for Blog, `items` for
  API) so Enter-to-accept keeps working.
- **`--app <name>` flag on `rustio init`** — non-interactive equivalent
  of the new prompt. Example:
  `rustio init readlist --preset blog --app books`.
- **Richer model scaffold.** The generated `models.rs` now has three
  fields spanning the three supported types — `title: String`,
  `is_active: bool`, `priority: i32` — instead of a lone `name: String`.
  The scaffold is a working multi-type example out of the box.
- **Module doc comment** on the generated `models.rs` explaining how to
  add fields + write a follow-up migration. Replaces the silent "what
  do I edit?" moment reported in user testing.
- **Tutorial view page.** `GET /<app>` returns a small styled HTML page
  confirming the wire-up is working, pointing at `apps/<app>/views.rs`
  for customization, and linking to the admin. Replaces the prior
  `{{STRUCT}} views — placeholder` plain-text line.

### Changed

- Wizard is now a **four-step flow** (name → preset → first model → confirm)
  instead of three. Basic preset still skips the model step.
- Preset labels in the wizard are slightly less "blog-specific" — they
  describe shape ("one app with admin + views") rather than domain
  ("scaffolds a posts app"). Preset enum names are unchanged.

### Documentation

- README: new **"♻️ Starting Fresh"** section explaining how to reset
  `app.db` safely. Migrations are idempotent; schema lives in the `.sql`
  files, not the database.
- All `curl` examples are single-line (copy-paste friendly across
  shells, including zsh with strict continuation handling).
- CLI + main README Quick Start now shows the four-prompt wizard with a
  custom app name as the example.

### Upgrading from 0.2.x

1. Bump `rustio-core` in generated projects to `"0.3.0"` and
   `cargo update`.
2. Existing apps generated under 0.2.x stay on disk with their old
   `name: String`-only schema — no automatic rewrite. New apps created
   via `rustio new app <name>` use the new scaffold.

### Note on session auth / CSRF

Session cookies + CSRF tokens originally targeted 0.3.0 based on the
earlier SECURITY.md note. 0.3.0 pivoted to close visible first-run UX
gaps first. Session auth is now targeted for a future `0.x` release;
Bearer-based admin remains not directly CSRF-exploitable per SECURITY.md.

## [0.2.2]

### Added

- **Production guard on built-in auth.** `authenticate` now refuses to
  recognize the dev tokens (`dev-admin`, `dev-user`) when
  `RUSTIO_ENV=production` (or `RUSTIO_ENV=prod`) is set. A process that
  boots into production mode and forgets to register a real auth
  middleware will simply 401 every admin request instead of silently
  accepting `dev-admin`.
- **One-time production warning** on stderr the first time the
  `authenticate` middleware runs under `RUSTIO_ENV=production`, pointing
  the user at the correct fix.
- **Friendly 401 / 403 HTML pages on the admin.** Browsers hitting
  `/admin` without auth no longer see three characters of plain text —
  they get a small HTML page with the status code and, in development
  mode only, a `curl -H "Authorization: Bearer dev-admin"` hint. The
  dev hint is suppressed under `RUSTIO_ENV=production`.
- **First-compile hint.** The first time `rustio run` is invoked in a
  project (no `target/` yet), the CLI prints `first run compiles
  dependencies (~1 min). Subsequent runs are instant.` — ending the
  common "did this hang?" moment.
- **`rustio_core::auth::in_production()`** public helper so custom
  middleware can branch on the same env signal.

### Documentation

- `SECURITY.md` updated with the precise Bearer-vs-CSRF threat model
  and the new production guard. Note: CSRF tokens on admin forms are
  tied to cookie-based session auth and ship with 0.3.0 — Bearer auth
  is not directly CSRF-exploitable.

## [0.2.1]

### Added

- **`rustio init` interactive wizard.** Running `rustio init` with no arguments
  launches a three-prompt flow — project name, starter preset, confirm — and
  calls the same scaffolding helpers as the flag-driven commands, so both
  paths produce identical on-disk output.
- **Presets:** `basic` (empty project), `blog` (scaffolds a `posts` app), and
  `api` (scaffolds an `items` app). Pickable in the wizard or via
  `rustio init <name> --preset <kind>`.
- **Non-interactive form:** `rustio init <name>` scaffolds directly without
  prompting. `--db sqlite` is accepted and reserved for future drivers.
- **Off-TTY safety:** when stdin is not a terminal, the wizard exits with a
  clear hint to pass arguments instead of hanging.

### Dependencies

- `inquire = "0.7"` added to `rustio-cli` for the wizard prompts.

## [0.2.0]

### Added

- **`rustio_core::admin::Admin` builder.** Collect multiple admin models on
  one `Admin`, then call `.register(router, db)` to install:
  - a `/admin` index page listing every registered model, and
  - CRUD routes at `/admin/<admin_name>` for each.
  Replaces the previous "no admin index" gap. Addresses the fresh-user
  friction where "Go to Admin" from the homepage led to a dead end.
- **`AdminModel::singular_name()`** method. Used for "New X" and "Edit X"
  labels. Defaults to `DISPLAY_NAME` for back-compat; the
  `#[derive(RustioAdmin)]` macro generates the proper singular.
- **`AdminEntry`** metadata struct exposed for inspection via
  `Admin::entries()`.
- Admin header `"RustIO Admin"` is now a link back to `/admin`, giving
  every page a way to return to the index.
- CLI scaffolds generate singular struct names: `rustio new app listings`
  now produces `pub struct Listing`, table `listings`, admin `/admin/listings`.
- Required-field validation in `#[derive(RustioAdmin)]`: empty/missing
  `String`, `i32`, `i64` fields now return `400 BadRequest("field X is
  required")` instead of silently inserting empty or zero values.
  `bool` fields keep HTML checkbox semantics (absent = false).

### Changed (breaking)

- `rustio_core::defaults::with_defaults` no longer registers the `/admin`
  placeholder. `/admin` is now owned by the admin layer. Projects that do
  not register any admin models get `404` on `/admin` (instead of a
  "coming soon" stub).
- `rustio_core::defaults::admin_placeholder` has been removed.
- CLI-generated `apps/mod.rs` now builds an `Admin` and each app exposes
  `admin::install(admin)` instead of `admin::register(router, db)`. Old
  0.1.x-generated projects continue to compile but need a small migration
  to get the `/admin` index (see Upgrading below).

### Upgrading from 0.1.x

1. Bump `rustio-core` (and the CLI) to `"0.2.0"`.
2. In your `apps/mod.rs`, replace per-app `admin::register` calls with
   an `Admin` builder:

   ```rust
   use rustio_core::admin::Admin;

   pub fn register_all(mut router: Router, db: &Db) -> Router {
       let mut admin = Admin::new();
       admin = blog::admin::install(admin);
       admin = listings::admin::install(admin);
       router = admin.register(router, db);
       router = blog::views::register(router);
       router = listings::views::register(router);
       router
   }
   ```

3. In each `apps/<name>/admin.rs`, switch from a `register(router, db)`
   function to an `install(admin)` function:

   ```rust
   use rustio_core::admin::Admin;
   use super::models::MyModel;

   pub fn install(admin: Admin) -> Admin {
       admin.model::<MyModel>()
   }
   ```

4. If you manually implement `AdminModel`, consider overriding
   `singular_name()`. Otherwise it falls back to `DISPLAY_NAME`.

## [0.1.2]

### Fixed

- `rustio new app <name>` and `#[derive(RustioAdmin)]` no longer double the
  trailing `s` on names that already end in `s`. Running
  `rustio new app posts` now produces table `posts` (not `postss`), admin
  path `/admin/posts` (not `/admin/postss`), and display name `Posts`
  (not `Postss`).

## [0.1.1]

### Added

- `rustio --version` (and `-V`, `version`) prints the CLI version.
- `rustio migrate apply -v` (or `--verbose`) prints each SQL statement as it runs.
- `rustio_core::migrations::ApplyOptions` and `apply_with(db, dir, opts)` for
  programmatic verbose control.
- `rustio_core::migrations::status(db, dir)` and `applied(db)` (public API for the
  `rustio migrate status` output).
- `rustio_core::http::json_raw(body)` — `200 OK` with `application/json` content
  type. Pair with `serde_json::to_string(&value)?` for typed output.
- `rustio_core::http::FormData` (moved from `admin`) is now re-exported at the
  crate root. `admin::FormData` remains as an alias for macro-generated code.
- `Request::query()` returns a `FormData` parsed from the URL query string.
- Module-level docs across `rustio_core` for a cleaner docs.rs experience.
- GitHub Actions CI (fmt / clippy / test) and release workflow.
- `CONTRIBUTING.md`, `SECURITY.md`, issue and PR templates.

### Changed

- **Security:** `Error::Internal(msg).into_response()` no longer leaks the
  internal message to clients. The HTTP body is now always
  `"Internal Server Error"`. `Display` and `Error::message()` still expose the
  original detail for logs.
- **Migrations:** the SQL splitter no longer breaks on `;` inside single-quoted
  string literals or line / block comments. Doubled `''` inside a literal is
  recognized as an escape.
- Crate metadata `repository` link now points to
  `https://github.com/abdulwahed-sweden/rustio` (fixes a wrong URL in 0.1.0).

## [0.1.0]

First public release.

### Added

- **HTTP layer**: hyper-backed server, custom router with `:param` paths, middleware
  chain (`Fn(Request, Next) -> Result<Response, Error>`).
- **Request context**: typed per-request store via `req.ctx()` / `req.ctx_mut()`.
- **Error model**: unified `Error` enum mapping to 400/401/403/404/405/500; safety
  net in `Router::dispatch` converts unhandled `Err` to `Response`.
- **Auth middleware**: additive `authenticate`; `require_auth` and `require_admin`
  helpers; `Identity` in context. Dev tokens `dev-admin` / `dev-user`.
- **ORM**: `Model` trait over SQLite via `sqlx`. `find` / `all` / `create` / `update`
  / `delete`. Row getters for `i32`, `i64`, `String`, `bool`.
- **Admin**: `#[derive(RustioAdmin)]` auto-generates list, create, edit, delete pages
  and routes; admin-only auth enforced.
- **Migrations**: versioned `.sql` files in `migrations/`, tracked in
  `rustio_migrations`, transactional, idempotent.
- **CLI** (`rustio`): `new project`, `new app`, `migrate generate`, `migrate apply`,
  `migrate status`, `run`. Colored output, `NO_COLOR`-aware.
- Three crates published to crates.io: `rustio-macros`, `rustio-core`, `rustio-cli`.

### Known limitations

- SQLite only.
- Naive plural naming in admin scaffolds (`Person` → `persons`).
- No CSRF on admin forms.
- No session auth — dev tokens only.
- Forward-only migrations (no `down`).
- `rustio-core = "x.y.z"` in generated projects is pinned to match CLI; lockstep
  releases expected until this stabilizes.

[Unreleased]: https://github.com/abdulwahed-sweden/rustio/compare/v0.3.1...HEAD
[0.3.1]: https://github.com/abdulwahed-sweden/rustio/compare/v0.3.0...v0.3.1
[0.3.0]: https://github.com/abdulwahed-sweden/rustio/compare/v0.2.2...v0.3.0
[0.2.2]: https://github.com/abdulwahed-sweden/rustio/compare/v0.2.1...v0.2.2
[0.2.1]: https://github.com/abdulwahed-sweden/rustio/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/abdulwahed-sweden/rustio/compare/v0.1.2...v0.2.0
[0.1.2]: https://github.com/abdulwahed-sweden/rustio/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/abdulwahed-sweden/rustio/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/abdulwahed-sweden/rustio/releases/tag/v0.1.0
