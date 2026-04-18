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

RustIO is a small, opinionated web framework that gives you Django's "scaffold a project, get a working admin, ship a feature in an afternoon" workflow — built from scratch for Rust.

One installer. One command. A real server, a real database, a real admin, with the safety of the Rust compiler underneath.

---

## 🚀 Quick Start

```bash
cargo install rustio-cli
rustio init
```

That's the whole setup. `rustio init` opens a four-prompt wizard:

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

Open these in your browser:

- [http://127.0.0.1:8000/](http://127.0.0.1:8000/) — homepage
- [http://127.0.0.1:8000/tasks](http://127.0.0.1:8000/tasks) — your app's tutorial view
- [http://127.0.0.1:8000/admin](http://127.0.0.1:8000/admin) — sign-in form, then the admin

To sign in: type **`dev-admin`** and press Enter. You're in.

### Prefer flags over prompts?

```bash
rustio init taskwire --preset blog --app tasks
```

Or use the granular commands the wizard builds on:

```bash
rustio new project taskwire
cd taskwire
rustio new app tasks
rustio migrate apply
rustio run
```

---

## ✨ What you get

| capability | shipped today | how it works |
|---|---|---|
| **Interactive setup** | ✅ | `rustio init` — name, preset, model, confirm |
| **Built-in admin** | ✅ | `#[derive(RustioAdmin)]` → list / create / edit / delete + an index page at `/admin` |
| **Browser sign-in** | ✅ | `/admin` shows a sign-in form; submit a token, get an `HttpOnly` cookie |
| **Bearer auth** | ✅ | `Authorization: Bearer <token>` for API/curl callers — both paths work side by side |
| **ORM** | ✅ | `Model` trait over SQLite via `sqlx` (sqlx hidden from your code). `User::find(&db, id).await` style API |
| **Migrations** | ✅ | Versioned `.sql` files in `migrations/`, tracked in `rustio_migrations`, transactional + idempotent |
| **Production guard** | ✅ | `RUSTIO_ENV=production` disables dev tokens entirely |
| **Single binary deploy** | ✅ | Your whole app compiles to one ~15 MB executable |
| **Auto migrations from model diffs** | 🛠 0.6.0 | See [ROADMAP.md](ROADMAP.md) |
| **Relations (ForeignKey, has_many)** | 🛠 0.5.0 | See [ROADMAP.md](ROADMAP.md) |
| **`DateTime`, `Option<T>`, `Uuid`** | 🛠 0.4.0–0.6.0 | See [ROADMAP.md](ROADMAP.md) |
| **Real user accounts (passwords, sessions)** | 🛠 0.4.0 | See [ROADMAP.md](ROADMAP.md) |
| **PostgreSQL** | 🛠 0.8.0 | See [ROADMAP.md](ROADMAP.md) |

The full plan — what's next, what's deliberately out of scope, and the version-by-version horizon to 1.0 — is in [**ROADMAP.md**](ROADMAP.md).

---

## 🧠 Philosophy

- **Simplicity.** One obvious way to do each thing. No plumbing.
- **Performance.** No framework layers hiding the hot path. ~50–100× more throughput than Django, ~10× less memory, sub-50 ms cold start.
- **Type safety.** The compiler catches what Django catches at runtime. Rename a column and the build breaks at the read site.
- **Single-binary deploy.** No virtualenv, no `pip install`, no Python version games. `cargo build --release && scp` is the deploy script.

---

## 📖 Commands

| command | what it does |
|---|---|
| `rustio init` | Interactive wizard: name + preset + model + confirm |
| `rustio init <name>` | Non-interactive scaffold (default preset: `basic`) |
| `rustio init <name> --preset P` | Non-interactive with a preset (`basic` / `blog` / `api`) |
| `rustio init <name> --app X` | Override the scaffolded app name (e.g. `books`, `tasks`, `links`) |
| `rustio new project <name>` | Create a project directly (no wizard) |
| `rustio new app <name>` | Scaffold an app inside the current project |
| `rustio migrate generate <n>` | Create a new migration file |
| `rustio migrate apply [-v]` | Apply pending migrations (`-v` prints each statement) |
| `rustio migrate status` | Show applied + pending migrations |
| `rustio run` | Build and run the project in the current directory |
| `rustio --version` | Print the CLI version |

---

## 🔐 Authentication

RustIO ships with a development auth layer so you have a working admin from minute one. There are **two ways to authenticate**, both backed by the same token mapping:

```rust
let id = require_auth(req.ctx())?;    // 401 if missing
let id = require_admin(req.ctx())?;   // 401 if missing, 403 if not admin
```

- **Browser flow.** Visit `/admin`, you'll see a sign-in form. Submit a token (`dev-admin` or `dev-user`) and an `HttpOnly; SameSite=Strict` cookie is set. All subsequent requests authenticate via the cookie.
- **API / curl flow.** Send `Authorization: Bearer dev-admin`. No cookie required.

Both paths work simultaneously. Real session-based auth (a `User` table, password hashing, password reset) lands in 0.4.0 — see [ROADMAP.md](ROADMAP.md).

`RUSTIO_ENV=production` disables the dev tokens entirely. A real auth middleware is required to recognise any tokens in that mode; the `authenticate` middleware logs a one-time warning if you forget.

---

## ♻️ Starting Fresh

The default SQLite database is a single file (`app.db`) in the project root. Migrations are **idempotent** and tracked in the `rustio_migrations` table. To reset:

```bash
rm app.db
rustio migrate apply
```

Your schema (the `.sql` files in `migrations/`) is the source of truth; deleting `app.db` only drops rows, never code.

---

## 📦 Installation

```bash
cargo install rustio-cli
```

This installs the `rustio` binary to `~/.cargo/bin/rustio`. Generated projects depend on the matching `rustio-core` from crates.io.

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

This is the entire model. The admin UI at `/admin/tasks` (list, create, edit, delete + a `Task` entry on the index page) is generated from it. No HTML, no routing, no form handling to write.

---

## 🏗 Generated project structure

```
mysite/
├── Cargo.toml
├── README.md
├── main.rs                    # entry point (top-level by convention)
├── apps/
│   ├── mod.rs                 # aggregator: builds the Admin + mounts views
│   └── tasks/
│       ├── mod.rs
│       ├── models.rs          # struct + Model + RustioAdmin derive
│       ├── admin.rs           # `install(admin)` — adds this app to the index
│       └── views.rs           # tutorial page + your custom routes
├── migrations/                # versioned .sql files
├── static/                    # static asset directory (you wire it up)
├── templates/                 # template directory (you wire it up)
└── app.db                     # SQLite (gitignored)
```

---

## 🗺 Roadmap

The full plan is in [**ROADMAP.md**](ROADMAP.md). The headline targets, ordered by impact:

- **0.4.0** — Real user auth (User table, argon2, sessions, password reset) + `DateTime`
- **0.5.0** — Relations (ForeignKey / has_many) + admin inline forms
- **0.6.0** — Auto-generated migrations + `Option<T>`, `f64`, `Uuid`
- **0.7.0** — Admin search + filter + pagination + field attributes
- **0.8.0** — PostgreSQL + form validation framework
- **0.9.0** — File uploads + JSON request body + CSRF tokens
- **1.0.0** — API freeze + hot reload + test client + structured logging + docs site

---

## ⚙️ Configuration

| Variable | Purpose |
|---|---|
| `RUSTIO_DATABASE_URL` | Database URL (default `sqlite://app.db?mode=rwc`) |
| `RUSTIO_ENV` | Set to `production` (or `prod`) to disable built-in dev tokens |
| `NO_COLOR` | Disable colored CLI output |
| `RUSTIO_CORE_PATH` | Use a local `rustio-core` path in generated projects (for RustIO contributors only) |

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

CI runs the same checks on every push. For Tier 0 items in the roadmap, please open a design issue or email before starting — those need alignment.

---

## 🛡 Security

For vulnerability reports, see [`SECURITY.md`](SECURITY.md). Do **not** open a public issue for security problems.

---

## 📜 License

MIT
