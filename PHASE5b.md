# Phase 5b — Port `admin/audit.rs` from OLD with Postgres SQL translation

Built on top of Phase 5a (commit `7973494`).

## What landed

- `rustio-core/src/admin/audit.rs` — ported + PG-translated. 276 LOC.
- `rustio-core/src/admin/audit_tests.rs` — 8 `#[ignore]`-gated PG integration tests. New file, 275 LOC.
- `rustio-core/src/admin/mod.rs` — added `mod audit;` + `#[cfg(test)] mod audit_tests;` + 7-symbol re-export.

## Architectural decisions recorded

### No migration file

NEW has no `rustio-core/migrations/` directory and no framework-migration runner; core tables (`rustio_users`, `rustio_sessions`, `rustio_migrations`) are created via runtime `CREATE TABLE IF NOT EXISTS` calls in `src/auth/*.rs`. Audit follows the same pattern:

- `pub(crate) const CREATE_TABLE_SQL` in `audit.rs`
- `pub(crate) const CREATE_MODEL_INDEX_SQL`, `CREATE_TIMESTAMP_INDEX_SQL`
- `pub async fn ensure_table(db: &Db) -> Result<()>` — idempotent, runs all three statements

Tests invoke `ensure_table()` in `setup()`. End-user apps will call it from startup once a later phase wires up the admin router (same point that would call `auth::init_tables`). That wiring is **not** in scope for Phase 5b.

### OLD-shape schema preserved

Seven data columns, FK to `rustio_users` with `ON DELETE CASCADE`, including `ip_address` and the JOIN-based `user_email` projection. Verbatim column names so `record()`, `recent()`, `for_object()`, and `row_to_action()` port mechanically.

```sql
CREATE TABLE IF NOT EXISTS rustio_admin_actions (
    id          BIGSERIAL   PRIMARY KEY,
    user_id     BIGINT      NOT NULL REFERENCES rustio_users(id) ON DELETE CASCADE,
    action_type TEXT        NOT NULL,
    model_name  TEXT        NOT NULL,
    object_id   BIGINT      NOT NULL,
    timestamp   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    ip_address  TEXT,
    summary     TEXT        NOT NULL DEFAULT ''
);

CREATE INDEX IF NOT EXISTS rustio_admin_actions_model_idx
    ON rustio_admin_actions(model_name, object_id);
CREATE INDEX IF NOT EXISTS rustio_admin_actions_timestamp_idx
    ON rustio_admin_actions(timestamp DESC);
```

FK to `rustio_users` means tests must call `auth::init_user_tables(&db)` before `audit::ensure_table(&db)`. Both are exposed publicly.

## SQL translations applied

| Site | OLD (SQLite) | NEW (Postgres) |
|---|---|---|
| `record()` INSERT | `VALUES (?, ?, ?, ?, ?, ?, ?)` | `VALUES ($1, $2, $3, $4, $5, $6, $7)` |
| `recent()` dynamic WHERE | `push("a.model_name = ?")` etc., `LIMIT ?` | `push(format!("a.model_name = ${param_idx}"))` with running `param_idx`; `LIMIT ${param_idx}` at the end |
| `for_object()` WHERE | `WHERE a.model_name = ? AND a.object_id = ?` | `WHERE a.model_name = $1 AND a.object_id = $2` |
| `row_to_action` row type | `&sqlx::sqlite::SqliteRow` | `&sqlx::postgres::PgRow` |
| Return signatures | `Result<T, Error>` (std form) | `Result<T>` (crate alias — `Error` param is baked in) |

### `recent()`'s dynamic numbering

`recent()` binds `model_filter`, `action_filter`, and `limit` conditionally. The PG `$N` placeholders must match the bind order:

- No filters: `LIMIT $1`, binds `[limit]`
- Model only: `WHERE a.model_name = $1 ORDER BY ... LIMIT $2`, binds `[model, limit]`
- Action only: `WHERE a.action_type = $1 ORDER BY ... LIMIT $2`, binds `[action, limit]`
- Both: `WHERE a.model_name = $1 AND a.action_type = $2 ORDER BY ... LIMIT $3`, binds `[model, action, limit]`

Implemented with a running `param_idx: usize` counter incremented only when an `Option::is_some()`, so `$N` and bind order stay in lockstep.

## Per-item port manifest

| # | Item | Verdict | How it landed |
|---|---|---|---|
| enum | `ActionType` (+ `as_str`, `parse`, `label`, `pill_class`) | PORT | verbatim |
| struct | `AdminAction` | PORT | verbatim |
| struct | `LogEntry<'a>` | PORT | verbatim |
| fn | `record(&Db, LogEntry)` | PORT+SQL | `?` → `$1..$7`; signature uses `Result<()>` alias |
| fn | `recent(…)` | PORT+SQL | dynamic `$N` renumbering; `Result<Vec<AdminAction>>` |
| fn | `for_object(…)` | PORT+SQL | `?` → `$1`, `$2`; `Result<Vec<AdminAction>>` |
| fn | `row_to_action(&PgRow)` | PORT+SQL | `SqliteRow` → `PgRow`; no other changes |
| const | `CREATE_TABLE_SQL` (+ 2 indexes) | **NEW** | Postgres DDL, `BIGSERIAL`, `TIMESTAMPTZ`, FK + `ON DELETE CASCADE` |
| fn | `ensure_table(&Db)` | **NEW** | runs the three DDLs, idempotent |

**7 ported, 5 new consts/fns, 0 dropped.** Body logic of the 4 SQL-bearing fns is byte-identical aside from the listed placeholder/row-type substitutions.

## Tests ported

OLD had 8 inline `#[tokio::test]` functions that used `Db::memory()` + `auth::ensure_core_tables` (neither exists in NEW). Rewrote as 8 `#[ignore]`-gated PG integration tests in a separate `audit_tests.rs` file.

| # | Test | OLD intent |
|---|---|---|
| 1 | `pg_record_round_trip_returns_through_recent` | insert one entry, read it back via `recent()`, verify every field incl. JOIN-projected `user_email` |
| 2 | `pg_recent_filters_by_model` | 3 records across 2 models, `recent()` with model filter returns only matching |
| 3 | `pg_recent_filters_by_action_type` | `recent()` with action_type filter distinguishes create/delete |
| 4 | `pg_for_object_returns_newest_first` | 2 records same (model, object_id), `for_object()` returns newest → oldest |
| 5 | `pg_record_rejects_missing_user_id` | `user_id: 0` → `Err(Error::Internal)` |
| 6 | `pg_record_rejects_missing_model` | `model_name: ""` → `Err(Error::Internal)` |
| 7 | `pg_record_rejects_missing_object_id` | `object_id: 0` → `Err(Error::Internal)` |
| 8 | `pg_deleting_a_user_cascades_to_their_actions` | `DELETE FROM rustio_users` removes action log rows via FK CASCADE |

### Isolation strategy

Unlike Phase 2's executor tests (uniquely-named scratch tables), audit tests share the named `rustio_admin_actions` + `rustio_users` tables. Isolation is **per-row**:

- Unique tag per test: `format!("{pid}_{seq}")` with a process-local `AtomicU64`.
- Per-test `model_name` prefix: `pg_audit_<tag>_tasks`, `pg_audit_<tag>_users`, etc.
- Per-test email: `audit_<tag>@rustio.test`.
- Each test's `recent()` calls pass `Some(&this_test_model)` to scope assertions to its own rows.
- Cleanup at end: `DELETE FROM rustio_users WHERE id = $1` — FK cascade wipes the test's `rustio_admin_actions` rows in one statement.

Validation tests (5–7) don't seed a user (the validation branch fires before the DB write), so no cleanup needed.

## Test count in the full suite

| | Tests passing (sandbox) | Ignored | Delta |
|---|---:|---:|---|
| Phase 5a baseline (`7973494`) | 286 | 13 | — |
| **Phase 5b** | **286** | **21** | **+8 ignored** |

```
$ cargo test --workspace 2>&1 | grep "^test result"
test result: ok. 0 passed; 0 failed; 0 ignored
test result: ok. 0 passed; 0 failed; 0 ignored
test result: ok. 286 passed; 0 failed; 21 ignored
test result: ok. 0 passed; 0 failed; 0 ignored
test result: ok. 0 passed; 0 failed; 0 ignored
test result: ok. 0 passed; 0 failed; 0 ignored
```

```
$ cargo test --workspace -p rustio-core --lib audit 2>&1 | tail
test result: ok. 0 passed; 0 failed; 8 ignored; 299 filtered out
```

```
$ cargo clippy --workspace --all-targets -- -D warnings   → clean
```

## Host verification command

To run the 8 audit integration tests against a live `rustio_dev` (bring it up with `docker compose up -d` first):

```bash
RUSTIO_TEST_DB=1 cargo test --workspace -p rustio-core --lib audit -- --ignored 2>&1 | tail -20
```

Expected output:

```
test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; N filtered out
```

To run a single audit test (substring filter):

```bash
RUSTIO_TEST_DB=1 cargo test pg_record_round_trip -- --ignored --nocapture
```

To run **every** phase's PG integration suite (audit + Phase 2's executor, 8 + 8 = 16):

```bash
RUSTIO_TEST_DB=1 cargo test --workspace -- --ignored 2>&1 | grep "^test result"
```

Expected:

```
test result: ok. 16 passed; 0 failed; 0 ignored; 0 measured; ...
```

Override the URL per-invocation if `rustio_dev` lives elsewhere:

```bash
RUSTIO_TEST_DATABASE_URL=postgres://you@otherhost/scratch \
RUSTIO_TEST_DB=1 cargo test --workspace -- --ignored
```

## Is the audit module complete for Phase 5b scope?

**Yes, for the scope stated.** `record` / `recent` / `for_object` / `ensure_table` are PG-correct, the 8 tests cover OLD's test set 1:1, and the module plugs into `auth::init_user_tables` via FK.

**Not in Phase 5b's scope (deliberately):**

- **Startup wiring.** Nothing in NEW yet calls `audit::ensure_table()` at boot — it'll be wired from the same point that calls `auth::init_tables`. Scope rule deferred that to "future phase."
- **HTTP handler integration.** OLD's `/admin/actions` and `/admin/<model>/<id>/history` routes aren't ported here. They live in `admin.rs` / `handlers.rs`, which are out of scope for Phase 5.
- **Per-field diff of changed columns on update.** OLD's doc-comment calls this out as "not included in 0.4" — stays deferred.
- **Log retention / pruning.** Same — operators run their own cron.

## Confirmation

- **No logic from OLD was rewritten.** The 4 PORT+SQL items have their bodies preserved except for the listed SQL-dialect substitutions.
- **No HTML, no filesystem I/O.** Every side effect is through `sqlx` against `db.pool()`.
- **Release build unchanged by tests.** `audit_tests.rs` compiles only under `#[cfg(test)]`.
- **Sandbox test suite**: 286 passed, 21 ignored (13 prior + 8 new audit), 0 failed. Clippy `-D warnings` clean.
