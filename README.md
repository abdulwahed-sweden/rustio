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
- guided schema evolution

The goal is simple: **you write the domain; RustIO handles the repetitive system plumbing.**

<p align="center">
  <a href="https://github.com/sponsors/abdulwahed-sweden?metadata_source=rustio&metadata_campaign=value_cta"><strong>❤️ Sponsor continued RustIO development</strong></a>
</p>

---

## Five-minute start

```bash
cargo install rustio-cli
rustio init mysite
cd mysite
```

Use the guided setup or define models manually, then:

```bash
rustio migrate apply
rustio user create --email you@example.com --password secret --role admin
rustio run
```

Open:

```text
http://127.0.0.1:8000/admin
```

You now have a working admin system generated from your Rust model definitions.

![Admin list page](docs/screenshots/admin-tasks-list-light.png)

---

## A small mental model

A RustIO project revolves around three things:

1. **Rust models** — the source of truth for your domain.
2. **Migrations** — explicit SQL changes to the database.
3. **`rustio.schema.json`** — the stable machine-readable contract used by tooling.

Everything else — admin screens, login flow, schema export, and guided setup — exists to reduce repeated work around those three pieces.

---

## Evolve the system safely

Describe a change:

```bash
rustio evolve "add date_of_birth as DateTime to notes"
```

RustIO proposes a typed change plan, shows the risk, and lets you review it before anything lands.

The change pipeline is deliberately constrained:

```text
request → typed plan → review → apply
```

If a requested change cannot be represented safely in the supported vocabulary, RustIO should refuse rather than invent an approximation.

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

---

## Useful commands

```bash
rustio                    # context-aware next step
rustio doctor             # diagnose common project problems
rustio explain <topic>    # short built-in explanations
rustio start              # setup menu
rustio new app <name>     # add a model/app
rustio migrate apply      # apply migrations
rustio migrate status     # inspect migration state
rustio schema             # regenerate rustio.schema.json
rustio run                # build and serve
rustio user create        # create a user
```

Advanced schema-change workflow:

```bash
rustio ai plan "<change>"
rustio ai review <plan>
rustio ai apply <plan>
```

---

## Example project

The repository includes **[`examples/bookflow/`](examples/bookflow/)**, a multi-model booking system showing relationships, generated admin views, migrations, and seed data.

For deeper material, start with the [documentation site](https://rustio.vercel.app) or:

- [`docs/glossary.md`](docs/glossary.md)
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

- **RustIO** focuses on structs → schema → DB/admin/server plus guided evolution.
- **RustIO Admin** is a Postgres-first administrative framework with a different scope and runtime model.

The CLI binary for RustIO is `rustio`; RustIO Admin uses `rustio-admin`.

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
