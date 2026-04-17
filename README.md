# RustIO

A batteries-included web framework for Rust with auto-generated admin.

Type-safe, compile-time driven, single-binary. Inspired by Django's productivity; built from scratch for Rust.

## Install

    cargo install rustio-cli

## Quick start

    rustio new project mysite
    cd mysite
    rustio new app blog
    rustio migrate apply
    rustio run

Open [http://127.0.0.1:8000/](http://127.0.0.1:8000/) for the homepage.

Open [http://127.0.0.1:8000/admin/blogs](http://127.0.0.1:8000/admin/blogs) with header `Authorization: Bearer dev-admin` for the auto-generated CRUD admin.

## Example model

```rust
use rustio_core::{Error, Model, Row, RustioAdmin, Value};

#[derive(RustioAdmin)]
pub struct Post {
    pub id: i64,
    pub title: String,
    pub published: bool,
}

impl Model for Post {
    const TABLE: &'static str = "posts";
    const COLUMNS: &'static [&'static str] = &["id", "title", "published"];
    const INSERT_COLUMNS: &'static [&'static str] = &["title", "published"];

    fn id(&self) -> i64 { self.id }

    fn from_row(row: Row<'_>) -> Result<Self, Error> {
        Ok(Self {
            id: row.get_i64("id")?,
            title: row.get_string("title")?,
            published: row.get_bool("published")?,
        })
    }

    fn insert_values(&self) -> Vec<Value> {
        vec![self.title.clone().into(), self.published.into()]
    }
}
```

The admin UI at `/admin/posts` is generated from this struct. No HTML, no routing, no form handling to write.

## What's included

- **HTTP** — hyper-backed server, custom router with `:param` paths, middleware chain.
- **Request context** — typed, per-request store (`req.ctx_mut().insert(X)`, `req.ctx().get::<X>()`).
- **Errors** — unified `Error` enum mapping to HTTP status codes (400/401/403/404/405/500).
- **Auth** — additive middleware + `require_auth` / `require_admin` helpers with `Identity` in context. Dev tokens included; swap for real auth before deploying.
- **ORM** — `Model` trait over SQLite via `sqlx`. `User::find(&db, id)`, `User::all(&db)`, `user.create(&db)`, etc. No raw SQL in user code for CRUD.
- **Admin** — `#[derive(RustioAdmin)]` auto-generates list/create/edit/delete pages and routes from struct fields.
- **Migrations** — versioned `.sql` files, tracked in a `rustio_migrations` table, transactional + idempotent.
- **CLI** — `rustio new project`, `new app`, `migrate generate/apply/status`, `run`. Colored output with `NO_COLOR` respected.

## Configuration

- `RUSTIO_DATABASE_URL` — override the default `sqlite://app.db?mode=rwc`.
- `NO_COLOR` — disable colored CLI output.
- `RUSTIO_CORE_PATH` — when invoking the CLI, use a local path to `rustio-core` instead of the crates.io version (for RustIO contributors).

## Layout of a generated project

    mysite/
    ├── Cargo.toml
    ├── main.rs              # entry point (top-level by convention)
    ├── apps/
    │   ├── mod.rs           # aggregator
    │   └── blog/
    │       ├── mod.rs
    │       ├── models.rs    # struct + Model impl
    │       ├── admin.rs     # admin::register::<Blog>
    │       └── views.rs     # custom HTTP handlers
    ├── migrations/          # NNNN_*.sql files
    ├── static/
    ├── templates/
    └── app.db               # SQLite (gitignored)

## Status

Early alpha. APIs are expected to change before 1.0. Pre-production only.

Defaults assume dev (in-repo dev tokens, no CSRF, permissive SQLite pool). Review `rustio_core::auth` and the admin layer before deploying.

## Crates

- [`rustio-cli`](rustio-cli/) — the `rustio` binary, installed via `cargo install rustio-cli`.
- [`rustio-core`](rustio-core/) — runtime library (server, router, ORM, admin, migrations).
- [`rustio-macros`](rustio-macros/) — procedural macros.

## License

MIT
