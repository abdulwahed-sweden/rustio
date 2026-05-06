# Architecture notes

This doc describes how the pieces in `rustio-core` fit together. It's
aimed at someone who wants to change framework behaviour, not someone
using the framework.

For the per-phase chronology, see `docs/phases/`. For the changelog,
see `CHANGELOG.md`. For project-specific rules a Claude session needs
to be productive in this repo, see `CLAUDE.md`.

## Crate layout

```
rustio-macros   ──►  rustio-core   ──►  rustio-cli
                                  ──►  user crate (via [dependencies])
```

- `rustio-macros` is a proc-macro crate. It has one job: emit an
  `impl AdminModel for UserStruct { ... }` when it sees
  `#[derive(RustioAdmin)]`. All runtime behaviour lives in
  `rustio-core`; the macro just generates table names, per-field
  display logic, and form-parsing code.
- `rustio-core` is the library everything else depends on. It has zero
  circular dependencies — every module imports `error`, `http`, or
  `orm`, never the other way around.
- `rustio-cli` and the user crate both sit on top of `rustio-core`.
  They never talk to each other.

## Module dependency order

Within `rustio-core`:

```
error  ──►  http  ──►  router  ──►  server
                  ──►  orm    ──►  auth
                                ──►  migrations
                                ──►  admin  ──►  (uses: templates, auth)
                  ──►  schema  ──►  ai
                  ──►  templates
                  ──►  search
                  ──►  cache
                  ──►  middleware
                  ──►  background
```

- `error` knows about nothing.
- `http` depends on `error`.
- `router` depends on `http`.
- `server` depends on `router` + `http` (hyper glue).
- `orm` depends on `error` (for the error conversions).
- `auth` depends on `orm` (it stores users + sessions as rows).
- `migrations` depends on `orm`.
- `templates` depends on `error` only.
- `admin` is the thickest module; it glues router + orm + auth +
  templates together.
- `schema` is pure data and depends on nothing but serde.
- `ai` depends on `schema` (for the review stage).
- `search` depends on `error` + `http` (Meilisearch is a REST client).
- `cache` is in-process LRU; depends on `error`.
- `middleware` depends on `http` + `router`.
- `background` depends on `orm` (sweeps `rustio_sessions`).

## The admin module, in eleven files

`admin/` was once a single 7000-line file. It's now eleven source
files (plus four test siblings), each with one responsibility:

- `mod.rs` — re-exports + the `register_admin_routes` entry-point.
- `types.rs` — the data vocabulary: `AdminField` (with `choices`
  for closed enum lists), `AdminRelation` (with `multi` for M2M),
  `AdminModel`, `AdminEntry`, `Admin`. No HTTP, no HTML.
- `render.rs` — builds `serde::Serialize` context structs for the
  templates (`BaseContext`, `dashboard_ctx`, `list_ctx`, `form_ctx`,
  `confirm_delete_ctx`). Also holds the dynamic-form layer:
  `FormField` / `FormSection` (the field grouping that the generic
  form template iterates), `map_field_to_ui` (cascade: choices →
  relation+multi → relation → `FieldType` match → widget +
  input_type), `resolve_relation_options` (async, FK/M2M rows from
  `AdminOps::list`, capped at `FK_OPTIONS_LIMIT = 50`), and the
  search helpers `search_options` / `filter_options` powering the
  `/admin/search/:model` endpoint. **Phase 7.5** added
  `FormField.errors: Vec<String>`, the `field_errors` parameter on
  `form_ctx`, and the `apply_field_errors` walker that lets bespoke
  validators populate per-field error messages without changing
  AdminOps. **Phase 7.6** added `truncate_query` (200-char
  char-boundary-safe cap) and lifted the search resilience path
  (transient `list()` errors swallowed, return empty Vec). No HTTP,
  no HTML strings in Rust.
- `handlers.rs` — one `async fn` per generic admin action
  (list/new/create/edit/update/delete + login/logout/password-change
  + `show_search` for the FK lookup endpoint). No URL knowledge.
- `routes.rs` — the only file that knows about URL shapes. Wires
  handlers into the router, holds `role_guard` / `perm_guard` /
  `login_guard`. Registers `/admin/search/:model` (Staff-guarded,
  JSON, `?q=` search) before the project-level wildcards.
- `builtin.rs` — bespoke handlers for the built-in user/group pages
  (`/admin/users`, `/admin/groups`, plus the view + delete surfaces
  added in Phase 7a/0.5/f and /h). Builds `FormSection` lists via
  `render::user_new_form_sections` etc. so bespoke and generic forms
  share one rendering path.
- `entry_builder.rs` — derives `AdminEntry` lists from a `Schema`
  (the dynamic counterpart to `#[derive(RustioAdmin)]`).
- `audit.rs` — schema-vs-admin parity audit (catches missing fields).
- `relations.rs` — relation derivation for foreign-key navigation.
- `intelligence.rs` — schema-driven layout suggestions.
- `suggestions.rs` — surfaces the suggestions on the admin index.
- `icons.rs` — 16 lucide stroke icons, baked at compile time, served
  by the `icon(name, class="...")` minijinja function.

A new generic admin action (one that applies to every model) touches:
1. `handlers.rs` for the logic.
2. `render.rs` if it needs a new context struct.
3. `routes.rs` for the URL.

A new built-in page (one that's not derived from a model — like the
user-profile view in Phase 7a/0.5/h) touches:
1. `builtin.rs` for the handler + context struct.
2. `routes.rs` for the URL.
3. `assets/templates/admin/<name>.html` for the markup.
4. `templates.rs` (`EMBEDDED_TEMPLATES`) for the registry line.
5. `templates::tests` for the render test.
6. `make css` if the template uses any new Tailwind utility classes
   (Phase 7a/2 contract). The compiled `admin.css` is committed
   alongside the template; `make css-check` enforces parity.

The four template touch-points are the **(file, registry,
render-test) triple** — see `CLAUDE.md` for why all three are
load-bearing.

## Forms, sections, and the FK select pipeline

Every admin form — generic-derived OR bespoke (login, password
change, user new/edit, group new/edit) — flows through the same
two-step pipeline introduced in Phases 6 / 6.2:

1. A handler builds a `Vec<FormSection>`. `FormSection { title,
   fields }` partitions the model's fields into Default / Metadata /
   Advanced groupings. The generic path uses
   `group_fields_into_sections` (a name-heuristic partition); the
   bespoke handlers hand-build their sections so they can keep
   custom blocks (banners, danger zones, group-membership
   checkboxes, permission grids).
2. The template `admin/form.html` iterates the sections, and for
   every field includes `admin/includes/_form_field.html` — one
   shared renderer that handles all four widgets (`input` /
   `checkbox` / `textarea` / `select`) and all the optional
   attributes (`required`, `autofocus`, `disabled`, `maxlength`,
   `autocomplete`, `placeholder`, plus the search-input wrapper
   for FK / M2M selects).

Widget choice for a field comes from `map_field_to_ui(&AdminField)`
— a four-arm cascade: **choices** (closed enum list) → select;
**relation + multi** → multi-select; **relation** → single select;
otherwise → `FieldType` match (textarea heuristic for
body/description; checkbox for bool; text/number/datetime input
otherwise).

For relation-backed selects, `resolve_relation_options` (called by
the show handlers, **before** the sync `form_ctx`) fetches the
target rows via `AdminOps::list`, builds labels via the
display-field ladder (`display_field` → `name` → `title` → id),
and truncates at `FK_OPTIONS_LIMIT = 50`. The (options, has_more)
tuple is threaded into `form_ctx` and onto each FK `FormField`,
which carries `searchable: true`, `has_more`, and a
`search_url: Some("/admin/search/<Model>")` so the template can
upgrade the input client-side.

The search input has two progressively-enhanced modes, both
fall-back-safe:

- **Client-side filter** (default — Phase 7.2). With JS enabled,
  the input listener walks `<option>` children of the target select
  and toggles `option.hidden`. Selected option is exempt so a
  chosen value never disappears mid-edit.
- **Remote search** (Phase 7.3). When the field carries
  `data-search-url`, the input listener `fetch`es the URL with
  `?q=<query>` at ≥2 chars, replaces the `<option>` set with the
  JSON response, and re-prepends the previously-selected option if
  the search results don't contain it (so the form posts the
  operator's prior FK value if they don't change it). Errors are
  swallowed; the existing options remain.

With JS disabled, the truncated 50-row plain `<select>` is fully
functional. `FormData` is unchanged
(`HashMap<String, String>` — multi-value M2M form posts are out of
scope until Phase 7.4+).

The `/admin/search/:model` endpoint itself is Staff-guarded, returns
`application/json` (an array of `{value, label}` objects, capped at
`SEARCH_RESULT_LIMIT = 20`), and short-circuits to `[]` for unknown
models / empty queries / no matches — never 5xx.

## The AI layer, in three stages

All three are pure functions. Same inputs → same outputs.

1. `plan(prompt)` → `Plan` — rule-based grammar. Refuses instead of
   guessing when no rule matches.
2. `review(plan, schema)` → `Review` — deterministic risk + impact
   scoring against the current schema.
3. `apply_plan(plan, dir, opts)` → `ApplyOutcome` — writes migration
   `.sql` files. Destructive ops require `allow_destructive = true`.

The `Primitive` enum is `#[non_exhaustive]` + `deny_unknown_fields`.
External tools that match on it must include a wildcard arm. New
primitives can land without breaking them.

**Safety Guarantees** (Phase 9.1):
- AI-generated updates are validated and cannot produce empty schemas.
- All mutations require explicit confirmation or `--yes` flag.

## The ai_gen layer (Phases 8.0 → 9.1)

A separate, opt-in module — `rustio-core/src/ai_gen/` — that
provides LLM-assisted schema authoring **as a developer CLI tool**.
Strictly distinct from the rule-based `ai/` pipeline above:

- The deployed binary serving HTTP has no path into `ai_gen`. The
  HTTP handlers in `admin/handlers.rs` never call into it.
- `ai_gen` is reachable only through the `rustio` CLI subcommands
  `generate / update / analyze` (run from a developer's shell).
- LLM call count is bounded per command: 0 (plain analyze
  short-circuit), 1 (generate / update / `--apply`), 2 (`--pick`
  analyze+update, or update+explain), 3 (`--pick` + `--explain`).
  Never recursive.

Files:

- `mod.rs` — entry points (`generate / update / analyze /
  explain_diff`), `parse_response` family, error types, the
  `check_not_empty` empty-schema safety guard added in Phase 9.1.
- `client.rs` — Anthropic Messages API client (reqwest, ~1 POST
  per call). Reads `ANTHROPIC_API_KEY` env. Configurable provider
  base URL + model via `ANTHROPIC_API_BASE` / `RUSTIO_AI_MODEL`.
- `prompts.rs` — system + user templates for each path.
  Schema-version pin + allowed-type list are derived from
  `schema.rs` constants so the prompt cannot drift from the
  validator.
- `diff.rs` — minimal schema-diff (`Change` enum + `diff` + `render`)
  used by the CLI to render changes before y/N confirm.

Output of every path flows through validation (`Schema::validate`
for generate/update; tolerant text parser with section-header
detection for analyze/explain). The CLI owns file I/O, the
`--dry-run` decision, and the y/N confirmation; the library layer
is I/O-free.

## Templates

Every HTML template is compiled into the binary via `include_str!` in
`templates.rs`. At runtime, a project-local `templates/` directory can
override any template by name. The loader walks the directory once
during `Templates::new` — no filesystem calls per request.

Rust code never produces HTML. Handlers build a typed `serde::Serialize`
context and hand it to `templates.render(name, &ctx)`. That's the only
way HTML gets made.

Adding a template requires three changes in lockstep — the file under
`assets/templates/`, an `include_str!` line in
`EMBEDDED_TEMPLATES`, and a sandbox render test. Skipping the
registry line means the template renders fine in dev (the disk
loader picks it up via `RUSTIO_TEMPLATE_DIR=...`) but the production
single-binary path returns "template not found" → 500 at request
time. Skipping the render test means the missing registry isn't
caught by `cargo test`, only by browser smoke. The triple is one
edit unit; treat it that way.

### Styling pipeline (Phase 7a/2)

`rustio-core/assets/static/css/admin.css` is **generated**, not
authored. The source is `rustio-core/assets/css/input.css` (Tailwind
directives + an `@layer components` block defining the public-API
class contract: `.btn-primary`, `.module`, `.results`, `.empty-list`,
etc.). Tailwind scans the templates under `assets/templates/` for
class usage and emits a minified bundle.

Build pipeline (lives at the workspace root):

| File | Role |
|---|---|
| `package.json` | tailwindcss + autoprefixer + postcss as devDependencies |
| `tailwind.config.js` | `theme.extend` mirrors `docs/brand.md` (palette, Inter, radii, shadows) |
| `postcss.config.js` | tailwind + autoprefixer plugin chain |
| `Makefile` targets | `make css`, `make css-watch`, `make css-check` |

`make css` regenerates the minified `admin.css`. The compiled output
**is committed** so anyone running `cargo build` without Node sees a
working UI; `make css-check` diffs the committed file against a
fresh build and fails if they drift, suitable for a pre-commit hook.

Inter font ships as four self-hosted woff2 weights under
`rustio-core/assets/static/fonts/`, served by routes registered in
`register_admin_routes` (each weight is its own explicit route, not
a path-wildcard, so the binary can't be tricked into serving
arbitrary files from the assets dir).

### Admin chrome v2 (1.10.0)

The framework's default `admin/base.html`, `admin/list.html`,
`admin/index.html`, and `admin/user_view.html` ship a refreshed
"v2" design system: Geist + Geist Mono fonts, a Zinc neutral
palette, Cobalt + Violet two-accent system with strict role
separation (Cobalt = action, Violet = decoration), card / form /
table primitives at `.data-card` / `.form-card` /
`.card-section--inset`, a smart filter bar with sort + filter +
columns dropdowns on every list page, a refreshed dashboard with
a hero card and lazy-loaded model count badges, and a single-
column user-detail page.

The retired classes (`.splitview`, `.pane-list`, `.pane-detail`,
`.stat-strip`, `.stat-card`, `.show-grid`, `.dashboard-models`,
`.dashboard-recent`, `.toolbar-form`) keep their CSS rules in
`base.html` so downstream projects that hand-wrote markup against
them continue to render correctly. Only the framework's *default*
templates moved to the v2 markup; per-project overrides at
`templates/admin/<page>.html` win as before.

The runtime theme override block (`<style id="rio-accent-override">`)
still rewrites `--rio-bg`, `--rio-bg-surface-1`, `--rio-text`,
`--rio-text-muted`, `--rio-border`, and `--rio-accent` from
`Admin::theme(AdminTheme { … })`, so projects that drove the v1.8.x
chrome with a custom `AdminTheme` keep the same single-call
re-skin path.

### Icons (Phase 7a/2)

A custom minijinja function `icon(name, class="...")` is registered
in `Templates::new`. It looks up an inline SVG fragment from
`admin/icons.rs` (16 lucide stroke icons baked at compile time) and
emits a `<svg fill="none" stroke="currentColor">` so colour follows
the rendering context. Templates write `{{ icon("home", class="w-4 h-4") }}`;
unknown names render as empty strings (silent, never panic) so a
typo can't crash the page.

To add a new icon: drop the lucide inner SVG fragment into
`ICONS` in `admin/icons.rs` and update the unit-test catalogue.

### Theming (1.8.x)

The admin chrome is themable per-project without rebuilding Tailwind
or shipping per-project assets. Three layers cooperate:

| Layer | Where | Role |
|---|---|---|
| Design tokens | `docs/design-system.json` + `--ds-color-*` block in `input.css` | Static palette values; mirrored by hand in both files. The Cobalt Blue framework default lives here (`accent: "#2563EB"`, `accentBg: "#EFF6FF"`, `accentBorder: "#BFDBFE"`, plus `--ds-color-table-divider` scoped to `.results`). |
| `--rio-*` design tokens | `<style>` block in `admin/base.html` | What the admin shell actually reads. Topbar, sidebar, body, cards, headings, hairlines all reference `var(--rio-bg / -text / -accent / …)`. |
| Runtime override | `<style id="rio-accent-override">` injected into `<head>` per render | The bridge. Reads `AdminTheme` fields off the operator's `Admin` builder, redefines `--rio-*` tokens at `:root`, and the entire chrome cascades to the new palette in one repaint. |

The operator API:

```rust
use rustio_core::admin::{Admin, AdminTheme};

// One-line accent change. Full chrome inherits framework defaults.
let admin = Admin::new().accent_color("#7C3AED");

// Or full palette override:
let admin = Admin::new().theme(AdminTheme {
    accent:     "#7C3AED".into(),
    bg:         "#FAFAFC".into(),
    surface:    "#FFFFFF".into(),
    text:       "#1A1A2E".into(),
    text_muted: "#6B7280".into(),
    border:     "#D1D5DB".into(),
});

// `..AdminTheme::default()` rest-spreads framework defaults into
// the fields the operator doesn't care about.
let admin = Admin::new().theme(AdminTheme {
    accent: "#7C3AED".into(),
    ..AdminTheme::default()
});
```

`Admin::new()` with no theme chain inherits `AdminTheme::default()`,
which mirrors `themes.light` from `docs/design-system.json` exactly
(Cobalt Blue framework default).

Two derived surfaces are computed at render time via CSS
`color-mix`, so the operator only specifies the primary accent:

- `--rio-accent-bg`     = `color-mix(srgb, accent 8%, white)`  — active sidebar tint
- `--rio-accent-border` = `color-mix(srgb, accent 25%, white)` — accent badge ring

Button hover darkening uses the same recipe (`88% accent + black`)
in `.btn-primary:hover`, so themed projects get a coherent darker
hover automatically.

Single-binary deploy contract is preserved: no per-project asset
pipeline, no Tailwind rebuild, no extra stylesheet. Cost is one
small `<style>` block per page render.

## Sessions

Sessions are rows in `rustio_sessions`. The token is a 32-byte
random value, URL-safe base64-encoded, set as an HttpOnly cookie
with `SameSite=Strict` and a 14-day `Max-Age`. Expiry is checked at
lookup time.

A background sweeper (`background::spawn_session_sweeper`) clears
expired rows every 10 minutes; the request path doesn't pay for
cleanup. The sweeper logs an INFO line on boot (`background session
sweeper spawned (10 min interval)`) so it's visible in production
logs.

Password hashing is argon2id via the `argon2` crate. The parameters
come from `Argon2::default()`, which as of argon2 0.5 is OWASP's
current recommendation.

## Authorization

Two parallel grammars, never conflated (see `CLAUDE.md` for the
mental-model statement):

- **Role** — linear ladder, one per user:
  `User < Staff < Supervisor < Administrator < Developer`. Use
  `role_guard(min: Role)` at the route layer to set a floor.
- **Permission** — bag of codenames (`posts.add_post`,
  `posts.change_post`, …), granted directly to a user OR via a
  group. Use `perm_guard(perm: &str)` at the route layer.

`Administrator` and `Developer` bypass permission checks
(`Role::bypasses_group_checks()`). `is_active = FALSE` short-circuits
both — always checked **before** the bypass (defense-in-depth, see
Phase 7a/0.5/sec2 in `docs/phases/`).

Permissions are cached for 60s in a `DashMap` keyed by `user_id`.
Wholesale writes that bypass the per-pair helpers
(`add_user_to_group` / `remove_user_from_group`) must call
`invalidate_user_cache(user_id)` explicitly — see Phase 7a/0.5/sec3.

## User profile (Phase 10)

Every project gets a built-in user-profile page at
`GET /admin/users/:id` without writing a handler, route, or template.
The page renders four tabs (Overview / Activity / Permissions /
Sessions) using the splitview / tabs / timeline / show-grid /
stat-strip vocabulary from `admin/base.html`. Phase 10 ships in
three sub-phases:

- **`/a`** — additive schema migration. `rustio_users` gains
  `full_name`, `locale`, `timezone` (all `TEXT NULL`); `rustio_sessions`
  gains `ip`, `user_agent` (both `TEXT NULL`). Plus the public
  read-only `auth::UserProfile` struct (no `password_hash`) and its
  `auth::load_user_profile(db, user_id)` constructor.
- **`/b`** — built-in handler in `admin/builtin.rs::show_user_view`,
  built-in template at `assets/templates/admin/user_view.html`. Tab
  routing via `?tab=overview|activity|permissions|sessions`; activity
  tab paginates 50/page via `?tab=activity&page=N`. Tab links strip
  `&page`; only the pager preserves it. Inline Delete on the show
  page was removed in `/b` — destructive ops live exclusively at
  `/admin/users/:id/delete` with their own confirm flow.
- **`/c`** — extension mechanism. Projects register a single closure
  on `Admin` to contribute extra sections; the framework template
  defines `{% block project_user_fields %}` whose default body
  renders those sections, so zero-config projects render nothing
  and projects that want full markup control override the block.

### Extending the profile page

For the common case (key-value rows in a labeled section), register
a closure on the `Admin` builder:

```rust
use rustio_core::admin::{Admin, UserProfileRow, UserProfileSection};

let admin = Admin::new()
    .model_with_search::<Post>(indexer.clone())
    .user_profile_extension(|_db, user| Box::pin(async move {
        Ok(vec![UserProfileSection {
            label: "Halal certification".into(),
            rows: vec![
                UserProfileRow {
                    label: "Certified by".into(),
                    value: "ICCV Halal Authority".into(),
                },
                UserProfileRow {
                    label: "License #".into(),
                    value: "HC-2025-0042".into(),
                },
            ],
        }])
    }));
```

The closure receives `(Db, auth::UserProfile)` (both owned —
`Db` is cheap to clone, `UserProfile` is small) and returns
`Result<Vec<UserProfileSection>>`. It's invoked on every render of
the Overview tab; other tabs skip the call. The `UserProfile`
struct deliberately excludes `password_hash` so extensions cannot
leak credential material. Real projects typically do a SQL query
against a project-specific table here (`halalops` joins on
`halal_certifications`, a school admin joins on `enrollments`) —
the `Db` handle is exactly the framework pool used by the rest
of the request.

### Going beyond key-value rows

When a project needs richer layout (a chart, a status grid, a chip
list with badges), the closure isn't expressive enough. In that
case, drop a project template at
`templates/admin/user_view.html`:

```jinja
{% extends "admin/base.html" %}
{% block project_user_fields %}
    {# arbitrary HTML — has access to `user`, `project_fields`, and
       every other context variable the framework renders. #}
    <h2 style="margin-top: 32px;">Restaurant assignments</h2>
    <ul class="chips">
        {% for r in user.assigned_restaurants %}
        <li class="chip">{{ r.name }}</li>
        {% endfor %}
    </ul>
{% endblock %}
```

The closure and the template-block override aren't mutually
exclusive — the closure can pre-compute data the override consumes
via `project_fields` or via a richer custom context variable
threaded through a project-side handler.

The reference example is in `examples/blog/src/main.rs` — a minimal
two-row "Blog account" section computed from `UserProfile` alone,
no extra schema.

## What's deliberately small

- The HTTP layer knows HTTP/1.1 only (`hyper::server::conn::http1`).
  HTTP/2 is a future switch.
- The router matches literal segments and one-off `:param` captures.
  There are no regex routes, no priority sorting, no path globs.
  Insertion order matters: first match wins, so static segments must
  be registered before wildcards (`/admin/users/new` before
  `/admin/users/:id`).
- The ORM is PostgreSQL-only. `Db` is a thin wrapper around
  `sqlx::PgPool`; the `Value` enum and `Row` wrapper are the seam
  we'd widen if we ever supported a second backend, but there's no
  immediate plan to.
- The rate limiter (`middleware::rate_limit`) is per-IP, not
  per-user. A logged-in user behind a shared IP shares the bucket.
  Per-user buckets are deferred until the load profile justifies the
  complexity.
- 2FA / WebAuthn aren't in 1.0. Sessions are username + password +
  cookie.
- Per-row authorization isn't in 1.0. The current model is
  "permission to change a Post", not "permission to change Post
  #42". Future phases (likely 7a/1+) may add row-scoped checks for
  domain models that need them.
