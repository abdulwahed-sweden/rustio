# RustIO

[![release](https://img.shields.io/badge/release-v1.10.0-brightgreen)](https://github.com/abdulwahed-sweden/rustio/releases/tag/v1.10.0)
[![license](https://img.shields.io/badge/license-MIT-blue)](LICENSE)
[![rust](https://img.shields.io/badge/rust-1.75%2B-orange)](https://www.rust-lang.org)
[![schema](https://img.shields.io/badge/schema-contract-informational)](docs/architecture.md)
[![tests](https://img.shields.io/badge/tests-610_passing-success)](#running-the-test-suite)

> **Write one Rust struct. Get an admin UI, search, validation, and
> CRUD — without glue code, without YAML, without a separate ORM
> layer.**

RustIO is a production-grade, strict-by-construction web framework
for Rust. Its phase-14/15 **schema contract system** treats your
model definition as the single source of truth: admin pages, search
indexing, runtime DB validation, CLI doctoring, and migrations all
flow from one `#[derive(RustioModel)]`. No manual `AdminModel`,
no manual `Searchable`, no shadow registry.

```text
   Model (#[derive(RustioModel)])
        │
        ▼
   T::SCHEMA  (compile-time contract)
        │
        ├─► Validator ──► Doctor CLI       (drift detection vs live PG)
        ├─► Admin runtime                  (auto-generated UI + CRUD)
        └─► Search runtime                 (Meili index, gated by validator)
```

## Try it in 30 seconds

The `examples/freelance/` crate is a complete end-to-end demo of
the schema-driven pipeline — three models (`Client`, `Project`,
`Invoice`), three migrations, zero glue code:

```bash
git clone https://github.com/abdulwahed-sweden/rustio
cd rustio/examples/freelance

# Optional — if you have PostgreSQL handy:
createdb rustio_freelance
psql rustio_freelance -f migrations/0001_create_clients.sql
psql rustio_freelance -f migrations/0002_create_projects.sql
psql rustio_freelance -f migrations/0003_create_invoices.sql

cargo run
```

Output (one block per model):

```text
=== projects ===
validator: status=Ok errors=0 warnings=0
search:    enabled (index=projects)
           searchable = ["name", "description"]
           filterable = ["client_id"]
           sortable   = ["name", "client_id", "budget_cents", "created_at"]
admin:     6 fields auto-generated
           - id             label="Id"            editable=false flags=pk,readonly
           - name           label="Name"          editable=true  flags=searchable,sortable
           - description    label="Description"   editable=true  flags=searchable,textarea
           - client_id      label="Client"        editable=true  flags=filterable,sortable
           - budget_cents   label="Budget (cents)" editable=true flags=sortable
           - created_at     label="Created At"    editable=false flags=sortable,readonly
```

That's the entire wiring: every label, every widget, every search
attribute, every CRUD operation comes from the model declaration.
No `AdminModel`, no `Searchable`, no glue.

A schema-drift check is one CLI flag away:

```bash
rustio doctor --check-schema           # human-readable
rustio doctor --check-schema --json    # CI-friendly JSON, exit 1 on errors
```

See `examples/freelance/README.md` for the full architecture walk-through.

## Two examples, two flows

| Example | Flow | Best for |
|---|---|---|
| **`examples/freelance/`** | Schema-driven (Phase 14/15). One `#[derive(RustioModel)]` per struct → admin + search + doctor are auto-wired. | New projects, the recommended starting point as of v1.8.2. |
| `examples/blog/` | Classic flow (Phase 1–13). Manual `AdminModel` + `Searchable` + `Model` impls; full HTTP server with templates, RBAC bootstrap, demo users. | Reference for the full middleware / RBAC / templates / migrations stack. The two flows compose — a project can mix both. |

What you write to add a model in the schema-driven flow:

```rust
#[derive(rustio_macros::RustioModel)]
#[rustio(table = "projects")]
pub struct Project {
    #[rustio(sql = "BIGSERIAL PRIMARY KEY", readonly)]
    pub id: i64,

    #[rustio(sql = "TEXT NOT NULL", searchable, sortable)]
    pub name: String,

    #[rustio(sql = "TEXT", searchable, widget = "textarea")]
    pub description: Option<String>,

    #[rustio(sql = "BIGINT NOT NULL", filterable, sortable, references = "clients(id)")]
    pub client_id: i64,

    #[rustio(sql = "TIMESTAMPTZ NOT NULL DEFAULT NOW()", readonly, sortable)]
    pub created_at: chrono::DateTime<chrono::Utc>,
}

// At startup:
let admin = rustio_core::admin::Admin::new()
    .from_schemas(&[Project::SCHEMA, /* ... */]);

let _indexer = rustio_core::search::from_schema::indexer_from_schema::<Project>(
    meili_client, &db, 1024,
).await;

// `rustio doctor --check-schema` validates each schema against the live DB.
```

The `searchable` flag drives the Meili index. The `_id` suffix
turns into a "Client" label automatically. `widget = "textarea"`
overrides the default `<input type="text">`. Validator drift
disables search before bad documents are indexed. None of this is
configured anywhere else — the struct is the contract.

---

A production-grade, strict-by-construction web framework for Rust.

Write a model struct, derive `RustioAdmin`, and you get the admin UI,
HTTP server, Postgres ORM, migrations, full-text search, sessions, and
granular permissions — without writing the glue. Optional developer
AI tooling on top: generate / update / analyze schemas in plain
English, then run the deterministic plan / review / apply pipeline.

## What's in 1.x

| Layer | Implementation |
|---|---|
| HTTP | hyper 1.x, HTTP/1.1 + keep-alive, graceful shutdown |
| Database | PostgreSQL via sqlx, 30-conn pool, prepared-statement cache |
| Search | Meilisearch (REST), async batch indexer with backpressure |
| Templates | minijinja, embedded defaults + on-disk overrides |
| Auth | argon2id passwords, DB-backed sessions, HttpOnly cookies |
| Authorization | Users + Groups + per-action permissions, 60s LRU cache |
| Caching | In-process LRU query cache with prefix invalidation |
| Middleware | logger, csrf, rate_limit, gzip, security_headers |
| Admin UI | sections + responsive grid, schema-driven widget mapping, FK / M2M selects with client-side filter and remote search, Inter font (self-hosted), 16 lucide icons |
| Styling | Tailwind at build time → single minified `admin.css` baked into the binary at deploy. Tokens in `docs/design-system.json` are the source of truth |

## Quick start

Prereqs: PostgreSQL 14+, Meilisearch 1.10+.

```bash
# Spin up the dev backends (postgres + meilisearch). Containers come
# up as `rustio-postgres` and `rustio-meilisearch`; named volumes are
# `rustio_pg_data` and `rustio_meili_data`.
docker compose up -d

# Pick up the example env (DATABASE_URL, MEILI_URL, etc.). Edit
# `.env` afterwards if you need to point at non-default Postgres /
# Meili instances. `.env` is gitignored — see `.env.example` for
# the full list of variables and what each one does.
cp .env.example .env
set -a && source .env && set +a

# Run the example. First boot creates tables, applies migrations,
# and seeds the default admin (see "What happens on first run" below).
cd examples/blog
rustio run        # convenience wrapper around `cargo run`; either works

# Open http://127.0.0.1:8000/admin
# Log in with admin@example.com / admin
```

### What happens on first run

`rustio run` (or `cargo run` directly — they do the same thing) does
the boring bootstrap for you:

1. Connects to Postgres and creates the `rustio_*` system tables
   (users, groups, sessions, permissions) if they're missing.
2. Applies any pending migrations under `examples/blog/migrations/`.
3. Seeds a default administrator account
   (`admin@example.com` / `admin`) if `rustio_users` is empty —
   confirm in the log: `seeded default admin: admin@example.com / admin`.
4. Seeds an `editors` group with `posts.add_post` /
   `posts.change_post` / `posts.view_post` if it doesn't exist.
5. Connects to Meilisearch and configures the `posts` index. If
   Meili isn't reachable, the server still starts; search routes
   surface a "search unavailable" notice and everything else
   keeps working.

The seeded admin password is for first-run convenience only —
sign in, open `/admin/users`, and change it before doing anything
else. For production, also set up at least one non-demo
administrator and unset `RUSTIO_DEMO_MODE` (see Configuration below).

## Configuration

Rustio reads configuration from environment variables only — there's
no config file. `.env.example` at the repo root is the canonical list;
copy it to `.env` for local dev. Below is the same list grouped by
how often you'll touch it.

### Required (runtime + CLI)

| Variable | Notes |
|---|---|
| `DATABASE_URL` | Postgres connection string. Required by the server, the example crate, and every DB-touching `rustio` CLI subcommand. CLI subcommands accept `--db` as an explicit override. |

### Optional (runtime)

| Variable | Default | Notes |
|---|---|---|
| `MEILI_URL` | `http://localhost:7700` (example crate) | Meilisearch endpoint. If unreachable, search features silently degrade — the rest of the admin keeps working. |
| `MEILI_MASTER_KEY` | unset | Set in production. Local Meili `MEILI_ENV=development` is permissive without it. |
| `MIGRATIONS_DIR` | `<crate>/migrations` | Override for packaged binaries that don't ship the source tree. |
| `RUSTIO_TEMPLATE_DIR` | `templates` | Disk overrides for embedded templates. Edits land on the next request — no restart. |

### Demo mode

| Variable | Default | Notes |
|---|---|---|
| `RUSTIO_DEMO_MODE` | unset | Setting `=1` seeds five demo users with public passwords (one per role) and renders a red DEMO banner above every page. **Leave unset in production.** This is the only env switch that needs to flip between dev and prod. |

### AI developer tooling (optional)

These are read by the `rustio ai *` developer CLI only. The deployed
HTTP server has no path into the AI layer.

| Variable | Default | Notes |
|---|---|---|
| `ANTHROPIC_API_KEY` | unset | Required for `ai generate / update / analyze`. |
| `ANTHROPIC_API_BASE` | `https://api.anthropic.com` | Override for proxies. |
| `RUSTIO_AI_MODEL` | (built-in default) | Override the model the CLI uses. |

### Behaviour when env vars are missing

- **`DATABASE_URL` missing**: the example crate falls back to the
  local-dev default (`postgres://postgres:dev@localhost/rustio_dev`).
  CLI subcommands fail fast with a clear error because they require
  either `--db` or `DATABASE_URL`.
- **`MEILI_URL` missing**: the example crate uses the local-dev
  default. If Meilisearch isn't reachable at that URL either, search
  routes return a friendly "unavailable" message and admin traffic
  is unaffected.
- **`RUSTIO_DEMO_MODE` unset**: production-default behaviour — no
  demo users seeded, no banner.
- **AI env vars missing**: only `rustio ai *` subcommands fail (with
  a clear "set ANTHROPIC_API_KEY" hint). The deployed server is
  unaffected because it never imports the AI layer.

## Running the test suite

Two modes:

```bash
# Sandbox: pure unit tests, no infrastructure needed.
# 388 (rustio-core) + 14 (rustio-cli) = 402 passing as of v1.1.1.
cargo test --workspace

# Integration suite — 41 PG-gated tests. Needs `docker compose up
# -d` (postgres on rustio_dev). Override the URL via
# RUSTIO_TEST_DATABASE_URL if your local Postgres lives elsewhere.
RUSTIO_TEST_DB=1 cargo test --workspace -- --ignored
```

## A model end-to-end

```rust
use chrono::{DateTime, Utc};
use rustio_core::{Error, Model, Row, RustioAdmin, Searchable, Value};
use serde_json::json;

#[derive(Debug, RustioAdmin)]
pub struct Post {
    pub id: i64,
    pub title: String,
    pub body: String,
    pub published: bool,
    pub created_at: DateTime<Utc>,
}

impl Model for Post {
    const TABLE: &'static str = "posts";
    const COLUMNS: &'static [&'static str] =
        &["id", "title", "body", "published", "created_at"];
    const INSERT_COLUMNS: &'static [&'static str] =
        &["title", "body", "published", "created_at"];

    fn id(&self) -> i64 { self.id }

    fn from_row(row: Row<'_>) -> Result<Self, Error> {
        Ok(Self {
            id: row.get_i64("id")?,
            title: row.get_string("title")?,
            body: row.get_string("body")?,
            published: row.get_bool("published")?,
            created_at: row.get_datetime("created_at")?,
        })
    }

    fn insert_values(&self) -> Vec<Value> {
        vec![
            self.title.clone().into(),
            self.body.clone().into(),
            self.published.into(),
            self.created_at.into(),
        ]
    }
}

impl Searchable for Post {
    const INDEX_NAME: &'static str = "posts";
    const SEARCHABLE_ATTRIBUTES: &'static [&'static str] = &["title", "body"];
    const FILTERABLE_ATTRIBUTES: &'static [&'static str] = &["published"];

    fn to_search_doc(&self) -> serde_json::Value {
        json!({
            "id": self.id,
            "title": self.title,
            "body": self.body,
            "published": self.published,
        })
    }
}
```

Wire it up:

```rust
let admin = Admin::new()
    .model_with_search::<Post>(indexer.clone());
admin.seed_permissions(&db).await?;
```

You now have:
- `/admin/posts` — list with edit/delete
- `/admin/posts/new` — add form (CSRF-protected)
- `/admin/posts/:id/edit` — edit form
- `/admin/posts/:id/delete` — confirm-and-delete
- Auto-emitted permissions: `posts.add_post`, `posts.change_post`,
  `posts.delete_post`, `posts.view_post`
- Auto-indexing into Meilisearch on every write

### Theme it (1.8.3+)

The admin chrome ships in Cobalt Blue (`#2563EB`) by default. To
re-skin the whole shell — topbar, sidebar, body, cards, hairlines,
accent — pass an `AdminTheme` to the builder. Operators usually
override only one or two fields and let `..AdminTheme::default()`
fill the rest:

```rust
use rustio_core::admin::{Admin, AdminTheme};

let admin = Admin::new()
    .theme(AdminTheme {
        accent:  "#7C3AED".into(), // your brand
        ..AdminTheme::default()
    })
    .model::<Post>();
```

`Admin::accent_color("#7C3AED")` is a one-line shortcut for the
common case (only the accent changes). Hover darkening, the active
sidebar tint, and the accent badge border are all derived from the
single accent value at render time via `color-mix` — no extra
config, no Tailwind rebuild. See `docs/architecture.md → Theming`
for the layered model.

### Polished landing page (1.10.0)

`rustio startproject foo` now writes a self-contained
`templates/home.html` matching the v2 admin chrome's design
language — Geist + Geist Mono fonts, hero card with cobalt
status pulse, three-up feature grid, "First run" setup card with
the `rustio user create` hint. Visible at `/` from the moment
the server boots; edit freely.

### Admin chrome v2 (1.10.0)

Every project that uses RustIO inherits a refreshed admin design
system out of the box — no per-project copy required.

- **Type:** Geist + Geist Mono via Google Fonts; 16-px-anchored
  scale; tabular numbers everywhere.
- **Palette:** Zinc neutrals on a `#fafafa` page; pure-white
  cards; pale cool gray-blue (`#f4f6fa`) for the left nav and
  the right aside on dashboard / form pages.
- **Two accents, strict roles:** Cobalt `#2563eb` for *actions*
  (buttons, links, focus rings, active tabs) — Violet `#8b5cf6`
  for *decoration* (filter chips, sidebar stripe, inline `<code>`,
  section accent underlines). Form bodies sit on a 4 % violet
  wash; the inputs themselves stay pure white so they float
  visibly above it.
- **Smart list page.** Search input with `/` shortcut, Sort
  dropdown, Add-filter dropdown (auto-populates from rendered
  rows for `status` text columns), **Columns** dropdown that
  toggles column visibility and persists to `localStorage`,
  active-filter chip row with Clear-all. The ID column is hidden
  by default; each model's primary / tertiary columns bucket
  through a small JS map.
- **Redesigned dashboard.** Hero greeting card with date and
  pulsing operational status; single unified data-models grid
  with `<article class="model-card">` items that lazy-fetch
  their row count via `fetch('/admin/<model>/')`; a Recent
  activity card and a System info card on the side rail.
- **Single-column user detail.** 960-px max-width card with a
  back-link, header (name + role + Active + Edit), one-line meta
  (`email · N sessions · M events · last seen X`), tab strip,
  and a 2-column profile grid with a violet decorative underline
  beneath each section heading.

The retired classes (`.splitview`, `.pane-list`, `.dashboard-*`,
`.toolbar-form`, `.stat-strip`, `.show-grid`) keep their CSS rules
in `base.html` — any project that hand-wrote markup against them
keeps rendering correctly. Only the framework's default templates
have moved to the v2 markup.

To override anything, drop a file at
`templates/admin/<page>.html` in your project; the runtime template
loader checks that path first and falls back to the embedded
default. See `docs/architecture.md → Templates` for the override
contract.

## Users, groups, permissions

Two parallel grammars:

- **Role** — the linear access ladder, one per user:
  `User < Staff < Supervisor < Administrator < Developer`. Routes
  set a floor with `role_guard(min: Role)`. `Administrator` and
  `Developer` bypass per-permission checks (the trusted-operator
  tier); `Staff` and `Supervisor` go through the permission machinery.
  `is_active = FALSE` short-circuits both, checked **before** the
  bypass.
- **Permission** — a bag of codenames (`posts.add_post`,
  `posts.change_post`, …), granted to a user directly or inherited
  from a group. Routes call `perm_guard(perm: &str)`.

Permission lookups are cached for 60s in a `DashMap` per user.
Wholesale group writes (`DELETE FROM rustio_user_groups WHERE
user_id = $1`) must call `invalidate_user_cache` explicitly — the
per-pair helpers `add_user_to_group` / `remove_user_from_group`
invalidate internally.

Granting permissions, two paths:

```bash
# Direct grant (rare)
rustio perm grant-user --email alice@x.com --permission posts.change_post

# Via group (preferred)
rustio group create editors --description "Can manage posts"
rustio group grant --group editors --permission posts.add_post
rustio group grant --group editors --permission posts.change_post
rustio user add-to-group --email alice@x.com --group editors
```

### Replacing default authentication

The default flow is email + password against `rustio_users`, with
argon2id-hashed passwords and DB-backed sessions. To swap in
SSO / OIDC / LDAP / magic-link / anything else, the surface area
is small — three named functions:

- **Login form POST** — `do_login` in
  `rustio-core/src/admin/handlers.rs`. The entry point. Reads
  email + password from the form, calls `auth::login(&db, email,
  password)`, and on success sets the `rustio_token` cookie
  carrying a freshly-minted session token.
- **Per-request identity** — `login_guard` in
  `rustio-core/src/admin/routes.rs`. Reads the cookie, looks up
  the session, and produces an `Identity { user_id, email, role,
  is_active, .. }`. Every admin handler downstream consumes that
  struct.
- **Session storage** — `auth::create_session` /
  `identity_from_session` / `delete_session` in
  `rustio-core/src/auth/users.rs`. Sessions are rows in the
  `rustio_sessions` table.

The simplest swap pattern: keep the cookie + session-row machinery
unchanged, replace just the credential check. Your provider verifies
the user upstream, then your code calls `auth::create_session` to
mint the same kind of token. Everything downstream — RBAC,
permission cache, audit log, the entire admin — works unchanged
because it operates on `Identity`, not on the credential type.

For first-run user provisioning, see `seed_initial_admin` in
`examples/blog/src/main.rs` — typical SSO swaps replace this with
a "create user on first successful upstream login" pattern instead.

## CLI reference

```
rustio new project <name>             scaffold a new project
rustio new app <name>                 scaffold an app inside one

rustio migrate apply                  apply pending migrations
rustio migrate generate <name>        create a new empty migration file
rustio migrate status                 show which migrations are applied

rustio user create --email --role     create a user (prompts for password)
rustio user set-password --email      reset a password
rustio user add-to-group --email --group

rustio group create <name>            create a group
rustio group grant --group --permission

rustio perm list                      list every registered permission
rustio perm grant-user --email --permission

rustio ai plan "<prompt>"             rule-based plan from prose
rustio ai review <plan.json>          score risk against current schema
rustio ai apply <plan.json>           write migration files
```

**Safety:**
- AI updates will refuse destructive operations that result in empty schemas.
- Use `--dry-run` to preview changes safely.

Every command that talks to the DB takes `--db` or reads `DATABASE_URL`.

## Performance notes

- **Connection pool: 30** by default. SQLx prepares statements per
  connection and caches them; first query on a connection pays the
  prepare cost, subsequent queries don't.
- **Read cache: 2048 entries.** Tune with `DbOptions { cache_capacity }`.
  Invalidates by table prefix on every write.
- **Permission cache: 60s TTL.** Hot admin endpoints don't touch the
  permission tables on every request.
- **Search batching: 100ms / 500 docs.** Bulk indexing is ~10× faster
  than per-doc round-trips. Backpressure via a 1024-entry channel.
- **Session reads: O(1) DB hit + an out-of-band `last_seen` UPDATE
  fired on a tokio task** — request doesn't wait for it.
- **gzip kicks in at 1KB.** Below that, compression overhead beats
  the savings.

## Architecture

```
rustio-core/                          ~31,700 LOC, 388 sandbox + 41 PG-gated tests
├── src/
│   ├── admin/        types / render / handlers / routes / builtin /
│   │                 audit / relations / intelligence / suggestions /
│   │                 entry_builder / icons (16 lucide stroke icons)
│   ├── ai/           rule-based planner, reviewer, executor (deterministic)
│   ├── ai_gen/       LLM client + prompts + diff (developer-tool only)
│   ├── auth/         users, sessions, permissions (split files)
│   ├── middleware/   logger, csrf, rate_limit, gzip, security
│   ├── search/       Meilisearch client + async indexer
│   ├── orm.rs        Postgres pool, Model trait, CRUD helpers
│   ├── cache.rs      LRU query cache
│   ├── server.rs     hyper glue + graceful shutdown
│   └── ...
└── assets/
    ├── templates/    21 admin templates (19 pages + 2 includes)
    ├── css/          input.css — Tailwind source (authored)
    └── static/
        ├── css/      admin.css — minified Tailwind output (generated, ~65KB)
        ├── fonts/    Inter Regular/Medium/SemiBold/Bold (woff2, ~95KB)
        └── js/       search.js — vanilla search UI helper

rustio-macros/                        ~400 LOC
└── src/lib.rs        #[derive(RustioAdmin)]

rustio-cli/                           ~900 LOC, 14 tests
└── src/main.rs       the `rustio` binary

examples/blog/                        ~150 LOC
└── full Postgres + search + auth example
```

See `docs/architecture.md` for the longer version, `docs/brand.md`
for the visual brand spec, and `docs/phases/` for the per-phase
chronology. `CHANGELOG.md` rolls release-level highlights.

## AI tooling (developer-only)

Optional LLM-assisted schema authoring. Set `ANTHROPIC_API_KEY` and
run any of:

```bash
# Prose → validated Schema JSON. Validated by `Schema::validate()`
# before write; refuses to overwrite existing files without --force.
rustio ai generate "blog system with posts, users, comments" --out schema.json

# Evolve a schema: single LLM call, diff against current, y/N confirm.
# Preserve-by-default — existing models / fields / types never silently
# change. v1.1.1 hard-rejects results that empty the schema.
rustio ai update schema.json "add tags and post status"

# Read-only audit: issues + suggestions + score (0–10).
rustio ai analyze schema.json

# Bridge analyze → update without retyping the suggestion.
rustio ai analyze schema.json --pick 1
rustio ai analyze schema.json --apply "add user roles"

# Preview / explain flags work on any mutating flow.
rustio ai update schema.json "add tags" --dry-run
rustio ai update schema.json "add tags" --explain
```

LLM exposure is strictly developer-tool: the deployed `rustio`
binary serving HTTP has no path into `ai_gen`. The deterministic
`plan / review / apply` pipeline runs separately and uses no LLM
at any stage.

**Safety:**
- AI updates will refuse destructive operations that result in empty schemas.
- Use `--dry-run` to preview changes safely.
- Use `--yes` on `ai update` / `ai analyze --apply` / `--pick` for
  scripted flows; `--dry-run` always wins over `--yes`.

## Releases

| Tag | Scope |
|---|---|
| `v1.0-admin` | Admin system through Phase 7.6 (production hardening) |
| `v1.1-ai` | + AI developer tooling (generate / update / analyze / explain, Phases 8.0–8.4) |
| `v1.1.1` | + AI safety hardening (Phase 9.1) |

See `CHANGELOG.md` for per-release notes.

## Design principles (unchanged from 0.9)

1. **Single binary.** Default templates and CSS are baked in via
   `include_str!`. Override anything by dropping a file in
   `templates/` or `static/`.
2. **No magic.** The macro emits readable code (`cargo expand`).
3. **Tailwind at build time, single binary at deploy.** As of Phase 7a/2,
   admin styles are authored in Tailwind and compiled into a single
   minified `admin.css` that's `include_str!`-baked into the binary
   alongside the templates. Inter font weights ship as self-hosted
   woff2 (~95KB). The deployed binary still has zero CDN dependencies.
   No React, no SPA, no JS framework — just plain server-rendered
   HTML with ~30 lines of inline JS for the sidebar drawer and search
   keyboard shortcut.
4. **Deterministic AI at runtime, opt-in LLM at build time.** The
   `plan / review / apply` pipeline is rule-based and runs inside
   the deployed binary — no LLM ever. The Phase 8 `ai_gen` layer
   (`generate / update / analyze / explain`) is a developer CLI
   tool that runs only when you invoke it; the deployed binary
   has no path into it.
5. **Strict by construction.** The AI's `Primitive` enum is
   `#[non_exhaustive]` + `deny_unknown_fields`. Destructive
   operations refuse without `--yes`. v1.1.1 adds a hard guard
   that refuses any `ai update` result that would empty a
   non-empty schema — no bypass flag.

## License

MIT
