//! Admin action log — every create / update / delete driven through
//! the admin writes a row to `rustio_admin_actions`. The audit trail
//! powers two user-visible surfaces:
//!
//! - `GET /admin/actions` — project-wide timeline with filters.
//! - `GET /admin/<model>/<id>/history` — per-object history.
//!
//! The table ships in [`crate::auth::ensure_core_tables`] and is
//! FK-cascaded to `rustio_users`: deleting a user wipes the log
//! entries they produced, matching how sessions cascade.
//!
//! ## Integrity
//!
//! [`record`] rejects entries that are missing any of `user_id`,
//! `model_name`, or `object_id`. The caller gets an
//! [`Error::Internal`] so the admin handler can fail loudly rather
//! than silently losing the audit trail — that's what the spec
//! means by *"No logging = FAIL"*.
//!
//! ## Not included in 0.4
//!
//! - Per-field diff of what changed on update (requires reading the
//!   pre-update row and diffing; deferred).
//! - Retention / pruning (no cron). Projects that need a bounded
//!   log should run `DELETE FROM rustio_admin_actions WHERE
//!   timestamp < …` on their own cadence.

use chrono::{DateTime, Utc};
use sqlx::Row as _;

use crate::error::Error;
use crate::orm::Db;

/// The three classes of admin mutation we track. `delete` covers
/// both individual and bulk deletions — each bulk-delete row writes
/// its own `Delete` entry so object history is per-row complete.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionType {
    Create,
    Update,
    Delete,
}

impl ActionType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Create => "create",
            Self::Update => "update",
            Self::Delete => "delete",
        }
    }

    /// Parse the DB-level string back into a typed `ActionType`. Named
    /// `parse` rather than `from_str` so it doesn't shadow the standard
    /// `FromStr` trait (which returns `Result<_, _>`, not `Option<_>`).
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "create" => Some(Self::Create),
            "update" => Some(Self::Update),
            "delete" => Some(Self::Delete),
            _ => None,
        }
    }

    /// Human-readable label for the timeline.
    pub fn label(self) -> &'static str {
        match self {
            Self::Create => "Created",
            Self::Update => "Updated",
            Self::Delete => "Deleted",
        }
    }

    /// CSS pill class used by the renderer so the Recent Actions
    /// timeline reads at a glance.
    pub fn pill_class(self) -> &'static str {
        match self {
            Self::Create => "rio-pill rio-pill-emerald",
            Self::Update => "rio-pill rio-pill-indigo",
            Self::Delete => "rio-pill rio-pill-rose",
        }
    }
}

/// One action-log row as loaded from the DB. The `user_email` is
/// joined in by [`recent`] and [`for_object`] so the timeline can
/// render the acting user without a second round-trip.
#[derive(Debug, Clone)]
pub struct AdminAction {
    pub id: i64,
    pub user_id: i64,
    pub user_email: Option<String>,
    pub action_type: String,
    pub model_name: String,
    pub object_id: i64,
    pub timestamp: DateTime<Utc>,
    pub ip_address: Option<String>,
    pub summary: String,
}

/// What callers hand to [`record`]. Kept as a borrow-friendly
/// struct so handlers don't need to clone field strings.
pub struct LogEntry<'a> {
    pub user_id: i64,
    pub action_type: ActionType,
    pub model_name: &'a str,
    pub object_id: i64,
    pub ip_address: Option<&'a str>,
    pub summary: String,
}

/// Write one row to the action log.
///
/// Validates that `user_id`, `model_name`, and `object_id` are all
/// present before touching the DB — a missing field returns
/// [`Error::Internal`] and the caller propagates that as a 500. That
/// behaviour is deliberate: the admin spec requires "no logging =
/// FAIL", so a broken audit pipeline must be visible, not silent.
pub async fn record(db: &Db, entry: LogEntry<'_>) -> Result<(), Error> {
    if entry.user_id <= 0 {
        return Err(Error::Internal("admin audit: missing user_id".to_string()));
    }
    if entry.model_name.trim().is_empty() {
        return Err(Error::Internal(
            "admin audit: missing model_name".to_string(),
        ));
    }
    if entry.object_id <= 0 {
        return Err(Error::Internal(
            "admin audit: missing object_id".to_string(),
        ));
    }

    let now = Utc::now();
    sqlx::query(
        "INSERT INTO rustio_admin_actions
             (user_id, action_type, model_name, object_id, timestamp, ip_address, summary)
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(entry.user_id)
    .bind(entry.action_type.as_str())
    .bind(entry.model_name)
    .bind(entry.object_id)
    .bind(now)
    .bind(entry.ip_address)
    .bind(&entry.summary)
    .execute(db.pool())
    .await?;
    Ok(())
}

/// Fetch the most recent `limit` admin actions, newest first.
/// Optional filters by `model_name` and by `action_type` string
/// (the UI passes both through as URL query params, so we take
/// them as `&str` rather than typed enums).
pub async fn recent(
    db: &Db,
    limit: i64,
    model_filter: Option<&str>,
    action_filter: Option<&str>,
) -> Result<Vec<AdminAction>, Error> {
    // We build the query defensively with bound params — string
    // interpolation is confined to `WHERE` branches that only ever
    // interpolate known column names, never user input.
    let mut sql = String::from(
        "SELECT a.id, a.user_id, u.email AS user_email, a.action_type,
                a.model_name, a.object_id, a.timestamp, a.ip_address, a.summary
         FROM rustio_admin_actions a
         LEFT JOIN rustio_users u ON u.id = a.user_id",
    );
    let mut clauses: Vec<&'static str> = Vec::new();
    if model_filter.is_some() {
        clauses.push("a.model_name = ?");
    }
    if action_filter.is_some() {
        clauses.push("a.action_type = ?");
    }
    if !clauses.is_empty() {
        sql.push_str(" WHERE ");
        sql.push_str(&clauses.join(" AND "));
    }
    sql.push_str(" ORDER BY a.timestamp DESC, a.id DESC LIMIT ?");

    let mut q = sqlx::query(&sql);
    if let Some(m) = model_filter {
        q = q.bind(m);
    }
    if let Some(a) = action_filter {
        q = q.bind(a);
    }
    q = q.bind(limit);

    let rows = q.fetch_all(db.pool()).await?;
    rows.iter().map(row_to_action).collect()
}

/// All actions for one `(model, object_id)`, newest first.
pub async fn for_object(
    db: &Db,
    model_name: &str,
    object_id: i64,
) -> Result<Vec<AdminAction>, Error> {
    let rows = sqlx::query(
        "SELECT a.id, a.user_id, u.email AS user_email, a.action_type,
                a.model_name, a.object_id, a.timestamp, a.ip_address, a.summary
         FROM rustio_admin_actions a
         LEFT JOIN rustio_users u ON u.id = a.user_id
         WHERE a.model_name = ? AND a.object_id = ?
         ORDER BY a.timestamp DESC, a.id DESC",
    )
    .bind(model_name)
    .bind(object_id)
    .fetch_all(db.pool())
    .await?;
    rows.iter().map(row_to_action).collect()
}

fn row_to_action(r: &sqlx::sqlite::SqliteRow) -> Result<AdminAction, Error> {
    Ok(AdminAction {
        id: r.try_get("id")?,
        user_id: r.try_get("user_id")?,
        user_email: r.try_get("user_email")?,
        action_type: r.try_get("action_type")?,
        model_name: r.try_get("model_name")?,
        object_id: r.try_get("object_id")?,
        timestamp: r.try_get("timestamp")?,
        ip_address: r.try_get("ip_address")?,
        summary: r.try_get("summary")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth;

    async fn setup() -> Db {
        let db = Db::memory().await.unwrap();
        auth::ensure_core_tables(&db).await.unwrap();
        db
    }

    async fn seeded_user(db: &Db) -> i64 {
        auth::user::create(db, "x@y.co", "pw", auth::ROLE_ADMIN)
            .await
            .unwrap()
            .id
    }

    #[tokio::test]
    async fn record_round_trip_returns_through_recent() {
        let db = setup().await;
        let uid = seeded_user(&db).await;
        record(
            &db,
            LogEntry {
                user_id: uid,
                action_type: ActionType::Create,
                model_name: "tasks",
                object_id: 1,
                ip_address: Some("127.0.0.1"),
                summary: "Created Task #1: Ship".to_string(),
            },
        )
        .await
        .unwrap();

        let rs = recent(&db, 10, None, None).await.unwrap();
        assert_eq!(rs.len(), 1);
        assert_eq!(rs[0].user_id, uid);
        assert_eq!(rs[0].user_email.as_deref(), Some("x@y.co"));
        assert_eq!(rs[0].action_type, "create");
        assert_eq!(rs[0].model_name, "tasks");
        assert_eq!(rs[0].object_id, 1);
        assert_eq!(rs[0].summary, "Created Task #1: Ship");
    }

    #[tokio::test]
    async fn recent_filters_by_model() {
        let db = setup().await;
        let uid = seeded_user(&db).await;
        for (model, obj) in [("tasks", 1), ("users", 1), ("tasks", 2)] {
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
        let tasks_only = recent(&db, 10, Some("tasks"), None).await.unwrap();
        assert_eq!(tasks_only.len(), 2);
        assert!(tasks_only.iter().all(|a| a.model_name == "tasks"));
    }

    #[tokio::test]
    async fn recent_filters_by_action_type() {
        let db = setup().await;
        let uid = seeded_user(&db).await;
        record(
            &db,
            LogEntry {
                user_id: uid,
                action_type: ActionType::Create,
                model_name: "tasks",
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
                model_name: "tasks",
                object_id: 1,
                ip_address: None,
                summary: "d".into(),
            },
        )
        .await
        .unwrap();
        let deletes = recent(&db, 10, None, Some("delete")).await.unwrap();
        assert_eq!(deletes.len(), 1);
        assert_eq!(deletes[0].action_type, "delete");
    }

    #[tokio::test]
    async fn for_object_returns_newest_first() {
        let db = setup().await;
        let uid = seeded_user(&db).await;
        record(
            &db,
            LogEntry {
                user_id: uid,
                action_type: ActionType::Create,
                model_name: "tasks",
                object_id: 7,
                ip_address: None,
                summary: "first".into(),
            },
        )
        .await
        .unwrap();
        // tiny sleep so timestamps differ
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        record(
            &db,
            LogEntry {
                user_id: uid,
                action_type: ActionType::Update,
                model_name: "tasks",
                object_id: 7,
                ip_address: None,
                summary: "second".into(),
            },
        )
        .await
        .unwrap();
        let hist = for_object(&db, "tasks", 7).await.unwrap();
        assert_eq!(hist.len(), 2);
        assert_eq!(hist[0].summary, "second");
        assert_eq!(hist[1].summary, "first");
    }

    #[tokio::test]
    async fn record_rejects_missing_user_id() {
        let db = setup().await;
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
    async fn record_rejects_missing_model() {
        let db = setup().await;
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
    async fn record_rejects_missing_object_id() {
        let db = setup().await;
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

    #[tokio::test]
    async fn deleting_a_user_cascades_to_their_actions() {
        let db = setup().await;
        let uid = seeded_user(&db).await;
        record(
            &db,
            LogEntry {
                user_id: uid,
                action_type: ActionType::Create,
                model_name: "tasks",
                object_id: 1,
                ip_address: None,
                summary: "c".into(),
            },
        )
        .await
        .unwrap();
        sqlx::query("DELETE FROM rustio_users WHERE id = ?")
            .bind(uid)
            .execute(db.pool())
            .await
            .unwrap();
        let rs = recent(&db, 10, None, None).await.unwrap();
        assert!(
            rs.is_empty(),
            "FK cascade should have removed the action log entry"
        );
    }
}
