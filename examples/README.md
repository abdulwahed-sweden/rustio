# Rustio examples

These schemas represent real production-style systems with complex
data, filtering, and workflows. Each one is JSON only — no Rust,
no `Cargo.toml`. Use them as the starting shape for `rustio new
project ... --schema <file>` and adapt to your domain.

## Quick start

The blog is the **runnable reference implementation** — it shows
the full Rust + PostgreSQL + Meilisearch + admin wiring. The
schemas in the gallery below are study material: real-world
domain shapes you can scaffold from with `rustio new project ...
--schema <file>`.

Open `examples/blog/`. It's the only example with `src/`,
`Cargo.toml`, and migrations.

`cargo run` builds the project, connects to PostgreSQL using
`DATABASE_URL` (defaulting to
`postgres://postgres:dev@localhost/rustio_dev` if unset), applies
migrations, seeds the default admin, and serves the admin UI at
`http://127.0.0.1:8000/admin`.

Read its `README.md` for the full wiring.

## Real-world systems

| #  | Example                            | Complexity    | Domain                                                                                                            |
|----|------------------------------------|---------------|-------------------------------------------------------------------------------------------------------------------|
| 01 | [Foundation](01-foundation/)       | ⭐☆☆☆☆       | Smallest meaningful schema — primitive types, one FK, two models.                                                |
| 02 | [Healthcare](02-healthcare/)       | ⭐⭐⭐⭐⭐   | Clinic / hospital. Patients, doctors, availability, appointments, prescriptions, records. **Flagship example.** |
| 03 | [School system](03-school-system/) | ⭐⭐⭐☆☆     | Students, teachers, courses, enrollments, weighted grading.                                                       |
| 04 | [SaaS core](04-saas-core/)         | ⭐⭐⭐⭐☆    | Multi-tenant: organizations, members, projects, tasks, subscriptions.                                            |
| 05 | [Queue system](05-queue-system/)   | ⭐⭐⭐⭐☆    | HomeQ-style housing queue. Priority scoring, eligibility, ranking.                                               |
| 06 | [Commerce](06-commerce/)           | ⭐⭐⭐⭐☆    | Storefront. Categories (tree), products, customers, carts, orders.                                                |

Each example's `README.md` documents:

* the models and their relations,
* the realistic daily filtering scenarios the schema enables,
* the status / lifecycle vocabulary used (string enums),
* the production gaps the example deliberately doesn't model,
  with explicit guidance on what to add when adapting for real use.

## Conventions across the catalogue

* **Currency** — all money fields are `i64` in the smallest unit
  (cents for USD, öre for SEK). UI formatting is project-level.
* **Status** — modelled as `String`. Each example fixes a
  vocabulary in its README; treat those as closed sets in your
  application code.
* **Timestamps** — every model has `created_at` and `updated_at`,
  both auto-managed (`editable: false`). Domain-specific timestamps
  (`scheduled_at`, `placed_at`, etc.) are explicitly declared.
* **Derived fields** — fields like `Course.enrolled_count` and
  `QueueEntry.priority_score` are stored, not computed on read.
  Each example documents the staleness contract for its derived
  fields.
* **Tenant isolation** — `04-saas-core` carries `organization_id`
  on every queryable model. Enforce the filter at the query layer;
  the schema does not.

## What's intentionally not here

* **No `Cargo.toml`** — these examples don't compile. They're
  schemas + documentation. The compiled artefact is
  `examples/blog/`.
* **No CRUD wiring** — once you scaffold a project from one of
  these schemas, follow `examples/blog/` for the wiring template.
* **No domain-specific business logic** — the schema describes
  shape; behaviour belongs in your code.
