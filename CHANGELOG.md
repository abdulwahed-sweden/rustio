# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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

[Unreleased]: https://github.com/abdulwahed-sweden/rustio/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/abdulwahed-sweden/rustio/compare/v0.1.2...v0.2.0
[0.1.2]: https://github.com/abdulwahed-sweden/rustio/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/abdulwahed-sweden/rustio/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/abdulwahed-sweden/rustio/releases/tag/v0.1.0
