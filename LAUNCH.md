# RustIO Launch Messages

Drafts for the channels listed in the launch brief. Copy, adjust tone, post manually.

---

## Hacker News (Show HN)

**Title:**
Show HN: RustIO – Django-like developer experience, built for Rust

**Body:**
Hi HN,

I built RustIO because I wanted Django's productivity in Rust without giving up type-safety or runtime performance.

Fresh install to running admin in 4 commands:

    cargo install rustio-cli
    rustio new project mysite && cd mysite
    rustio new app blog
    rustio migrate apply && rustio run

Open `http://127.0.0.1:8000/` for the homepage.
Open `/admin/blogs` with `Authorization: Bearer dev-admin` — a full CRUD admin generated from a struct via `#[derive(RustioAdmin)]`, backed by SQLite.

What's in 0.1.0:
- HTTP server, router, middleware chain with typed request context
- Error model mapped to 4xx/5xx
- Auth middleware + `require_auth` / `require_admin` helpers
- ORM over SQLite (sqlx hidden from user code; no raw SQL in CRUD)
- Migrations (versioned .sql files, tracked, transactional)
- CLI: scaffold projects/apps, apply migrations, run
- Macro-generated admin UI

This is not a thin wrapper over an existing framework. HTTP is hyper directly; everything else (router, middleware, ORM facade, admin, migrations) is purpose-built.

Still early — APIs will change before 1.0. Pre-production only. Feedback wanted.

Repo: https://github.com/abdulwahed-sweden/rustio
Crates: rustio-cli, rustio-core, rustio-macros

---

## Reddit — r/rust

**Title:**
[Release] RustIO 0.1.0 — Django-like framework with auto-generated admin

**Body:**
First public release. RustIO is an opinionated, batteries-included web framework with a single design goal: Django productivity, built from scratch for Rust.

Quick start:

    cargo install rustio-cli
    rustio new project mysite
    cd mysite
    rustio new app blog
    rustio migrate apply
    rustio run

Then visit `/admin/blogs` with `Authorization: Bearer dev-admin` for an auto-generated CRUD admin. Deriving `RustioAdmin` + implementing `Model` is all the code you write:

```rust
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
    // from_row, id, insert_values
}
```

Architecture highlights:
- HTTP: hyper directly (no framework wrapper)
- Router: custom, `:param` paths, 404/405 distinction
- Middleware: `async fn(Request, Next) -> Result<Response, Error>`, short-circuit-friendly
- Context: typed HashMap keyed by TypeId, no Clone bound
- Errors: one enum, unified pipeline, middleware sees errors and can transform
- ORM: `Model` trait, sqlx hidden, primitives `i32 | i64 | String | bool`
- Admin: macro emits metadata only; runtime library renders HTML + handles forms
- Migrations: `.sql` files, `rustio_migrations` tracking table, transactional

Status: 0.1.0, early alpha. Dev defaults throughout (dev tokens, in-memory admin auth, no CSRF). Not for production. Feedback, issues, and PRs welcome.

Repo: https://github.com/abdulwahed-sweden/rustio

---

## Reddit — r/webdev

**Title:**
RustIO — Django's experience in Rust: type-safe, single-binary, with auto-generated admin

**Body:**
Just released RustIO 0.1.0. It's a Rust web framework with Django-style ergonomics: define a struct, get a working admin UI and REST-ish CRUD.

Install once:

    cargo install rustio-cli

Then:

    rustio new project mysite
    cd mysite
    rustio new app blog
    rustio migrate apply
    rustio run

That's it. Homepage at `/`, admin at `/admin/blogs`.

Why Rust?
- Single binary deploy (no Python venv, no node_modules)
- Compile-time schema checks (rename a field, compiler points at every broken query)
- 10–20 MB RAM instead of 200+

Status: early alpha. Looking for feedback. Repo: https://github.com/abdulwahed-sweden/rustio

---

## Twitter / X

Option A (short):

> Shipped RustIO 0.1.0 — a batteries-included web framework for Rust with auto-generated admin.
>
> `cargo install rustio-cli && rustio new project mysite`
>
> Django DX, Rust performance. Early alpha — feedback welcome.
>
> https://github.com/abdulwahed-sweden/rustio

Option B (thread):

1/
> RustIO 0.1.0 is out. Django-like developer experience, built from scratch for Rust. Install + scaffold + run in 4 commands, get a working CRUD admin backed by SQLite.

2/
> `#[derive(RustioAdmin)]` on a struct → list page, create/edit forms, delete action, auto-routed at `/admin/<table>`. No HTML, no handlers to write.

3/
> Core: HTTP server (hyper), custom router, middleware with typed context, unified error model, auth, ORM facade over sqlx, migrations. No framework wrapping — everything is purpose-built.

4/
> Early alpha. Pre-production only. Feedback wanted.
> Repo: https://github.com/abdulwahed-sweden/rustio
> Crates: rustio-cli / rustio-core / rustio-macros

---

## Notes before posting

- Double-check the admin URL wording on each platform — `Authorization: Bearer dev-admin` is dev-only; don't make it sound like a production auth scheme.
- Respond to "but why not use axum + sea-orm + …" with: yes, you could glue it yourself; RustIO's bet is that most projects never should have to.
- Be ready to answer: CSRF? Session auth? Production hardening? Answer: all phase-2.
