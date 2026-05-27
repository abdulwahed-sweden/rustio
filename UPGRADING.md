# Upgrading RustIO

A per-release migration guide. Items here only cover externally-observable changes; minor refactors without a user-visible surface are not listed.

---

## 0.9.0 → 0.9.1

Destructive-op gate lights up. No action required for projects that don't plan `remove_field` / `remove_relation`.

### New behaviour (opt-in)

- **`rustio ai apply <plan> --force`** now unlocks the two destructive primitives the planner has always been allowed to emit:
  - `remove_field` — drops a column from both the Rust struct and the database.
  - `remove_relation` — drops a belongs_to FK column.

  Without `--force`, both continue to refuse with `primitive `remove_field` is destructive — re-run `rustio ai apply` with `--force` to open the destructive gate`.

- **`remove_model`** stays refused regardless of `--force` until 0.9.2. Scope cap — dropping a whole model has to coordinate the struct, the admin registration, and every downstream FK.

### What `--force` does NOT bypass

Three gates live one layer above the destructive-op gate and are **not** reachable via `--force`:

- **Critical-risk plans** — review-layer decisions like "this plan touches core models" or "this plan mixes add + remove on the same field". Regenerate the plan under a changed posture.
- **Developer-only primitives** (e.g. `create_migration`) — these are never allowed through `rustio ai apply`, by design.
- **PII policy refusals** — removing a field flagged under `rustio.context.json` as personal data escalates to Critical before the destructive gate even runs.

### SQL generation

The `remove_field` / `remove_relation` migration is FK-aware: every surviving relation on the same table keeps its `REFERENCES <parent>(id) ON DELETE <policy>` clause during the recreate-table. Operations on tables that have no FKs also work (the guard path was only in `change_field_type`; `remove_field` uses its own recreate).

### Compatibility

- **API.** `ExecuteOptions.allow_destructive` existed as a field in 0.8.x but was silently ignored. In 0.9.1 it becomes load-bearing. Code that set `allow_destructive: true` on 0.8.x saw no effect; on 0.9.1 it does. Review callers that construct `ExecuteOptions` directly before upgrading.
- **CLI.** `rustio ai apply` gains `--force`. Existing `--yes` / `--dry-run` composition is unchanged.
- **Tests.** Crate-internal tests that asserted "remove_field is refused even with allow_destructive" have been flipped; if you forked that test, flip it the same way.

---

## 0.8.x → 0.9.0

Phase 2 close-out. Ships SQL foreign-key enforcement and a retrofit path for existing projects.

### New behaviour (automatic)

- **New `belongs_to` relations emit a real SQL `FOREIGN KEY`.** Running `rustio ai apply <plan>` on a plan that contains `AddRelation { kind: BelongsTo, .. }` now generates a migration that says `REFERENCES <parent>(id) ON DELETE RESTRICT` instead of just adding a bare `<via>_id` column. The generated migration also emits `PRAGMA foreign_keys = ON;` so the constraint is enforced against the connection running the migration.

- **The FK column is nullable by default.** SQLite cannot add a `NOT NULL + REFERENCES` column via `ALTER TABLE` (the implicit `DEFAULT NULL` is what makes existing rows satisfy the new FK without a backfill). If you need a required FK, use the retrofit path below.

- **New natural-language grammar** for relation phrases accepts trailing options:
  - `link A to B` — nullable FK, `ON DELETE RESTRICT` (default).
  - `link A to B required` — `NOT NULL` FK. **Refuses at executor** — use the retrofit.
  - `link A to B on_delete:cascade` — deletes children when the parent is deleted.
  - `link A to B on_delete:set_null` — nulls children when the parent is deleted.
  - Combinations allowed: `link A to B required on_delete:set_null`.
  - Unknown options / policies **refuse**, never silently default.

### Retrofit for existing 0.8.x projects

If your project was generated on 0.8.x, your `_id` columns exist but have no SQL FK constraint. Run:

```bash
rustio migrate add-fks           # dry run — prints what would change
rustio migrate add-fks --write   # commits one migration per affected table
```

The command reads `rustio.schema.json`, finds every `belongs_to` relation that lacks `on_delete` metadata, and generates one migration per affected table using the SQLite recreate-table pattern (`CREATE TABLE <t>__new … FOREIGN KEY …` + `INSERT … SELECT` + `DROP` + `RENAME`).

After writing, **review each generated SQL file** before running `rustio migrate apply`. The recreate-table pattern drops and rebuilds the table — any column or index you added outside of `rustio.schema.json` will be lost unless it's reflected in the schema.

### Breaking changes

- **`schema::Relation` gains two fields**, `required: Option<bool>` and `on_delete: Option<String>`. Both are serde-default, so 0.8.x `rustio.schema.json` files parse unchanged. Rust code that constructs `Relation { … }` literally (only in-tree tests and tooling) needs those two fields added.

- **`ai::AddRelation` gains two fields**, `required: bool` and `on_delete: OnDelete`. Both are serde-default, so saved 0.8.x plan documents still deserialise. Rust code that constructs `AddRelation { … }` literally needs both added; `required: false` + `on_delete: OnDelete::Restrict` matches the 0.8.0 implicit behaviour.

### Review-layer risk

- Default (`required: false`, `ON DELETE RESTRICT`): **Low**.
- Either `required: true` OR `ON DELETE CASCADE`: **Medium** — new warning in `warnings_for`.
- Both `required: true` AND `ON DELETE CASCADE`: **High** — the cascade-with-strict-FK combination is the most destructive policy combination.

### Verified against medflow

The example clinic system has 27 `belongs_to` relations across 13 tables. `rustio migrate add-fks` identifies all 27 and writes 13 retrofit migrations. Nullability on each FK column is preserved from the existing schema (e.g. `department_id` stays nullable, `patient_id` stays `NOT NULL`).

---

## Earlier releases

See `CHANGELOG.md` for the full history.
