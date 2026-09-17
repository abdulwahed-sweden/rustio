<p align="center">
  <strong>RustIO</strong><br>
  Build real web/admin systems in Rust without rebuilding the boring foundation every time.
</p>

<p align="center">
  <a href="https://crates.io/crates/rustio-cli"><img alt="rustio-cli on crates.io" src="https://img.shields.io/crates/v/rustio-cli?label=rustio-cli&color=orange"></a>
  <a href="https://docs.rs/rustio-core"><img alt="rustio-core docs" src="https://img.shields.io/docsrs/rustio-core?label=docs.rs&color=blue"></a>
  <a href="https://github.com/abdulwahed-sweden/rustio/actions/workflows/ci.yml"><img alt="CI status" src="https://img.shields.io/github/actions/workflow/status/abdulwahed-sweden/rustio/ci.yml?label=ci"></a>
  <img alt="beta" src="https://img.shields.io/badge/status-beta-blueviolet">
  <img alt="MIT license" src="https://img.shields.io/badge/license-MIT-black">
  <a href="https://github.com/sponsors/abdulwahed-sweden?metadata_source=rustio&metadata_campaign=readme_top"><img alt="Sponsor RustIO" src="https://img.shields.io/badge/Sponsor-%E2%9D%A4-db61a2?logo=githubsponsors&logoColor=white"></a>
</p>

---

## What RustIO gives you

Define your data as Rust structs. RustIO gives you the foundation around it:

- admin UI
- database schema and migrations
- authentication and sessions
- HTTP server
- generated model views
- typed schema export
- plain-English schema evolution

The goal is simple: **you write the domain; RustIO handles the repetitive system plumbing.**

<p align="center">
  <a href="https://github.com/sponsors/abdulwahed-sweden?metadata_source=rustio&metadata_campaign=value_cta"><strong>❤️ Sponsor continued RustIO development</strong></a>
</p>

---

## Five-minute start

You need [Rust](https://rustup.rs/) installed. Nothing else.

```bash
# 1. Install the CLI
cargo install rustio-cli

# 2. Start a project
rustio init booklend
```

When `init` finishes, RustIO asks one question:

```text
✔ Created project "booklend"

  How do you want to start?

  > 1. Empty      — add your own models with `rustio add model`
    2. Template   — clinic, blog, shop, crm, tasks
```

**Empty** hands the project straight back to you. **Template** walks a
ready-made shape with you one model at a time — accept or skip each one;
nothing is written until you have seen the whole list.

Taking the Empty path:

```bash
cd booklend

# 3. Add your models — one folder per model
rustio add model book         # repeat for member, loan
rustio migrate apply

# 4. Shape the fields in plain English
rustio change "add author as String to Book"
rustio change "add relation from Loan to Book"

# 5. Make a login for yourself
rustio user create --email you@example.com --password secret --role admin

# 6. Start the server
rustio run
```

`rustio change` shows the change, asks before writing, then offers to apply
the migration in the same breath:

```text
  Ready to make this change:

    · add Book.author  (String, required)

? Apply? (Y/n) y
✔ wrote models/book/models.rs
✔ wrote migrations/0004_add_author_to_books.sql
? Apply the migration now? (Y/n) y
  ✔ applied 0004_add_author_to_books.sql
```

`rustio run` tells you where it is and who to sign in as:

```text
  booklend is running → http://127.0.0.1:8000/admin
  sign in as you@example.com
  Ctrl+C to stop
```

Open it, sign in, and you have a working admin for every model you defined.

> **Stuck?** Run `rustio doctor` from inside the project — it checks the
> common "why isn't this working" causes and names the command that fixes
> each one.

---

## What you just did

| Step | What actually happened |
|---|---|
| `rustio init booklend` | Scaffolded a Rust project with the framework wired up, then asked Empty or Template. The Template path walks a ready-made shape with you and writes a model plus a `CREATE TABLE` migration for every model you accept. |
| `rustio add model book` | Wrote `models/book/models.rs` (the struct — your source of truth), its admin registration, an empty views file, and a migration. Every model starts with `title`, `priority`, `is_active`; the CLI says so, so you never discover them by colliding with one. |
| `rustio migrate apply` | Ran the pending SQL migrations against `app.db` (SQLite, created on first run) and regenerated `rustio.schema.json`. |
| `rustio change "…"` | Parsed the sentence into typed schema operations, showed them, and — on your yes — edited the struct, wrote a migration, and offered to apply it. |
| `rustio user create …` | Inserted a row into `rustio_users` with an argon2-hashed password and the `admin` role. |
| `rustio run` | Checked the port was free, built, and served on `:8000` (`--port <n>` for another). `/admin/*` is gated by the auth middleware. |

---

## A small mental model

A RustIO project revolves around three things:

1. **Rust models** — the source of truth for your domain.
2. **Migrations** — explicit SQL changes to the database.
3. **`rustio.schema.json`** — the stable machine-readable contract used by tooling.

Everything else — admin screens, login flow, schema export, the setup menu —
exists to reduce repeated work around those three pieces.

Each model lives in its own folder: `models/<name>/models.rs` is the struct,
with `admin.rs` and `views.rs` beside it. (Projects scaffolded before 0.11
keep that folder named `apps/`; nothing is moved, and every command reads
whichever layout your project has.)

---

## Change the system safely

Describe a change:

```bash
rustio change "add date_of_birth as DateTime to notes"
```

RustIO proposes a typed change plan, shows the risk, and lets you review it before anything lands.

The change pipeline is deliberately constrained:

```text
request → typed plan → review → apply
```

`change` shows the change and asks before writing anything, then offers to
apply the migration it just wrote — so the schema on disk and the schema in
the database never drift apart while you remember a second command.

Five shapes are accepted:

```text
add <field> as <Type> to <Model>
rename <old> to <new> in <Model>
change <field> to <Type> in <Model>
add relation from <Model> to <Model>
remove <field> from <Model>
```

If a requested change cannot be represented safely in that vocabulary, RustIO
refuses rather than inventing an approximation — and a refusal prints the whole
grammar, so one "no" teaches the shape of every "yes".

---

## What makes it different

RustIO is intentionally opinionated:

- Rust-first, typed core
- async with Tokio
- generated admin instead of a separate frontend app
- explicit migrations
- schema-driven tooling
- human-reviewed change plans
- single-binary style deployment

It is **not** trying to replace Axum, Actix, or Rocket as a general web framework. It sits at a higher level for people who want to build operational systems faster.

Nor is it an AI gadget: the plain-English change pipeline works precisely because the core is strict and the vocabulary is closed — the friendliness is a surface, the type system is the substance.

---

## Useful commands

Type `rustio` with no arguments for one status line — project, models, pending
migrations, whether the server is up — and the commands most likely to be next.
Pass `--why` to any command for a short explanation without running it.

```bash
rustio                          # one status line + the likely next commands
rustio help                     # the full command list, grouped by purpose
rustio doctor                   # health check: migrations, schema, admin user, port
rustio explain <topic>          # short docs on `model`, `migration`, `admin`, `ai`, …

rustio init <name>              # new project, then Empty or a template
rustio start                    # reopen that menu inside an existing project
rustio add model <name>         # new model + admin entry + migration stub
rustio change "<change>"        # change the schema from plain English
rustio migrate apply            # apply pending migrations (regenerates the schema)
rustio migrate status           # what's applied, what's pending
rustio schema                   # regenerate rustio.schema.json
rustio run [--port <n>]         # build + serve on :8000
rustio user create [...]        # add a user (interactive when args missing)
```

Advanced schema-change workflow:

```bash
rustio ai plan "<change>" [--save PATH]
rustio ai review <plan>
rustio ai apply  <plan> [--yes] [--dry-run] [--force]
```

---

## Example project

The repository includes **[`examples/bookflow/`](examples/bookflow/)**, a multi-model booking system showing relationships, generated admin views, migrations, and seed data.

For deeper material, start with the [documentation site](https://rustio.vercel.app) or:

- [`docs/glossary.md`](docs/glossary.md)
- [`docs/cli.md`](docs/cli.md)
- [`docs/design-system.md`](docs/design-system.md)
- [`docs/advanced/composition-editor-and-i18n.md`](docs/advanced/composition-editor-and-i18n.md)
- [`docs/advanced/`](docs/advanced/)
- [`ROADMAP.md`](ROADMAP.md)
- [`CHANGELOG.md`](CHANGELOG.md)
- [`CONTRIBUTING.md`](CONTRIBUTING.md)

---

## Performance goals

Project targets for a simple endpoint in a release build:

- ≥ 50,000 req/s
- 10–30 MB resident memory
- < 50 ms cold start
- ~15 MB stripped release binary

Treat these as project benchmark targets, not universal application guarantees. Real performance depends on workload, database access, hardware, and application logic.

---

## RustIO vs RustIO Admin

There is a separate project, **[`rustio-admin`](https://github.com/abdulwahed-sweden/rustio-admin)**.

- **RustIO** focuses on structs → schema → DB/admin/server plus plain-English evolution.
- **RustIO Admin** is a Postgres-first administrative framework with a different scope and runtime model.

The CLI binary for RustIO is `rustio`; RustIO Admin uses `rustio-admin`. Through its v0.21.x line RustIO Admin also shipped a binary called `rustio`, so installing both silently overwrote one in `~/.cargo/bin`; since its v0.22.0 they no longer collide.

---

## Why sponsor?

Framework maintenance is continuous work: compatibility, migrations, documentation, examples, bug fixes, release engineering, and keeping the safe-change workflow predictable.

Sponsorship helps fund exactly that work.

If your team uses RustIO, experiments with it for internal tooling, or simply wants this kind of Rust infrastructure to keep improving, support is directly useful.

<p align="center">
  <a href="https://github.com/sponsors/abdulwahed-sweden?metadata_source=rustio&metadata_campaign=readme_bottom">
    <img src="https://img.shields.io/badge/Support_RustIO_on_GitHub_Sponsors-%E2%9D%A4-db61a2?style=for-the-badge&logo=githubsponsors&logoColor=white" alt="Support RustIO on GitHub Sponsors">
  </a>
</p>

---

Stuck? Open an [issue](https://github.com/abdulwahed-sweden/rustio/issues).

License: [MIT](LICENSE).
