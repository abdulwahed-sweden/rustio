# Changelog

## [1.10.0] - 2026-05-06

The admin chrome design system **v2** — every page that ships with
the framework picks up a dramatically cleaner default look without
any per-project work. Existing template overrides and the
`AdminTheme` re-skin path keep working unchanged.

### Added

- **v2 design system** (`assets/templates/admin/base.html`). Geist
  + Geist Mono fonts via Google Fonts; Zinc / Cobalt / Violet
  token palette; layered shadow scale; 16-px-anchored type scale;
  consolidated card / form / table / sidebar / topbar styling.
  New CSS primitives: `.data-card`, `.form-card`,
  `.card-section--inset`, `.filter-bar`, `.filter-chip`,
  `.dropdown__menu`, `.model-card`, `.stat-card`, `.kbd-card`,
  `.env-pill`.
- **Smart filter bar** on every list page (`admin/list.html`).
  Inset search input with `/` keyboard shortcut, Sort dropdown,
  Add-filter dropdown (auto-populates from rendered rows for
  `status` text columns), Columns dropdown (toggles column
  visibility per-model, persisted to `localStorage`),
  active-filter chip row with Clear-all.
- **Column-priority defaults** per built-in model. `id` is hidden
  by default; each model's remaining columns bucket into primary
  (always shown) and tertiary (toggle-on via the Columns
  dropdown). Cells get `data-col` / `data-kind` attributes so
  future tooling can drive column behaviour declaratively.
- **Redesigned dashboard** (`admin/index.html`). Hero greeting
  card with date eyebrow, JS time-of-day greeting, friendly-name
  derivation from email, "All systems operational" pulse. Single
  unified data-models grid with `<article class="model-card">`
  items that lazy-fetch their row count from `/admin/<model>/`.
  Recent-activity card with empty state. System info card with
  Environment + Database + Total-records.
- **Single-column user-detail** (`admin/user_view.html`). 960-px
  max-width card with `← Users` back-link, header (name + role +
  Active badge + Edit), one-line meta (`email · N sessions · M
  events · last seen X`), tab strip with cobalt active state,
  2-column profile grid with violet decorative underline beneath
  each section heading.
- **Sidebar v2 treatment.** Cool-gray-blue side-panel background
  (`--rio-bg-sidebar = #f4f6fa`, applied to the left nav and the
  right aside on dashboard / form pages). Active link is a
  pure-white pill with a 3-px violet decorative stripe.
- **Form-body wash.** 4 % violet over white (`--rio-form-bg =
  #faf8ff`) painted only inside `<form> .card-body` so input
  fields visibly float above a faintly tinted plane. Inputs
  themselves are forced back to pure white.
- **Secondary accent token.** `--rio-accent-2 = #8b5cf6` (violet)
  + `--rio-accent-2-bg` / `--rio-accent-2-border` / `--rio-form-bg`.
  Strict role separation: cobalt for actions (buttons, links,
  focus rings, active tabs); violet for decoration / selection
  cues only (filter chips, sidebar stripe, code tint, section
  underlines).

### Changed

- **Templates ported to v2 markup.** `admin/list.html`,
  `admin/index.html`, `admin/user_view.html` rewritten on the new
  primitives. Eight test assertions updated in lockstep
  (`list_dispatches_cell_renderer_by_field_kind`,
  `list_filter_only_empty_omits_cta`,
  `list_filtered_empty_omits_cta`,
  `list_numeric_dispatch_is_kind_driven_not_name_driven`,
  `list_renders_rows_via_field_keyed_lookup`,
  `list_template_has_no_per_page_inline_styles`,
  `user_view_overview_renders_with_groups`,
  `user_view_overview_without_groups_shows_empty_marker`).
  All 501 sandbox tests still pass.
- **Empty-state copy** on the list page: "No results match your
  search" → "No results match your filters".

### Backwards compatibility

- The retired classes (`.splitview`, `.pane-list`, `.pane-detail`,
  `.stat-strip`, `.stat-card`, `.show-grid`, `.dashboard-models`,
  `.dashboard-recent`, `.toolbar-form`) keep their CSS rules in
  `base.html`. Downstream projects that hand-wrote markup against
  those classes continue to render correctly; only the
  framework's default templates have moved to the v2 markup.
- `Admin::theme(...)` and `Admin::accent_color(...)` keep working
  unchanged; the runtime override block in `base.html` still
  rewrites the same `--rio-*` tokens at render time.

### Deferred (Phase 7+ work)

- `infer_filters` / `handlers.rs` filter rendering logic is
  unchanged: only **Bool** filter dropdowns render server-side.
  The new UI surfaces distinct `status` text values
  template-side, but other text columns (`level`, `priority`,
  `room_type`, …), date ranges, numeric ranges, and FK relation
  filters still need the Phase 7+ widget + relation plumbing.
- Search is still the in-memory substring scan over all cells;
  no fuzzy, no ranking, no Meili hookup on the list page.
- Pagination still honours `?p=N` only; `?per_page=N` is not
  read by the handler.

## [1.9.0] - 2026-05-05

The schema contract system. One `#[derive(RustioModel)]` per
struct now drives the admin UI, search indexing, runtime DB
validation, doctor CLI, and per-column behaviour — without a
manual `AdminModel` impl, `Searchable` impl, `Model` impl, or
any glue. Phase 14 builds the contract + bridges + runtime
integration; Phase 15 polishes the developer experience on top.

The classic v1.x flow (manual `AdminModel` / `Searchable` /
`Model`) is preserved verbatim. Both flows compose on the same
`Admin` builder — no breaking changes.

### Added

- **Schema contract types** (`rustio_core::contract`,
  Phase 14 commit 3 base extended through this release).
  `RustType`, `ModelColumn`, `ModelSchema`, `SchemaFlags`, plus
  the `HasSchema` trait. Five non-negotiable type rules
  (`i64` for IDs, `DateTime<Utc>` for timestamps, `Decimal` for
  money, `JSONB` only, `TEXT` default for strings) encoded as
  `is_compatible_with` lookups against PG `data_type` /
  `udt_name`.
- **Runtime validator** (`rustio_core::contract_validator`,
  commit 3). `validate_schema::<T>(&Db)` and
  `validate_all(&db, &[&schema])` issue read-only `SELECT`s
  against `information_schema.columns` + `key_column_usage` and
  return a `SchemaReport` of typed `SchemaIssue`s
  (`MissingTable`, `MissingColumn`, `TypeMismatch`,
  `NullabilityMismatch`, `WrongPrimaryKey`, `ExtraDbColumn`,
  `QueryFailed`).
- **Doctor CLI** (`rustio doctor --check-schema [--json]`,
  commit 4). Spawns the project binary with a magic flag
  (`--rustio-doctor-schema-check`), captures the JSON output,
  renders as human-readable or passes through. Exit `0` on
  Ok / Warning, `1` on errors. Wire-contract tests on both
  sides lock the magic flag literals against silent drift.
  Project hook: `contract_doctor::maybe_handle_subprocess(&db,
  &schemas)`.
- **Admin bridge** (`rustio_core::admin::from_schema`,
  commit 5). `bridged_fields_from_schema` + `BridgedField`
  side-channel struct expose every column flag
  (`searchable`, `filterable`, `sortable`, `readonly`,
  `widget`, `primary_key`) without modifying the existing
  `AdminField` shape. Existing manual `AdminModel` impls are
  untouched.
- **Search bridge** (`rustio_core::search::from_schema`,
  commit 6). `SearchConfig` derived from `ModelSchema`;
  `SearchEnablement` (`NotSearchable` / `Disabled` / `Enabled`)
  resolved by `enable_search::<T>(&db)` or the pure
  `enablement_from(&schema, report)`. Validator gate is
  fail-safe: `Error` → search refused with diagnostics;
  `Warning` and `Ok` enable search; `search_index = None`
  short-circuits to `NotSearchable`.
- **Freelance showcase** (`examples/freelance/`, commit 7
  base, kept current through commit 9). Three realistic
  models — `Client`, `Project`, `Invoice` — defined entirely
  with `#[derive(RustioModel)]`. Self-contained workspace
  (separate `[workspace]` block) so the parent gates stay
  unaffected. Three migrations + a startup pipeline
  demonstration + 10 tests covering every commit's behaviour.
- **Admin runtime** (`Admin::from_schema::<T>` /
  `Admin::from_schemas(&[ModelSchema])`, commit 8). Generic
  `SchemaOps` provides full CRUD via raw SQL using only the
  `ModelSchema`'s column metadata. List / find / create /
  update / delete / object_label dispatch by `RustType`. SQL
  identifiers come exclusively from `&'static str` schema
  fields — no path for HTTP input to inject identifiers.
- **Search runtime**
  (`search::from_schema::indexer_from_schema::<T>`, commit 8).
  Validator → `MeiliClient::configure_index` → `Indexer::spawn`
  in one async call. Returns `Option<Indexer>`; `None` on
  validator errors or `NotSearchable`.
- **Macro auto-derives `search_index`** (commit 8). When any
  field declares `#[rustio(... searchable ...)]`, the emitted
  `ModelSchema` carries `search_index = Some(table)`
  automatically. Eliminates the per-model
  `with_search_index(...)` workaround the freelance example
  used in commit 7.
- **UX polish** (Phase 15 / commit 9). `label_for` humanises
  column names to Title Case and strips trailing `_id` foreign-
  key suffixes (`client_id` → `"Client"`, `published_at` →
  `"Published At"`). `BridgedField::effective_widget()`
  performs conservative name-based widget inference (email /
  phone / url / password) on top of explicit overrides.
  `singularise` handles `-ies → -y` (`companies` →
  `"Company"`). `Admin::from_schemas` logs each registration
  one-line. The freelance example prints a startup banner +
  readiness summary.

### Changed

- **`rustio_core::admin::Admin` gained two builders**
  (`from_schema::<T>`, `from_schemas(&[ModelSchema])`).
  Existing `Admin::model::<M>()` /
  `Admin::model_with_search::<M>(...)` still work unchanged;
  the two flows compose on the same `Admin` instance.
- **`rustio-macros`'s `RustioModel`** now emits
  `with_search_index(table)` when any column is `searchable`.
  Previous behaviour (no auto-`search_index`) was opaque to
  downstream callers; the new behaviour matches what every
  consumer would do manually.
- **`rustio-core/tests/macro_rustio_model.rs`** updated to
  expect the auto-derived `search_index` per the new macro
  behaviour.

### Notes

- **No breaking changes.** Every v1.x API still works. The
  schema-driven flow is purely additive — projects can mix
  manual `AdminModel` impls with schema-driven entries on the
  same builder.
- **Test surface grew from 402 (v1.1.1) → 501 lib + 109 cli
  = 610 across the workspace.** PG-gated integration tests
  (`RUSTIO_TEST_DB=1 cargo test -- --ignored`) still 46 of
  them, all green.
- **No new dependencies** were added across all seven feature
  commits. The schema-driven runtime reuses
  `sqlx`, `chrono`, `uuid`, `serde_json`, `log` — all already
  in the workspace.
- **Forbidden modules respected** through every commit:
  `rustio-core/src/contract.rs`,
  `rustio-core/src/contract_validator.rs`,
  `rustio-core/src/contract_doctor.rs` are each frozen after
  the commit that introduced them, and the validator's logic
  was never touched after commit 3.
- **Single-binary deploy contract preserved.** No new asset
  pipeline, no Tailwind rebuild for this release. Schema-
  driven CRUD is pure SQL string building; SchemaOps holds
  no template state.

## [1.8.2] - 2026-05-04

Admin chrome theming. Driven by the v1.8.1 vetcare review: the
`[design] brand` field in `.rustio/project.lock` was inert metadata
because nothing in the runtime read it, and overriding the admin
shell required either a partial template copy (fragile) or a full
fork. This release closes the loop with a small, additive builder
on `Admin`. No breaking changes.

### Added

- **`Admin::accent_color(impl Into<String>) -> Self`** — one-line
  shortcut to set just the brand accent. Hex form, with or without
  the leading `#`. Drives a runtime `<style>` block in
  `admin/base.html` that overrides every Tailwind utility class
  baking the framework's default teal as a literal RGB value
  (`bg-brand-600`, `text-brand-*`, `::selection`, link colour,
  focus rings, the `--ds-color-accent` CSS variable).
- **`Admin::theme(AdminTheme) -> Self`** — full chrome palette in
  one call. `AdminTheme` exposes six hex tokens that map onto the
  framework's existing `--rio-*` design tokens (accent, bg, surface,
  text, text_muted, border). The runtime override block redefines
  those tokens at the document `:root`, and the existing chrome
  cascades to the new palette in a single repaint — topbar, sidebar,
  body, cards, headings, and hairlines all follow because every
  framework component already reads these tokens via `var(--rio-X)`.
- **`AdminTheme` struct** with `Default` matching the framework's
  current chrome. Operators typically override 1–3 fields with
  `..AdminTheme::default()` to keep the rest at framework values.
- **`Admin::active_theme()` and `Admin::accent()` accessors** for
  read-only inspection (used internally by `BaseContext`; rarely
  needed by projects directly).
- **Hex parser with safe fallback.** `hex_to_rgb_triplet` parses
  `#rrggbb` (or `rrggbb`) into the space-separated form Tailwind
  expects in CSS variables. On any malformed input — wrong length,
  non-hex characters, empty string — falls back to the framework
  default RGB rather than panic. The admin path never breaks over
  a config typo.

### Changed

- **`SiteBranding` is untouched.** Adding the new fields there
  would have broken every project that constructs the struct
  directly. Putting accent and theme on `Admin` instead is fully
  additive — projects that don't call `.theme(...)` or
  `.accent_color(...)` render identically to v1.8.1.

### Notes

- 10 new tests across the workspace (424 lib, 101 CLI total) cover
  hex parsing, the accent shortcut, the theme builder, the override
  block schema, and a full template render proving all six theme
  colours flow into the rendered HTML.
- Single-binary deploy contract preserved: no per-project asset
  pipeline, no Tailwind rebuild, no separate stylesheet to ship.
  Cost is one tiny `<style id="rio-accent-override">` block per
  page render.
- Future work: auto-load the accent from `.rustio/project.lock`
  `[design] brand` so the lock value flows without an explicit
  builder call. Lives in the rustio-cli scaffold (or a future
  Phase 12/d brand-token pipeline), not in rustio-core.

## [1.8.1] - 2026-05-04

Polish release in response to the v1.8.0 end-to-end review. Five DX-grade improvements; no breaking changes.

### Added

- **Orphan-admin-file detection (`rustio-core`).** `Templates::new`'s startup scan now also walks `templates/admin/*.html` and warns about any file whose name doesn't match an embedded template — catches the inverted-typo case where a developer writes `templates/admin/baes.html` thinking they're overriding `base.html` but actually shadow nothing. Symmetrical to the v1.8.0 stub-override warning.
- **`rustio doctor --json`.** New flag emits a single canonical JSON document on stdout: `status` (`ready` / `ready_degraded` / `not_ready`), `blockers`, `warnings`, and a `checks[]` array with lowercase severity strings (`ok`, `blocker`, `warning`, `skipped`). Suppresses the human banner. Designed for CI gating: `rustio doctor --json | jq .status`.
- **`BIND` and `PORT` env vars in scaffolded `MAIN_RS`.** Production deploys can now change the listening address without editing `src/main.rs`. Defaults preserved (127.0.0.1:8000) so existing projects need no change. The "no admin user" startup banner interpolates the actual addr so the URL it shows is correct when overridden.
- **`RUSTIO_ENV` env var in scaffolded `MAIN_RS`.** Defaults to `development`. When set to `production`, the dev-only "no admin user" onboarding banner is suppressed (it's a developer hint, not an operational signal). Active mode is logged at startup for operator visibility.
- **`.env.example` documents the new knobs.** `BIND`, `PORT`, and `RUSTIO_ENV` ship as commented-out lines so an operator sees them when configuring `.env`.

### Changed

- **Per-project default `DATABASE_URL` in scaffolded `.env.example`.** Was a hardcoded `rustio_dev`; now interpolates `<name>_dev` so two rustio projects on one Postgres no longer collide on the auth tables (`rustio_users`, `rustio_sessions`, …) by default. Existing projects are unaffected — only new scaffolds change.
- **Hardened scaffold `.gitignore`.** Was 14 bytes (`/target` + `.env` only); now includes `.env.local`, `.env.*.local`, `**/*.rs.bk`, `/.idea`, `/.vscode`, `*.iml`, `.DS_Store`, `Thumbs.db`. Reduces accidental commits of secrets, IDE state, and OS metadata.
- **Doctor JSON severity uses string identifiers**, not integer codes — easier to grep and read in CI logs.

### Fixed

- **Backfilled `CHANGELOG.md` for 1.7.0 and 1.7.1.** Both releases had shipped without entries; the v1.8.0 changelog noted the gap as a TODO. Now closed; v1.8.0's stale "1.7.x shipped without entries" note was also removed.

### Notes

- All workspace crates republished at 1.8.1 to keep version inheritance lockstep, even though `rustio-macros` has no source changes.
- The override safety net (1.8.0) and orphan detection (1.8.1) are non-fatal: the framework still serves whatever the developer put on disk — these scans only make the silent-failure modes observable in the log.
- 414 lib + 101 CLI tests pass against this release. Clippy clean. CSS in sync.

## [1.8.0] - 2026-05-04

### Added

- **AI-native project scaffold (Phase 12/a).** `rustio startproject <name>` now writes `.ai/context.md` (a human + AI-readable description of the project — domain, do/don't rules, design system, template conventions) and `.rustio/project.lock` (machine-readable TOML metadata: project name, CLI version that created it, database backend, brand colour, design system tag). Both are committed to git; neither belongs in `.gitignore`. Lays the foundation for AI agents to read project context in a single file before making changes.
- **Phase 12/b template scaffold + override boundary.** Scaffold writes `templates/home.html` with the project name baked in at build time (`format!()` substitution), no runtime template variables. The runtime template loader resolves overrides by **exact path match** under `templates/`: to override `admin/foo.html`, save your version at `templates/admin/foo.html`. There is no separate `overrides/` directory — that path was documented in early drafts as a fiction (the loader never read from it) and has been removed.
- **Phase 12/c doctor checks.** `rustio doctor` learned three new project-state checks, slotted between Project root and DATABASE_URL:
  - `AI context` — `.ai/context.md` exists and is non-empty.
  - `Project lock` — `.rustio/project.lock` parses as TOML and contains `[project]` with `name` + `rustio_version`.
  - `CLI version` — installed CLI version vs the version recorded in `project.lock`. Mismatch is a warning, not a blocker.
  All three are warnings (not blockers): legacy projects (created before 1.8.0) have none of these files and the app still boots fine.
- **Override safety net (`rustio-core`).** `Templates::new` now scans the disk root once at startup, INFO-logs every override of an embedded template, and WARN-logs any override file missing all of `{% extends %}`, `{% block %}`, and `<html>`. A 14-byte `<h1>TEST</h1>` override of `admin/base.html` no longer silently destroys the admin UI — the framework still serves the override (non-fatal) but the operator now sees the failure mode in the log.
- **Non-blocking update check.** `rustio` commands fetch the latest published version from crates.io on a detached background thread once per day, cache the result, and print a one-line "newer version available" hint at the end of the next command. Silent on network failure; honours `RUSTIO_NO_UPDATE_CHECK=1` and `CI=1/true`.

### Changed

- **Scaffold's `rustio-core` pin.** Generated `Cargo.toml` now interpolates the CLI's own major.minor (e.g. CLI 1.8.0 → `rustio-core = "1.8"`). Was a literal `"1.4"` since v1.4, which cargo's caret semantics quietly resolved to "any 1.x ≥ 1.4" — a developer reading the file got 1.7.x with no signal in the manifest. The pin now matches what cargo actually resolves and stays in lockstep with the CLI as it bumps minors.
- **Doctor heading** — "checking your environment" → "checking your project" to match the new project-state framing.

### Fixed

- **`rustio doctor` no longer silently misclassified the legacy-project case.** With Phase 12/a/c shipped, projects without `.rustio/project.lock` now show a clear warning rather than passing checks against an absent file.
- **Disk-override breakage is observable.** See override-safety-net above.

### Notes

- Phase 12/d (templates take/diff/upgrade tooling) and 12/e (`rustio ai` subcommands) are documented in `docs/phases/PHASE12_PLAN.md` but **not** in this release.
- Scaffolded projects against 1.8.0 will still pull `rustio-core = "1.8"` from crates.io transparently. Existing projects on 1.7.x continue to work unchanged.

## [1.7.1] - 2026-05-02

### Changed

- **Admin UI refinement + design-system alignment.** Patch release closing out Phase 11 work: teal brand migration, Windmill-style design tokens, and per-template alignment (`list.html`, `form.html`, `confirm_delete.html`).

### Notes

- Backfilled entry — released without a CHANGELOG entry at the time. See `git log v1.7.0..v1.7.1` for the per-commit breakdown.

## [1.7.0] - 2026-05-02

### Added

- **Built-in user profile** (Phase 10). New `rustio_users` and `rustio_sessions` schema tables, a built-in user profile page (handler + template), and a "user-profile extension closure" that lets a project add custom fields without forking the framework.
- **Phase 11/a teal brand migration.** Brand accent moved from the previous indigo to a teal token system inspired by Windmill, applied across the admin chrome.
- **Phase 11/b template alignment.** `list.html`, `form.html`, and `confirm_delete.html` rewritten against the new design tokens for visual consistency with the rest of the admin UI.

### Notes

- Backfilled entry — released without a CHANGELOG entry at the time. See `git log v1.6.0..v1.7.0` for the per-commit breakdown.

## [1.6.0] - 2026-05-01

### Added

- **First-run banner.** Scaffolded projects now print a clear "⚠ No admin user found" banner on startup when the `rustio_users` table is empty, with the exact `rustio user create --email admin@<project>.local` command and the admin URL. Disappears once any user exists. Read-only probe; never blocks startup.
- **Welcome page at `GET /`.** Replaces the implicit `/` → `/admin` redirect with a minimal landing page (`templates/home.html`) — links to `/admin` and the docs repo, plus the create-admin hint. Plain HTML + ~15 lines of CSS, no framework, no JS.
- **Interactive `rustio user create`.** Run with no flags (or partial flags) to be prompted: `Email:`, `Password:`, `Confirm password:`. Password validated (min 8, no all-same character, no common passwords). Polished error wording: "Password must be at least 8 characters.", "Password is too weak.", "Passwords do not match." Non-interactive flag-only invocation still works.

### Changed

- **`rustio user create --role` default:** `"admin"` → `"administrator"`. The previous default never parsed (`Role::parse` only accepts the canonical name); this had been silently broken since v1.0.
- **CLI loads `.env` at startup.** Subcommands like `rustio user create`, `rustio migrate apply`, etc. now resolve `--db` from the project's `.env` without requiring a shell `export DATABASE_URL=...`. Mirrors what the scaffold's `MAIN_RS` does at runtime — same env, same behavior.
- **Scaffold README quickstart** now includes the `rustio user create --email admin@<project>.local` step between `cargo run` and the admin URL.
- **Scaffold `Cargo.toml` template** adds `sqlx = "0.8"` (with `runtime-tokio`, `postgres` features) for the new `SELECT COUNT(*) FROM rustio_users` probe. Already transitive via `rustio-core`, so the binary size is unchanged.

### Notes

- No `rustio-core` or `rustio-macros` changes — `cargo publish -p rustio-cli` only.
- No auto-seeding of users; the framework remains explicit-by-default.
- Admin auth flow unchanged — `/admin` still requires login and redirects unauthenticated visitors to `/admin/login`.
- Scripted callers of `rustio user create` that explicitly passed `--role admin` were already failing pre-1.6 (see "Changed"). After 1.6, those scripts get the correct default by simply omitting the flag.

## [1.5.0] - 2026-05-01

### Added

- **`rustio doctor`** — pre-flight diagnostics for first-run setup. Five read-only checks: project root, `DATABASE_URL`, PostgreSQL TCP reachability, PostgreSQL connection (via `SELECT 1`), Meilisearch reachability. Distinguishes auth failure / missing role / missing database / insufficient privileges via SQLSTATE codes (`28P01`, `28000`, `3D000`, `42501`) with a tailored fix recipe per case.
- **Three result states:** `READY ✓`, `READY (DEGRADED) ⚠` (warnings only), and `NOT READY`. Green and degraded states append `Next: cargo run` so beginners always know the next command. Exit 0 for READY/DEGRADED, exit 1 for NOT READY.
- **`--quiet` / `--verbose` / `--no-color`** flags on `rustio doctor`. Honors the `NO_COLOR` env var convention.
- **`.env` loading in scaffolded projects.** New projects' `MAIN_RS` now calls `dotenvy::dotenv().ok()` at startup, so `cp .env.example .env` actually works (this was a hidden first-run trap pre-1.5.0).

### Changed

- Scaffold's DB-failure banner now ends with *"For a step-by-step diagnosis, run: rustio doctor"* — discoverability without gating `cargo run` on doctor.
- Generated project `Cargo.toml` template adds `dotenvy = "0.15"` so the new `.env` loading compiles in fresh projects.

### Notes

- No `rustio-core` or `rustio-macros` changes — patch on `rustio-cli` only. Existing v1.4.x consumers upgrade by `cargo install rustio-cli`.
- PostgreSQL remains the only supported backend (per project policy).
- Doctor is read-only by contract — never creates databases, never runs migrations, never mutates `.env`. Doctor diagnoses; the user executes.

## [1.4.2] - 2026-05-01

### Fixed

- **`rustio new app` outside a project polluted the working directory.** The command previously created `src/apps/<name>` in any directory (including $HOME) without validation. It now requires being inside a RustIO project and fails with a clear, beginner-friendly error.

### Added

- **Django-style commands:**
  - `rustio startproject <name>` — create a new project
  - `rustio startapp <name>` — create an app inside a project
- **Project detection helper** — shared foundation for future CLI features.
- **README.md in project scaffold** with a minimal quickstart.

### Changed

- Generated projects now depend on `rustio-core = "1.4"` instead of "1.0".

### Notes

- Existing commands `rustio new project` and `rustio new app` are still supported for backward compatibility.

## [1.4.1] - 2026-05-01

### Fixed

- **List view column proportions.** Type-driven width hints on `<td>` (id 72px, checkbox 110px, datetime 170px, actions 100px) so variable-text columns get the remaining space. Fixes cramped layouts on wide-table models.
- **Row hover state.** Previous hover used the same colour as zebra striping → no feedback. New `--rio-bg-soft` token applied to `tr:hover td` with a 120ms transition.

### Changed

- **Primary label is now the row's edit anchor.** First non-typed cell renders as `<a class="row-link">` to the edit page. Pure type-driven (no field-name lookup); ID and typed cells are skipped so the link lands on the meaningful column.
- **Datetime cell layout.** Date on top, time below, in a two-line stack.
- **Default pagination size.** 50 → 25 rows per page for scanability. Optional `?per_page=10|25|50|100` query override; invalid values fall back to 25.

### Added

- **Generic pagination on admin list views.** Server-side slicing, out-of-range page numbers clamp to the last valid page, single `<div class="pager">` block matches the existing toolbar styling.

### Notes

- No API breaks; v1.4.0 consumers upgrade with no code changes.
- Multi-select / bulk actions deferred to v1.5.0 — see RFC.

## [1.4.0] - 2026-04-30

A UI / design-system release. The admin chrome has been migrated end
to end onto a single, finalized design system; every legacy
component-class has been pruned. Type-driven list and form rendering
is locked down. No API changes — existing models, handlers, and
project layouts continue to work without modification.

### Highlights

* **Finalized admin design system.** Every admin template now uses
  one set of bare-named components (`.card`, `.btn-primary`,
  `.field`, `.alert`, `.table`, `.crumbs`, `.page-head`, `.save-row`,
  `.danger-zone`, `.status-card`, `.login-card`, and friends). All
  rules are token-driven through `--rio-*` CSS custom properties so
  a one-line palette change ripples through the whole admin.
* **Full admin template migration.** All twenty-plus admin templates
  (login, dashboard, model lists, model forms, user CRUD, group CRUD,
  password change, confirm-delete pages, error / forbidden /
  coming-soon, audit views) speak the same vocabulary. The shared
  `_form_field.html` include is now v14-only, so login,
  password_change, every model form, every user / group form
  inherits the same chrome through one source of truth.
* **CSS pruning.** The legacy admin.css component layer has been
  removed (about 65% of the prior file's bytes). Compiled
  `admin.css` shrank from ~66 KiB to ~23 KiB. Inline
  `<style>` (the design system) and `input.css` (Tailwind base +
  legacy survivors) have **zero overlapping selectors** — exactly
  one definition site per component.
* **Type-driven table / form rendering — locked.** Every admin list
  cell dispatches on `field.kind` from the macro-emitted
  `FieldType::widget()`; every form input is rendered through the
  shared `_form_field.html` include. No string-shape detection, no
  per-model templates, no field-name hardcoding.
* **Long-text table truncation.** Tables that carry summary /
  description / notes columns no longer blow up the layout.
  `table-layout: fixed` plus a `.cell-text` 2-line clamp keeps the
  column stable; the full text is preserved on hover via the
  `title=` attribute.
* **DateTime fixes.** List cells render the canonical
  `YYYY-MM-DDTHH:MM` ISO-8601 separator that `<input
  type="datetime-local">` requires (`T`, not space). Native
  date / datetime-local inputs match the v14 input chrome.
* **`Option<String>` display fix.** `#[derive(RustioAdmin)]`
  previously refused to compile any model declaring an
  `Option<String>` field; the `display_values` arm now splits
  `String` and `OptionalString` correctly (None → empty string,
  Some(v) → v).
* **Login page rewritten.** Now renders inside the v14 design
  language (white card, rust-accent submit, focus ring on
  inputs) instead of the previous dark-navy legacy card.

### Improved

* **Generic table column behaviour.** Numeric columns use tabular
  figures and right-alignment via `.num`. Boolean cells render
  through consistent badge components.
* **Stress-tested at scale.** The bookshelf consumer seeds 100+
  rows across multiple model shapes (numbers, booleans, datetime,
  nullable fields, long text, large numbers, RTL text) — used to
  validate truncation, pagination, and the kind-driven dispatch
  before this release.
* **Documented test-pinned classnames.** `<span class="required">`,
  `<span class="btn-danger">` (disabled-Delete), `<button
  class="btn-danger">` (confirm-delete submit), `<table
  class="results row-clickable">` (users_list), and the
  `.badge-v14 / .badge-yes-v14 / .badge-no-v14` boolean-cell trio
  in list.html are pinned by render tests; each is annotated at
  its callsite so future migrations don't accidentally rename them.

### Removed

* Legacy `@layer components` rules in `assets/css/input.css`:
  `.admin-shell`, `.topbar*`, `.sidebar*`, `.demo-banner*`,
  `.btn-primary` / `.btn-secondary` / `.btn-ghost` / `.btn-back` /
  `.btn-edit` / `.btn-danger` rule body, `.deletelink-inline`,
  `.card` / `.card-compact`, `.module*`, `.object-tools*`,
  `#toolbar*`, `.form-row*` / `.form-input` / `.field-input`,
  `fieldset.module*`, `.submit-row*`, `.warningnote`, `.paginator*`,
  `#changelist*`, `body.login` / `#login-form*`, `.forbidden-page`,
  `.coming-soon-body`, `.danger-zone` (legacy), `.user-view*`,
  `.confirm-form .submit-row`, `.input`, `.table*`, `.page-header`,
  `.page-actions`, `.subhead-note`, `.hero-icon*`. Every removed
  rule had zero template consumers after the migration.
* Three orphan rule blocks from the inline `<style>` in `base.html`
  (`.toggle-row`, `.toggle`, the legacy
  `.form-row .field-input input[type=…]` font-size override).

### Kept (with `Used by:` notes in `input.css`)

`.breadcrumbs` (audit views), `.results` / `.row-clickable`
(users_list test pin + audit views), `.user-row`, `.actions` /
`.action-*` (bulk-action UI gated until Phase 8), `.required`,
`.errornote` (`_field_errors` include), `.messagelist` /
`.message-*` (flash banner), `.empty-list` (audit views),
`.checkbox-list` / `.checkbox-item` (user_edit Groups),
`.cascade-list` (confirm-delete), `.code-pill`, `.badge-success` /
`-warning` / `-danger` / `-neutral`, `.btn-*:disabled`.
Theme-switch classes (`.dark`, `.theme-rust`, `.theme-brand`)
preserved as public API.

### Accessibility

* Form labels link via `for="id_<name>"` ↔ `id="id_<name>"`.
* Skip-to-content link present in every admin page.
* `:focus-visible` rules fire only on keyboard navigation.
* Icon-only buttons carry `aria-label`.
* Text contrast: `--rio-text` on `--rio-bg` = **16.5:1** (AAA);
  `--rio-text-muted` on white = **8.6:1** (AAA);
  `--rio-text-faint` on white = **4.7:1** (AA).

### API

No breaking changes. `AdminModel`, `Model`, `RustioAdmin` derive,
`FieldType`, `AdminEntry`, `Admin::new()` builder, route
registration — all stable. Existing v1.3.x consumers can upgrade
without code changes.

### Performance

* Compiled `admin.css`: ~23 KiB (was ~66 KiB).
* Inline `<style>` block: ~46 KiB / 953 lines.
* Total CSS payload: ~69 KiB.
* Selector depth: max 3 levels; no deep nesting.

### Notes

* PostgreSQL-only (unchanged from v1.3.x).
* Sandbox tests: 402 passing. Pg-gated tests: 41 (require
  `RUSTIO_TEST_DB=1` + a running Postgres).

## [1.3.1] - 2026-04-29

### Fixed

* Added `rustio run` convenience command
* Added missing `.env.example` to scaffolded projects
* Fixed misleading scaffold next steps
* Removed unused import warning from generated app
* Improved database connection error messages

### Notes

* No API changes
* Hotfix for v1.3.0 first-run DX regressions

## [1.3.0] - 2026-04-29

### Added

* Full examples catalogue (6 production-style schemas)
* examples/README.md gallery index
* CONVENTIONS.md (shared cross-cutting rules)

### Notes

* First crates.io release of the v1 architecture (PostgreSQL-only, admin system, AI layer)

## v1.2.0 — UX & Developer Experience Upgrade

A polishing pass focused on stability, consistency, and onboarding —
no new features, no breaking changes. Six surfaces touched: admin
error rendering, form-validation UX, admin UI cohesion, the
post-logout flow, configuration discoverability, and CLI output.

### Added

- **Inline form validation with field-level errors**. The generic
  `do_create` / `do_update` paths bucket the flat `Vec<String>` from
  `from_form` by humanised label via the new
  `bucket_errors_by_label`; per-field errors flow through the
  existing `apply_field_errors` → `_form_field.html` path with
  `aria-invalid` and `aria-describedby` already wired. Unparseable
  errors fall through to the global banner so nothing is lost.
- **Post-logout success message**. `do_logout` redirects to
  `/admin/login?logout=1`; `show_login` surfaces a green
  "You've been signed out." banner via the existing `FlashCtx`
  shape and `base.html`'s shared flash block. No template change
  required.
- **`.env.example` with documented configuration**. Repo-root file
  listing every runtime / CLI / demo / AI variable, with one-line
  comments per entry. The README Quick start now points at it as
  the canonical list.

### Improved

- **Admin error handling** (consistent HTML responses). New
  `admin/error.html` template plus a path-aware middleware in
  `register_admin_routes` that traps `Err(_)` for `/admin/*` paths
  and renders through `render_admin_error_response`. Non-admin
  routes still bubble through `response_from_error` as `text/plain`
  (the consistent minimal format for API consumers).
- **Admin UI consistency**. Generic `form.html`'s Delete moved out
  of the submit row into a `danger-zone` fieldset, mirroring
  `user_edit.html` / `group_edit.html`. `user_edit.html` and
  `group_edit.html` gained matching Cancel buttons. Bespoke `_new`
  form submit buttons normalised to plain `Save` (no leading `+`
  icon).
- **CLI output formatting and next-step guidance**. `✓ ` prefix
  standardised across every success line (migrate, user, group,
  perm, scaffold, `ai apply`, `ai generate`, `ai update`). AI
  subcommands' existing markers are now framework-wide. `WARNING:`
  → `warning:` in `confirm_orphan`. `scaffolded` → `created project`
  to match `created app`. `rustio new app` and `rustio migrate
  generate` now print short next-step blocks after the success line.
- **README clarity**. New "What happens on first run",
  "Configuration", and "Replacing default authentication" sections.
  Quick start updated to use `cp .env.example .env`; Configuration
  section enumerates every runtime env var with default and
  missing-behaviour notes.

### Fixed

- **Form input loss on validation errors**. `form_ctx` gained a
  trailing `submitted: Option<&FormData>` parameter; when set,
  field values come from the user's posted form, not from `existing`
  and not from blank. `do_create` and `do_update` pass `Some(&form)`
  on the validation-error branch. Unchecked checkboxes correctly
  re-render as unchecked because the no-fallback semantics ignore
  `existing` once `submitted` is set.
- **Inconsistent admin error responses**. 400, 404, 405, 409, and
  500 on `/admin/*` paths used to render as bare `text/plain`; now
  they render styled HTML via the new error template and
  status-specific headings (`admin_error_heading`).
- **Cancel button UX regression**. The first-pass `<button
  onclick="history.back()">` broke the Esc keyboard shortcut (no
  `data-cancel.href` to read) and could send users to a foreign tab
  on direct entry. Replaced with `<a href="..." data-cancel
  onclick="if (history.length > 1) { history.back(); return false; }">`:
  Esc still navigates via the href fallback, `history.back()`
  upgrades the click when there's a prior page, and direct loads
  land on the safe href.

### Notes

- No breaking changes to public APIs, the database schema, the
  template-registry shape, or any trait surface (additive only).
- No new commands, no new dependencies, no AI changes.
- `Cargo.toml` workspace version unchanged at `1.0.0`, consistent
  with the v1.1-ai / v1.1.1 release pattern.

Tests: 395 (core) + 14 (cli) = 409 passing.

## v1.1.1 — AI Safety Hardening (Phase 9.1)

Two surgical fixes from the Phase 9 real-world validation report.

- **Prevent empty schema writes** (critical safety fix). `ai_gen::update`
  now refuses any model response that would clear a non-empty schema:
  `non-empty → empty` returns `GenerateError::EmptyResult` with the
  message *"Refusing to apply update: schema would become empty"*. No
  bypass flag. Empty-input → empty-output and any → non-empty paths
  pass through unchanged. Closes the data-loss vector where
  `ai update "remove everything"` could clobber a schema.
- **Enable `--yes` for `ai analyze --apply` / `--pick`**. The Analyze
  CLI variant now exposes `--yes` and threads it into the existing
  `ai_update` save path, matching `ai update --yes`. Phase 8.3.1's
  truth table is preserved: `--dry-run` still wins over `--yes`.

No changes to AI behavior (prompts, generate, analyze, explain are
byte-identical). No new commands, no new dependencies. `ai_update`'s
internal logic untouched — only the literal `false` at the analyze
dispatch sites was replaced with the threaded `yes`.

Tests: `update_refuses_empty_result` (rustio-core, exercises the full
truth table), `analyze_yes_skips_confirmation` (rustio-cli, pins the
`SaveOutcome` mapping). 388 + 14 passing.

## v1.1-ai — AI Developer Tooling (Phases 8.0–8.4)

Optional `rustio ai ...` CLI surface for LLM-assisted schema
authoring. Strictly developer-tool: the deployed binary serving
HTTP has no path into the LLM client. Single-call-per-command
discipline; the deterministic `plan / review / apply` pipeline
runs separately.

### Added

- **`rustio ai generate`** (Phase 8.0). Prose → validated `Schema`
  JSON. New module `rustio-core/src/ai_gen/` with `mod.rs` (entry
  + `parse_response` + fence-strip), `client.rs` (Anthropic
  Messages API via reqwest), `prompts.rs` (system + user
  templates derived from `SCHEMA_VERSION` and `VALID_TYPE_NAMES`).
  Output validated through `Schema::validate()` before write.
  CLI refuses to overwrite without `--force`.
- **`rustio ai update`** (Phase 8.1). Single LLM call to evolve a
  schema with a free-form instruction. Diff vs current,
  interactive y/N (or `--yes`), atomic write. PRESERVE-BY-DEFAULT
  prompt contract enforced by 5 NEVER rules. New `diff` submodule
  renders human-readable change lines (model add/remove, field
  add/remove, relation churn). Empty diff yields `(no changes)`.
- **`rustio ai analyze`** (Phase 8.2). Read-only audit: structured
  text response (ISSUES / SUGGESTIONS / SCORE) parsed into
  `AnalyzeReport`. Tolerant parser: case-insensitive section
  headers, bullet-stripping, `(none)` placeholder, full-text
  fallback when no headers found.
- **`ai analyze --pick N` / `--apply <instruction>`** (Phase 8.3).
  Bridge analyze → update without retyping. `--pick` runs analyze
  + extracts suggestion #N + hands it to update (max 2 LLM calls).
  `--apply` skips analyze, routes straight to update (1 LLM call).
  Mutually exclusive at the clap parser layer.
- **`--dry-run`** (Phase 8.3.1) on `ai update`,
  `ai analyze --pick / --apply`. Runs the full LLM flow + diff
  print but skips the y/N confirmation and never writes.
  `SaveOutcome::DryRun` is the single decision point that all
  save paths funnel through. Defense in depth: `--dry-run` wins
  over `--yes`.
- **`--explain`** (Phase 8.4) on `ai update`,
  `ai analyze --pick / --apply`. ONE additional LLM call after
  the diff to narrate `Why` + `Impact`. Strict prompt contract:
  no inventing, no further suggestions, no echoing the schema.
  Tolerant section-header parser; bullets-only inside sections so
  trailing prose is dropped.

### Constraints (preserved)

- AdminOps trait, FormData shape, handler signatures, routes,
  templates: all byte-identical surfaces from v1.0-admin.
- Existing rule-based `ai/` pipeline (plan / review / apply):
  untouched.
- LLM call cap per command: 0 (plain analyze), 1 (generate /
  update / `--apply`), 2 (`--pick` analyze + update; or
  update + explain), 3 (`--pick` + `--explain`). Never recursive.

### Verification snapshot (v1.1-ai)

```
cargo test --workspace                    387 (core) + 13 (cli) passed; 41 ignored
cargo clippy --workspace --all-targets    clean
make css-check                            clean
```

## v1.0-admin — Production Admin (Phases 0–7.6)

Post-1.0 work on the admin surface: a design-system pass, a
schema-driven form-rendering layer, foreign-key navigation, a
Tailwind build pipeline, inline-error UX + keyboard a11y, and
production-readiness hardening. The single-binary deploy invariant
is preserved — admin.css and Inter woff2 are still
`include_str!`-baked.

### Added

- **Design system** (Phase 2). `docs/design-system.json` is the single
  source of truth for tokens (palette, typography ramp, spacing,
  radii, shadows). `tailwind.config.js` mirrors it via `theme.extend`.
  Two themes: light default + optional `theme-brand` (alias
  `theme-rust`) on `<html class="theme-brand">`. Dark surfaces are
  component-level (top bar, code blocks); no theme-wide dark toggle.
- **Tailwind build pipeline** (Phase 7a/2). Source at
  `rustio-core/assets/css/input.css`; compiled `admin.css` at
  `rustio-core/assets/static/css/admin.css` is committed. `make css`
  rebuilds; `make css-check` enforces parity in CI / pre-commit.
- **Self-hosted Inter** (Phase 7a/2). Four woff2 weights
  (Regular/Medium/SemiBold/Bold) under
  `rustio-core/assets/static/fonts/`, served by explicit per-weight
  routes registered in `register_admin_routes`. Adds ~95KB to the
  binary. No CDN dependency.
- **lucide icon set** (Phase 7a/2). 16 stroke icons baked at compile
  time in `admin/icons.rs`; templates write
  `{{ icon("home", class="w-4 h-4") }}`. Unknown names render as
  empty strings (silent, never panic).
- **Sidebar navigation** (Phase 7a/2). Top-bar brand mark + collapsible
  sidebar with per-link active highlight (driven by
  `window.location.pathname`, no per-page context plumbing). Mobile
  drawer toggle. ~30 lines of inline JS, no framework.
- **Auto timestamps** (Phase 1/a). `created_at` / `updated_at` of type
  `DateTime<Utc>` are auto-promoted to `DateTimeAuto` in the admin —
  hidden on create, read-only on edit, populated by the framework.
- **Empty-state UX** (Phase 1/c). List pages distinguish *true-empty*
  (no rows in the table) from *filtered-empty* (filters are too
  restrictive); each gets a different message and CTA.
- **Dynamic list rendering** (Phase 5/a). List pages are no longer
  hand-rolled per model. The list template iterates a
  `Vec<ListField>` produced by `list_ctx`, with per-row values
  via `#[serde(flatten)] HashMap<String, String>`.
- **Schema-driven widget mapping** (Phase 5/c). `map_field_to_ui`
  picks widget + input_type from `AdminField` via a four-arm cascade
  (choices → relation+multi → relation → `FieldType` match). One
  layer; backend-driven; no template-side switches.
- **Enum + relation-driven selects** (Phase 5/d). `AdminField.choices`
  surfaces closed enum lists as `<select>`. `AdminRelation.multi`
  surfaces M2M relations as multi-`<select>`. Additive — `FieldType`
  remains `#[derive(Copy)]`.
- **Layout intelligence** (Phase 6). `FormSection { title, fields }`
  partitions a model's fields into Default / Metadata / Advanced
  sections via name heuristics. The generic form template renders
  each section as a responsive 1-col / 2-col grid.
- **Unified form rendering** (Phase 6.2). All six bespoke forms
  (login, password change, user new/edit, group new/edit) now use
  `FormField` + `FormSection` + the shared
  `admin/includes/_form_field.html` partial. One renderer for every
  widget; custom blocks (banners, danger zones, checkbox lists,
  permission grids) stay bespoke alongside the FormFields.
- **Real FK / M2M options** (Phase 7.1). Relation selects are
  populated from the database via `AdminOps::list`, not stub
  `[("1", "Item 1"), ("2", "Item 2")]` placeholders. `form_ctx`
  stays sync; show handlers fetch via `resolve_relation_options`
  (async) and pass the option map in.
- **FK truncation + searchable selects** (Phase 7.2). Relation
  options are capped at `FK_OPTIONS_LIMIT = 50` so a relation with
  1000+ rows is still usable. Each FK select gains a sibling text
  input that filters `<option>`s client-side; the selected option
  is exempt so a chosen value never disappears mid-edit. Plain
  `<select>` remains fully functional with JS disabled.
- **Remote-search FK endpoint** (Phase 7.3). New
  `GET /admin/search/:model?q=<query>` route, Staff-guarded, returns
  `application/json` (`[{value, label}, ...]` capped at 20). With
  JS enabled the search input fetches against this endpoint; with
  JS disabled the truncated 50-row plain `<select>` still works.
- **Inline field errors + keyboard / a11y / power UX** (Phase 7.5,
  Path A). `FormField.errors: Vec<String>` plus a
  `field_errors: HashMap<String, Vec<String>>` parameter on
  `form_ctx`; bespoke validators (user_new / user_edit /
  group_new / password_change) push errors into a parallel
  field-keyed map alongside the global Vec, then `apply_field_errors`
  walks the sections and attaches them per FormField. Template
  renders `<p id="error_<name>">` blocks with `aria-invalid` and
  `aria-describedby` wired correctly. Plus 200ms FK search
  debounce, loading + empty hints, ArrowDown→select, Enter→blur,
  auto-select on focus, table arrow nav, double-submit guard,
  global keyboard shortcuts (`/` focus, Cmd/Ctrl+S submit, Esc →
  `data-cancel` anchor), `:focus-visible` ring, sticky submit row,
  inline checkbox layout.
- **Production hardening** (Phase 7.6). `OptionalI64` no longer
  silently swallows garbage input — empty stays None, non-empty
  unparseable surfaces a validation error. String fields trim
  whitespace; whitespace-only required input triggers the
  required-field error. Postgres constraint violations (FK,
  UNIQUE) lifted from `Error::Internal` (→ 500) to `Error::Conflict`
  (→ 409) inside `From<sqlx::Error>`; `ConcreteOps::create / update`
  catch the lifted Conflict and convert to a validation-error
  `Vec<String>` so the form re-renders with an inline message
  instead of crashing the request. `search_options` swallows
  transient DB errors with `log::warn`. `show_search` caps the
  query at 200 chars (UTF-8 char-boundary safe).
- **Granular role ladder** (Phase 7a/0.5). The previous Admin / Staff
  / User trio expanded to a five-rung linear ladder: `User < Staff
  < Supervisor < Administrator < Developer`. `Administrator` and
  `Developer` bypass per-permission checks; `Staff` and `Supervisor`
  go through the permission machinery. `is_active = FALSE` short-
  circuits both, checked **before** the bypass.
- **Demo mode** (Phase 7a/0.5). `RUSTIO_DEMO_MODE=1` seeds one demo
  user per role (5 users) on boot; email is `<role>@<domain>`,
  password is the role slug. A demo banner renders site-wide while
  the session is owned by a demo user. Same binary serves demo and
  prod; the env var is the only toggle.
- **CLI escape hatch for orphaned roles** (Phase 7a/0.5/f). When the
  UI guard refuses a destructive role change, `rustio role set
  --email --role` (with stdin `I UNDERSTAND` confirmation, or
  `--yes` for scripted operators) lets an authorized operator
  proceed.

### Changed

- **Admin module file count** held at 11 (plus four test siblings)
  through every phase. New surface area landed inside existing
  files — most density in `render.rs` (which grew from
  context-struct serialisation to also house the dynamic-form
  layer, FK option resolution, and the search helpers).
- **Roles vocabulary** in code, templates, and CLI updated end-to-end
  (Phase 7a/0.5). Old `Role::Admin` is gone; the closest replacement
  is `Role::Administrator`. Migration: re-grant operators by role
  on first boot.
- **Recursion limit** bumped to `#![recursion_limit = "256"]` in
  `rustio-core/src/lib.rs` (Phase 7.3). The default 128 was
  insufficient for the render-test fixtures' hand-built
  `serde_json::json!` literals once `FormField` grew to ~17 fields.

### Removed

- **Stub FK / M2M options** (Phase 7.1). The placeholder pairs
  `[("1", "Item 1"), ("2", "Item 2")]` are gone — every relation
  select is now backed by real data.
- **Dead `error.html` + `.cancel-link` rule** (Phase 0
  stabilization).

### Verification snapshot (end of v1.0-admin / Phase 7.6)

```
cargo test --workspace --lib              359 passed; 0 failed; 41 ignored
cargo clippy --workspace --all-targets    clean
make css-check                            clean
```

## 1.0.0 — Production stack

This release pivots RustIO from a single-machine, SQLite-backed admin
toolkit into a production-grade web framework. Almost every module
has been rewritten or extended.

### Added

- **PostgreSQL backend.** `Db` is now a `PgPool` wrapper with sensible
  defaults: 30 max connections, 1s acquire timeout, 5min idle timeout,
  30min max-lifetime. Configurable via `DbOptions`.
- **In-process query cache** (`cache::QueryCache`). LRU keyed by
  `"table:fragment"`, automatic prefix invalidation on every
  `create/update/delete`. Default capacity 2048 entries.
- **Full-text search via Meilisearch.** `MeiliClient` is a lean REST
  client; `Indexer` runs a background batching worker that drains
  pending `IndexJob`s every 100ms (or 500 docs, whichever comes first).
- **`Searchable` trait** for opting models into the search index, plus
  `Admin::model_with_search::<M>(indexer)` which auto-wires
  create/update/delete to the indexer.
- **Users, groups, permissions** — full RBAC, modelled on Django:
  - `Role::Admin` short-circuits all permission checks (superuser).
  - `Role::Staff` gets fine-grained permissions per `add/change/delete/view`.
  - `Role::User` has no admin access at all.
  - Permissions are inherited from groups OR granted directly to users.
  - Permission lookups are cached for 60s in a `DashMap` per user.
  - Every model registered in the admin auto-emits its four
    canonical permissions on startup via `Admin::seed_permissions`.
- **Built-in users/groups admin pages** — `/admin/users` and
  `/admin/groups` ship out of the box, admin-only.
- **CSRF protection** (`middleware::csrf_protect`) — double-submit
  cookie pattern, `SameSite=Strict`. Every form in the framework now
  carries a `_csrf` hidden input.
- **Rate limiting** (`middleware::rate_limit`) — per-IP token bucket.
  `RateLimiter::default_limits()` gives 120 req/min; tune via
  `RateLimiter::new(capacity, window)`.
- **gzip compression** (`middleware::gzip`) — kicks in for text
  responses ≥1KB when the client accepts gzip.
- **Security headers** (`middleware::security_headers`) — sensible
  defaults for X-Content-Type-Options, X-Frame-Options,
  Referrer-Policy, Permissions-Policy.
- **Background tasks** (`background::spawn_housekeeping`) — runs the
  session sweeper every 10 minutes; intended as a hook for future
  recurring jobs.
- **Graceful shutdown.** Server listens for SIGTERM/Ctrl-C and stops
  accepting new connections, giving in-flight requests a moment to
  drain.
- **HTTP/1.1 keep-alive** — explicitly enabled on the server builder.
- **CLI grew**: `rustio user create/set-password/add-to-group`,
  `rustio group create/grant`, `rustio perm list/grant-user`. Password
  prompts use `rpassword` so credentials never leak into shell history.

### Changed

- **Workspace version → 1.0.0.**
- **Migrations splitter** now understands Postgres dollar-quoted
  bodies (`$$ ... $$`, `$tag$ ... $tag$`) so PL/pgSQL functions can
  ship in migrations without being chopped up.
- **Sessions** now have a background expiry sweeper instead of
  cleaning up on every read. Session reads also asynchronously update
  `last_seen` without blocking the request.
- **Identity model** got `is_active` and split `Role::Admin / Staff /
  User`. `Role::Staff` is the new "can use admin, but only what
  permissions allow" tier.
- **`Value` enum** in the ORM gained `Uuid` and `Json` variants, in
  addition to the existing `I32 / I64 / Bool / Text / DateTime / Null`.
- **`Row` wrapper** got `get_uuid` and `get_json` helpers.
- **Cookie names** now use the `auth::SESSION_COOKIE` constant
  everywhere; no hardcoded strings.

### Dependencies

| Crate | Version | Why |
|---|---|---|
| sqlx | 0.8 (postgres + uuid + json) | the database |
| reqwest | 0.12 (rustls) | Meilisearch REST client |
| dashmap | 6 | concurrent permission cache + rate-limit buckets |
| lru | 0.12 | the query cache |
| flate2 | 1 | gzip middleware |
| subtle | 2 | constant-time CSRF token compare |
| rpassword | 7 | CLI password prompts |

### Removed

- **SQLite backend.** PostgreSQL is now the only supported database.
  If you need SQLite, pin to `0.9.x`.

### Migration from 0.9.x

1. Set `DATABASE_URL=postgres://...` (or pass `--db` to the CLI).
2. Replace `Db::connect("sqlite::memory:")` with
   `Db::connect("postgres://...")`.
3. SQL migrations: change `INTEGER PRIMARY KEY AUTOINCREMENT` to
   `BIGSERIAL PRIMARY KEY` and `TEXT` timestamps to `TIMESTAMPTZ`.
4. Add the new middleware to your router (recommended):
   ```rust
   .middleware(middleware::rate_limit(RateLimiter::default_limits()))
   .middleware(middleware::logger)
   .middleware(middleware::security_headers)
   .middleware(middleware::gzip)
   .middleware(middleware::csrf_protect)
   ```
5. Call `admin.seed_permissions(&db).await?` after registering models.
6. If you want search: spin up Meilisearch, build an `Indexer`, and
   register models with `.model_with_search::<M>(indexer.clone())`.

## 0.9.0 — Clean rewrite

See git history for 0.9 release notes.
