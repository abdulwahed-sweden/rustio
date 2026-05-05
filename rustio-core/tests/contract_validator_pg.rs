//! PG-gated integration tests for the Schema Contract validator
//! (Phase 14, commit 3).
//!
//! Each test:
//!   1. Connects to a real Postgres (RUSTIO_TEST_DATABASE_URL or
//!      the default `rustio_dev`).
//!   2. Creates an ephemeral table with a unique name (PID + nanos)
//!      so concurrent tests don't collide.
//!   3. Builds a Rust `ModelSchema` over that table — table name
//!      and column slice are leaked into `&'static` because
//!      `ModelSchema` requires `'static` references.
//!   4. Runs the validator and asserts the report shape.
//!   5. DROPs the table.
//!
//! All tests are gated by `#[ignore]` AND a runtime check on
//! `RUSTIO_TEST_DB=1`. Without the env var the test body returns
//! immediately. Run with:
//!
//! ```sh
//! RUSTIO_TEST_DB=1 cargo test -p rustio-core --test contract_validator_pg -- --ignored
//! ```

use rustio_core::contract::{ModelColumn, ModelSchema, RustType, SchemaFlags};
use rustio_core::contract_validator::{
    validate_all, IssueKind, ReportStatus, SchemaReport,
};
use rustio_core::orm::{Db, DbOptions};

// ---------------------------------------------------------------------------
// Test infrastructure
// ---------------------------------------------------------------------------

const IGNORE_REASON: &str =
    "needs `RUSTIO_TEST_DB=1` + a running postgres (URL via RUSTIO_TEST_DATABASE_URL or default)";

/// `true` iff the test process has opted into PG-gated tests via
/// `RUSTIO_TEST_DB=1`. Tests check this at the top of their body
/// and return early when unset — `#[ignore]` handles the default
/// "skip", this guard makes tests safe to run via `--include-ignored`
/// flags or other invocations.
fn pg_enabled() -> bool {
    std::env::var("RUSTIO_TEST_DB").is_ok()
}

async fn connect_test_db() -> Db {
    let url = std::env::var("RUSTIO_TEST_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://postgres:dev@localhost:5432/rustio_dev".into());
    let opts = DbOptions {
        max_connections: 2,
        ..DbOptions::default()
    };
    Db::connect_with(&url, opts)
        .await
        .expect("test DB connect")
}

/// Make a unique table name and leak it into `&'static str`. The
/// validator needs `&'static` for `ModelSchema::table`; in tests we
/// pay a tiny one-shot leak per test in exchange for full
/// concurrency safety (each test owns its own table).
fn unique_static(prefix: &str) -> &'static str {
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let s = format!("rcv_{prefix}_{pid}_{nanos}");
    Box::leak(s.into_boxed_str())
}

/// Build a `&'static ModelSchema` from owned column data, using
/// `Box::leak` for both the column slice and the schema itself.
/// Lives only in tests; production code constructs `ModelSchema`
/// at compile time via the macro.
fn leak_schema(
    table: &'static str,
    columns: Vec<ModelColumn>,
    pk: &'static str,
    search_index: Option<&'static str>,
) -> &'static ModelSchema {
    let cols_static: &'static [ModelColumn] = Box::leak(columns.into_boxed_slice());
    let mut s = ModelSchema::new(table, cols_static, pk);
    if let Some(idx) = search_index {
        s = s.with_search_index(idx);
    }
    Box::leak(Box::new(s))
}

/// Run the SQL `CREATE TABLE ...`. Helper to keep test bodies
/// readable.
async fn create_table(db: &Db, sql: &str) {
    sqlx::query(sql).execute(db.pool()).await.expect("create table");
}

async fn drop_table(db: &Db, table: &str) {
    let _ = sqlx::query(&format!("DROP TABLE IF EXISTS \"{table}\""))
        .execute(db.pool())
        .await;
}

/// Run validation against a single schema. `validate_all` returns
/// a `Vec<SchemaReport>`; tests want the single report.
async fn validate(db: &Db, schema: &'static ModelSchema) -> SchemaReport {
    validate_all(db, &[schema])
        .await
        .into_iter()
        .next()
        .expect("validate_all returns one report per input schema")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// Happy path: Rust contract matches the live PG table exactly →
/// status Ok, no errors, no warnings.
#[tokio::test]
#[ignore = "needs `RUSTIO_TEST_DB=1` + a running postgres (URL via RUSTIO_TEST_DATABASE_URL or default)"]
async fn validator_reports_ok_when_schema_matches_db() {
    if !pg_enabled() {
        return;
    }
    let _ = IGNORE_REASON; // keep the constant referenced
    let db = connect_test_db().await;
    let table = unique_static("ok");

    create_table(
        &db,
        &format!(
            "CREATE TABLE \"{table}\" (
                id     BIGSERIAL PRIMARY KEY,
                name   TEXT NOT NULL,
                email  TEXT,
                amount NUMERIC(12,2) NOT NULL
            )"
        ),
    )
    .await;

    let schema = leak_schema(
        table,
        vec![
            ModelColumn::new("id", "BIGSERIAL PRIMARY KEY", RustType::I64).primary_key(),
            ModelColumn::new("name", "TEXT NOT NULL", RustType::String),
            ModelColumn::new("email", "TEXT", RustType::String).nullable(),
            ModelColumn::new("amount", "NUMERIC(12,2) NOT NULL", RustType::Decimal),
        ],
        "id",
        None,
    );

    let report = validate(&db, schema).await;
    drop_table(&db, table).await;

    assert_eq!(
        report.status,
        ReportStatus::Ok,
        "expected Ok, got {:?}: errors={:?} warnings={:?}",
        report.status,
        report.errors,
        report.warnings
    );
    assert!(report.errors.is_empty(), "errors: {:?}", report.errors);
    assert!(report.warnings.is_empty(), "warnings: {:?}", report.warnings);
}

/// Rust declares a column the DB doesn't have → `MissingColumn`
/// error, status Error.
#[tokio::test]
#[ignore = "needs `RUSTIO_TEST_DB=1` + a running postgres (URL via RUSTIO_TEST_DATABASE_URL or default)"]
async fn validator_detects_missing_column() {
    if !pg_enabled() {
        return;
    }
    let db = connect_test_db().await;
    let table = unique_static("miss_col");

    // DB has only id + name.
    create_table(
        &db,
        &format!(
            "CREATE TABLE \"{table}\" (
                id   BIGSERIAL PRIMARY KEY,
                name TEXT NOT NULL
            )"
        ),
    )
    .await;

    // Rust contract claims an extra `email` column.
    let schema = leak_schema(
        table,
        vec![
            ModelColumn::new("id", "BIGSERIAL PRIMARY KEY", RustType::I64).primary_key(),
            ModelColumn::new("name", "TEXT NOT NULL", RustType::String),
            ModelColumn::new("email", "TEXT NOT NULL", RustType::String),
        ],
        "id",
        None,
    );

    let report = validate(&db, schema).await;
    drop_table(&db, table).await;

    assert_eq!(report.status, ReportStatus::Error);
    let missing: Vec<_> = report
        .errors
        .iter()
        .filter(|e| e.kind == IssueKind::MissingColumn)
        .collect();
    assert_eq!(
        missing.len(),
        1,
        "expected exactly one MissingColumn error, got: {:?}",
        report.errors
    );
    assert_eq!(missing[0].column.as_deref(), Some("email"));
}

/// DB column type doesn't match the Rust `RustType` → `TypeMismatch`
/// error.
#[tokio::test]
#[ignore = "needs `RUSTIO_TEST_DB=1` + a running postgres (URL via RUSTIO_TEST_DATABASE_URL or default)"]
async fn validator_detects_type_mismatch() {
    if !pg_enabled() {
        return;
    }
    let db = connect_test_db().await;
    let table = unique_static("type_mm");

    // DB column `amount` is INTEGER (i32-sized).
    create_table(
        &db,
        &format!(
            "CREATE TABLE \"{table}\" (
                id     BIGSERIAL PRIMARY KEY,
                amount INTEGER NOT NULL
            )"
        ),
    )
    .await;

    // Rust contract says `amount` is Decimal — incompatible with PG INTEGER.
    let schema = leak_schema(
        table,
        vec![
            ModelColumn::new("id", "BIGSERIAL PRIMARY KEY", RustType::I64).primary_key(),
            ModelColumn::new("amount", "NUMERIC(12,2) NOT NULL", RustType::Decimal),
        ],
        "id",
        None,
    );

    let report = validate(&db, schema).await;
    drop_table(&db, table).await;

    assert_eq!(report.status, ReportStatus::Error);
    let mm: Vec<_> = report
        .errors
        .iter()
        .filter(|e| e.kind == IssueKind::TypeMismatch)
        .collect();
    assert_eq!(
        mm.len(),
        1,
        "expected exactly one TypeMismatch error, got: {:?}",
        report.errors
    );
    assert_eq!(mm[0].column.as_deref(), Some("amount"));
}

/// Rust says `Option<String>` (nullable=true), DB column is
/// `NOT NULL` → `NullabilityMismatch` error.
#[tokio::test]
#[ignore = "needs `RUSTIO_TEST_DB=1` + a running postgres (URL via RUSTIO_TEST_DATABASE_URL or default)"]
async fn validator_detects_nullability_mismatch_rust_optional_db_not_null() {
    if !pg_enabled() {
        return;
    }
    let db = connect_test_db().await;
    let table = unique_static("nul_mm_a");

    // DB: email NOT NULL.
    create_table(
        &db,
        &format!(
            "CREATE TABLE \"{table}\" (
                id    BIGSERIAL PRIMARY KEY,
                email TEXT NOT NULL
            )"
        ),
    )
    .await;

    // Rust: Option<String>.
    let schema = leak_schema(
        table,
        vec![
            ModelColumn::new("id", "BIGSERIAL PRIMARY KEY", RustType::I64).primary_key(),
            ModelColumn::new("email", "TEXT", RustType::String).nullable(),
        ],
        "id",
        None,
    );

    let report = validate(&db, schema).await;
    drop_table(&db, table).await;

    assert_eq!(report.status, ReportStatus::Error);
    let nm: Vec<_> = report
        .errors
        .iter()
        .filter(|e| e.kind == IssueKind::NullabilityMismatch)
        .collect();
    assert_eq!(
        nm.len(),
        1,
        "expected exactly one NullabilityMismatch error, got: {:?}",
        report.errors
    );
    assert_eq!(nm[0].column.as_deref(), Some("email"));
}

/// Reverse direction of the previous test: Rust says non-Optional,
/// DB allows NULL → still a `NullabilityMismatch`.
#[tokio::test]
#[ignore = "needs `RUSTIO_TEST_DB=1` + a running postgres (URL via RUSTIO_TEST_DATABASE_URL or default)"]
async fn validator_detects_nullability_mismatch_rust_required_db_nullable() {
    if !pg_enabled() {
        return;
    }
    let db = connect_test_db().await;
    let table = unique_static("nul_mm_b");

    // DB: email allows NULL.
    create_table(
        &db,
        &format!(
            "CREATE TABLE \"{table}\" (
                id    BIGSERIAL PRIMARY KEY,
                email TEXT
            )"
        ),
    )
    .await;

    // Rust: String (not Optional).
    let schema = leak_schema(
        table,
        vec![
            ModelColumn::new("id", "BIGSERIAL PRIMARY KEY", RustType::I64).primary_key(),
            ModelColumn::new("email", "TEXT NOT NULL", RustType::String),
        ],
        "id",
        None,
    );

    let report = validate(&db, schema).await;
    drop_table(&db, table).await;

    assert_eq!(report.status, ReportStatus::Error);
    let nm: Vec<_> = report
        .errors
        .iter()
        .filter(|e| e.kind == IssueKind::NullabilityMismatch)
        .collect();
    assert_eq!(nm.len(), 1, "errors: {:?}", report.errors);
}

/// Table named in the contract doesn't exist in the DB →
/// `MissingTable` error.
#[tokio::test]
#[ignore = "needs `RUSTIO_TEST_DB=1` + a running postgres (URL via RUSTIO_TEST_DATABASE_URL or default)"]
async fn validator_detects_missing_table() {
    if !pg_enabled() {
        return;
    }
    let db = connect_test_db().await;

    // Pick a name we KNOW won't exist (no CREATE TABLE before
    // validation).
    let table = unique_static("doesnt_exist");
    let schema = leak_schema(
        table,
        vec![ModelColumn::new("id", "BIGSERIAL PRIMARY KEY", RustType::I64).primary_key()],
        "id",
        None,
    );

    let report = validate(&db, schema).await;

    assert_eq!(report.status, ReportStatus::Error);
    assert_eq!(report.errors.len(), 1);
    assert_eq!(report.errors[0].kind, IssueKind::MissingTable);
    assert!(report.errors[0].column.is_none());
}

/// DB has an extra column that the Rust contract doesn't mention →
/// `ExtraDbColumn` warning, status Warning (no errors).
#[tokio::test]
#[ignore = "needs `RUSTIO_TEST_DB=1` + a running postgres (URL via RUSTIO_TEST_DATABASE_URL or default)"]
async fn validator_warns_on_extra_db_columns() {
    if !pg_enabled() {
        return;
    }
    let db = connect_test_db().await;
    let table = unique_static("extra");

    // DB has id + name + audit (Rust will know nothing about audit).
    create_table(
        &db,
        &format!(
            "CREATE TABLE \"{table}\" (
                id    BIGSERIAL PRIMARY KEY,
                name  TEXT NOT NULL,
                audit TEXT
            )"
        ),
    )
    .await;

    let schema = leak_schema(
        table,
        vec![
            ModelColumn::new("id", "BIGSERIAL PRIMARY KEY", RustType::I64).primary_key(),
            ModelColumn::new("name", "TEXT NOT NULL", RustType::String),
        ],
        "id",
        None,
    );

    let report = validate(&db, schema).await;
    drop_table(&db, table).await;

    assert_eq!(
        report.status,
        ReportStatus::Warning,
        "expected Warning, got {:?}",
        report.status
    );
    assert!(report.errors.is_empty(), "errors should be empty: {:?}", report.errors);
    assert_eq!(report.warnings.len(), 1, "warnings: {:?}", report.warnings);
    assert_eq!(report.warnings[0].kind, IssueKind::ExtraDbColumn);
    assert_eq!(report.warnings[0].column.as_deref(), Some("audit"));
}

/// Rust contract claims `id` is the PK; DB has the PK on a
/// different column → `WrongPrimaryKey` error.
#[tokio::test]
#[ignore = "needs `RUSTIO_TEST_DB=1` + a running postgres (URL via RUSTIO_TEST_DATABASE_URL or default)"]
async fn validator_detects_wrong_primary_key() {
    if !pg_enabled() {
        return;
    }
    let db = connect_test_db().await;
    let table = unique_static("wrong_pk");

    // DB has the PK on `code`, not `id`.
    create_table(
        &db,
        &format!(
            "CREATE TABLE \"{table}\" (
                id   BIGINT NOT NULL,
                code TEXT PRIMARY KEY
            )"
        ),
    )
    .await;

    let schema = leak_schema(
        table,
        vec![
            // Rust claims `id` is PK, but it's not in the DB.
            ModelColumn::new("id", "BIGINT NOT NULL", RustType::I64).primary_key(),
            ModelColumn::new("code", "TEXT NOT NULL", RustType::String),
        ],
        "id",
        None,
    );

    let report = validate(&db, schema).await;
    drop_table(&db, table).await;

    assert_eq!(report.status, ReportStatus::Error);
    let pk: Vec<_> = report
        .errors
        .iter()
        .filter(|e| e.kind == IssueKind::WrongPrimaryKey)
        .collect();
    assert_eq!(
        pk.len(),
        1,
        "expected exactly one WrongPrimaryKey error, got: {:?}",
        report.errors
    );
    assert_eq!(pk[0].actual.as_deref(), Some("code"));
}

/// `validate_all` returns one report per input schema, in the
/// same order. Mix of pass + fail in a single call.
#[tokio::test]
#[ignore = "needs `RUSTIO_TEST_DB=1` + a running postgres (URL via RUSTIO_TEST_DATABASE_URL or default)"]
async fn validate_all_returns_one_report_per_schema_in_order() {
    if !pg_enabled() {
        return;
    }
    let db = connect_test_db().await;
    let good_table = unique_static("multi_good");
    let bad_table = unique_static("multi_bad");

    create_table(
        &db,
        &format!(
            "CREATE TABLE \"{good_table}\" (
                id BIGSERIAL PRIMARY KEY
            )"
        ),
    )
    .await;
    // Note: bad_table is intentionally NOT created.

    let good = leak_schema(
        good_table,
        vec![ModelColumn::new("id", "BIGSERIAL PRIMARY KEY", RustType::I64).primary_key()],
        "id",
        None,
    );
    let bad = leak_schema(
        bad_table,
        vec![ModelColumn::new("id", "BIGSERIAL PRIMARY KEY", RustType::I64).primary_key()],
        "id",
        None,
    );

    let reports = validate_all(&db, &[good, bad]).await;
    drop_table(&db, good_table).await;

    assert_eq!(reports.len(), 2, "one report per input schema");
    assert_eq!(reports[0].table, good_table);
    assert_eq!(reports[0].status, ReportStatus::Ok);
    assert_eq!(reports[1].table, bad_table);
    assert_eq!(reports[1].status, ReportStatus::Error);
    assert_eq!(reports[1].errors[0].kind, IssueKind::MissingTable);
}

/// Sanity: every error issue carries a non-empty message that names
/// the table and (for column-scoped issues) the column. Renderers
/// rely on this.
#[tokio::test]
#[ignore = "needs `RUSTIO_TEST_DB=1` + a running postgres (URL via RUSTIO_TEST_DATABASE_URL or default)"]
async fn validator_messages_include_table_and_column_names() {
    if !pg_enabled() {
        return;
    }
    let db = connect_test_db().await;
    let table = unique_static("msgs");

    create_table(
        &db,
        &format!("CREATE TABLE \"{table}\" (id BIGSERIAL PRIMARY KEY)"),
    )
    .await;

    // Rust expects an extra column → MissingColumn error.
    let schema = leak_schema(
        table,
        vec![
            ModelColumn::new("id", "BIGSERIAL PRIMARY KEY", RustType::I64).primary_key(),
            ModelColumn::new("name", "TEXT NOT NULL", RustType::String)
                .with_flags(SchemaFlags::searchable()),
        ],
        "id",
        None,
    );

    let report = validate(&db, schema).await;
    drop_table(&db, table).await;

    assert_eq!(report.status, ReportStatus::Error);
    let m = &report.errors[0].message;
    assert!(
        m.contains(table) && m.contains("name"),
        "error message must name the table and column — got: {m}"
    );
}
