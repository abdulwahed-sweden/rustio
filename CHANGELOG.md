# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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

[Unreleased]: https://github.com/abdulwahed-sweden/rustio/compare/v0.1.1...HEAD
[0.1.1]: https://github.com/abdulwahed-sweden/rustio/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/abdulwahed-sweden/rustio/releases/tag/v0.1.0
