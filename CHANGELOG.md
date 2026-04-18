# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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

[Unreleased]: https://github.com/abdulwahed-sweden/rustio/compare/v0.3.1...HEAD
[0.3.1]: https://github.com/abdulwahed-sweden/rustio/compare/v0.3.0...v0.3.1
[0.3.0]: https://github.com/abdulwahed-sweden/rustio/compare/v0.2.2...v0.3.0
[0.2.2]: https://github.com/abdulwahed-sweden/rustio/compare/v0.2.1...v0.2.2
[0.2.1]: https://github.com/abdulwahed-sweden/rustio/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/abdulwahed-sweden/rustio/compare/v0.1.2...v0.2.0
[0.1.2]: https://github.com/abdulwahed-sweden/rustio/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/abdulwahed-sweden/rustio/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/abdulwahed-sweden/rustio/releases/tag/v0.1.0
