//! Phase 5b: live-PostgreSQL integration tests for the admin audit log.
//!
//! Every test is `#[ignore]` so the default `cargo test --workspace`
//! runs the pure unit tests only. To run the integration suite:
//!
//! ```bash
//! RUSTIO_TEST_DB=1 cargo test --workspace -- --ignored
//! ```
//!
//! Connection URL is read from `RUSTIO_TEST_DATABASE_URL` (falls back
//! to the docker-compose default). Mirrors the `ai::executor_pg_tests`
//! convention from Phase 2.
//!
//! ## Isolation
//!
//! Unlike the executor tests (which create uniquely-named scratch
//! tables) the audit tests share the named `rustio_admin_actions`
//! table. Isolation is per-row: each test claims a unique `model_name`
//! prefix (`pg_audit_<pid>_<seq>_*`) plus a unique email
//! (`audit_<pid>_<seq>@rustio.test`), so concurrent runs can't collide.
//! Every test cleans up its rows at the end via FK cascade on the
//! seeded user.

use std::sync::atomic::{AtomicU64, Ordering};

use tokio::sync::OnceCell;

use super::audit::{ensure_table, for_object, record, recent, ActionType, LogEntry};
use crate::auth::{create_user, init_user_tables, Role};
use crate::error::Error;
use crate::orm::{Db, DbOptions};

fn default_dev_url() -> String {
    "postgres://postgres:dev@localhost:5432/rustio_dev".to_string()
}

static SEQ: AtomicU64 = AtomicU64::new(0);

fn fresh_tag() -> String {
    let seq = SEQ.fetch_add(1, Ordering::SeqCst);
    let pid = std::process::id();
    format!("{pid}_{seq}")
}

async fn connect_db() -> Db {
    let url = std::env::var("RUSTIO_TEST_DATABASE_URL").unwrap_or_else(|_| default_dev_url());
    let opts = DbOptions {
        max_connections: 2,
        ..DbOptions::default()
    };
    Db::connect_with(&url, opts)
        .await
        .unwrap_or_else(|e| panic!("could not connect to {url}: {e}"))
}

/// Postgres' `CREATE TABLE IF NOT EXISTS` is NOT race-safe: two
/// concurrent DDL calls can both pass the existence check, and the
/// loser gets `"relation already exists"`. Cargo's test harness runs
/// tests in parallel on a thread pool, so the 8 audit tests all hit
/// `init_user_tables` + `ensure_table` simultaneously. This gate
/// runs the DDL exactly once per process; subsequent callers await
/// the completion and then skip.
static TABLES_READY: OnceCell<()> = OnceCell::const_new();

/// Open a PG connection, apply both required `CREATE TABLE`s, and
/// return a (db, tag) pair where `tag` is the per-test unique suffix
/// the caller uses to scope its rows.
async fn setup() -> (Db, String) {
    let db = connect_db().await;
    TABLES_READY
        .get_or_init(|| async {
            init_user_tables(&db).await.unwrap();
            ensure_table(&db).await.unwrap();
        })
        .await;
    let tag = fresh_tag();
    (db, tag)
}

/// Seed one user keyed to the test's `tag`. Returns the user id so the
/// caller can drive `record()` with it. When the test ends we delete
/// this user; the FK cascade on `rustio_admin_actions.user_id` wipes
/// every action the user produced in one shot.
async fn seed_user(db: &Db, tag: &str) -> i64 {
    let email = format!("audit_{tag}@rustio.test");
    create_user(db, &email, "pw-for-test", Role::Administrator).await.unwrap()
}

async fn cleanup(db: &Db, uid: i64) {
    // Cascade on user_id takes care of rustio_admin_actions rows.
    // The IF EXISTS wrapping lets the cascade test (which already
    // deleted the user) run this without hitting an error.
    let _ = sqlx::query("DELETE FROM rustio_users WHERE id = $1")
        .bind(uid)
        .execute(db.pool())
        .await;
}

// ---------------------------------------------------------------------------
// (1) record round-trip
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "needs `RUSTIO_TEST_DB=1` + a running postgres (URL via RUSTIO_TEST_DATABASE_URL or default)"]
async fn pg_record_round_trip_returns_through_recent() {
    let (db, tag) = setup().await;
    let uid = seed_user(&db, &tag).await;
    let model = format!("pg_audit_{tag}_tasks");

    record(
        &db,
        LogEntry {
            user_id: uid,
            action_type: ActionType::Create,
            model_name: &model,
            object_id: 1,
            ip_address: Some("127.0.0.1"),
            summary: "Created Task #1: Ship".to_string(),
        },
    )
    .await
    .unwrap();

    let rs = recent(&db, 10, Some(&model), None).await.unwrap();
    assert_eq!(rs.len(), 1);
    assert_eq!(rs[0].user_id, uid);
    assert_eq!(rs[0].user_email.as_deref(), Some(format!("audit_{tag}@rustio.test").as_str()));
    assert_eq!(rs[0].action_type, "create");
    assert_eq!(rs[0].model_name, model);
    assert_eq!(rs[0].object_id, 1);
    assert_eq!(rs[0].summary, "Created Task #1: Ship");
    assert_eq!(rs[0].ip_address.as_deref(), Some("127.0.0.1"));

    cleanup(&db, uid).await;
}

// ---------------------------------------------------------------------------
// (2) recent() model filter
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "needs `RUSTIO_TEST_DB=1` + a running postgres (URL via RUSTIO_TEST_DATABASE_URL or default)"]
async fn pg_recent_filters_by_model() {
    let (db, tag) = setup().await;
    let uid = seed_user(&db, &tag).await;
    let tasks_model = format!("pg_audit_{tag}_tasks");
    let users_model = format!("pg_audit_{tag}_users");

    for (model, obj) in [
        (tasks_model.as_str(), 1),
        (users_model.as_str(), 1),
        (tasks_model.as_str(), 2),
    ] {
        record(
            &db,
            LogEntry {
                user_id: uid,
                action_type: ActionType::Create,
                model_name: model,
                object_id: obj,
                ip_address: None,
                summary: format!("Created {model} #{obj}"),
            },
        )
        .await
        .unwrap();
    }

    let tasks_only = recent(&db, 10, Some(&tasks_model), None).await.unwrap();
    assert_eq!(tasks_only.len(), 2);
    assert!(tasks_only.iter().all(|a| a.model_name == tasks_model));

    cleanup(&db, uid).await;
}

// ---------------------------------------------------------------------------
// (3) recent() action_type filter
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "needs `RUSTIO_TEST_DB=1` + a running postgres (URL via RUSTIO_TEST_DATABASE_URL or default)"]
async fn pg_recent_filters_by_action_type() {
    let (db, tag) = setup().await;
    let uid = seed_user(&db, &tag).await;
    let model = format!("pg_audit_{tag}_tasks");

    record(
        &db,
        LogEntry {
            user_id: uid,
            action_type: ActionType::Create,
            model_name: &model,
            object_id: 1,
            ip_address: None,
            summary: "c".into(),
        },
    )
    .await
    .unwrap();
    record(
        &db,
        LogEntry {
            user_id: uid,
            action_type: ActionType::Delete,
            model_name: &model,
            object_id: 1,
            ip_address: None,
            summary: "d".into(),
        },
    )
    .await
    .unwrap();

    let deletes = recent(&db, 10, Some(&model), Some("delete")).await.unwrap();
    assert_eq!(deletes.len(), 1);
    assert_eq!(deletes[0].action_type, "delete");

    cleanup(&db, uid).await;
}

// ---------------------------------------------------------------------------
// (4) for_object newest-first
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "needs `RUSTIO_TEST_DB=1` + a running postgres (URL via RUSTIO_TEST_DATABASE_URL or default)"]
async fn pg_for_object_returns_newest_first() {
    let (db, tag) = setup().await;
    let uid = seed_user(&db, &tag).await;
    let model = format!("pg_audit_{tag}_tasks");

    record(
        &db,
        LogEntry {
            user_id: uid,
            action_type: ActionType::Create,
            model_name: &model,
            object_id: 7,
            ip_address: None,
            summary: "first".into(),
        },
    )
    .await
    .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    record(
        &db,
        LogEntry {
            user_id: uid,
            action_type: ActionType::Update,
            model_name: &model,
            object_id: 7,
            ip_address: None,
            summary: "second".into(),
        },
    )
    .await
    .unwrap();

    let hist = for_object(&db, &model, 7).await.unwrap();
    assert_eq!(hist.len(), 2);
    assert_eq!(hist[0].summary, "second");
    assert_eq!(hist[1].summary, "first");

    cleanup(&db, uid).await;
}

// ---------------------------------------------------------------------------
// (5)–(7) validation refuses partial entries
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "needs `RUSTIO_TEST_DB=1` + a running postgres (URL via RUSTIO_TEST_DATABASE_URL or default)"]
async fn pg_record_rejects_missing_user_id() {
    let (db, _tag) = setup().await;
    let err = record(
        &db,
        LogEntry {
            user_id: 0,
            action_type: ActionType::Create,
            model_name: "tasks",
            object_id: 1,
            ip_address: None,
            summary: "nope".into(),
        },
    )
    .await;
    assert!(matches!(err, Err(Error::Internal(_))));
}

#[tokio::test]
#[ignore = "needs `RUSTIO_TEST_DB=1` + a running postgres (URL via RUSTIO_TEST_DATABASE_URL or default)"]
async fn pg_record_rejects_missing_model() {
    let (db, _tag) = setup().await;
    let err = record(
        &db,
        LogEntry {
            user_id: 1,
            action_type: ActionType::Create,
            model_name: "",
            object_id: 1,
            ip_address: None,
            summary: "nope".into(),
        },
    )
    .await;
    assert!(matches!(err, Err(Error::Internal(_))));
}

#[tokio::test]
#[ignore = "needs `RUSTIO_TEST_DB=1` + a running postgres (URL via RUSTIO_TEST_DATABASE_URL or default)"]
async fn pg_record_rejects_missing_object_id() {
    let (db, _tag) = setup().await;
    let err = record(
        &db,
        LogEntry {
            user_id: 1,
            action_type: ActionType::Create,
            model_name: "tasks",
            object_id: 0,
            ip_address: None,
            summary: "nope".into(),
        },
    )
    .await;
    assert!(matches!(err, Err(Error::Internal(_))));
}

// ---------------------------------------------------------------------------
// (8) FK cascade
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "needs `RUSTIO_TEST_DB=1` + a running postgres (URL via RUSTIO_TEST_DATABASE_URL or default)"]
async fn pg_deleting_a_user_cascades_to_their_actions() {
    let (db, tag) = setup().await;
    let uid = seed_user(&db, &tag).await;
    let model = format!("pg_audit_{tag}_tasks");

    record(
        &db,
        LogEntry {
            user_id: uid,
            action_type: ActionType::Create,
            model_name: &model,
            object_id: 1,
            ip_address: None,
            summary: "c".into(),
        },
    )
    .await
    .unwrap();

    sqlx::query("DELETE FROM rustio_users WHERE id = $1")
        .bind(uid)
        .execute(db.pool())
        .await
        .unwrap();

    let rs = recent(&db, 10, Some(&model), None).await.unwrap();
    assert!(
        rs.is_empty(),
        "FK cascade should have removed the action log entry",
    );
}
