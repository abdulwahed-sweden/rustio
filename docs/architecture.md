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
- `types.rs` — the data vocabulary: `AdminField`, `AdminModel`,
  `AdminEntry`, `Admin`. No HTTP, no HTML.
- `render.rs` — builds `serde::Serialize` context structs for the
  templates (`BaseContext`, `dashboard_ctx`, `list_ctx`, `form_ctx`,
  `confirm_delete_ctx`). No HTTP, no HTML strings in Rust.
- `handlers.rs` — one `async fn` per generic admin action
  (list/new/create/edit/update/delete + login/logout/password-change).
  No URL knowledge.
- `routes.rs` — the only file that knows about URL shapes. Wires
  handlers into the router, holds `role_guard` / `perm_guard` /
  `login_guard`.
- `builtin.rs` — bespoke handlers for the built-in user/group pages
  (`/admin/users`, `/admin/groups`, plus the view + delete surfaces
  added in Phase 7a/0.5/f and /h).
- `entry_builder.rs` — derives `AdminEntry` lists from a `Schema`
  (the dynamic counterpart to `#[derive(RustioAdmin)]`).
- `audit.rs` — schema-vs-admin parity audit (catches missing fields).
- `relations.rs` — relation derivation for foreign-key navigation.
- `intelligence.rs` — schema-driven layout suggestions.
- `suggestions.rs` — surfaces the suggestions on the admin index.

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

The four template touch-points are the **(file, registry,
render-test) triple** — see `CLAUDE.md` for why all three are
load-bearing.

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
