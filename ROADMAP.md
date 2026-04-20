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
  │  clinic · crm · inventory · workflow     │   (v1.0.0 and beyond)
  └──────────────────────────────────────────┘
                      ▲
  ┌──────────────────────────────────────────┐   ✅ shipped across
  │  Phase 2  ·  Intelligence                │     0.5.0 → 0.8.0
  │  planner · review · executor · context · │
  │  admin intelligence · relations          │
  └──────────────────────────────────────────┘
                      ▲
  ┌──────────────────────────────────────────┐   ✅ shipped in
  │  Phase 1  ·  Foundation                  │     0.4.x
  │  auth · DateTime · Option · schema.json  │
  └──────────────────────────────────────────┘
```

### Why this order is non-negotiable

If the foundation isn't deterministic and typed, intelligence is unreliable. An AI cannot safely extend a system whose shape isn't fully knowable. If there are no higher primitives (real users, dates, relations, nullables), there's nothing substantial to build on top of.

- Do **not** ship Systems before Intelligence is stable.
- Do **not** ship Intelligence before the Foundation is final.

---

## Phase 1 · Foundation — shipped in 0.4.x ✅

Deterministic, typed, and schema-exportable. All four goals shipped:

- **Authentication.** Built-in `User`, `argon2id` passwords, DB-backed sessions. Dev tokens removed.
- **Core types.** `DateTime<Utc>` and `Option<T>` round-trip end-to-end.
- **`rustio.schema.json`.** Stable shape (see sample below); emitted by `rustio schema`; regenerated on every `rustio migrate apply`.
- **Macro ↔ core contract.** Every field type the derive macro accepts is mirrored in the schema.

Sample shape (current at 0.8.0):

```json
{
  "version": 1,
  "rustio_version": "0.8.0",
  "models": [
    {
      "name": "Visit",
      "table": "visits",
      "admin_name": "visits",
      "display_name": "Visits",
      "singular_name": "Visit",
      "core": false,
      "fields": [
        { "name": "id",         "type": "i64",    "nullable": false, "editable": false },
        { "name": "patient_id", "type": "i64",    "nullable": false, "editable": true,
          "relation": { "model": "Patient", "field": "id", "kind": "belongs_to" } },
        { "name": "doctor_id",  "type": "i64",    "nullable": false, "editable": true,
          "relation": { "model": "Doctor",  "field": "id", "kind": "belongs_to" } }
      ],
      "relations": [
        { "kind": "belongsto", "to": "Patient", "via": "patient_id" },
        { "kind": "belongsto", "to": "Doctor",  "via": "doctor_id"  }
      ]
    }
  ]
}
```

---

## Phase 2 · Intelligence — shipped across 0.5.x → 0.8.0 ✅

### Shipped

- **0.5.0 Planner.** Rule-based grammar over a closed `Primitive` enum (`AddField`, `RemoveField`, `RenameField`, `RenameModel`, `ChangeFieldType`, `ChangeFieldNullability`, `AddRelation`, `UpdateAdmin`, `CreateMigration` (developer-only)). Refuses instead of guessing.
- **0.5.1 Plan review.** Deterministic risk / impact / warnings from `(plan, schema, context)`. Saved plans are stale-detected against the live schema.
- **0.5.2 Safe executor.** Atomic apply of a reviewed plan. Destructive primitives (`remove_field`, `remove_model`) refuse without explicit flags.
- **0.5.3 Advanced mutations.** SQLite recreate-table for `change_field_type`, `change_field_nullability`, `rename_model`. Tables with FK constraints are refused until 0.9.0.
- **0.6.0 Context layer.** `rustio.context.json` carries country / industry / compliance signals; `pii_fields()` drives review warnings and executor policy refusals; industry schemas flag expected field sets.
- **0.7.0–0.7.2 Admin intelligence.** Per-field role classification, `FieldUI` metadata, filter inference, search-intent inference, masking, and a trust dashboard.
- **0.7.3 Runtime truth.** Admin dashboard reads the schema on disk live; `[Reload schema]` updates suggestions without a process restart.
- **0.8.0 Relations (foundational).** `link X to Y` / `connect X to Y` / `add relation from X to Y` grammar. Executor adds a `<target>_id i64` column. **No SQL `FOREIGN KEY`** — enforcement is the 0.9.0 follow-up. The review layer warns about the gap and raises a GDPR-minimisation flag when the target model carries PII.

### Remaining Intelligence-layer work

- **0.9.0 Relations — enforcement.** Emit SQL `FOREIGN KEY` clauses; extend the SQLite recreate-table pattern to rewrite FKs safely during type/nullability changes; unlock the table-has-FK refusal in the advanced-mutations path.
- **0.9.x destructive gate.** An explicit opt-in (`allow_destructive` on `ExecuteOptions`) so `remove_field` can apply when the operator has reviewed and confirmed data loss.
- **`has_many` materialisation.** Today 0.8.0 refuses non-`BelongsTo` kinds. The reverse accessor is a runtime convenience, not a schema change — likely lands alongside 0.9.0 FK enforcement.

### Exit criteria (still open)

- End-to-end relation flow including SQL FK enforcement.
- Documented destructive gate that a reviewer must opt into, with tests proving the default posture still refuses.

---

## Phase 3 · Systems (v1.0.0 and beyond)

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

*Last updated alongside the 0.8.0 release.*
