# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

The **presentation layer**: a declarative `ViewSpec` that owns how each model
renders, a visual composition editor to author it, full internationalisation of
the admin shell (field + value display labels, per-user language), and a working
list filter bar — plus per-model RBAC and context-gated PII masking on the live
list. Throughout, one **iron rule** holds: the backend stays English (field
sources, stored values, sorting, links, data are never translated); only the
*displayed* shell text resolves through the active language.

### Added

#### Admin shell i18n (UI translation)

- **The admin's own UI strings now translate with the active language.** Until
  now only data-derived labels (column headers, enum values from the ViewSpec)
  followed the language switch; the framework's chrome — navigation, topbar,
  list toolbar, buttons, forms, the account page, the composition editor — was
  hardcoded English, so switching to another language left most of the screen
  untranslated. A new `t("English source")` template function (registered on
  the minijinja environment, reading the active language from
  `current_user.active_language`) resolves every shell string for the active
  language, falling back to the English source when a translation is missing —
  never blank. (`rustio-core/src/admin/uilang.rs`.) Rust-generated UI prose is
  translated through the same catalog too — the account page's role narrative,
  permission rows (label + description), and roles reference all follow the
  language switch, not just the static template strings.
- **Right-to-left layout.** When the active language is RTL (Arabic, Persian,
  Urdu, …), the document renders `dir="rtl"` and the entire admin layout
  mirrors — sidebar to the right, text right-aligned, accents and dividers
  flipped. The Console theme's directional CSS was converted to logical
  properties (`margin-inline-start`, `border-inline-end`, `text-align: start`,
  …), so a single stylesheet serves both directions with no duplication and no
  change to the left-to-right rendering. The example `rustio.locale.json` ships
  an Arabic locale to demonstrate it.
- **`rustio.locale.json` — an editable translation file.** A new optional
  project-root input (sibling to `rustio.design.json` / `rustio.context.json`):
  `{ "sv": { "Add": "Lägg till" }, "de": { … } }`. Keys are the English source
  string; entries override or extend the built-in catalog, and a language code
  present here that isn't built-in becomes **selectable in the switcher and
  settable** (the offered set is the built-in registry ∪ the locale file, with
  best-effort endonyms). A stray `"_comment"` key (or any non-object section) is
  ignored rather than rejecting the file. **Swedish ships built-in** as the
  reference locale; data is never translated here (the iron rule holds).

#### ViewSpec & list rendering

- **`ViewSpec` — a per-model presentation document.** Saved as
  `<model_snake_case>.view.json` next to the schema, it declares field display
  order, the role each field plays, which fields are filters, field merging,
  the default layout, and i18n labels. Pure declarative data: versioned,
  `deny_unknown_fields`-locked, and byte-for-byte deterministic on serialise
  (sorted maps, no `HashMap`, no timestamps). `ViewSpec::from_schema_model`
  derives a sensible default from any schema; `validate()` + atomic
  temp-and-rename writes keep a broken view off disk.
- **Deterministic renderer.** `viewspec::render` turns a ViewSpec + data rows
  into a structured `RenderedView` (no HTML, no AI) — the contract the admin
  and the `rustio view` CLI both consume.
- **`rustio view` CLI command** — render a model's ViewSpec against the schema
  (and optional sample rows) from the terminal.
- **The admin list renders through the ViewSpec.** Columns, order, and the
  Hidden guarantee (ids, `*_hash`/`password`/`token`, opaque PII are omitted)
  all come from the view. A **`?layout=` switcher** picks Table / List / Cards /
  Compact; the active layout can be **persisted as the model's default**
  ("Set as default", CSRF-protected, written back into the ViewSpec).

#### Composition editor (`/admin/<model>/view`, edit-gated)

- **Visual ViewSpec editing — one page, one Save.** Everything flows through
  the same `build_edited_spec → validate → save_view_spec` path:
  - **Field roles** — set each field's role (Title / Subtitle / Badge /
    Timestamp / Meta / Hidden).
  - **Order** — reorder fields with ▲ ▼ controls (order = display order),
    via explicit indices so it works without JavaScript.
  - **Filters** — a checkbox per field marks it a list filter; a Hidden field
    can never be a filter (UI + server enforced).
  - **Merge** — combine fields into one cell via a "Merge into" select
    (joined with " · "); members are removed from the field list and restored
    on unmerge. No overlaps, Hidden never merges, validate backstops a group
    of fewer than two.

#### Internationalisation (i18n) — display labels only

- **Per-language field labels.** `ViewSpec.labels` (`source → lang → label`)
  translates a field's *header*; an unlabelled field falls back to the admin's
  humanised English name, so label-less views render exactly as before.
  Editable in the composition editor's **Display label** column.
- **Per-value (enum) labels.** `ViewSpec.value_labels`
  (`source → value → lang → label`) translates a field's stored values
  (e.g. status `assigned` → `"Tilldelad"`). The stored value, sorting, and the
  status-pill **colour** stay English; only the shown text changes. The editor
  **auto-discovers** values for status-shaped fields and low-cardinality
  (`≤ 12` distinct) String fields and offers a label input per value.
- **Editing language switch (`?lang=`).** Switching the language you edit in
  is a GET reload, never a save, so it can't clobber another language's labels;
  inputs prefill strictly for the editing language only. A separate "Set as
  default" control makes the editing language the view's stored
  `default_language`.
- **Per-user language preference.** Each admin user picks their UI language
  (stored on `rustio_users.preferred_language`). The active render language
  resolves as **user preference → the view's `default_language` → `"en"`** and
  **never** mutates a ViewSpec.
- **Reusable language switcher.** One component, shown in the topbar and the
  sidebar bottom; displays **endonyms** (`English`, `Svenska`) while storing
  ISO 639-1 codes. The language set is open/extensible.

#### List filtering, RBAC & PII (admin list)

- **Working filter bar.** The toolbar renders a control per `ViewSpec.filters`
  field — tri-state select for booleans, a **value dropdown** for enum-like
  columns (option labels translated via value labels; value = English token), a
  **related-row dropdown** for foreign keys (showing the target's display
  label), and a free-text box for high-cardinality columns. Controls
  auto-submit; a **Clear** link resets; filters compose with search, sort, and
  layout. Dropdown choices match exactly (`=`); text filters match by substring
  (`LIKE`). (The live filter query now honours `ViewSpec.filters`, fixing a gap
  where new-style models — which set no macro-`filterable` flag — had no working
  filters at all.)
- **Per-model RBAC on list actions.** Create / edit / delete are gated on the
  signed-in user's role (`rbac::Role::permissions_for(table)`): SuperAdmin /
  Admin get full CRUD on app models, Editor loses delete, Viewer is view-only;
  framework `rustio_*` tables are stricter.
- **Context-gated PII masking.** When the project declares a
  `rustio.context.json`, shown sensitive cells (email / phone / personal id) are
  masked (short prefix, rest as `•`), including each sensitive source inside a
  merged cell. Without that context, nothing is masked — masking is an explicit
  posture, not a silent default. Hidden fields are always omitted.

### Changed

- **Examples replaced with `bookflow`.** The `medflow` and `taskhub` example
  projects are removed in favour of a single canonical example,
  `examples/bookflow/` — a general-purpose seven-model booking system
  (customers, resources, bookings, locations, schedules, assignments,
  invoices). It is domain-agnostic on purpose: the same schema fits container
  logistics, equipment rental, or appointments, reshaped purely by editing the
  ViewSpec. The README walks through the `rustio view` step end to end.

### Fixed

- **CSRF tokens now render in templated admin forms (SF-1).** The create /
  edit / delete forms (and every other admin POST form) now emit their `_csrf`
  hidden input, so CSRF verification works as intended on the templated path.

### Upgrading

- **A `preferred_language` column is added to `rustio_users`** (per-user i18n).
  Existing projects must run **`rustio migrate apply`** to back-port it — the
  column is added by `ensure_core_tables`, which the migration driver calls; the
  server does **not** migrate on boot. Until then, setting a language returns a
  500 (`no such column: preferred_language`); rendering is unaffected (no
  preference → the view's `default_language`, as before). Fresh databases get
  the column automatically. See `UPGRADING.md`.

## [2.0.5] - 2026-05-30

### Changed

- **"Bureau" admin theme.** The admin is restyled to a classic
  institutional look: a confident navy/institutional-blue accent
  (`#1E4C8A`) on a cool slate-and-paper neutral scale, large bold
  typography (17px base, extra-bold headings, uppercase bold-sans labels
  with monospace reserved for ids/counts/code), and crisp formal borders.
  Ships **light and dark themes** via `data-theme` on `<html>`; the dark
  theme is a soft, low-glare slate with a logical elevation ladder
  (canvas `#171B22` → border-strong `#454D5C`), off-white text, and a
  gentle blue accent — all meeting WCAG AA (body ≥ 4.5:1, UI ≥ 3:1).
  Fonts are the OS-native stack only (no webfont/`@import`, offline-safe).
  The list renderer tags the id column (`.rio-cell-id`), primary-name
  cell (`.rio-cell-primary`), and status cells (`.rio-pill-*`), and the
  topbar carries an environment chip (`.rio-env-chip`).

### Fixed

- **Admin URLs no longer render as `&#x2f;admin&#x2f;…`.** minijinja's
  built-in HTML escaper also escapes `/`, so every templated URL
  (`href`, `action`, sidebar/pagination links) across every admin page
  came out with `&#x2f;` instead of `/` — valid (browsers decode it) but
  noisy, non-standard HTML. The admin template environment now installs a
  custom formatter that escapes the Jinja2/Django character set
  (`<`, `>`, `&`, `"`, `'`) and leaves `/` alone. Safe strings and the
  XSS-relevant escaping are unchanged; only the spurious slash escaping is
  removed. Covered by new tests in `admin::templating`.

### Changed

- **Tuned `[profile.release]`** at the workspace root (`opt-level = 3`,
  fat `lto`, `codegen-units = 1`, `strip = true`) toward the project's
  stated ~15 MB stripped-binary / fast-cold-start targets. Unwinding is
  intentionally kept (no `panic = "abort"`) so a panic in one request
  handler unwinds that task rather than aborting the server. A minimal
  scaffolded app builds to ~6 MB stripped (vs ~27 MB debug); warm startup
  is unchanged (boot logic is identical).

### Fixed

- **Phantom demo `Order` model no longer leaks into every project's admin.**
  The admin runtime unconditionally registered a leftover demo model
  (`build_orders_config`, table `admin_new_demo_orders`) into the
  `AdminRegistry`, so every scaffolded project exposed a spurious
  `/admin/orders` route and an "Orders" dashboard card for a model the
  project never defined — absent from the project's own
  `rustio.schema.json`. Removed the demo registration and the dead
  `build_orders_config` helper; `register_generated` / `register_from_table`
  remain available for real config-driven models. Projects now show only
  their own models plus the built-in `User`.


## [2.0.4] - 2026-05-29

Documentation-only patch. Ships fresh **crate-level** READMEs onto
the crates.io listings — separate from the workspace root README
that github.com renders, which was already up to date.

The previous patch (2.0.3) re-published 2.0.2's bits with the new
workspace README, but each Cargo workspace member has its own
README file that crates.io serves on its individual listing. Those
crate-level files had drifted since the 2.0.x rebrand. 2.0.4
rolls the fresh text forward.

### Changed

- **`rustio-cli/README.md`** — substantial refresh.
  - Quick start now leads with `rustio init <name>` opening the
    setup menu and shows a representative Guided-mode interaction
    (clinic intake → blueprint summary → walkthrough).
  - New "Change something later" section showcases
    `rustio evolve "<request>"` with the three-way choice
    (Apply / Show technical details / Cancel).
  - Common-commands table mirrors `rustio help` —
    `start`, `evolve`, `migrate apply / status`, `doctor`,
    `explain`, `--why`. The legacy `ai plan / review / apply /
    validate` rows are replaced by a single pointer at
    `rustio help advanced`.
  - The v0.x wizard-output mock ("RustIO / Let's set up your
    project / Project name: readlist") is gone; the new excerpt
    matches what users actually see today.

- **`rustio-core/README.md`** — two surgical lines.
  - "the AI planner/review/executor pipeline" → "the typed
    schema-evolution pipeline that backs `rustio evolve`".
  - Example uses `rustio init mysite` instead of the long-
    deprecated `rustio new project mysite`.

- **`rustio-macros/README.md`** — untouched. Already neutral; just
  describes the proc-macro crate.

### Notes

- **No code changes.** All three crates are byte-for-byte
  equivalent to 2.0.3 modulo their respective READMEs. The bump
  exists solely to let crates.io re-serve the new crate-level
  README copy that 2.0.3 missed.

## [2.0.3] - 2026-05-29

Documentation-only patch — re-publishes the v2.0.2 codebase so that
the README displayed on the crates.io listings reflects the new
`rustio evolve` framing that landed in v2.0.2 but never made it
into the registry-served copy.

### Changed

- **README's "Evolving the schema later" section** now leads with
  `rustio evolve "<request>"` as the everyday verb (with the
  three-way Apply / Show technical details / Cancel choice
  explained inline). The scriptable `ai plan/review/apply`
  pipeline is mentioned as the CI-friendly low-level surface
  reachable via `rustio help advanced`, not the primary path.
- **README's "Naming — what about `rustio-admin`?" section** —
  "AI-augmented schema pipeline" → "guided schema-evolution wizard
  (`rustio evolve`)". The reciprocal note in `rustio-admin`'s
  README was updated in the same window so the two project pages
  describe each other accurately.
- **README's "What RustIO is NOT" list** — "Not an AI toy" →
  "Not an AI gadget", rewritten to acknowledge that `evolve`'s
  friendliness is a UX surface while the substance is the strict
  typed core and closed-vocabulary pipeline that make safe change
  possible.
- **README's "Want a fuller example?" paragraph** — taskhub's tour
  now points at "the `evolve` pipeline" instead of "the AI pipeline".

### Notes

- **No code changes.** All three crates are byte-for-byte
  equivalent to 2.0.2 modulo the README. The bump exists so that
  someone reading the crates.io listing today sees the new framing
  instead of the old.

## [2.0.2] - 2026-05-29

The simplification release. Hides every visible "AI" reference from
the day-one user surface, cuts the default help from ~19 commands
across 8 sections to ~10 commands across 6, and introduces a single
new verb that becomes the framework's signature change experience.

### Added

- **`rustio evolve "<request>"`.** Friendly interactive verb for
  changing the schema after the project is up. Reads a plain-English
  request, asks the planner to parse it into a typed change, runs
  the standard review for risk and warnings, and presents the same
  three-way choice the setup wizard uses:

      RustIO is ready to make this change:
        · add Task.summary  (String, required)

      ? Ready?
        › Apply — write the files
          Show technical details — plan, risk, warnings
          Cancel — don't change anything

  The blueprint summary is one bullet per primitive in plain English;
  the typed operation list, risk classification, and warnings only
  appear behind the "Show technical details" choice. When the
  planner refuses (closed vocabulary, won't guess), the refusal
  surfaces as a plain-English message — not the `PrimitiveError`
  Debug repr.

  Underneath, the pipeline is unchanged: `generate_plan` →
  `review_plan` → `build_plan_document` → `execute_plan_document`.
  Same atomic file-write path `rustio ai apply` composes by hand.

- **`rustio help advanced`** — a second help surface for scripting,
  rarely-needed project ops, and one legacy retrofit. Reached
  through a dedicated subcommand so the default `rustio help` can
  stay short. The CONTEXT section inside it renders only when
  `rustio.context.json` exists in the current directory — projects
  without one don't see noise about GDPR / PII detection on day one.

### Changed

- **`rustio help` is now ~10 commands across 6 sections.** Down
  from ~19 across 8. Dropped from the default surface: the
  scripting pipeline (`ai plan/review/apply/validate`), the
  standalone `SCHEMA` section, `migrate add-fks`, `context show /
  validate`, the entire `ENVIRONMENT` block. Every removed item is
  still callable; most also appear in `rustio help advanced` with
  cleaner framing.

- **The opening paragraph in `rustio help`** now names the full
  everyday loop including the change verb:

      If you're new: `rustio init <name>` creates a project and opens
      the setup menu — a guided walkthrough that proposes a starting
      shape. Run `rustio migrate apply` then `rustio run` to bring it
      up. To change something later: `rustio evolve "<what you want>"`.
      That's the whole loop.

- **The "AI" section in help is gone.** The pipeline commands
  (`ai plan / review / validate / apply`) keep their existing names
  for back-compat with CI scripts, but they appear only in
  `rustio help advanced` under a `SCRIPTING (composes evolve by
  hand)` heading. The `(deterministic, refusal-first)` sub-line is
  gone — the substance survives in the behaviour, not the framing.

- **Post-`rustio schema` hint** updated to point at
  `rustio evolve "<change>"` instead of the old `rustio ai plan`
  invocation.

### Notes

- **No new primitives, no new executor paths.** The substance of
  `evolve` is the existing typed pipeline. The work was at the
  product-orchestration layer, not the engine.
- **`ai plan/review/apply/validate` are still public API.** Scripts
  written against 2.0.x continue to work unchanged. The change is
  *what's discoverable*, not *what's callable*.

## [2.0.1] - 2026-05-29

A short maintenance release on top of 2.0.0. No API changes.

### Added

- **`docs/design-system.md`.** Documents the two admin stylesheets
  in the repo — what ships today (`rustio-core/assets/static/admin.css`)
  vs the v7 spec sitting at `rustio-core/assets/admin.css` — and the
  six-step migration path between them. Linked from the README's
  "Going further" list.

### Changed

- **Admin chrome: drop the Google Fonts dependency.** `base.html`
  no longer preconnects to `fonts.googleapis.com` / `fonts.gstatic.com`
  and no longer pulls Inter via a CDN stylesheet link. `--font-sans`
  keeps Inter at the head of the list (so projects that self-host a
  copy via their own `<link>` still pick it up), then falls through
  to the OS native UI stack: SF Pro Display on macOS, Segoe UI on
  Windows, Roboto / Oxygen / Ubuntu / Cantarell on Linux. The mono
  stack gets the same treatment with JetBrains Mono at the head and
  SFMono / Menlo / Consolas / Liberation Mono as fallbacks. The admin
  now renders identically offline, behind a strict CSP, or on an
  air-gapped network — the framework's single-binary promise extends
  to its chrome.

### Fixed

- **Release workflow is idempotent.** `release.yml` now wraps each
  `cargo publish` step in `.github/scripts/publish-if-new.sh`, which
  queries the crates.io API for the crate's `max_version` and skips
  upload when it equals the workspace version. A re-run at the same
  tag after a partial-success publish (a real situation that hit the
  2.0.0 release) is now safe from any state — published crates skip
  with a `::notice::` annotation; unpublished crates proceed
  normally.

## [2.0.0] - 2026-05-29

### Major-version jump — re-publishing the post-restart codebase

This release jumps from `0.11.0` to `2.0.0`. The reason is purely
about crates.io ordering, not a redesign: the pre-restart line of
this project published up to `v1.10.0`, then the codebase was
reset to `0.10.0` for a clean foundation. The 0.10.x and 0.11.0
releases happened on that new foundation but couldn't supersede
`v1.10.0` on crates.io, so `cargo install rustio-cli` kept handing
users the *pre-restart* `v1.10.0` binary — a codebase that no
longer matches this repository.

`2.0.0` cuts that knot:

- Code is the cumulative post-restart line (the work shipped under
  `0.10.0`, `0.10.1`, and `0.11.0` — see those entries below for
  the per-step record).
- `cargo install rustio-cli` from this release onward installs the
  current `main` branch, not the pre-restart codebase.
- The version jump explicitly signals "different major than v1.x —
  the two share a name but not a shape" per SemVer.

No new feature work in this entry beyond the version bump and the
compatibility note below — the actual surface is what `0.11.0`
already shipped.

### Compatibility

- **External: `rustio-admin` v0.22.0 (2026-05-29) renamed its CLI
  binary from `rustio` to `rustio-admin`.** The unrelated [`rustio-admin`](https://github.com/abdulwahed-sweden/rustio-admin)
  project (a Postgres-first admin framework) previously shipped a
  `rustio` binary through its v0.21.x line. From its v0.22.0 release
  onward the binary is named `rustio-admin`, so `cargo install
  rustio-cli` and `cargo install rustio-admin-cli` no longer
  silently overwrite each other in `~/.cargo/bin`. No change in this
  project; the note is for users who tracked the collision.


## [0.11.0] - 2026-05-28

The product-orchestration release. The pieces shipped in 0.10.x — a typed
core, a deterministic plan/review/apply pipeline, a templated admin —
were correct but felt like three adjacent features. 0.11 stitches them
into one onboarding experience built around a single new command.

### Added

- **`rustio start` — onboarding entry point.** A three-way menu
  (Guided / Manual / Import) that opens automatically at the end of
  `rustio init` and is also reachable at any time inside a project.
  Guided is the new conversational wizard; Manual prints the
  `rustio new app <name>` hints; Import is reserved for a future
  schema-replay flow.
- **`rustio_core::ai::intake` — deterministic free-text → typed
  `ProjectSketch`.** Five curated domain templates (clinic, blog,
  shop, crm, tasks). Returns `None` for ambiguous input rather than
  guessing — the same refusal-first posture the planner uses. 6 unit
  tests. No LLM in the loop.
- **`scaffold_app_with_fields` helper in the CLI** — sibling of
  `new_app` that takes an explicit `FieldSpec` list and emits a
  FK-aware `CREATE TABLE` migration. The wizard uses it to materialise
  each accepted model.
- **Light theme as the default** for the admin. Warm-neutral 9-step
  palette (`--color-light-50…900`), `--color-accent-soft` for soft
  highlights. Dark mode opt-in via `data-theme="dark"` on `<html>` +
  `localStorage.rio-theme`. No-FOUC bootstrap script runs before any
  stylesheet link.
- **Django-style internal pages.** `admin/list.html`, `admin/form.html`,
  and the sidebar were rebuilt around a `.rio-page-header` (breadcrumb
  + right-aligned actions), `.rio-detail-grid` (main column + 320 px
  side), `.rio-pagination` ("Showing 1–25 of 142" + page links), and
  a `.rio-empty` dashed-border standalone empty state. Sidebar groups
  models under named sections with optional count badges fed from
  `SidebarEntryView.count`.
- **Theme toggle button** in the topbar, with the icon kept in sync
  with `<html data-theme>` and persisted to `localStorage.rio-theme`.
- **Light + dark variants of every canonical admin screenshot** under
  `docs/screenshots/` (`admin-{login,dashboard,task-edit,tasks-list,
  empty}-{light,dark}.png`), plus `cli-start-{menu,blueprint}.png`
  for the new wizard surface.

### Changed

- **The summary screen in the wizard is now a system blueprint, not
  an operation log.** Before: `models queued / risk / warnings`.
  After: `RustIO is ready to create: 3 connected models, 2
  relationships, admin screens for every model, search/filters/
  pagination, 3 starter migrations`. The typed plan operations + risk
  classification + warnings live one keystroke deeper behind a
  *"Show technical details"* choice — progressive disclosure, not
  amputation.
- **"AI" is off the primary surface.** Help reorganised so `ai plan /
  review / apply` live in an `ADVANCED` section with the sub-line
  "deterministic, refusal-first". Wizard banner is *"Let's shape your
  project together,"* not *"rustio ai · …"*. Generated `apps/<x>/
  models.rs` files attribute themselves to `rustio start`. Internal
  module names (`ai::intake`, `ai::executor`, …) are unchanged because
  they're honest implementation labels.
- **README rewritten to lead with `rustio start`.** Quickstart drops
  from 6 steps to 5 (the `new app` step folds into the menu), inlines
  the menu + blueprint screenshots, retitles "The AI layer" to
  "Evolving the schema later (advanced)" without losing substance.
  Opening pitch trades *"so the AI layer can extend your system
  without breaking it"* for *"so changes to your schema, by hand or
  via the guided setup, stay safe-by-construction."*
- **`PaginationView`** extended with `per_page` / `total` / `from` /
  `to` so list templates render the "Showing N–M of T" caption
  without arithmetic in the template.
- **`SidebarEntryView`** gains a `count` field (-1 sentinel hides the
  badge for legacy `AdminEntry` sources that don't carry a row
  count).
- **`base_admin.html` consolidates the two-column shell** and the
  page-header / breadcrumb pattern is now the canonical interior
  layout for every model view.
- **CI runners**: `actions/checkout` v4 → v6, `actions/cache` v4 → v5
  (Node.js 24 baseline).

### Removed

- **`rustio ai start`.** Promoted to `rustio start`. Typing the old
  command now prints a one-line redirect.
- **The post-init `Want me to propose a starting shape?` Y/n prompt.**
  `rustio init` now chains straight into the `rustio start` menu so
  the onboarding is one continuous experience, not two stitched-
  together commands.
- **Bootstrap CSS + JS bundle** from the admin assets (was already
  superseded in 0.10.x; this release locks it out).
- **Three pre-existing un-suffixed screenshots** (`admin-dashboard
  .png` / `admin-tasks-list.png` / `admin-task-edit.png`) in favour
  of the new `-light` / `-dark` pairs.

### Fixed

- The Tailwind v4 build step (`build.rs`) now strips `@theme` only
  when it's a real rule, never in prose mentions inside comments.
- Sentence-case field labels in list headers + form labels.

### Numbers

- 475 core + 54 CLI + 7 macros tests pass (was 469 + 54 + 7 in 0.10.1).
- Performance posture unchanged from the roadmap: ≥50,000 req/s on a
  simple endpoint, 10–30 MB resident memory, <50 ms cold start,
  ~15 MB stripped binary.

## [Unreleased]

> The 0.5.0 → 0.8.0 entries below describe work that accumulated in this section without distinct release cuts at the time. They remain in `[Unreleased]` until each is retroactively tagged or rolled forward into a future release. The 0.9.0 / 0.9.1 / 0.10.0 work that was previously here has been promoted to dated releases below.

### Changed — Admin visual refresh (2026-05-27)

Cherry-picks the design language of a slate-and-blue product-table reference into the admin: Sora display + Source Sans 3 body, soft-shadow rounded cards (16 px radius), uppercase tracked column headers, tabular numeric figures, pill-shaped status badge utility, refined buttons + form focus rings.

- **Typography.** `base.html` adds a Google Fonts link for **Sora** (400/500/600/700) and **Source Sans 3** (400/500/600/700). `admin.css` sets `--admin-font-display: 'Sora'` for headings, table-column labels, button text, and stat-card values; `--admin-font-body: 'Source Sans 3'` for everything else. Falls back to system sans if the fonts fail to load.
- **Brand colour.** Default `Design::primary_color` / `accent_color` shifts from indigo-600 (`#4f46e5`) to **blue-600 (`#2563eb`)** to match the reference. Projects with `rustio.design.json` pinning a colour continue to override. The existing `default_palette_is_indigo_as_of_0_10` test is renamed to `default_palette_is_blue_as_of_0_10_1` and updated.
- **Cards.** New `.admin-card` baseline — 16 px radius, soft `0 4px 14px rgba(15,23,42,.08)` shadow, optional `.admin-card-top` header strip and `.admin-card-foot` band (replaces the old `card-foot` shape). The list page uses the card-top for "All <model> · N <model>" + the +Add button; the card-foot holds pagination.
- **Tables.** `.admin-table` heading row gets `#f8fafc` background, uppercase 12 px Sora with `.04em` letter-spacing. Body rows get hover shading + 1 px soft border between rows. New `.admin-num` / `.num` class applies `font-variant-numeric: tabular-nums` so currency / count columns line up.
- **Status badges.** New utility — `<span class="badge-status active">…</span>` or `<span data-status="todo">…</span>`. Variants: active (green) · pending / in_progress (amber) · inactive / todo (slate) · info (blue). Ready to wire into Rust-side list renderers in a follow-up; meanwhile any template that wants a badge can use them today.
- **Buttons + forms.** All Bootstrap `.btn` variants pick up Sora 14 px 600. Primary buttons gain a focus glow that matches the brand (`box-shadow: 0 0 0 3px rgba(37,99,235,.15)`). `.form-control` / `.form-select` use the same focus ring. Labels switch to Sora 13 px 600.
- **Dashboard.** New `.admin-stat-card` markup — uppercase tracked label + large Sora-bold value (36 px). Replaces the previous `.display-6 text-muted text-uppercase` Bootstrap combo with first-class styles.

What stayed:
- The dark slate sidebar (`#0f172a`) and active-row indicator are unchanged — that's part of RustIO's identity since 0.10.0.
- Bootstrap 5 is still the base; the refresh layers on top via targeted CSS, not a full framework swap.
- Template structure is unchanged for `form.html`, `actions.html`, `profile.html`, etc. Only `base.html` (font link), `admin/list.html` (card-top + card-foot), and `admin/dashboard.html` (stat-card markup) were touched.

Verification: cargo fmt / clippy -D warnings clean; cargo test --workspace --all-targets — **524 passed, 0 failed**.

### Changed — Signed commits required on `main` (2026-05-27)

The `main` branch ruleset now includes a `required_signatures` rule alongside the existing `non_fast_forward` / `deletion` / `required_status_checks` rules. Every commit landing on `main` must carry a signature GitHub can verify — direct pushes by repo admins still go through via the ruleset's admin bypass, but PRs from contributors need a valid signature on every commit.

- **Setup (SSH, recommended on macOS / Linux).** `git config --global gpg.format ssh`, `git config --global user.signingkey ~/.ssh/<key>.pub`, `git config --global commit.gpgsign true`. Register the **same** public key as a *Signing Key* at <https://github.com/settings/ssh/new> — that's a separate slot from the auth key, even though the underlying material can be identical.
- **Local signature verification** needs an `allowed_signers` file: write one line `you@example.com <pubkey contents>` to `~/.config/git/allowed_signers`, then `git config --global gpg.ssh.allowedSignersFile ~/.config/git/allowed_signers`. After that `git log --format='%G?'` resolves to `G` (good) instead of erroring.
- **GPG works too** — the rule only requires "GitHub can verify it", not a specific signing format. Pick whichever you already have working.
- **History is not rewritten.** Pre-existing unsigned commits stay unsigned; the rule only checks pushes going forward.
- **The full `main` ruleset is now**: `non_fast_forward` + `deletion` + `required_status_checks(fmt + clippy + test, strict)` + `required_signatures`, with `RepositoryRole=Admin` bypass on `mode=always`. The `archive/**` ruleset stays at `non_fast_forward` + `deletion` only (frozen-history branches don't get a CI check or a signature requirement).

### Added — 0.8.0 Relations Layer (Foundational)

First pass at first-class relations. Additive only — existing schemas,
plans, and generated projects behave identically. The executor
materialises a `belongs_to` as a single i64 column; it does **not**
emit a SQL `FOREIGN KEY` clause. Referential enforcement is deferred
to 0.9.0, so the review layer flags the gap as a warning.

#### Schema

- `SchemaField.relation: Option<Relation>` — absent by default, serde-
  skipped when `None` so on-disk schemas remain byte-identical for
  projects without relations.
- `Relation { model, field, kind }` and `RelationKind` (`BelongsTo` /
  `HasMany`, `#[non_exhaustive]`). `RelationKind` is the same type
  used by `Primitive::AddRelation` — re-exported from `ai` so old
  `use crate::ai::RelationKind;` imports continue to work.
- `Schema::relation_for(model, field) -> Option<&Relation>` and
  `Schema::incoming_relations(model) -> Vec<IncomingRelation>`.

#### Planner

- New grammar: `add relation from X to Y`, `link X to Y`, and
  `connect X to Y`. All three parse to the same `Primitive::AddRelation
  { from, kind: BelongsTo, to, via }`.
- FK column name is inferred as `<target_singular_lowercased>_id`
  (`applicant_id`, `post_id`). Existing field-already-exists and
  core-model guards apply — the planner refuses rather than
  overwriting.

#### Review

- `AddRelation` is `Low` risk (additive).
- Two schema-aware warnings wired through `review_plan`: an FK-gap
  warning, and a GDPR warning when the target model carries fields
  flagged as PII under the active context.

#### Executor

- `apply_add_relation` materialises a `belongs_to` by delegating to
  `apply_add_field` for an i64 NOT NULL DEFAULT 0 column. The
  migration SQL must not emit `FOREIGN KEY` or `REFERENCES`
  (asserted in tests and via a debug-assert in the executor).
- `remove_relation` and any non-`BelongsTo` kinds refuse with a
  descriptive `UnsupportedPrimitive` reason — ambiguity gets refused,
  not guessed.
- `apply_schema_shadow` projects `AddRelation` into the shadow
  schema as a new field with `relation: Some(...)`, so later steps in
  the same plan see the relation shape.

#### Intelligence

- `FieldUI.relation_label: Option<String>` carries the target
  model's singular name. List views render `Applicant #42` via
  `format_relation_cell`; forms render "Foreign key to Applicant"
  via the new `field_ui_metadata_with_relation`.
- `FilterKind::RelationSelect { target_model }` — wired by
  `infer_filters_with_relations(fields, ctx, relation_of)`. The old
  `infer_filters` stays untouched for callers that don't need
  relation awareness.
- `SearchIntent::RelationId { model, id }` — produced only by
  `classify_search_for_field(query, relation_target)`. Plain
  `classify_search` never emits it.

#### Suggestions

- `derive_relation_suggestions(&Schema)` — detects an `<thing>_id`
  field with no `relation` and proposes linking it to a matching
  model. Refuses on ambiguous or missing targets. `Medium`
  confidence because the target is inferred from naming.
- `find_relation_suggestion` — the route-guard sibling.

#### Safety posture

- Zero breakage to existing schemas, plans, generated projects.
- No SQL `FOREIGN KEY` emitted — the gap is visible in review
  warnings, not hidden behind silent success.
- Every ambiguous case refuses rather than guessing.

#### Tests (32 new)

- Planner (6), Review (4), Executor (4), Intelligence (10),
  Suggestions (8). Total core test count: 403.

### Added — 0.7.3 Runtime Truth Layer

Closes the 0.7.2 gap where the schema cache was populated but the
dashboard still derived suggestions from compile-time
`AdminEntry[]`. After this pass, the dashboard's **alerts** +
**suggestion engine** + **suggestion routes** read from the schema
on disk; clicking `[Reload schema]` actually updates the UI without
a restart.

#### New module: `rustio-core::admin::entry_builder`

- `DynamicAdminEntry` / `DynamicAdminField` — owned-string mirrors
  of the compile-time `AdminEntry` / `AdminField`. Built either
  from a schema model or from a compile-time entry.
- `build_admin_entries(&Schema) -> Vec<DynamicAdminEntry>` — the
  canonical schema → admin-entry projection.
- `field_type_from_str(&str) -> FieldType` — total mapping; an
  unknown type string falls through to `FieldType::String`
  (PlainText). The admin never panics on an unfamiliar type.
- `entries_effective(&[AdminEntry]) -> Vec<DynamicAdminEntry>` —
  the single call every renderer uses. Returns schema-derived
  entries when the cache is warm; falls back to the compile-time
  slice otherwise. Preserves `core: true` from the compile-time
  side so core-model protection stays intact.

#### Suggestion engine upgrade

- `derive_suggestions_from_entries(&[DynamicAdminEntry], context)`
  — the schema-driven sibling of `derive_suggestions`.
- `find_suggestion_from_entries` — the route-guard sibling of
  `find_suggestion`.
- Old functions kept for backward compatibility; no breaking
  changes.

#### Admin handler wiring

- `render_dashboard_alerts` now calls `entries_effective(...)` and
  feeds the result into `derive_suggestions_from_entries`. After a
  `[Reload schema]` click, missing-field suggestions correctly
  disappear on the next dashboard render.
- Suggestion review + apply handlers use
  `find_suggestion_from_entries` so a URL that was valid yesterday
  (but is covered in today's schema) correctly 404s.
- The GDPR-inventory loop still iterates compile-time entries —
  it lists sensitive fields the live admin can actually display,
  which is bound to what's compiled in. Documented.
- Route registration stays compile-time: the admin needs real
  `AdminModel` impls to read row values, so registering a route
  for a schema-only model isn't possible today. No change there.

#### Safety posture

- Unknown type strings render as `PlainText`, never panic.
- Deterministic ordering: schema path follows `schema.models`
  order; fallback path follows the compile-time slice order.
- No breaking API changes — only additions.

#### Tests

5 new tests in `suggestions_tests.rs` alongside the existing
10-test suite, plus 3 new tests inside `entry_builder`:
- `field_type_fallback_is_string_for_unknown` — the defence-in-
  depth canary.
- `build_admin_entries_mirrors_the_schema` — round-trip invariant.
- `entry_from_admin_round_trips_compile_time_shape` — the
  "existing behaviour unchanged when schema matches compiled
  structs" invariant.
- `schema_driven_suggestion_fires_for_missing_field` — happy path
  through the new function.
- `schema_driven_suggestion_disappears_when_field_present` — the
  **self-heal** property the whole pass exists to prove.
- `schema_driven_and_compile_time_derivations_agree_when_shapes_match`
  — dual-path consistency canary.
- `schema_driven_find_rejects_crafted_urls` — URL-safety on the
  new path.
- `schema_driven_skips_core_models` — core-protection parity.

#### Smoke-tested end-to-end

Against `~/Desktop/sveahousing` with housing context and
`annual_income` removed from both struct and schema:
1. Dashboard before reload: `missing 1 housing` alert + `[Add
   annual_income]` button.
2. External edit to `rustio.schema.json` adds `annual_income`.
3. Dashboard still stale (cache hasn't refreshed).
4. `POST /admin/schema/reload` → 303.
5. Dashboard **after reload**: alert gone, button gone.
6. Crafted URL `/admin/suggestions/applicants/annual_income` now
   returns **404** (the `(admin_name, field)` pair isn't in the
   fresh suggestion list).

### Added — 0.7.2 Trust & Feedback Layer

Makes every action understandable and predictable. The operator
sees what will change, how confident the system is, and what
happened — and can refresh the schema without restarting the
server.

- **Runtime schema reload.** New `rustio-core::admin::schema_cache`
  module: `snapshot()` / `refresh()` / `refresh_best_effort()`
  sit behind a `OnceLock<RwLock<Option<CachedSchema>>>`. A
  poisoned lock degrades to "cache empty" rather than panic; a
  failed reload preserves the previous cached value. New route
  `POST /admin/schema/reload` (CSRF-protected) refreshes the cache
  and redirects back to `/admin?schema_reload=ok|err`. The
  dashboard renders a header row reading *"Schema loaded at
  YYYY-MM-DD HH:MM:SS UTC"* with a `[Reload schema]` button
  alongside. Every successful apply also calls
  `refresh_best_effort()` so the dashboard self-heals when
  `rustio.schema.json` changes in the background.
- **Type inference upgrade.** The planner now resolves monetary
  field names to `i64` (we store amounts in minor units where
  `i32` overflows around 21 million):
  - `annual_income`, `total_income`, etc. (any `*_income`)
  - `balance`, `amount`, `total_amount`, `order_total` (any
    `*_amount` / `*_total`)
  - `price`, `total_price` (any `*_price`)
  Existing `priority` / `score` / `*_count` → `i32` rules are
  untouched and covered by a regression test.
- **Suggestion confidence.** `Suggestion` gained a
  `confidence: Confidence` field (`High` | `Medium`). Industry-
  required fields are `High` — the engine isn't guessing when the
  convention list names the field explicitly. Rendered as a pill
  next to both the dashboard action button and the review-page
  title.
- **Visual schema diff on the review page.** A new
  `.rio-schema-diff` block shows the target model's current field
  list in monospace, with added fields highlighted (`+ field: T`
  in green). The operator sees the shape change before the apply,
  not just "Add field X to model Y".
- **Improved success message.** The apply result page now lists
  specific per-step bullets ("Added field `annual_income` (i64)
  to Applicant") and per-file bullets labelled by kind
  ("Updated apps/applicants/models.rs", "Created migration
  0005_…") instead of a generic "Applied 1 step".
- **Dashboard header row.** The schema-reload block renders on
  every dashboard response — operators always see the loaded
  timestamp and have the refresh button one click away, even when
  no suggestions are pending.
- **Tests.**
  - `monetary_names_infer_i64` — 5 name shapes produce `i64`.
  - `count_suffix_still_infers_i32` — regression canary for the
    pre-existing numeric rules.
  - `schema_cache::tests::format_loaded_at_produces_stable_shape`
    — pins the `YYYY-MM-DD HH:MM:SS UTC` format so a chrono
    bump can't silently drift it.
  - `schema_cache::tests::snapshot_returns_same_value_when_not_refreshed`
    — invariant check on the cache.
  - Existing suggestion flow tests verify the `confidence` field
    surfaces correctly; no regressions across the 418-test
    workspace suite.

Smoke-tested end-to-end against `~/Desktop/sveahousing` with
housing context + `annual_income` temporarily removed from the
Applicant model:
- Dashboard shows `[Reload schema]` and `Schema loaded at 2026-04-19 …`
  plus the suggestion button with a green `High confidence` pill.
- `POST /admin/schema/reload` (CSRF-protected) redirects to
  `/admin?schema_reload=ok`; the dashboard renders a green flash
  banner.
- Review page carries the confidence pill, a visible diff block
  highlighting `+ annual_income: i64`, and "Approve and apply".
- Apply result page says "Added field `annual_income` (i64) to
  Applicant", "Updated apps/applicants/models.rs", "Created
  migration migrations/0005_…" — matching the files actually
  written.
- The generated migration is
  `ALTER TABLE applicants ADD COLUMN annual_income INTEGER NOT NULL DEFAULT 0`
  and the Rust struct field is `pub annual_income: i64`,
  confirming the planner's upgraded inference lands correctly
  through the whole chain.

### Added — 0.7.1 Actionable Intelligence Layer

Turns the 0.7.0 dashboard alerts into actions. Each "missing
industry convention field" alert now renders an `[ Add <field> ]`
button; clicking it opens a review page that runs the existing
planner + review chain, and the Approve button fires the existing
executor. **No safety gate is bypassed** — suggestion is just a
convenient way to phrase a planner prompt.

- **`rustio-core::admin::suggestions`** — new module with two pure
  functions:
  - `derive_suggestions(entries, context)` — enumerates
    `AddField` suggestions for industry-required fields a model is
    missing. Returns empty when no context, no industry schema,
    or no model overlaps the convention list. Skips core models
    entirely.
  - `find_suggestion(entries, context, admin_name, field)` — URL
    router guard. A suggestion URL is only honoured when the
    `(admin_name, field)` pair is in the currently-derived list,
    so a crafted URL like `/admin/suggestions/users/nickname`
    404s in-shell instead of running the planner on something
    the engine never proposed.
- **`Suggestion`** carries the minimum the review page needs:
  model display / singular / admin name, the field, the natural-
  language `prompt`, a human reason (`"housing industry
  convention"`), and a `url_path()` helper so the dashboard and
  the router agree on the route shape.
- **Routes (auth-gated, CSRF-protected):**
  - `GET  /admin/suggestions/:admin/:field` — renders the review
    page: planned changes, explanation, risk badge, impact,
    validation status, warnings. The **Approve** button is
    disabled when risk is `Critical` or validation fails — the
    spec's explicit safety requirement.
  - `POST /admin/suggestions/:admin/:field` — runs the full
    planner → `build_plan_document` → `execute_plan_document`
    chain. Any refusal at any step (planner error, critical risk,
    policy violation, file conflict) re-renders the review page
    with an inline error banner. The executor only writes when
    every gate returns `Ok`.
- **Dashboard alerts** now carry an inline action button per
  missing field, rendered inside a `rio-suggestion-card` wrapper.
  GDPR inventory alerts stay informational — they don't imply a
  single specific action.
- **CSS**: `rio-suggestion-card`, `rio-suggestion-actions`,
  `rio-suggestion-action`, `rio-plan-preview`, `rio-risk-badge`.
  No JS added — the review page is plain HTML forms. The risk
  badge reuses the existing `.rio-pill-*` color classes so the
  palette stays consistent with status pills.
- **Post-apply page** carries the explicit next-steps list: stop
  server → `rustio migrate apply` → `cargo build` → `rustio
  schema` → restart. The live admin doesn't recompile itself;
  operators need to know that.
- **Tests** — 10 in `rustio-core/src/admin/suggestions_tests.rs`:
  no-context → no suggestions, no industry schema → no
  suggestions, unrelated model → no suggestions, fully-covered
  model → no suggestions, missing field fires exactly one
  suggestion, core models skipped, deterministic ordering,
  `find_suggestion` honours the pair, rejects crafted URLs,
  `url_path()` format stability.

Smoke-tested end-to-end against `~/Desktop/sveahousing` with
`{"country":"SE","industry":"housing"}` and `annual_income`
temporarily removed from the Applicant model:
- Dashboard renders `[ Add annual_income ]` button under the
  alert.
- `GET /admin/suggestions/applicants/annual_income` returns 200
  with the full review (planned changes, Low risk, Validation
  passes, Approve button enabled).
- `GET /admin/suggestions/applicants/email` returns **404** — the
  URL isn't in the derived list (crafted-URL refusal).
- `POST` without CSRF returns **403**.
- `POST` with CSRF: executor writes `apps/applicants/models.rs`
  and `migrations/0005_add_annual_income_to_applicants.sql`
  atomically, renders the "Applied 1 step" page with the
  next-steps list.
- Running the apply twice without restarting surfaces a clean
  `FileConflict` (COLUMNS already contains the new field), not a
  silent duplicate — the executor's existing idempotency gate
  holds inside this flow too.

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

## [0.10.1] - 2026-05-27

### Fixed — Admin dashboard cards include legacy `AdminEntry` models ([#2](https://github.com/abdulwahed-sweden/rustio/issues/2))

Before this patch, `/admin` only showed cards for models registered through the new `AdminUiModel` registry — every `rustio new app`-scaffolded model was invisible on the dashboard, even though it appeared correctly in the sidebar.

- The seam was in `admin/layout.rs::dashboard_render`. It received `legacy_entries: &[AdminEntry]` (it'd been passing them to `sidebar_merged`), but the `cards` list was built only from the new registry. New helper `collect_legacy_dashboard_entries` walks the legacy entries (skipping `core: true` framework-internal ones), dedupes against the new-engine registry by slug, and emits one `DashboardEntry` per remaining model — same shape as the new-engine half so both lists are interchangeable downstream.
- Sidebar continues to be built from `sidebar_merged`'s own legacy walk — unchanged. Dedup rules match between sidebar and cards, so a model registered through both paths never double-counts.
- Both the minijinja path and the no-template fallback path get the combined list, so a template failure can't silently regress the fix.
- **Tests:** +4 in `admin::layout::tests` (happy path, `core`-filter, slug-dedup, missing-table-fallback). Workspace test count: **524 passed** (was 520).
- Commit: [`a294287 fix(admin): include legacy AdminEntry models on the dashboard cards`](https://github.com/abdulwahed-sweden/rustio/commit/a294287).

## [0.10.0] - 2026-05-27

### Changed (breaking) — Admin rebuild

The admin UI is rebuilt from the ground up on `minijinja` templates + Bootstrap 5 + a first-class RBAC layer. Landed in five stages on `feat/admin-templates-v2`; this is the first cut after merge to `main`.

- **Rendering.** Rust code in `rustio-core::admin` now passes typed context dicts to `minijinja`; it no longer concatenates HTML. Default templates ship bundled in `rustio-core/assets/templates/` via `include_str!`.
- **Per-project override.** User projects can override any admin template by placing a file of the same relative path under their project's `templates/` directory. Override is additive by filename — no patch format. This reverses the 0.8.x rule that admin templates had no override hook.
- **Bootstrap 5** bundled via `include_bytes!`, served under `/admin/static/…`. Accent colour still driven by `rustio.design.json`, now passed as template context.
- **RBAC.** New `admin::rbac` module; `Role` enum (`SuperAdmin` / `Admin` / `Editor` / `Viewer`); per-model `view` / `create` / `edit` / `delete`. Migration for `roles` + `user_roles` tables. Lacking `view` hides a model from the sidebar entirely; lacking `create` / `edit` / `delete` disables the corresponding UI paths, and a direct URL returns 403.
- **Removed.** Every admin page request now flows through `minijinja` — there is no string-concat HTML left in the live request path. The legacy `admin.rs` dashboard helpers (`dashboard_response`, `render_dashboard_alerts`, `fetch_model_row_counts`), the `admin/layout.rs` drawer/bulk-action chain (`admin_index_post`, `admin_index_bulk`, `admin_index_with_drawer`, the `render_users_*` / `render_bulk_*` / `build_filter_*` helpers, `render_layout`, `render_admin_sidebar_for`, `build_admin_sidebar`, `build_admin_form`), the bundled `THEME_CSS` / `COMPONENTS_CSS` / `ADMIN_JS` consts, and the hand-rolled CSS at `assets/admin-new/` are gone. Two commits land this on `feat/admin-templates-v2`:
  - `3bd405d refactor(admin): stage 5 — delete unreferenced legacy renderers` — drops the `#[allow(dead_code)]` block (`dashboard_response`/`fetch_model_row_counts`/`render_dashboard_alerts`), `admin_dashboard_get`, `admin_index_get`, `build_drawer_for_get`, the three `demo_*` helpers, and the `User`/`impl FormModel` pair that backed `demo_auto_form`. **2 files, +17 / –694.**
  - `2f01283 refactor(admin): remove orphaned POST /admin/:model + drawer/bulk chain` — drops the `POST /admin/:model` mount + handler, both `/admin-new/:model` aliases, the entire string-concat chain in `admin/layout.rs` (16 functions), and deletes `assets/admin-new/{theme.css,components.css,admin.js}`. **7 files, +19 / –2645.**
  - `b198079 refactor(admin): trim admin/ui.rs orphans` — drops `TopbarConfig`/`render_topbar`, `SidebarGroup`/`SidebarItem`/`render_sidebar`, `Breadcrumb`/`PageAction`/`PageHeaderConfig`/`render_page_header` (orphaned once the chain above was removed). `admin/ui.rs` shrinks from ~255 lines to ~38 — only `html_escape` (still consumed by FK cell links in `list_render` + field controls in `form_render`) and its test remain. **1 file, +7 / –224.**
  - The legacy `AdminEntry` registration path (`mount_model<T>`) is untouched — those models keep their literal `/admin/<NAME>/{create,bulk_action,:id/edit,:id/delete}` routes; the catch-all that's gone was the *new* admin engine's drawer-based POST, which no template ever submitted to.
  - Bulk delete / bulk edit in the rebuilt admin is no longer wired and is *not* in 0.10.0 scope. If re-introduced it will be a fresh template feature: a checkbox column in `list.html`, a new `POST /admin/:model/bulk` route, and a handler that mutates and re-renders through `list_render`. Projects that relied on scraping admin HTML will break.
- **Palette.** Default `Design::primary_color` / `accent_color` shifts from rust-orange `#B84318` to indigo. Projects with `rustio.design.json` pinning a colour are unaffected.

This is a pre-1.0 breaking change.

**Verification at release** (commit `b198079`): `cargo fmt --all --check` clean; `cargo clippy --workspace --all-targets -- -D warnings` clean; `cargo test --workspace --all-targets` — **517 passed, 0 failed**. The 519 → 517 delta is exactly the two `render_topbar` / `render_sidebar` tests removed alongside the helpers they covered; no production test churn across the three cleanup commits.

## [0.9.1] - 2026-05-27

### Added — Destructive-op gate

Turns the `ExecuteOptions.allow_destructive` placeholder into real behaviour. `rustio ai apply <plan> --force` now lets reviewers opt into `remove_field` / `remove_relation` primitives. Critical-risk plans, developer-only primitives, and PII policy refusals stay bypass-proof — `--force` is scoped narrowly on purpose.

- **`apply_remove_field`.** New executor path. Uses the existing recreate-table pattern (`CREATE TABLE t__new` → `INSERT … SELECT` → `DROP` → `RENAME`) so SQLite's "ALTER TABLE DROP COLUMN won't work on FK-bearing tables" limitation doesn't bite. Patches `models.rs` to remove the field from the struct, `COLUMNS`, `INSERT_COLUMNS`, `from_row`, and `insert_values`. Fails cleanly with `ExecutionError::UnsupportedPrimitive { op: "remove_field", reason: "cannot drop the id primary key …" }` when asked to drop `id`.
- **`apply_remove_relation`.** Delegates to `apply_remove_field` on the `<via>` column; keeps the summary line honest as "Remove relation" rather than "Remove field".
- **FK-aware recreate.** The migration preserves every *other* surviving FK on the table by re-emitting its `REFERENCES <parent>(id) ON DELETE <policy>` clause from the schema (extends the 0.9.0 `column_def_with_relation_context`). No collateral constraint loss.
- **Multi-model file support.** `find_table_for_struct`, COLUMNS/INSERT_COLUMNS array removal, and `from_row` / `insert_values` patching are all now scoped to the matching `impl Model for <struct>` block. Medflow-style apps (multiple models per file) are first-class.
- **`remove_model` still refused.** Dropping a struct + its admin registration + downstream FKs is 0.9.2 scope — `ExecutionError::UnsupportedPrimitive { op: "remove_model" }` even with `allow_destructive: true`.
- **CLI.** `rustio ai apply <plan> [--yes] [--dry-run] [--force]`. `--force` sets `allow_destructive` on the executor. Preview prints "destructive gate open: --force" so operators see what changed about the pass.
- **Tests.** +5 core (remove_field with `--force` drops column + patches models.rs; `remove_primary_key_id` refused; `remove_relation` refused without `--force`, works with it; `remove_model` refused even with `--force`); +3 CLI (arg parser accepts `--force`, defaults to `false`, composes with `--yes --dry-run`). Total 519 passing (499 baseline → 514 after 0.9.0 → 519 now).
- **Verified against medflow.** `ai plan "remove allergies from patients"` + `ai apply --yes --force` drops `Patient.allergies` cleanly: struct field + COLUMNS + INSERT_COLUMNS + from_row + insert_values all stripped; migration `0020_drop_allergies_from_patients.sql` emits a recreate-table with every other Patient column preserved, wrapped in `PRAGMA foreign_keys OFF/BEGIN/COMMIT/ON`. Without `--force` the same apply refuses with `primitive `remove_field` is destructive — re-run … with `--force``.

## [0.9.0] - 2026-05-27

### Added — FK enforcement (Phase 2 exit criterion)

Phase 2's final exit criterion: `AddRelation { kind: BelongsTo }` now emits a real SQL `FOREIGN KEY` constraint, and existing 0.8.x projects have a one-shot retrofit path. Replaces the 0.8.0 "soft linkage via `rustio.schema.json` only" stopgap.

- **Primitive surface.** `ai::AddRelation` gains two serde-default fields — `required: bool` (default `false`) and `on_delete: OnDelete` (default `Restrict`). Saved 0.8.x plan documents still parse byte-identically because both fields are `#[serde(default)]`. New `ai::OnDelete` enum with `Restrict` / `Cascade` / `SetNull`.
- **Schema surface.** `schema::Relation` gains `required: Option<bool>` and `on_delete: Option<String>`, both `skip_serializing_if = "Option::is_none"`. Schema JSON written by 0.8.x projects round-trips through 0.9.0 unchanged; only projects that adopt the new metadata start writing the new keys.
- **Executor.** `apply_add_relation` now emits `ALTER TABLE <child> ADD COLUMN <via> INTEGER REFERENCES <parent>(id) ON DELETE <policy>;` plus `PRAGMA foreign_keys = ON;`. The generated column is nullable — SQLite cannot add a `NOT NULL + REFERENCES` column via `ALTER TABLE`. A `required: true` primitive refuses with a clear pointer at the retrofit CLI.
- **Planner grammar.** Relation phrases accept trailing options: `link A to B required`, `link A to B on_delete:cascade`, `link A to B required on_delete:set_null`. Unknown options / policies refuse at plan time — no silent default.
- **Review layer.** `AddRelation` risk is Low by default, Medium for either `required` or `on_delete: cascade`, High for both combined. New warnings: a required-FK hint to use the retrofit path, a cascade blast-radius note.
- **CLI.** New `rustio migrate add-fks` subcommand. Default dry-run; `--write` commits one `NNNN_retrofit_fks_<table>.sql` per affected table, using the SQLite recreate-table pattern (`CREATE TABLE …__new` + `INSERT … SELECT` + `DROP` + `RENAME`). Reviewed against medflow: 13 migrations, 27 relations upgraded, every FK column keeps its existing nullability.
- **Tests.** +15 on top of the 499 baseline, taking `cargo test --workspace --all-targets` to 514 passing. Covers every `OnDelete` variant, nullable/required combos, the `required` refusal + retrofit hint, grammar extension, risk reclassification, and the retrofit no-op path for already-annotated schemas.
- **Upgrade path.** See `UPGRADING.md` § "0.8.x → 0.9.0".

### Added — Relation Intelligence Layer (admin)

The admin stops treating foreign keys as anonymous integers. This is a
framework-level feature — the runtime registry, compile-time macro
validation, and every admin rendering / filter / delete-guard hook all
read from the same declarative source (`#[rustio(belongs_to = "...")]`
on a struct field). Additive: projects without annotations continue to
render and behave exactly like 0.8.x.

#### Schema

- `Relation.display_field: Option<String>` — optional column on the
  target whose value the admin renders as the human label for a FK.
  `None` → admin renders `#<id>` and **never guesses** a column. No
  inference, no fallback chain.
- `SCHEMA_VERSION` bumped `1` → `2`. Additive (new optional field,
  `#[serde(default)]` + `skip_serializing_if = "Option::is_none"`), so
  0.8.x tools still parse the new schema — they just ignore the
  `display_field` key.

#### Macro (`#[derive(RustioAdmin)]`)

- New field attribute: `#[rustio(belongs_to = "Model")]` and
  `#[rustio(belongs_to = "Model", display = "column")]`.
- **Two compile-time checks** per declaration:
  1. The target type must exist and implement `Model` (forces
     `<Target as Model>::TABLE` resolution — unknown targets fail the
     build with Rust's normal "cannot find type" error).
  2. When `display = "col"` is set, `col` must appear in the target's
     `Model::COLUMNS`. Verified via a `const _: ()` assertion using
     a byte-wise `const fn str_eq` — missing column fails at
     const-eval with a readable message including the offending field
     name.
- The attribute is only legal on `i32` / `i64` fields; any other type
  produces a compile error at the derive site.
- `#[rustio(display = "…")]` without `belongs_to = "…"` is rejected.
- Unknown keys inside `#[rustio(...)]` are rejected.

#### Runtime — `admin/relations.rs`

- New module `rustio_core::admin::relations`:
  - `RelationRegistry` built pure-functionally from `&Schema`. No I/O,
    no interior mutability, no background refresh.
  - `ResolvedRelation { source_model, source_field, target_model,
    target_table, target_admin_name, target_display_field, kind }`.
  - `InverseRelation { source_model, source_table, source_admin_name,
    source_display_name, source_field, target_model }`.
  - `RegistryError::UnknownTarget` / `UnknownDisplayField` — surfaced
    by `validate(&schema)` for hand-edited `rustio.schema.json` files
    that reach past the macro's compile-time checks.
  - `RELATION_FILTER_DROPDOWN_CAP = 500` — soft cap on filter
    dropdown size.
- `AdminRelation` (runtime mirror of `schema::Relation`) on
  `AdminField`. Populated by the macro; consumed by
  `SchemaField::from_admin_field` so `rustio schema` output always
  matches the compiled types.

#### Admin rendering (list + detail)

- FK columns on list pages render as
  `<a href="/admin/<target>/<id>">Display</a> <span>#id</span>` when
  a `display_field` resolves.
- Missing target row (stale schema, deleted row) or no `display_field`
  declared: renders `<a href="...">#<id></a>` — link without name,
  **never the raw integer**.
- Label prefetch: one `SELECT id, display FROM target WHERE id IN (…)`
  per FK column per list render — 1+K queries total, not N+1.
  **v1 strategy; marked in code as a future JOIN optimisation point.**
- Edit page shows a `Linked: <Name> (#id)` hint below every FK input.
  Input itself stays numeric in this pass.

#### Admin — inverse relation panels (Phase 4)

- Edit page renders a "Related" card listing every `has_many` inverse
  of the current model: `Appointments (12) · Invoices (3)`. Each card
  links to the filtered list page. Counts only; future evolution
  (preview rows, in-page drill-in) documented as extension points.

#### Admin — relation-aware filters (Phase 5)

- List toolbar carries one `<select>` per `belongs_to` relation on
  the model, populated via `SELECT id, display FROM target ORDER BY
  display LIMIT 501`.
- When the target row count ≥ 500 or no `display_field` is declared,
  the filter falls back to a numeric input with a visible muted-text
  hint explaining which case fired. Cap value is
  `relations::RELATION_FILTER_DROPDOWN_CAP`.
- Query string support: `/admin/appointments?patient_id=7` filters
  the in-memory list. Reset button clears all relation filters.

#### Admin — FK-aware delete guard (Phase 6)

- `POST /admin/<slug>/<id>/delete` pre-checks every `has_many` inverse
  via `SELECT COUNT(*)`. If any count > 0, the admin returns
  **HTTP 409 Conflict** with a page listing every blocker (model
  name + count + link to the filtered list) instead of the previous
  opaque 500.
- Defence in depth: a SQLite FK constraint violation surfacing through
  the driver (`"FOREIGN KEY constraint failed"`) is caught at the
  DELETE site and rendered through the same 409 page, covering the
  pre-check→delete race window.

#### Example — `examples/medflow/`

- Every FK column across `apps/{people,care,billing}/models.rs`
  carries a `#[rustio(belongs_to, display)]` annotation.
- `rustio.schema.json` regenerates with relation metadata on all 8
  FKs.
- Live admin:
  - `/admin/appointments` renders `Ahmed Hassan (#23)` instead of `23`.
  - `/admin/patients/1/edit` shows `Appointments (3) · Invoices (2) ·
    Prescriptions (2)`.
  - `/admin/appointments?patient_id=1` filters the list to 3 of 120.
  - Deleting Cardiology (has 2 doctors) returns 409 with blockers
    listed.

#### Tests

- `rustio-core`: 12 new tests in `admin/relations_tests.rs` covering
  serde round-trip, registry indexing, inverse computation, dangling-
  target and unknown-display-field handling, empty-schema safety, and
  iteration determinism. Total 415 (up from 403).

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

[Unreleased]: https://github.com/abdulwahed-sweden/rustio/compare/v0.10.1...HEAD
[0.10.1]: https://github.com/abdulwahed-sweden/rustio/compare/v0.10.0...v0.10.1
[0.10.0]: https://github.com/abdulwahed-sweden/rustio/compare/v0.9.1...v0.10.0
[0.9.1]: https://github.com/abdulwahed-sweden/rustio/compare/v0.9.0...v0.9.1
[0.9.0]: https://github.com/abdulwahed-sweden/rustio/compare/v0.3.1...v0.9.0
[0.3.1]: https://github.com/abdulwahed-sweden/rustio/compare/v0.3.0...v0.3.1
[0.3.0]: https://github.com/abdulwahed-sweden/rustio/compare/v0.2.2...v0.3.0
[0.2.2]: https://github.com/abdulwahed-sweden/rustio/compare/v0.2.1...v0.2.2
[0.2.1]: https://github.com/abdulwahed-sweden/rustio/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/abdulwahed-sweden/rustio/compare/v0.1.2...v0.2.0
[0.1.2]: https://github.com/abdulwahed-sweden/rustio/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/abdulwahed-sweden/rustio/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/abdulwahed-sweden/rustio/releases/tag/v0.1.0
