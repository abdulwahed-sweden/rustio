# Phase 2 — Rewrite AI executor SQL from SQLite to PostgreSQL

Built on top of Phase 1 (commit `13f1d58`).

## Inventory: SQL statements rewritten

| `apply_*` function | OLD shape (SQLite) | NEW shape (Postgres) |
|---|---|---|
| `sql_for_add_field` | `INTEGER` / `TEXT` types, `'1970-01-01 00:00:00'` default | `INTEGER` / `BIGINT` / `BOOLEAN` / `TEXT` / `TIMESTAMPTZ`, `FALSE` / `'1970-01-01 00:00:00+00'` defaults |
| `apply_remove_field` | recreate-table dance | `ALTER TABLE … DROP COLUMN … CASCADE` |
| `apply_remove_relation` | recreate-table dance | delegates to remove_field → `DROP COLUMN CASCADE` (FK constraint goes with the column) |
| `apply_change_field_type` | recreate-table + `CAST(… AS TEXT)` | `ALTER TABLE … ALTER COLUMN … TYPE … USING (col::TARGET)` |
| `apply_change_field_nullability` | recreate-table + COALESCE backfill in `INSERT … SELECT` | tightening: `UPDATE` backfill + `ALTER COLUMN SET NOT NULL`; relaxing: `ALTER COLUMN DROP NOT NULL` |
| `apply_add_relation` | `ALTER TABLE … ADD COLUMN INTEGER REFERENCES …` + `PRAGMA foreign_keys = ON` | `ALTER TABLE … ADD COLUMN BIGINT REFERENCES …`, no PRAGMA |
| `apply_rename_model` | (ALTER TABLE RENAME — already cross-DB) + FK guard | same SQL, FK guard removed (PG auto-updates dependent FKs) |
| `plan_retrofit_foreign_keys` | recreate-table dance per affected table, wrapped in `PRAGMA foreign_keys = OFF/ON` | one `ALTER TABLE … ADD CONSTRAINT … FOREIGN KEY (…) REFERENCES …(id) ON DELETE …` per relation, wrapped in `BEGIN`/`COMMIT` |

Total: **8 `apply_*` functions touched**, 4 of them dramatically simplified (the recreate-table dance was 25–35 lines per call site; PG ALTER fits in 3).

## Test counts

| | Tests passing in sandbox | Ignored |
|---|---:|---:|
| Phase 1 baseline (commit `13f1d58`) | 209 | 4 (FieldType + core User gaps) |
| End of Phase 2a (unignore) | 213 | 0 |
| End of Phase 2b (a)–(g) rewrites, all PG tests un-ignored | 221 | 0 |
| **After Path B retrofit** (PG tests gated) | **213** | **8** (PG integration suite) |

The 213 are the pure unit tests — including every "the SQL string is exactly X" assertion for each `apply_*` rewrite. The 8 ignored are the live-Postgres integration tests in `rustio-core/src/ai/executor_pg_tests.rs`. They were verified passing against live PG during the session (commits `5a99806` through `150acfe`); the gate keeps the default suite runnable in any sandbox without a DB.

### Running the PG integration tests

On a host with the docker-compose stack up:

```bash
docker compose -f ~/Documents/rustio/docker-compose.yml up -d
RUSTIO_TEST_DB=1 cargo test --workspace -- --ignored
```

Or one test at a time, runnable standalone:

```bash
RUSTIO_TEST_DB=1 cargo test pg_retrofit_adds_fk_constraint_in_place -- --ignored --exact
```

Connection URL is read from `DATABASE_URL`; falls back to
`postgres://postgres:dev@localhost:5432/blog`. The `RUSTIO_TEST_DB=1`
flag is operator-facing only — `--ignored` is what actually opts in.

### The 8 PG integration tests (each runnable standalone)

| # | Test | Verifies |
|---|---|---|
| 1 | `pg_add_field_appends_column_with_pg_type` | i32→INTEGER, i64→BIGINT, bool→BOOLEAN with `false` default, DateTime→TIMESTAMPTZ; nullability propagates |
| 2 | `pg_remove_field_drops_column_and_dependent_constraints` | `DROP COLUMN CASCADE` removes the column AND the dependent FK constraint AND the dependent index in one statement |
| 3 | `pg_change_field_type_rewrites_column_in_place` | `ALTER COLUMN … TYPE TEXT USING (score::TEXT)` casts every row from i32 to text representation |
| 4 | `pg_change_field_type_works_on_fk_bearing_table` | The SQLite "FK guard" refusal is gone; PG accepts the type change while preserving the dependent FK |
| 5 | `pg_change_field_nullability_relax_then_tighten` | DROP NOT NULL ↔ SET NOT NULL roundtrip; backfill UPDATE replaces existing NULLs with the type default |
| 6 | `pg_add_relation_creates_fk_constraint` | New BIGINT column with FK constraint; ON DELETE RESTRICT enforces (parent delete fails when children exist) |
| 7 | `pg_remove_relation_drops_fk_column_and_constraint` | `DROP COLUMN CASCADE` on the FK column also removes the FK constraint |
| 8 | `pg_retrofit_adds_fk_constraint_in_place` | The boss fight: retrofit adds an `ALTER TABLE ADD CONSTRAINT FOREIGN KEY` on an existing column, the constraint is enforced (RESTRICT blocks parent delete), and the planner's emitted SQL contains no recreate-table |

Each test creates uniquely-named scratch tables (`pg_t_<pid>_<seq>`), runs its SQL, asserts against `information_schema`, then drops its tables. No shared state, no test ordering required. Run individually with `cargo test <name> -- --ignored --exact`.

## Every PRAGMA / AUTOINCREMENT removal

PRAGMA emission sites (all SQLite-specific):
- `executor.rs:524–528` — comment block in `apply_add_relation` doc explaining PRAGMA → removed
- `executor.rs:660–664` — `sql_for_add_fk_column` doc + emission line → removed
- `executor.rs:830–865` — `generate_sqlite_recreate_table_migration_fk_aware` body → entire fn deleted
- `executor.rs:3100–3126` — `plan_retrofit_foreign_keys` recreate-table block → replaced with ADD CONSTRAINT
- `executor.rs:1349–1382` — `generate_sqlite_recreate_table_migration` body → entire fn deleted

AUTOINCREMENT emission sites:
- `executor.rs:1404` — `column_def` returned `id INTEGER PRIMARY KEY AUTOINCREMENT` for the id field → entire fn deleted (PG uses BIGSERIAL on table CREATE, which lives in user-written migrations, not in executor-generated SQL)

Final sweep result: zero `PRAGMA` / `AUTOINCREMENT` in any string the executor emits. Remaining hits are all in test assertions verifying the *absence* of those keywords, plus doc comments explaining the PG behavior.

## Helper functions deleted (dead after the rewrites)

- `generate_sqlite_recreate_table_migration` (~40 LOC)
- `generate_sqlite_recreate_table_migration_fk_aware` (~50 LOC)
- `column_def` + `column_def_with_relation_context` (~50 LOC)
- `table_has_any_foreign_key` (~30 LOC)

Net diff for the rewrite: **+200 LOC of new code (mostly test plumbing) and –250 LOC of deleted SQLite machinery**. The executor is roughly **50 LOC shorter** after Phase 2 despite gaining a parallel integration-test layer.

## Non-obvious decisions

1. **`apply_add_relation` keeps the `required: true` refusal.** PG can `ALTER TABLE … ADD COLUMN BIGINT NOT NULL REFERENCES …` only on an *empty* table. On a populated table it raises "column contains null values". Since the AI primitive doesn't carry a backfill expression, the executor refuses and points at `rustio migrate --add-fks` (the retrofit, which sequences add-nullable / backfill / SET NOT NULL into three statements). The error message updated to reflect the PG-specific reason.

2. **Retrofit emits one ALTER per relation, not per table.** Each `ALTER TABLE … ADD CONSTRAINT` is independent; wrapping multiple ALTERs in a single migration file is a pure UX choice (one file per affected child table) — the BEGIN/COMMIT around them gives operators atomic apply per table.

3. **Constraint naming convention.** Phase 2 names every retrofitted FK as `<child_table>_<via>_fk` (e.g. `applications_applicant_id_fk`). PG would auto-generate `<child>_<col>_fkey` if no name were given; making the name explicit lets future migrations reference the constraint by name (DROP CONSTRAINT, etc.).

4. **`rename_model` migration SQL is unchanged.** Per the spec table, `ALTER TABLE … RENAME TO …` is identical in PG and SQLite. Only the upstream FK guard was removed. The previous "FK rewriting scheduled for 0.6.0" deferral is moot in PG (FK refs auto-update).

5. **`pg_retrofit_adds_fk_constraint_in_place` test built the SQL directly.** The `plan_retrofit_foreign_keys` function uses `fallback_table_name(model.name)` (snake-plural of the model name) to derive table names. Building a schema fixture whose model names snake-plural to the live test-table names is awkward, so the integration test exercises the SQL shape end-to-end against PG using a hand-built ALTER (matching what the planner produces, which is also asserted separately on a deterministic fixture in the second half of the same test). The planner's deterministic test fixture is in `executor_tests.rs::retrofit_reports_every_unannotated_belongs_to`.

## Verified

In-sandbox (default suite, no DB):

- `cargo check --workspace --all-targets` — clean
- `cargo clippy --workspace --all-targets -- -D warnings` — clean
- `cargo test --workspace` — **213 passed, 0 failed, 8 ignored**

Against live Postgres (verified during the session before the Path B retrofit, commits `5a99806` → `150acfe`):

- All 8 PG integration tests passed against `postgres://postgres:dev@localhost:5432/blog`
- Blog example boots: `rustio listening on http://127.0.0.1:8000`, `/admin/login` returns 200 HTML, `/admin` returns 303

To re-verify on the host:

```bash
RUSTIO_TEST_DB=1 cargo test --workspace -- --ignored 2>&1 | grep "^test result"
```

Expected output (8 PG tests):

```
test result: ok. 8 passed; 0 failed; 0 ignored; …
```
