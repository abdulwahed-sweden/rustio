# RustIO — Roadmap

This document is the canonical plan for RustIO. It complements [`CHANGELOG.md`](CHANGELOG.md) (what shipped) by laying out **what's coming, why, and roughly when**. Items here are commitments to direction, not to specific dates — a release lands when its scope is correct, not on a calendar.

---

## Vision

> Django's developer experience, built from scratch for Rust.

RustIO exists to give a single developer the productivity of Django's "scaffold a project, get an admin, ship a feature in an afternoon" workflow, while keeping the operational properties of a compiled, statically-typed Rust binary: single-file deploy, ~10× less memory, ~50–100× more raw throughput, and zero classes of bugs that the compiler can rule out.

We do not aim to clone Django. We aim to be **the obvious choice** when a small team wants Django's velocity without Python's runtime cost.

---

## Where we are today (0.3.1)

- HTTP server (hyper) · custom router with `:param` paths
- Async middleware chain · typed per-request context
- Unified `Error` enum with HTTP status mapping
- Bearer + cookie auth, browser-friendly login form, production guard
- ORM over SQLite (sqlx hidden) supporting `i32 / i64 / String / bool`
- `#[derive(RustioAdmin)]` macro · admin index + list / create / edit / delete
- Migrations (versioned `.sql`, tracked, transactional, idempotent)
- CLI (`init` wizard, `new project`, `new app`, `migrate`, `run`)
- 117 tests · 3 published crates · documented in CHANGELOG.

This is a **complete, runnable prototype**. It is not yet a complete framework.

---

## How we read the gap

The framework is mature enough to scaffold a useful side project. It is not yet mature enough to back a real product. The honest gap, ordered by what blocks the most real-world use cases, is:

| gap | why it matters |
|---|---|
| No relations (`ForeignKey`, `belongs_to`, `has_many`) | Real schemas have joins; without them, an app is a single table |
| Limited field types (`i32 / i64 / String / bool` only) | No `DateTime`, `Option<T>`, `Decimal`, `Uuid`, `Json`, file types |
| No real user auth | Dev tokens only; no `User` table, no password hashing, no reset |
| Hand-written migrations | Django auto-generates from model diffs; we don't |
| SQLite only | Postgres / MySQL drivers needed for production deploys |
| Admin: no search / filter / pagination | Once a table has >50 rows, the admin becomes painful |
| No form validation framework | "Field is required" is the only validator |
| No file uploads, no JSON request body, no CSRF on cookie sessions | Each blocks specific real-world apps |

These are not weaknesses of the architecture — they are scope. The roadmap closes them.

---

## Roadmap by tier

We bucket work by impact, not by ease. **Tier 0 is non-negotiable** before RustIO can be recommended for production use. Each tier compounds the value of the one below.

### Tier 0 — required to graduate from prototype

| # | item | scope |
|---|---|---|
| T0-1 | **Relations** | `#[rustio(foreign_key = "user_id")]` + `belongs_to` / `has_many` helpers. Eager (`Post::with_user(&db)`) and lazy (`post.user(&db).await`) variants. Inline form support in admin. |
| T0-2 | **`DateTime` field type** | `chrono::DateTime<Utc>` in `Model`, rendered as `<input type="datetime-local">` in admin, stored as ISO-8601 in SQLite. |
| T0-3 | **`Option<T>` field type** | Nullable columns end-to-end: NULL in DB, `None` in Rust, empty input in admin. Removes the "every String must be non-empty" footgun. |
| T0-4 | **Real user auth** | `User` table baked in · `argon2` password hashing · login form with email + password · session storage in DB · "remember me" · password-reset email hook. Dev tokens stay as a development override. |

### Tier 1 — needed for any serious project

| # | item | scope |
|---|---|---|
| T1-1 | **Auto-generated migrations** | `rustio migrate make` reads model definitions, diffs against the live schema, writes the SQL. Django's killer feature. |
| T1-2 | **PostgreSQL support** | `Db::connect("postgres://…")` works. sqlx already supports it; mostly a feature-flag + driver-detection job. |
| T1-3 | **Admin search / filter / pagination** | `#[rustio(searchable, filter, list_display = ["title", "author"])]`. Pagination at 25 rows by default. |
| T1-4 | **Form validation framework** | `#[rustio(min_length = 3, max_length = 100, regex = "...")]`. Field-level errors rendered inline in the admin form. |
| T1-5 | **Field attributes** | `label`, `help_text`, `placeholder`, `widget`, `readonly`, `order`. Replaces the current "field name = column name = label" identity. |
| T1-6 | **`f64` and `Uuid` field types** | Round out the primitive set. |
| T1-7 | **JSON request body** | `Request::json::<T>()` extractor (serde-backed, behind a feature flag so projects that don't use serde don't pay for it). |
| T1-8 | **File uploads** | Multipart parsing. Without this, no admin can store images, attachments, or imports. |

### Tier 2 — production polish

| # | item | scope |
|---|---|---|
| T2-1 | **CSRF tokens on cookie sessions** | Per-session token in a hidden form field, validated on every POST. Combines with `SameSite=Strict` for defence in depth. |
| T2-2 | **Template engine integration** | Wire `Tera` or `Askama` cleanly. Hot-reload in dev. Generated `templates/` directory becomes useful. |
| T2-3 | **Background jobs** | A small "task queue with SQLite-backed retries" — not Celery, just enough to send emails or process uploads off the request thread. |
| T2-4 | **Email helper** | `rustio_core::mail::send(to, subject, body)` with SMTP transport. Templated bodies. |
| T2-5 | **Test client** | `TestClient::new(app).post("/admin/login", form).await` — equivalent to Django's `Client()`. Removes the need for `cargo run` + `curl` loops in tests. |
| T2-6 | **Hot reload** in `rustio run` | Watch source files, rebuild, restart. Cuts the 30-second feedback loop. |
| T2-7 | **Structured logging** | Integrate the `tracing` crate. Generated middleware writes one structured event per request. |

### Tier 3 — once we have users

| # | item | scope |
|---|---|---|
| T3-1 | **Admin actions** | Bulk delete, bulk update, custom actions per model. |
| T3-2 | **Admin history / audit log** | Track who edited what when. Surfaced as a tab on each row's edit page. |
| T3-3 | **i18n / l10n** | Fluent-rs or gettext-style. Admin UI translatable first; user code follows. |
| T3-4 | **Caching layer** | In-memory + Redis backends. Decorator-style `cache!(key, ttl, compute)`. |
| T3-5 | **`rustio shell`** | REPL with the DB connection ready (probably evcxr-based). |
| T3-6 | **Documentation site** | Real docs site (mdbook + custom theme), not just a README. Django's docs are half the reason Django is loved. |
| T3-7 | **Sub-routers / route groups** | Per-group middleware. Lets users nest admin under arbitrary prefixes, build versioned API trees, etc. |

---

## Roadmap by version

A realistic ~6-month horizon. Each release ships **two Tier-0 / Tier-1 items** plus polish. Order maximises the value compounding effect.

| version | scope (proposed) | unlocks |
|---|---|---|
| **0.4.0** | Real user auth (T0-4) · `DateTime` field type (T0-2) | First release that can host an app with real accounts |
| **0.5.0** | Relations (T0-1) · admin inline form support | Multi-table apps. The biggest single jump in capability. |
| **0.6.0** | `Option<T>` (T0-3) · `f64` + `Uuid` (T1-6) · auto-generated migrations (T1-1) | End of the "toy schema" era; migrations become Django-grade. |
| **0.7.0** | Admin search + filter + pagination (T1-3) · field attributes (T1-5) | Admin survives tables with thousands of rows. |
| **0.8.0** | Postgres support (T1-2) · form validation framework (T1-4) | Production-grade DB + production-grade input handling. |
| **0.9.0** | File uploads (T1-8) · JSON request body (T1-7) · CSRF (T2-1) | API and image-upload apps both viable. |
| **1.0.0** | API freeze · hot reload (T2-6) · test client (T2-5) · tracing (T2-7) · documentation site (T3-6) | Stable surface. Production-recommended. |

After 1.0 the cadence shifts to Tier 2 / Tier 3 features and ecosystem work (third-party crates, deployment guides, hosting recipes).

---

## Explicitly out of scope

Being clear about what RustIO **won't** do is as important as listing what it will. Choices below are deliberate; pull requests adding any of them will be politely declined unless the calculus changes.

- **Not a thin wrapper over an existing framework.** RustIO talks to hyper directly. We are not Axum-with-extras.
- **Not a microframework.** "Bring your own everything" is exactly the experience RustIO exists to *not* deliver. Opinionated defaults are the product.
- **Not all databases.** SQLite + PostgreSQL is the working set. MySQL might happen if a contributor really wants it; Oracle and SQL Server will not.
- **Not a clone of Django's API.** We borrow philosophy, not signatures. `User::find(&db, id).await` will never look like `User.objects.get(id=1)` — it should look like idiomatic Rust.
- **Not a synchronous framework.** Tokio-only. We will not maintain a sync variant.
- **Not a frontend framework.** Server-rendered HTML + plain forms. If you want SPA scaffolding, ship JSON from RustIO and use a separate frontend.

---

## Performance targets (for context)

Numbers we expect to maintain at and beyond 1.0, on commodity hardware (single Apple M2-class core, SQLite, no caching):

| metric | RustIO | Django (Gunicorn, sync) | Django (Uvicorn, async) |
|---|---|---|---|
| Throughput, simple endpoint | 50,000–200,000 req/s | 1,000–3,000 req/s | up to ~5,000 req/s |
| Memory (resident, idle) | 10–30 MB total | 100–300 MB per worker | similar |
| Cold start | <50 ms | 1–3 s | 1–3 s |
| Deploy artifact | one ~15 MB binary | venv + Python interpreter + packages (~200 MB) | same |

Performance is a property of the architecture, not a feature to be added later. Releases that regress these numbers will not ship.

---

## How to influence this roadmap

1. **Open an issue** describing a real use case the current scope blocks. Concrete > theoretical.
2. **Vote** by 👍 on existing issues; we look at it.
3. **Send a PR** referencing the relevant T-number. Match the existing test bar (lint clean, tests added, CHANGELOG updated).
4. For Tier 0 items, **email** [`abdulwahed.sweden@gmail.com`](mailto:abdulwahed.sweden@gmail.com) before starting — these need design alignment.

A good design question is more valuable than a good PR. Either is welcome.

---

## What to expect from each release

- **Patch releases (`0.x.y`)** — bug fixes, doc fixes, dependency bumps. No API changes.
- **Minor releases (`0.x.0`)** — new features per the table above. May contain breaking changes pre-1.0; the CHANGELOG documents the upgrade.
- **Major releases (`x.0.0`)** — only `1.0.0` is on the horizon; no further breaking changes after that without a strong reason.

---

*Last updated alongside the 0.3.1 release.*
