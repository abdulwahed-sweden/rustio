# Changelog

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
