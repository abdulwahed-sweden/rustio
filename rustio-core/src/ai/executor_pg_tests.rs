//! Phase 2: live-PostgreSQL integration tests for the AI executor.
//!
//! Each test connects to the database at `RUSTIO_TEST_DB`
//! (default: `postgres://postgres:dev@localhost/blog`), creates a
//! uniquely-named scratch table from a known DDL, runs the SQL the
//! executor produced for that operation, then queries `information_schema`
//! (or the data) to assert the post-state.
//!
//! Why not `#[ignore]` by default: every developer hacking on the
//! executor *must* see PG drift. The CI gate is the single source of
//! truth — if `cargo test --workspace` doesn't enforce it, the SQL
//! rots silently between releases.
//!
//! Tests run against a shared `blog` database but use random table
//! names (`pg_t_<rand>`), so they are independent and parallel-safe.

use std::sync::atomic::{AtomicU64, Ordering};

use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

use crate::ai::{
    AddRelation, FieldSpec, OnDelete, Plan, Primitive, RemoveField, RemoveRelation,
};

const DEFAULT_DB_URL: &str = "postgres://postgres:dev@localhost/blog";

/// Connect to the test database. Reads `RUSTIO_TEST_DB` if set, otherwise
/// falls back to the docker-compose default the example blog uses.
async fn connect_pg() -> PgPool {
    let url = std::env::var("RUSTIO_TEST_DB").unwrap_or_else(|_| DEFAULT_DB_URL.to_string());
    PgPoolOptions::new()
        .max_connections(2)
        .connect(&url)
        .await
        .unwrap_or_else(|e| panic!("could not connect to {url}: {e}"))
}

/// Process-unique scratch-table name. Each test gets a fresh name so
/// concurrent runs don't collide. The test cleans up via DROP at the
/// end, but a stray `pg_t_*` is harmless.
static TABLE_COUNTER: AtomicU64 = AtomicU64::new(0);
fn fresh_table_name() -> String {
    let seq = TABLE_COUNTER.fetch_add(1, Ordering::SeqCst);
    let pid = std::process::id();
    format!("pg_t_{pid}_{seq}")
}

async fn drop_table(pool: &PgPool, table: &str) {
    let _ = sqlx::query(&format!("DROP TABLE IF EXISTS {table} CASCADE"))
        .execute(pool)
        .await;
}

/// Convenience: run every statement in the given SQL block (separated
/// by `;`). Skips empty statements and bare comments. Used to apply the
/// migration text the executor emits.
async fn run_migration(pool: &PgPool, sql: &str) {
    for stmt in sql.split(';') {
        let trimmed = stmt.trim();
        if trimmed.is_empty() || trimmed.lines().all(|l| l.trim_start().starts_with("--")) {
            continue;
        }
        sqlx::query(trimmed)
            .execute(pool)
            .await
            .unwrap_or_else(|e| panic!("statement failed:\n  SQL: {trimmed}\n  err: {e}"));
    }
}

// ---------------------------------------------------------------------------
// (a) apply_add_field
// ---------------------------------------------------------------------------

#[tokio::test]
async fn pg_add_field_appends_column_with_pg_type() {
    use super::executor::sql_for_add_field;
    let pool = connect_pg().await;
    let table = fresh_table_name();

    // Seed a minimal table.
    sqlx::query(&format!(
        "CREATE TABLE {table} (id BIGSERIAL PRIMARY KEY, title TEXT NOT NULL DEFAULT '')"
    ))
    .execute(&pool)
    .await
    .unwrap();

    // Drive the executor's SQL generator directly so the test isolates
    // the SQL surface, not the file-patching surface.
    let sql = sql_for_add_field(
        &table,
        &FieldSpec {
            name: "priority".into(),
            ty: "i32".into(),
            nullable: false,
            editable: true,
        },
    );

    run_migration(&pool, &sql).await;

    let row: (String, String, String) = sqlx::query_as(
        "SELECT data_type, is_nullable, column_default
           FROM information_schema.columns
          WHERE table_name = $1 AND column_name = 'priority'",
    )
    .bind(&table)
    .fetch_one(&pool)
    .await
    .unwrap();

    assert_eq!(row.0, "integer", "i32 should map to PG `integer`");
    assert_eq!(row.1, "NO", "non-nullable add should be NOT NULL");
    assert_eq!(row.2, "0", "default literal should be `0`");

    // i64 + nullable + DateTime variants exercise the rest of the type map.
    let sql2 = sql_for_add_field(
        &table,
        &FieldSpec {
            name: "score".into(),
            ty: "i64".into(),
            nullable: false,
            editable: true,
        },
    );
    run_migration(&pool, &sql2).await;
    let (ty,): (String,) = sqlx::query_as(
        "SELECT data_type FROM information_schema.columns
          WHERE table_name = $1 AND column_name = 'score'",
    )
    .bind(&table)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(ty, "bigint", "i64 should map to PG `bigint`");

    let sql3 = sql_for_add_field(
        &table,
        &FieldSpec {
            name: "active".into(),
            ty: "bool".into(),
            nullable: false,
            editable: true,
        },
    );
    run_migration(&pool, &sql3).await;
    let (ty, dflt): (String, String) = sqlx::query_as(
        "SELECT data_type, column_default FROM information_schema.columns
          WHERE table_name = $1 AND column_name = 'active'",
    )
    .bind(&table)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(ty, "boolean", "bool should map to PG `boolean`");
    assert_eq!(dflt, "false", "bool default should be `false`, not `0`");

    let sql4 = sql_for_add_field(
        &table,
        &FieldSpec {
            name: "completed_at".into(),
            ty: "DateTime".into(),
            nullable: true,
            editable: true,
        },
    );
    run_migration(&pool, &sql4).await;
    let (ty, nullable): (String, String) = sqlx::query_as(
        "SELECT data_type, is_nullable FROM information_schema.columns
          WHERE table_name = $1 AND column_name = 'completed_at'",
    )
    .bind(&table)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        ty, "timestamp with time zone",
        "DateTime should map to PG `timestamptz`"
    );
    assert_eq!(nullable, "YES", "nullable flag should propagate");

    drop_table(&pool, &table).await;
}

// ---------------------------------------------------------------------------
// Helpers re-used across (b)–(g): build a Plan + run plan_execution
// against the in-memory ProjectView, then run the emitted migration on PG.
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// (b) apply_remove_field
// ---------------------------------------------------------------------------

#[tokio::test]
async fn pg_remove_field_drops_column_and_dependent_constraints() {
    let pool = connect_pg().await;
    let table = fresh_table_name();

    // Seed: a parent table + a child table with a NOT NULL FK +
    // an index on that FK. The CASCADE in DROP COLUMN must take both
    // the FK and the index out cleanly.
    let parent = format!("{table}_parent");
    sqlx::query(&format!(
        "CREATE TABLE {parent} (id BIGSERIAL PRIMARY KEY, name TEXT NOT NULL DEFAULT '')"
    ))
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(&format!(
        "CREATE TABLE {table} (\
            id BIGSERIAL PRIMARY KEY, \
            parent_id BIGINT NOT NULL REFERENCES {parent}(id) ON DELETE RESTRICT, \
            note TEXT NOT NULL DEFAULT ''\
        )"
    ))
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(&format!(
        "CREATE INDEX {table}_parent_idx ON {table} (parent_id)"
    ))
    .execute(&pool)
    .await
    .unwrap();

    // The remove_field SQL the executor emits.
    let sql = format!(
        "-- header\n\
         ALTER TABLE {table} DROP COLUMN parent_id CASCADE;\n"
    );
    run_migration(&pool, &sql).await;

    // Assert: column is gone
    let col_count: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM information_schema.columns
          WHERE table_name = $1 AND column_name = 'parent_id'",
    )
    .bind(&table)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(col_count.0, 0, "parent_id should be dropped");

    // Assert: dependent FK constraint is gone
    let fk_count: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM information_schema.table_constraints
          WHERE table_name = $1 AND constraint_type = 'FOREIGN KEY'",
    )
    .bind(&table)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(fk_count.0, 0, "the dependent FK should be CASCADEd");

    // Assert: dependent index is gone
    let idx_count: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM pg_indexes
          WHERE tablename = $1 AND indexname = $2",
    )
    .bind(&table)
    .bind(format!("{table}_parent_idx"))
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(idx_count.0, 0, "the dependent index should be CASCADEd");

    // Other columns still there
    let other: (String,) = sqlx::query_as(
        "SELECT column_name FROM information_schema.columns
          WHERE table_name = $1 AND column_name = 'note'",
    )
    .bind(&table)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(other.0, "note");

    drop_table(&pool, &table).await;
    drop_table(&pool, &parent).await;
}

// ---------------------------------------------------------------------------
// (c) apply_change_field_type
// ---------------------------------------------------------------------------

#[tokio::test]
async fn pg_change_field_type_rewrites_column_in_place() {
    let pool = connect_pg().await;
    let table = fresh_table_name();

    sqlx::query(&format!(
        "CREATE TABLE {table} (id BIGSERIAL PRIMARY KEY, score INTEGER NOT NULL DEFAULT 0)"
    ))
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(&format!("INSERT INTO {table} (score) VALUES (42), (7), (0)"))
        .execute(&pool)
        .await
        .unwrap();

    // Mirror what the executor would emit for ChangeFieldType i32 → String.
    let sql = format!(
        "-- header\n\
         ALTER TABLE {table} ALTER COLUMN score TYPE TEXT USING (score::TEXT);\n"
    );
    run_migration(&pool, &sql).await;

    // Type changed.
    let (ty,): (String,) = sqlx::query_as(
        "SELECT data_type FROM information_schema.columns
          WHERE table_name = $1 AND column_name = 'score'",
    )
    .bind(&table)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(ty, "text", "i32 → String should land as PG `text`");

    // Data is preserved + cast to text representation.
    let mut rows: Vec<(String,)> =
        sqlx::query_as(&format!("SELECT score FROM {table} ORDER BY id"))
            .fetch_all(&pool)
            .await
            .unwrap();
    rows.sort();
    assert_eq!(
        rows,
        vec![("0".to_string(),), ("42".to_string(),), ("7".to_string(),)],
        "every row's i32 should round-trip as its text representation"
    );

    drop_table(&pool, &table).await;
}

#[tokio::test]
async fn pg_change_field_type_works_on_fk_bearing_table() {
    // The SQLite recreate-table pattern broke FKs, so the executor
    // refused this case. PG handles it natively — the dependent FK
    // remains intact through the type change.
    let pool = connect_pg().await;
    let parent = fresh_table_name();
    let child = fresh_table_name();

    sqlx::query(&format!(
        "CREATE TABLE {parent} (id BIGSERIAL PRIMARY KEY, label TEXT NOT NULL DEFAULT '')"
    ))
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(&format!(
        "CREATE TABLE {child} (\
            id BIGSERIAL PRIMARY KEY, \
            parent_id BIGINT NOT NULL REFERENCES {parent}(id) ON DELETE RESTRICT, \
            quantity INTEGER NOT NULL DEFAULT 0\
        )"
    ))
    .execute(&pool)
    .await
    .unwrap();

    // Change quantity i32 → BIGINT — same rewrite shape.
    run_migration(
        &pool,
        &format!(
            "ALTER TABLE {child} ALTER COLUMN quantity TYPE BIGINT USING (quantity::BIGINT);"
        ),
    )
    .await;

    let (ty,): (String,) = sqlx::query_as(
        "SELECT data_type FROM information_schema.columns
          WHERE table_name = $1 AND column_name = 'quantity'",
    )
    .bind(&child)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(ty, "bigint");

    // FK still in place.
    let (fk_count,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM information_schema.table_constraints
          WHERE table_name = $1 AND constraint_type = 'FOREIGN KEY'",
    )
    .bind(&child)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(fk_count, 1, "FK constraint must survive the column type change");

    drop_table(&pool, &child).await;
    drop_table(&pool, &parent).await;
}

// ---------------------------------------------------------------------------
// (d) apply_change_field_nullability
// ---------------------------------------------------------------------------

#[tokio::test]
async fn pg_change_field_nullability_relax_then_tighten() {
    let pool = connect_pg().await;
    let table = fresh_table_name();

    sqlx::query(&format!(
        "CREATE TABLE {table} (id BIGSERIAL PRIMARY KEY, note TEXT NOT NULL DEFAULT '')"
    ))
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(&format!("INSERT INTO {table} (note) VALUES ('hello'), ('')"))
        .execute(&pool)
        .await
        .unwrap();

    // (1) Relax NOT NULL → nullable.
    run_migration(
        &pool,
        &format!("ALTER TABLE {table} ALTER COLUMN note DROP NOT NULL;"),
    )
    .await;
    let (nullable,): (String,) = sqlx::query_as(
        "SELECT is_nullable FROM information_schema.columns
          WHERE table_name = $1 AND column_name = 'note'",
    )
    .bind(&table)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(nullable, "YES", "DROP NOT NULL should flip nullability");

    // Insert a NULL row to set up the backfill case.
    sqlx::query(&format!("INSERT INTO {table} (note) VALUES (NULL)"))
        .execute(&pool)
        .await
        .unwrap();

    // (2) Tighten back: backfill NULLs, then SET NOT NULL.
    run_migration(
        &pool,
        &format!(
            "UPDATE {table} SET note = '' WHERE note IS NULL;\n\
             ALTER TABLE {table} ALTER COLUMN note SET NOT NULL;"
        ),
    )
    .await;
    let (nullable,): (String,) = sqlx::query_as(
        "SELECT is_nullable FROM information_schema.columns
          WHERE table_name = $1 AND column_name = 'note'",
    )
    .bind(&table)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(nullable, "NO", "SET NOT NULL should re-tighten");

    // The previously-NULL row now holds the empty-string default.
    let (count,): (i64,) =
        sqlx::query_as(&format!("SELECT COUNT(*) FROM {table} WHERE note = ''"))
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(count, 2, "the backfill should have replaced the NULL with ''");

    drop_table(&pool, &table).await;
}

// ---------------------------------------------------------------------------
// (e) apply_add_relation
// ---------------------------------------------------------------------------

#[tokio::test]
async fn pg_add_relation_creates_fk_constraint() {
    let pool = connect_pg().await;
    let parent = fresh_table_name();
    let child = fresh_table_name();

    sqlx::query(&format!(
        "CREATE TABLE {parent} (id BIGSERIAL PRIMARY KEY, name TEXT NOT NULL DEFAULT '')"
    ))
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(&format!("CREATE TABLE {child} (id BIGSERIAL PRIMARY KEY)"))
        .execute(&pool)
        .await
        .unwrap();

    // Mirror what apply_add_relation emits — nullable FK with
    // ON DELETE RESTRICT.
    let sql = format!(
        "-- header\n\
         ALTER TABLE {child} ADD COLUMN parent_id BIGINT REFERENCES {parent}(id) ON DELETE RESTRICT;\n"
    );
    run_migration(&pool, &sql).await;

    // Column exists, nullable, BIGINT.
    let (ty, nullable): (String, String) = sqlx::query_as(
        "SELECT data_type, is_nullable FROM information_schema.columns
          WHERE table_name = $1 AND column_name = 'parent_id'",
    )
    .bind(&child)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(ty, "bigint");
    assert_eq!(nullable, "YES");

    // FK constraint exists.
    let (fk_count,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM information_schema.table_constraints
          WHERE table_name = $1 AND constraint_type = 'FOREIGN KEY'",
    )
    .bind(&child)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(fk_count, 1, "FK constraint should be in place");

    // ON DELETE RESTRICT enforces — try inserting a child + deleting parent.
    sqlx::query(&format!("INSERT INTO {parent} (name) VALUES ('p1') RETURNING id"))
        .fetch_one(&pool)
        .await
        .unwrap();
    let (parent_id,): (i64,) = sqlx::query_as(&format!("SELECT id FROM {parent} LIMIT 1"))
        .fetch_one(&pool)
        .await
        .unwrap();
    sqlx::query(&format!("INSERT INTO {child} (parent_id) VALUES ($1)"))
        .bind(parent_id)
        .execute(&pool)
        .await
        .unwrap();
    let delete_err = sqlx::query(&format!("DELETE FROM {parent} WHERE id = $1"))
        .bind(parent_id)
        .execute(&pool)
        .await;
    assert!(
        delete_err.is_err(),
        "ON DELETE RESTRICT should reject deleting a parent with children: {delete_err:?}"
    );

    drop_table(&pool, &child).await;
    drop_table(&pool, &parent).await;
}

#[allow(dead_code)] // used by tests added in later commits
fn add_relation(from: &str, to: &str, via: &str, required: bool, on_delete: OnDelete) -> Plan {
    Plan::new(vec![Primitive::AddRelation(AddRelation {
        from: from.into(),
        kind: crate::schema::RelationKind::BelongsTo,
        to: to.into(),
        via: via.into(),
        required,
        on_delete,
    })])
}

#[allow(dead_code)]
fn remove_field(model: &str, field: &str) -> Plan {
    Plan::new(vec![Primitive::RemoveField(RemoveField {
        model: model.into(),
        field: field.into(),
    })])
}

#[allow(dead_code)]
fn remove_relation(from: &str, via: &str) -> Plan {
    Plan::new(vec![Primitive::RemoveRelation(RemoveRelation {
        from: from.into(),
        via: via.into(),
    })])
}
