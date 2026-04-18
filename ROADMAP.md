# RustIO — Roadmap

## Positioning

**RustIO is the fastest way to build real systems — and evolve them safely with AI.**

It is a system builder with a strict, typed core. The shape of every model, field, relationship, and admin behavior is captured in a deterministic, machine-readable schema. That determinism is the foundation on which everything else — the admin layer, the CLI, the upcoming AI-assisted extension layer — becomes reliable.

This is not a framework competing with Django. It is a different category: **a strict-by-construction system builder with an AI layer that understands its own rules.**

---

## What RustIO is NOT

Stated explicitly so no contributor time is wasted on work that won't be merged.

- **Not a Django clone.** We do not port Django's API, module layout, or function names.
- **Not a generic framework.** We are not competing with Axum, Actix, or Rocket.
- **Not a microframework.** Opinionated defaults are the product.
- **Not a frontend framework.** Server-rendered admin + JSON. Use a separate frontend if you want an SPA.
- **Not a sync framework.** Tokio only.
- **Not an AI toy.** The AI layer exists because a typed core makes AI reliable — not as marketing.

---

## The three-phase progression

Phases are **strictly sequential**. Each phase is a prerequisite for the one above it.

```
  ┌──────────────────────────────────────────┐
  │  Phase 3  ·  Systems                     │   pre-built verticals
  │  clinic · crm · inventory · workflow     │   (v0.6.0 and beyond)
  └──────────────────────────────────────────┘
                      ▲
  ┌──────────────────────────────────────────┐
  │  Phase 2  ·  Intelligence                │   AI that understands
  │  rustio ai · relations · schema-aware    │   the typed core
  └──────────────────────────────────────────┘                (v0.5.0)
                      ▲
  ┌──────────────────────────────────────────┐
  │  Phase 1  ·  Foundation                  │   typed core +
  │  auth · DateTime · Option · schema.json  │   machine-readable schema
  └──────────────────────────────────────────┘                (v0.4.0)
```

### Why this order is non-negotiable

If the foundation isn't deterministic and typed, intelligence is unreliable. An AI cannot safely extend a system whose shape isn't fully knowable. If there are no higher primitives (real users, dates, relations, nullables), there's nothing substantial to build on top of.

- Do **not** ship Systems before Intelligence is stable.
- Do **not** ship Intelligence before the Foundation is final.

---

## Phase 1 · Foundation (v0.4.0)

**Goal:** make RustIO deterministic, typed, and schema-exportable.

### Scope

**1. Authentication (production-grade)**

- Built-in `User` table
- `argon2` password hashing
- Session storage in the database (backed by the existing cookie flow)
- Login form upgraded from dev-token → email + password
- The current 0.3.1 dev-token flow is demoted to a development-only override

**2. Core types**

- `DateTime<Utc>` field type (chrono-backed, ISO-8601 at rest)
- `Option<T>` for nullable fields (NULL in DB, `None` in Rust, empty input in admin)

**3. `rustio.schema.json` (the critical piece)**

A single file generated from the compiled project. It is the only interface that external tooling — including the upcoming AI layer — is allowed to use. Shape:

```json
{
  "version": 1,
  "generated_at": "2026-04-18T10:12:33Z",
  "rustio_version": "0.4.0",
  "models": [
    {
      "name": "User",
      "table": "users",
      "admin_name": "users",
      "display_name": "Users",
      "singular_name": "User",
      "fields": [
        { "name": "id",         "type": "i64",      "nullable": false, "editable": false },
        { "name": "email",      "type": "String",   "nullable": false, "editable": true,
          "attrs": { "searchable": true } },
        { "name": "created_at", "type": "DateTime", "nullable": false, "editable": false }
      ],
      "relations": []
    }
  ]
}
```

Emitted by a new `rustio schema` command. Regenerated on every `rustio migrate apply` and every `cargo build` via a small build-script hook.

### Not in 0.4.0

- Relations (moved to Phase 2 because they depend on the schema format being finalised).
- Dashboards, charts, custom templates.
- Hot reload.

### Exit criteria (must be true before 0.4.0 ships)

- `rustio.schema.json` format is documented and committed to as stable for the 0.x line.
- Built-in `User` with argon2 passwords is usable through the existing admin.
- `DateTime` and `Option<T>` fields render and round-trip correctly in admin forms and SQL.
- Every field the macro understands is reflected in the schema.

---

## Phase 2 · Intelligence Layer (v0.5.0)

**Goal:** make RustIO safely extensible by AI agents.

### Scope

**1. `rustio ai` command**

- Reads `rustio.schema.json`.
- Accepts a short natural-language intent: `rustio ai "add a published bool to Post"`.
- Translates the intent into a fixed set of edit primitives (see below).
- Produces a diff for review; writes only on explicit confirmation.
- Runs `cargo build` before finalising — any edit that doesn't compile is rejected.

**Fixed edit primitives (AI may only emit these):**

- `add-field <model> <name> <type>`
- `remove-field <model> <name>`
- `add-model <name> <fields>`
- `add-relation <from> <kind> <to>` (belongs_to, has_many)
- `add-admin-attribute <model> <field> <attr>`
- `add-migration <name> <sql>`

Free-form code generation is explicitly out of scope. The AI operates inside the schema, not outside it.

**2. Relations**

- `#[rustio(belongs_to = "User")]` — adds the foreign-key column and a `user(&db).await` lookup.
- `#[rustio(has_many = "Post")]` — generates the reverse accessor and an inline form in admin.
- Both reflect into `rustio.schema.json`.

### Constraints (enforced at the AI boundary)

- AI cannot touch files outside the RustIO layout (`apps/*/models.rs`, `apps/*/admin.rs`, `apps/*/views.rs`, `migrations/`).
- AI cannot emit code that doesn't match the macro's expected shape.
- AI failures return a clear "not possible in current scope" message instead of partial writes.
- The AI layer is fully optional at runtime. The framework works without it.

### Exit criteria

- `rustio ai "add a published bool to Post"` produces the correct changes across `models.rs`, a new migration, and `rustio.schema.json`.
- Relations work end-to-end: foreign key in DB, accessor in Rust, inline form in admin.
- There exists at least one documented AI intent type that the system correctly **refuses** (proving the boundary).

---

## Phase 3 · Systems (v0.6.0 and beyond)

**Goal:** `rustio init <system>` produces a running vertical solution, not a blank project.

### Scope

Pre-built vertical templates — each a full project + apps + migrations + admin configuration:

- `clinic` — patients, appointments, staff, records
- `crm` — contacts, deals, notes, activities
- `inventory` — items, locations, stock movements
- `workflow` — tasks, assignments, states, audits
- `registry` — generic entity registry with custom fields

### Dependencies

Systems require a final Phase 1 (types, auth, schema) and a stable Phase 2 (relations, AI layer). Building them earlier means shipping templates that must be rewritten every minor release.

Until Phase 3, the existing `basic` / `blog` / `api` presets remain the scaffolding path.

---

## Technical constraints

These are honest limits, not aspirational targets.

- **Hot reload** — true in-process live-patching is not realistic in Rust. What we will ship is a file watcher + automatic restart (~2s end-to-end) with an auto-refreshing browser tab. Calling it "hot reload" would over-sell what the language permits; we call it **fast restart** in docs.
- **Prerequisite features** — auth, DateTime, Option, relations, migrations. These exist on the roadmap because the experience we're building requires them, not for parity with any other framework.
- **Performance targets** — ≥50,000 req/s for a simple endpoint, 10–30 MB resident memory, <50ms cold start, ~15 MB stripped binary. Any release that regresses these numbers will not ship.

---

## Design principle

Every proposed feature must answer:

> *Does it make building a real system faster, clearer, or safer?*

If the answer is no, it does not belong in RustIO. This principle supersedes any other framing — including Django comparisons, benchmark ambitions, or developer-preference arguments.

---

## Explicitly out of scope

- **Template engines** beyond what the admin needs.
- **Frontend framework integrations.** Ship JSON, use a separate frontend.
- **Microservice tooling** (service discovery, RPC, mesh). RustIO targets monoliths.
- **MySQL, Oracle, SQL Server.** SQLite + PostgreSQL is the working set.
- **A synchronous variant.** Tokio only.
- **Django API compatibility.** We don't match function names, module layouts, or signatures.

---

## Release cadence

- **Patch (`0.x.y`)** — bug fixes, doc updates, dependency bumps. No API changes.
- **Minor (`0.x.0`)** — ships the next phase's items from the table above. May contain breaking changes pre-1.0, documented in the CHANGELOG.
- **1.0** — locks the public API of the Foundation and Intelligence layers. Phase 3 (Systems) iterations continue afterwards without breaking the core.

---

## How to influence this roadmap

1. Open an issue describing a real system the current scope cannot express.
2. Reference the phase (Foundation / Intelligence / Systems) your proposal targets.
3. For Phase 1 items, align on design before writing code — open a discussion first.
4. Match the existing bar: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, full test suite passing, CHANGELOG updated.

A sharp design question is more valuable than a broad PR. Either is welcome.

---

*Last updated alongside the 0.3.1 release.*
