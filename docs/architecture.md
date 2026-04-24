# Architecture notes

This doc describes how the pieces in `rustio-core` fit together. It's
aimed at someone who wants to change framework behaviour, not someone
using the framework.

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

## The admin module, in four files

`admin/` used to be one 7000-line file. It's now four, each with one
responsibility:

- `types.rs` — the data vocabulary: `AdminField`, `AdminModel`,
  `AdminEntry`, `Admin`. No HTTP, no HTML.
- `render.rs` — builds `serde::Serialize` context structs for the
  templates. No HTTP, no HTML strings in Rust.
- `handlers.rs` — one `async fn` per admin action
  (list/new/create/edit/update/delete + login/logout). No URL knowledge.
- `routes.rs` — the only file that knows about URL shapes. Wires
  handlers into the router, with the auth guard.

A new admin action touches exactly three files:
1. `handlers.rs` for the logic.
2. `render.rs` if it needs a new context struct.
3. `routes.rs` for the URL.

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

## Sessions

Sessions are rows in `rustio_sessions`. The token is a 32-byte random
value, URL-safe base64-encoded, set as an HttpOnly cookie with
`SameSite=Strict` and a 14-day `Max-Age`. Expiry is checked at lookup
time — no background cleanup yet.

Password hashing is argon2id via the `argon2` crate. The parameters
come from `Argon2::default()`, which as of argon2 0.5 is OWASP's
current recommendation.

## What's deliberately small

- There is no CSRF middleware yet. Forms are `POST`s with `SameSite=Strict`
  cookies, which cuts most real attacks, but the framework should grow a
  real CSRF token pipeline before 1.0.
- The HTTP layer knows HTTP/1.1 only (`hyper::server::conn::http1`).
  HTTP/2 is a future switch.
- The router matches literal segments and one-off `:param` captures.
  There are no regex routes, no priority sorting, no path globs.
- The ORM supports SQLite only. The `Value` enum and `Row` wrapper are
  the thin seam we'd widen if we added Postgres.
