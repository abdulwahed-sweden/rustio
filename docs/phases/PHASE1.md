# Phase 1 — Port `schema.rs` + `ai/` from OLD

Source of truth: `~/Documents/GitHub/rustio` (read-only).
Target: `~/Documents/rustio` (this repo).

## Files copied / changed

### Files copied verbatim from OLD (10 files)

| Destination | LOC | Source |
|---|---:|---|
| `rustio-core/src/ai.rs` | 1,450 | OLD `src/ai.rs` (the parent module — was missing from the spec's `cp -r`) |
| `rustio-core/src/ai/executor.rs` | 3,194 | OLD |
| `rustio-core/src/ai/planner.rs` | 1,403 | OLD |
| `rustio-core/src/ai/review.rs` | 1,018 | OLD |
| `rustio-core/src/ai/industry.rs` | 105 | OLD |
| `rustio-core/src/ai/executor_tests.rs` | 1,460 | OLD |
| `rustio-core/src/ai/executor_tests_advanced.rs` | 787 | OLD |
| `rustio-core/src/ai/planner_tests.rs` | 924 | OLD |
| `rustio-core/src/ai/review_tests.rs` | 929 | OLD |
| `rustio-core/src/ai/context_tests.rs` | 589 | OLD |
| `rustio-core/src/schema.rs` | 991 | OLD |

`schema_introspect.rs`: not a top-level file in OLD (`src/schema_introspect.rs` doesn't exist; OLD has `src/admin/schema_introspect.rs`). Skipped per the spec's `2>/dev/null || true`.

### Files deleted from NEW (5 stub files, replaced by OLD's port)

- `rustio-core/src/ai/mod.rs` (NEW stub, 20 LOC) — OLD uses `ai.rs` + `ai/` (no `mod.rs`)
- `rustio-core/src/ai/executor.rs` (NEW stub, 207 LOC)
- `rustio-core/src/ai/planner.rs` (NEW stub, 232 LOC)
- `rustio-core/src/ai/review.rs` (NEW stub, 189 LOC)
- `rustio-core/src/ai/primitive.rs` (NEW stub, 92 LOC) — OLD has `Primitive` inside `ai.rs`

### Files modified (6 places — in scope per ground rule 4)

| File | Lines | Why |
|---|---|---|
| `rustio-core/src/lib.rs` | 0 | `pub mod ai;` and `pub mod schema;` were already declared. No-op. |
| `rustio-core/src/schema.rs::SchemaModel::from_entry` | ~10 | NEW's `AdminEntry` lacks `table` and `core` fields. Mapped `table = entry.admin_name` (the snake-plural matches the scaffolded SQL table); `core = false` (NEW has no core-models concept yet). |
| `rustio-core/src/schema.rs::SchemaField::from_admin_field` | ~15 | NEW's `AdminField` uses `field_type: FieldType` (no separate `nullable` bool); NEW's `AdminRelation` uses `target_model` (no `model`/`kind`). Translated; relation kind hard-coded to `BelongsTo` (NEW only models that direction). |
| `rustio-core/src/schema.rs::field_type_name` | +2 | Added arms for NEW's `FieldType::OptionalI64` and `FieldType::OptionalString` (mapped to non-optional names; nullability lives separately in `SchemaField.nullable`). |
| `rustio-core/src/schema.rs` test fixtures (Post + Book impls) | ~30 | OLD's test fixtures used OLD's `AdminModel` surface (`singular_name()` method, `field_display()` method, `from_form(form, Option<i64>)`, `AdminField { ty, nullable, … }`). Adapted to NEW's surface (`SINGULAR_NAME` const, drop `field_display`, `from_form(&FormData) -> Result<_, Vec<String>>`, `AdminField { label, field_type, … }`, plus mandatory `display_values`/`object_label`/`id`/`values_to_update`). |
| `rustio-core/src/schema.rs` import | 1 | `use crate::admin::{… FormData}` → `use crate::http::FormData` (NEW exports it from `http`, OLD re-exported from `admin`). |
| `rustio-core/src/ai.rs` test fixture (Post impl) | ~25 | Same trait-surface adaptation as schema.rs's fixtures. |
| `rustio-core/src/ai.rs` import | 1 | Same FormData re-route. |

### Files modified beyond Phase-1 target list (Path A waiver, authorized)

| File | Why |
|---|---|
| `rustio-cli/src/main.rs` | NEW's CLI used NEW's stub `ai::{plan, review, apply_plan, ApplyOptions, Plan}` API, which doesn't exist in OLD's surface. Rewrote 6 call sites to use OLD's `generate_plan` / `review_plan` / `execute_plan_document` + `PlanRequest` / `LoadedPlan` / `PlanDocument` / `ExecuteOptions`. Also replaced `Schema::new(version)` with direct struct construction (OLD's `Schema` has no `::new` constructor). One unrelated import (`SCHEMA_VERSION`) added. ~73 lines diffed. |

## Cargo.toml additions

**None.** OLD's deps are a subset of NEW's. The ported code uses only `chrono` and `serde`, both already in NEW with the same features (`chrono` with `clock,std,serde`; `serde` with `derive`).

## `todo!("phase 2")` stubs

**None.** The ported `ai/` and `schema.rs` are pure / FS-only — no DB calls were stubbed. The `ai/executor.rs` writes files, but doesn't touch sqlx; everything compiles cleanly against NEW's PgPool-based ORM.

## Tests

```
$ cargo test -p rustio-core --lib ai::      → 172 passed, 0 failed, 0 ignored
$ cargo test -p rustio-core --lib schema::  →  15 passed, 0 failed, 4 ignored
$ cargo test --workspace                    → 209 passed, 0 failed, 4 ignored
```

### 4 tests `#[ignore]`'d (Path B per your call)

All four expect behavior NEW's `admin/types.rs` doesn't model yet. Fix is one targeted change to NEW's admin (out of Phase-1 scope):

| Test | Ignore reason |
|---|---|
| `schema::tests::core_user_model_is_always_present` | NEW's `Admin::new()` doesn't seed a core User model. OLD's did. |
| `schema::tests::schema_reflects_admin_registry` | Same — asserts `models.len() == 2` (User + Post); NEW returns 1. |
| `schema::tests::schema_models_are_sorted_by_name` | Same — depends on User+Post both being present. |
| `schema::tests::schema_snapshot_is_byte_for_byte_stable` | Two compounding gaps: (a) no User from `Admin::new()`, (b) NEW's `FieldType` has no `OptionalDateTime` variant — the golden expects `published_at.nullable=true` but NEW's enum can't express it. |

Each `#[ignore]` carries a one-line `"Phase 2: …"` reason string so `cargo test --ignored` prints exactly what's missing.

## Compile + lint state

- `cargo check --workspace --all-targets` — clean
- `cargo clippy --workspace --all-targets` — clean (no `-D warnings` flag yet; no warnings emitted either)

## Diff scope

```
 rustio-cli/src/main.rs          |   73 +-
 rustio-core/src/ai/executor.rs  | 3299 +++++++++++++++++++++++++++++++++++++--
 rustio-core/src/ai/mod.rs       |   20 -      (deleted)
 rustio-core/src/ai/planner.rs   | 1521 +++++++++++++++---
 rustio-core/src/ai/primitive.rs |   92 -      (deleted)
 rustio-core/src/ai/review.rs    | 1127 +++++++++++--
 rustio-core/src/schema.rs       | 1091 +++++++++++--
 + 7 untracked files in rustio-core/src/ai/ (the OLD ports lifted up)
 + rustio-core/src/ai.rs (new file from OLD)
```

Total: 6,508 insertions / 715 deletions across 7 modified files + 8 new files.
