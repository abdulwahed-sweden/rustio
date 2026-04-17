# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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

[Unreleased]: https://github.com/abdulwahed-sweden/rustio/compare/v0.3.0...HEAD
[0.3.0]: https://github.com/abdulwahed-sweden/rustio/compare/v0.2.2...v0.3.0
[0.2.2]: https://github.com/abdulwahed-sweden/rustio/compare/v0.2.1...v0.2.2
[0.2.1]: https://github.com/abdulwahed-sweden/rustio/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/abdulwahed-sweden/rustio/compare/v0.1.2...v0.2.0
[0.1.2]: https://github.com/abdulwahed-sweden/rustio/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/abdulwahed-sweden/rustio/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/abdulwahed-sweden/rustio/releases/tag/v0.1.0
