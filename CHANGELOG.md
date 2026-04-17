# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `rustio --version` (and `-V`, `version`) prints the CLI version.

## [0.1.0] — 2026-04-17

First public release.

### Added

- **HTTP layer**: hyper-backed server, custom router with `:param` paths, middleware
  chain (`Fn(Request, Next) -> Result<Response, Error>`).
- **Request context**: typed per-request store via `req.ctx()` / `req.ctx_mut()`.
- **Error model**: unified `Error` enum mapping to 400/401/403/404/405/500; safety net
  in `Router::dispatch` converts unhandled `Err` to `Response`.
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
- Migration SQL is split on `;` — literal `;` inside string values not handled.
- No CSRF on admin forms.
- No session auth — dev tokens only.
- Forward-only migrations (no `down`).
- `rustio-core = "0.1.0"` in generated projects is pinned to match CLI; lockstep
  releases expected until this stabilizes.

[Unreleased]: https://github.com/abdulwahed-sweden/rustio/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/abdulwahed-sweden/rustio/releases/tag/v0.1.0
