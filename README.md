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
  <b>Django-like developer experience — powered by Rust.</b>
</p>

---

## 🚀 Quick Start

```bash
cargo install rustio-cli
rustio init
```

**That's the whole setup.** `rustio init` is the doorway into RustIO — one command opens an interactive wizard that takes you from nothing to a running project with a working admin, a persisted model, and tracked migrations.

```text
  RustIO
  Let's set up your project.

> Project name: mysite
> Choose a starting preset:
    Basic — empty project, add apps later
  › Blog  — scaffolds a posts app with admin + views
    API   — scaffolds an items app with admin + views
> Proceed? (Y/n)
```

Three prompts. One confirm. Then:

```bash
cd mysite
rustio migrate apply
rustio run
```

- [http://127.0.0.1:8000/](http://127.0.0.1:8000/) — your homepage
- [http://127.0.0.1:8000/admin](http://127.0.0.1:8000/admin) — auto-generated admin (send `Authorization: Bearer dev-admin`)

### Prefer flags over prompts?

Everything the wizard does is reachable non-interactively:

```bash
rustio init mysite --preset blog    # same result, zero prompts
```

Or use the granular commands the wizard builds on:

```bash
rustio new project mysite
cd mysite
rustio new app posts
rustio migrate apply
rustio run
```

## ✨ Features

- **Interactive wizard** — `rustio init` walks you through project setup in three prompts.
- **Built-in admin** — `#[derive(RustioAdmin)]` gives you list, create, edit, delete, and an index at `/admin`.
- **ORM** — type-safe models over SQLite, no raw SQL in your code.
- **Migrations** — versioned, tracked, transactional; `rustio migrate apply` / `status` / `generate`.
- **Zero-config** — one command to scaffold, one to run.
- **Single binary** — your whole app compiles to one executable.

## 🧠 Philosophy

- **Simplicity.** One obvious way to do each thing. No plumbing.
- **Performance.** No framework layers hiding the hot path.
- **Type safety.** The compiler catches what Django catches at runtime.

## 🏗 Project Structure

```
mysite/
├── Cargo.toml
├── main.rs
├── apps/
│   └── blog/
│       ├── models.rs
│       ├── admin.rs
│       └── views.rs
├── migrations/
├── static/
├── templates/
└── app.db
```

## 📖 Commands

| Command                         | What it does                                                         |
| ------------------------------- | -------------------------------------------------------------------- |
| `rustio init`                   | Interactive wizard: name + preset + confirm                          |
| `rustio init <name>`            | Non-interactive scaffold (default preset: `basic`)                   |
| `rustio init <name> --preset P` | Non-interactive with a preset (`basic` / `blog` / `api`)             |
| `rustio new project <name>`     | Create a new project directly (no wizard)                            |
| `rustio new app <name>`         | Scaffold an app inside the current project                           |
| `rustio migrate generate <n>`   | Create a new migration file                                          |
| `rustio migrate apply [-v]`     | Apply pending migrations (`-v` prints each statement)                |
| `rustio migrate status`         | Show applied and pending migrations                                  |
| `rustio run`                    | Build and run the project in the current directory                   |
| `rustio --version`              | Print the CLI version                                                |

## 🔐 Authentication

Authentication is middleware-based. Identity lives in request context and handlers declare their own requirement:

```rust
let id = require_auth(req.ctx())?;    // 401 if missing
let id = require_admin(req.ctx())?;   // 401 if missing, 403 if not admin
```

Dev tokens (`dev-admin`, `dev-user`) are provided for bootstrapping. Replace with your own middleware before production.

## 📦 Installation

```bash
cargo install rustio-cli
```

## Example

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

The admin UI at `/admin/posts` is generated from this struct.

## Configuration

| Variable | Purpose |
|---|---|
| `RUSTIO_DATABASE_URL` | Database URL (default `sqlite://app.db?mode=rwc`) |
| `NO_COLOR` | Disable colored CLI output |

## Crates

- [`rustio-cli`](https://crates.io/crates/rustio-cli) — the `rustio` binary
- [`rustio-core`](https://crates.io/crates/rustio-core) — runtime library
- [`rustio-macros`](https://crates.io/crates/rustio-macros) — procedural macros

## License

MIT
