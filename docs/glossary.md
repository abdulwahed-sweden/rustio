# Glossary

Plain-English definitions for every framework term you'll meet in this repo. Keep this open in a tab; you don't have to memorize anything.

---

### Admin

The auto-generated web UI at `/admin`. Lists every model you registered, with create / edit / delete pages, search, filters, and pagination. You don't write the HTML — RustIO generates it from your structs.

### App

One folder inside `apps/`. Each app owns one "thing" your project knows about — a `notes` app, an `orders` app, an `accounts` app. An app contains a model (the data shape), a migration (the SQL to create the table), an admin file (one line that registers the model with the admin), and a views file (where you'd write public-facing routes if any).

### `apps/mod.rs`

The file that lists every app the project has. RustIO updates it automatically when you run `rustio new app <name>`. You can edit it by hand to add custom routes, but you usually don't need to.

### AI layer

A three-step pipeline: **plan → review → apply**. You describe a schema change in plain English (`rustio ai plan "add email to users"`). The planner produces a typed change. The review step rates the risk. The apply step writes the file changes atomically. If your request doesn't fit the vocabulary, the planner refuses — it never guesses.

### Audit log

Every change made through the admin (create, edit, delete) writes a row to the `rustio_actions` table with the user, the model, the operation, and a timestamp. View it at `/admin/actions`.

### CLI

The `rustio` command. Scaffolds projects, runs migrations, regenerates the schema, and drives the AI pipeline. Install it with `cargo install rustio-cli`.

### Context

A small JSON file (`rustio.context.json`) at the project root. Holds country, industry, and compliance flags. Drives PII detection — the AI layer refuses to delete a field flagged as personal under GDPR, for example. Optional; most projects don't need it.

### CSRF token

A short random string that proves a form submit came from your own page (not from another site). RustIO injects one into every admin form automatically; you don't have to think about it. If you ever see "missing CSRF token" in a 403 page, it means your client skipped the form's hidden input.

### Database

A file (`app.db` by default) that holds every row your project has stored. RustIO uses SQLite, so the database is literally a single file in your project directory. You can copy it, back it up, delete it, or open it in any SQLite tool.

### Foreign key (FK)

A column on one table that points at the `id` of a row in another table. Example: `tasks.project_id` points at `projects.id`. RustIO writes the SQL `FOREIGN KEY` clause for you when you mark the field with `#[rustio(belongs_to = "Project")]`. The admin renders the FK as a clickable name, not a raw number.

### Identity

The thing the auth middleware attaches to every request once the user signs in. Contains the user id, email, and role. Your handlers read it with `crate::auth::identity(req.ctx())`.

### Middleware

A function that runs before (and sometimes after) every request handler. RustIO has three by default: auth (`authenticate`), CSRF check, and body-size limit. They're stacked with `with_defaults(router)`.

### Migration

A `.sql` file that changes the database schema. Filenames are numbered (`0001_create_users.sql`, `0002_add_orders.sql`); RustIO applies them in order and remembers which ones it already applied. You can write them by hand or let `rustio ai apply` generate them.

### Model

A Rust struct that describes one "thing" — usually one table in the database. Annotated with `#[derive(RustioAdmin)]` so the framework knows how to render it in the admin and emit it in the JSON schema. Example: `Project`, `Task`, `Order`.

### Plan (`PlanDocument`)

The output of `rustio ai plan`. A JSON file containing the parsed primitives, an explanation, and metadata. You can `review` it, `validate` it (terse CI gate), and `apply` it. Saved plans round-trip — they're a stable contract.

### Primitive

One operation the AI layer can express: `AddField`, `RemoveField`, `RenameField`, `RenameModel`, `ChangeFieldType`, `ChangeFieldNullability`, `AddRelation`, `UpdateAdmin`. If a request can't be expressed as a primitive, the planner refuses. There is no escape hatch.

### RBAC

Role-Based Access Control. RustIO ships four roles (`SuperAdmin`, `Admin`, `Editor`, `Viewer`) and per-model `view` / `create` / `edit` / `delete` permissions. A user without `view` doesn't see the model in the sidebar at all; a user without `edit` sees the row but can't open the edit form. Roles live in the `roles` and `user_roles` tables.

### Relation

A typed link between two models. Today: `belongs_to` (a Task belongs to a Project). The schema records the link, the admin renders it as a clickable name, and the delete handler returns a `409 Conflict` instead of cascading-deleting children. `has_many` (the reverse direction) is on the roadmap.

### Role

A name a user has — `SuperAdmin`, `Admin`, `Editor`, or `Viewer`. Stored on the `user_roles` row. Determines what the user can do in the admin.

### Route

A path on your server + an HTTP method + a handler function. `GET /admin/projects` is a route. RustIO registers admin routes for you; you register your own public routes in `apps/<app>/views.rs`.

### Schema (`rustio.schema.json`)

A JSON file that lists every model, every field, every type, every relation. Regenerated on every `rustio migrate apply` (or by hand with `rustio schema`). The **only** interface external tools (including the AI layer) are allowed to use. Stable across patch releases.

### Server

Your compiled binary running and listening on a port (default `:8000`). Start it with `rustio run`, stop it with Ctrl-C. In development, restart it after every code change.

### Session

A signed cookie that tells the server "this browser is currently logged in as user 7". Sessions live in the `rustio_sessions` table, with a CSRF token tied to each one. They expire; the user logs in again.

### Static asset

CSS, JS, fonts, images. RustIO serves the admin's CSS + JS from `/admin/static/`. Your own assets go in the project's `static/` directory; they're served at `/static/`.

### Template

A `.html` file with `{{ variables }}` and `{% if … %}` tags. RustIO renders the admin's pages through `minijinja` templates bundled inside the framework. You can override any template by dropping a file at the same relative path under your project's `templates/` directory.

### User

A row in the `rustio_users` table — email + argon2-hashed password + role. Created with `rustio user create`. The auth middleware looks up the user from the session cookie on every request.

---

## See also

- **[`README.md`](../README.md)** — the beginner entry point.
- **[`docs/advanced/`](advanced/)** — deeper walkthroughs once these terms feel comfortable.
