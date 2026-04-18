<p align="center">
  <a href="https://crates.io/crates/rustio-cli">
    <img alt="rustio-cli on crates.io" src="https://img.shields.io/crates/v/rustio-cli?style=for-the-badge&color=orange&label=rustio-cli">
  </a>
  <a href="https://docs.rs/rustio-core">
    <img alt="rustio-core on docs.rs" src="https://img.shields.io/docsrs/rustio-core?style=for-the-badge&color=blue&label=docs.rs">
  </a>
  <a href="https://github.com/abdulwahed-sweden/rustio/actions/workflows/ci.yml">
    <img alt="CI status" src="https://img.shields.io/github/actions/workflow/status/abdulwahed-sweden/rustio/ci.yml?style=for-the-badge&label=ci">
  </a>
  <img alt="early alpha" src="https://img.shields.io/badge/status-early%20alpha-yellow?style=for-the-badge">
  <img alt="rust version: 1.75+" src="https://img.shields.io/badge/rust-1.75%2B-dea584?style=for-the-badge">
  <img alt="MIT license" src="https://img.shields.io/badge/license-MIT-black?style=for-the-badge">
</p>

<p align="center">
  <b>The fastest way to build real systems — and evolve them safely with AI.</b>
</p>

---

RustIO is a system builder with a strict, typed core. The shape of every model, field, and relationship is captured in a deterministic, machine-readable schema. That determinism is the foundation for both the admin layer you see today and the AI-assisted extension layer coming next.

This is not a clone of Django. It is a different kind of tool: one that treats your Rust struct as the complete definition of a working system, and that a well-scoped AI can safely extend.

---

## 🚀 Quick Start

```bash
cargo install rustio-cli
rustio init
```

`rustio init` opens a four-prompt wizard:

```text
  RustIO
  Let's set up your project.

> Project name: taskwire
> Choose a starting preset:
    Basic — empty project, add apps later
  › Blog  — scaffolds one app with admin + views
    API   — scaffolds one app with admin + views
> What should your first model track? tasks
> Proceed? (Y/n)
```

Then:

```bash
cd taskwire
rustio migrate apply
rustio run
```

Open these:

- [http://127.0.0.1:8000/](http://127.0.0.1:8000/) — homepage
- [http://127.0.0.1:8000/tasks](http://127.0.0.1:8000/tasks) — your app's tutorial view
- [http://127.0.0.1:8000/admin](http://127.0.0.1:8000/admin) — sign-in form → admin index

To sign in from a browser: type **`dev-admin`** and press Enter. You're in.

For non-interactive scaffolding:

```bash
rustio init taskwire --preset blog --app tasks
```

---

## What's shipped in 0.3.1

| capability | how it works |
|---|---|
| **HTTP + router** | hyper-backed server, `:param` path matching, 401/405 discrimination |
| **Middleware** | `async fn(Request, Next) -> Result<Response, Error>` · typed per-request context |
| **Admin** | `#[derive(RustioAdmin)]` generates list / create / edit / delete + an index at `/admin` |
| **Browser auth** | Sign-in form at `/admin`, `HttpOnly; SameSite=Strict` session cookie |
| **API auth** | `Authorization: Bearer <token>` — works alongside the cookie flow |
| **ORM** | `Model` trait over SQLite via `sqlx` (hidden from user code) |
| **Field types** | `i32 · i64 · String · bool` (DateTime, Option, Uuid land in 0.4.0) |
| **Migrations** | Versioned `.sql` in `migrations/`, tracked, transactional, idempotent |
| **Production guard** | `RUSTIO_ENV=production` disables dev tokens entirely |
| **CLI** | `init` wizard · `new project` · `new app` · `migrate generate/apply/status` · `run` |
| **Deploy** | One ~15 MB binary, sub-50 ms cold start, 10–30 MB resident memory |

---

## Where we're going

Three phases, in strict order. See [**ROADMAP.md**](ROADMAP.md) for the full plan.

### Phase 1 · Foundation — v0.4.0

Make the core deterministic, typed, and schema-exportable.

- Production-grade auth: built-in `User` table, `argon2` passwords, DB-backed sessions, login form upgraded to email + password.
- Core types: `DateTime<Utc>`, `Option<T>`.
- **`rustio.schema.json`** — a stable, machine-readable description of every model, field, type, and admin behavior in the project. This file is the contract the AI layer will consume.

### Phase 2 · Intelligence — v0.5.0

Make the system safely extensible by AI agents.

- `rustio ai "add a published bool to Post"` — reads the schema, emits a reviewable diff, runs `cargo build`, writes only if it compiles.
- A fixed set of edit primitives (add-field, add-model, add-relation, add-migration, add-admin-attribute). No free-form code generation.
- Relations: `belongs_to`, `has_many`. Inline forms in admin. Reflected into the schema.

### Phase 3 · Systems — v0.6.0 and beyond

`rustio init <system>` scaffolds a running vertical solution, not a blank project.

- `clinic`, `crm`, `inventory`, `workflow`, `registry`.
- Each is a complete project + apps + migrations + admin configuration.
- Built only after Phases 1 and 2 are final.

---

## Design principle

Every feature must answer:

> *Does it make building a real system faster, clearer, or safer?*

If the answer is no, it does not belong in RustIO.

---

## What RustIO is NOT

- Not a Django clone.
- Not a generic framework (Axum / Actix / Rocket cover that space).
- Not a microframework — opinionated defaults are the product.
- Not a frontend framework.
- Not a sync framework.
- Not an AI toy. The AI layer is enabled by the typed core; it is not the core.

---

## 📖 Commands

| command | what it does |
|---|---|
| `rustio init` | Interactive wizard: name + preset + model + confirm |
| `rustio init <name>` | Non-interactive scaffold (default preset: `basic`) |
| `rustio init <name> --preset P` | Non-interactive with a preset (`basic` / `blog` / `api`) |
| `rustio init <name> --app X` | Override the scaffolded app name |
| `rustio new project <name>` | Create a project directly (no wizard) |
| `rustio new app <name>` | Scaffold an app inside the current project |
| `rustio migrate generate <n>` | Create a migration file |
| `rustio migrate apply [-v]` | Apply pending migrations |
| `rustio migrate status` | Show applied + pending migrations |
| `rustio run` | Build and run the project in the current directory |
| `rustio --version` | Print the CLI version |

---

## 🔐 Authentication

RustIO ships with a development auth layer so the admin is usable from minute one. Two entry points share one token mapping:

- **Browser flow.** `/admin` shows a sign-in form. Submit `dev-admin` or `dev-user`; an `HttpOnly; SameSite=Strict` cookie is set and subsequent requests authenticate via the cookie.
- **API / curl flow.** `Authorization: Bearer dev-admin`.

Both paths work in parallel. Setting `RUSTIO_ENV=production` disables the built-in dev tokens — a real auth middleware is required in that mode.

Phase 1 replaces the dev tokens with a real `User` table, `argon2` password hashing, and session storage. The form shape stays; only what's behind it changes.

---

## ♻️ Starting fresh

The default SQLite database is a single file (`app.db`) in the project root. Migrations are **idempotent** and tracked in the `rustio_migrations` table. To reset:

```bash
rm app.db
rustio migrate apply
```

Your schema lives in the `.sql` files in `migrations/` — the source of truth. Deleting `app.db` drops rows, not code.

---

## 📦 Installation

```bash
cargo install rustio-cli
```

Installs the `rustio` binary to `~/.cargo/bin/rustio`. Generated projects depend on the matching `rustio-core` from crates.io.

---

## Example model

```rust
use rustio_core::{Error, Model, Row, RustioAdmin, Value};

#[derive(RustioAdmin)]
pub struct Task {
    pub id: i64,
    pub title: String,
    pub is_active: bool,
    pub priority: i32,
}

impl Model for Task {
    const TABLE: &'static str = "tasks";
    const COLUMNS: &'static [&'static str] = &["id", "title", "is_active", "priority"];
    const INSERT_COLUMNS: &'static [&'static str] = &["title", "is_active", "priority"];

    fn id(&self) -> i64 { self.id }

    fn from_row(row: Row<'_>) -> Result<Self, Error> {
        Ok(Self {
            id: row.get_i64("id")?,
            title: row.get_string("title")?,
            is_active: row.get_bool("is_active")?,
            priority: row.get_i32("priority")?,
        })
    }

    fn insert_values(&self) -> Vec<Value> {
        vec![self.title.clone().into(), self.is_active.into(), self.priority.into()]
    }
}
```

This struct is the entire model. The admin UI at `/admin/tasks` — list, create, edit, delete, plus an entry on the admin index — is generated from it. Once 0.4.0 ships, this same struct will also be the content of `rustio.schema.json`, which is what the AI layer will read.

---

## 🏗 Generated project structure

```
mysite/
├── Cargo.toml
├── README.md
├── main.rs                    # entry point (top-level by convention)
├── apps/
│   ├── mod.rs                 # aggregator — builds the Admin + mounts views
│   └── tasks/
│       ├── mod.rs
│       ├── models.rs          # struct + Model + RustioAdmin derive
│       ├── admin.rs           # `install(admin)` — adds this app to the index
│       └── views.rs           # tutorial page + your custom routes
├── migrations/                # versioned .sql files
├── static/                    # static assets
├── templates/                 # template directory
└── app.db                     # SQLite (gitignored)
```

---

## ⚙️ Configuration

| variable | purpose |
|---|---|
| `RUSTIO_DATABASE_URL` | Database URL (default `sqlite://app.db?mode=rwc`) |
| `RUSTIO_ENV` | Set to `production` (or `prod`) to disable built-in dev tokens |
| `NO_COLOR` | Disable colored CLI output |
| `RUSTIO_CORE_PATH` | Use a local `rustio-core` path in generated projects (for contributors) |

---

## 🧰 Crates

- [`rustio-cli`](https://crates.io/crates/rustio-cli) — the `rustio` binary
- [`rustio-core`](https://crates.io/crates/rustio-core) — runtime library
- [`rustio-macros`](https://crates.io/crates/rustio-macros) — procedural macros (`#[derive(RustioAdmin)]`)

---

## 🤝 Contributing

See [`CONTRIBUTING.md`](CONTRIBUTING.md). Required checks before opening a PR:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
```

CI runs the same checks on every push. For Phase 1 work, please align on design first — open an issue before writing code.

---

## 🛡 Security

For vulnerability reports, see [`SECURITY.md`](SECURITY.md). Do **not** open a public issue for security problems.

---

## 📜 License

MIT
