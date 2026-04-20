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
rustio init clinic_queue --preset basic
cd clinic_queue
rustio migrate apply
rustio user create --email you@example.com --password secret --role admin
rustio run
```

Then open [http://127.0.0.1:8000/admin](http://127.0.0.1:8000/admin) and sign in.

### Evolve the schema with the AI layer

```bash
rustio new app patient
rustio migrate apply

rustio ai plan "add date_of_birth as DateTime to patients" --save plan.json
rustio ai review plan.json          # inspect risk, impact, warnings
rustio ai apply plan.json --yes     # writes models.rs + migration
rustio migrate apply
rustio schema
```

The planner, review layer, and executor all read `rustio.schema.json` — the single stable contract for every AI-driven change.

---

## What's shipped

| capability | since | how it works |
|---|---|---|
| **HTTP + router** | 0.3 | hyper-backed server, `:param` path matching, 401/405 discrimination |
| **Middleware** | 0.3 | `async fn(Request, Next) -> Result<Response, Error>` · typed per-request context |
| **Admin UI** | 0.3 | `#[derive(RustioAdmin)]` generates list / create / edit / delete + an index at `/admin` |
| **Real auth** | 0.4 | `argon2` password hashing, DB-backed sessions, login at `/admin` |
| **ORM** | 0.3 | `Model` trait over SQLite via `sqlx` (hidden from user code) |
| **Field types** | 0.4 | `i32 · i64 · String · bool · DateTime<Utc> · Option<T>` |
| **`rustio.schema.json`** | 0.4 | The machine-readable contract every AI tool reads |
| **AI planner** | 0.5.0 | Rule-based grammar → typed [`Primitive`] vocabulary, refuses rather than guesses |
| **Plan review** | 0.5.1 | Risk / impact / warnings derived deterministically from a saved plan |
| **Safe executor** | 0.5.2 | Applies plans to the project tree atomically; destructive ops refuse without explicit flags |
| **Advanced mutations** | 0.5.3 | SQLite recreate-table for `change_field_type`, `change_field_nullability`, `rename_model` |
| **Context layer** | 0.6 | `rustio.context.json` carries country / industry / compliance; drives PII detection and policy refusals |
| **Admin intelligence** | 0.7 | Role classification, filters, search intent, masking — inferred from `(field, context)` |
| **Runtime schema cache** | 0.7.3 | Dashboard re-reads the schema without a restart |
| **Relations (additive)** | 0.8.0 | `link X to Y` grammar, `belongs_to` FK column (no SQL FK constraint until 0.9.0) |
| **Migrations** | 0.3 | Versioned `.sql` in `migrations/`, tracked, transactional, idempotent |
| **CLI** | 0.3+ | `init`, `new app`, `migrate`, `run`, `schema`, `ai plan/review/apply`, `context show`, `user create` |
| **Deploy** | 0.3 | One ~15 MB binary, sub-50 ms cold start, 10–30 MB resident memory |

---

## Where we're going

Three phases, in strict order. See [**ROADMAP.md**](ROADMAP.md) for the full plan.

### Phase 1 · Foundation — shipped in 0.4.x ✅

Typed core, real auth, `rustio.schema.json` as the external contract.

### Phase 2 · Intelligence — shipped across 0.5.x–0.8.0 ✅

- **Planner** (0.5.0) — rule-based grammar over a fixed `Primitive` vocabulary. No free-form code generation.
- **Plan review** (0.5.1) — risk / impact / warnings, purely deterministic.
- **Safe executor** (0.5.2) — atomic apply, destructive ops refuse without explicit flags.
- **Advanced mutations** (0.5.3) — type change, nullability change, rename model via SQLite recreate-table.
- **Context layer** (0.6.0) — country / industry / compliance drives PII detection and policy refusals.
- **Admin intelligence** (0.7.0–0.7.3) — role classification, filters, search intent, masking, runtime schema cache.
- **Relations** (0.8.0) — `link X to Y` grammar; `belongs_to` adds an i64 FK column. SQL `FOREIGN KEY` enforcement lands in 0.9.0.

### Phase 3 · Systems — v1.0.0 and beyond

`rustio init <system>` scaffolds a running vertical solution, not a blank project.

- `clinic`, `crm`, `inventory`, `workflow`, `registry`.
- Each is a complete project + apps + migrations + admin configuration, built on top of the Phase 2 primitives.

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
| `rustio new project <name>` | Create a project directly (no wizard) |
| `rustio new app <name>` | Scaffold an app inside the current project |
| `rustio migrate generate <n>` | Create a migration file |
| `rustio migrate apply [-v]` | Apply pending migrations |
| `rustio migrate status` | Show applied + pending migrations |
| `rustio schema` | Write `rustio.schema.json` from the compiled admin |
| `rustio run` | Build and run the project in the current directory |
| `rustio user create ...` | Create a real user in the auth tables |
| `rustio ai plan "<prompt>" [--save P]` | Plan a schema change (no side effects) |
| `rustio ai review <plan>` | Inspect a saved plan against the current schema |
| `rustio ai validate <plan>` | Terse validate-only gate for CI |
| `rustio ai apply <plan> [--yes]` | Apply a reviewed plan (writes files, never runs migrations) |
| `rustio context show` | Show the parsed `rustio.context.json` + inferred flags |
| `rustio context validate` | Parse context; exit 0 on success |
| `rustio --version` | Print the CLI version |

### AI grammar (0.5.0 → 0.8.0)

Prompts the planner understands today — all rule-based, deterministic, and refuse-first:

```text
add <field> [as <type>] to <model>
remove <field> from <model>
rename <from> to <to> in <model>
change <field> to <type> in <model>
make <field> optional in <model>
make <field> required in <model>
rename model <From> to <To>
add relation from <From> to <To>   # 0.8.0
link <From> to <To>                # 0.8.0 synonym
connect <From> to <To>             # 0.8.0 synonym
```

---

## 🔐 Authentication

Production-grade from the Foundation phase:

- **Browser flow.** `/admin` shows an email + password form. Successful sign-in sets an `HttpOnly; SameSite=Strict` session cookie validated on every request.
- **Passwords.** `argon2id` with project-salted hashes; the column is marked `editable: false` so no admin UI surfaces it.
- **Sessions.** DB-backed (`rustio_sessions`) with expiry and CSRF tokens on every mutating admin action.
- **Create users.** `rustio user create --email … --password … --role admin|user` (or run interactively).

No dev tokens. The `User` model is `core: true` — the schema exposes its shape but the admin router does not expose CRUD routes for it (modifications go through the CLI).

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

This struct is the entire model. The admin UI at `/admin/tasks` — list, create, edit, delete, plus an entry on the admin index — is generated from it. The same struct is what the schema exporter reads to produce `rustio.schema.json`, and that file is the only surface the AI layer touches.

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
