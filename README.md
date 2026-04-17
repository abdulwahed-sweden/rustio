<p align="center">
  <img src="https://img.shields.io/crates/v/rustio-cli?style=for-the-badge&color=orange" />
  <img src="https://img.shields.io/crates/d/rustio-cli?style=for-the-badge&color=blue" />
  <img src="https://img.shields.io/badge/status-stable-brightgreen?style=for-the-badge" />
  <img src="https://img.shields.io/badge/license-MIT-black?style=for-the-badge" />
</p>

<p align="center">
  <b>Django-like developer experience — powered by Rust.</b>
</p>

---

## 🚀 Quick Start

```bash
cargo install rustio-cli
rustio new project mysite
cd mysite
rustio new app blog
rustio migrate apply
rustio run
```

Open [http://127.0.0.1:8000/](http://127.0.0.1:8000/).

Admin at [http://127.0.0.1:8000/admin/blogs](http://127.0.0.1:8000/admin/blogs) — send `Authorization: Bearer dev-admin`.

## ✨ Features

- **Built-in admin** — `#[derive(RustioAdmin)]` gives you list, create, edit, delete.
- **ORM** — type-safe models over SQLite, no raw SQL in your code.
- **Migrations** — versioned, tracked, transactional.
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
