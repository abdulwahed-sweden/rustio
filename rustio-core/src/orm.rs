//! SQLite-backed ORM.
//!
//! Implement [`Model`] on your struct to get `find / all / create / update /
//! delete` for free. SQLx is used internally; user code never references it.
//!
//! Phase 1 supports `i32`, `i64`, `String`, and `bool` field types; the id
//! column is required to be `i64`.

use std::str::FromStr;

use chrono::{DateTime, Utc};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePool, SqlitePoolOptions, SqliteRow};
use sqlx::Row as _;

use crate::error::Error;

#[derive(Clone)]
pub struct Db {
    pool: SqlitePool,
}

impl Db {
    /// Open a pool against the given SQLite URL.
    ///
    /// Foreign-key enforcement is **always on** (`PRAGMA foreign_keys = ON`
    /// applied on every connection via sqlx's connect-time hook). SQLite
    /// ignores FK constraints unless this pragma is set per-connection,
    /// and relying on user configuration to enable it is unsafe —
    /// `ON DELETE CASCADE` in the schema would silently do nothing.
    pub async fn connect(url: &str) -> Result<Self, Error> {
        let opts = SqliteConnectOptions::from_str(url)
            .map_err(|e| Error::Internal(format!("invalid database URL: {e}")))?
            .foreign_keys(true);
        let pool = SqlitePoolOptions::new().connect_with(opts).await?;
        Ok(Self { pool })
    }

    /// Open an in-memory pool with FK enforcement on.
    ///
    /// Limited to a single connection because each `:memory:` connection
    /// opens its *own* database; multiple would not share rows.
    pub async fn memory() -> Result<Self, Error> {
        let opts = SqliteConnectOptions::from_str("sqlite::memory:")
            .map_err(|e| Error::Internal(format!("invalid database URL: {e}")))?
            .foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(opts)
            .await?;
        Ok(Self { pool })
    }

    pub async fn execute(&self, sql: &str) -> Result<(), Error> {
        sqlx::query(sql).execute(&self.pool).await?;
        Ok(())
    }

    pub(crate) fn pool(&self) -> &SqlitePool {
        &self.pool
    }
}

pub struct Row<'a> {
    inner: &'a SqliteRow,
}

impl<'a> Row<'a> {
    pub(crate) fn new(inner: &'a SqliteRow) -> Self {
        Self { inner }
    }

    pub fn get_i32(&self, name: &str) -> Result<i32, Error> {
        self.inner.try_get(name).map_err(Error::from)
    }

    pub fn get_i64(&self, name: &str) -> Result<i64, Error> {
        self.inner.try_get(name).map_err(Error::from)
    }

    pub fn get_string(&self, name: &str) -> Result<String, Error> {
        self.inner.try_get(name).map_err(Error::from)
    }

    pub fn get_bool(&self, name: &str) -> Result<bool, Error> {
        self.inner.try_get(name).map_err(Error::from)
    }

    pub fn get_datetime(&self, name: &str) -> Result<DateTime<Utc>, Error> {
        self.inner.try_get(name).map_err(Error::from)
    }

    // Nullable variants. Each returns `None` when the column is SQL NULL;
    // any other decode failure still surfaces as `Error::Internal`.

    pub fn get_optional_i32(&self, name: &str) -> Result<Option<i32>, Error> {
        self.inner.try_get(name).map_err(Error::from)
    }

    pub fn get_optional_i64(&self, name: &str) -> Result<Option<i64>, Error> {
        self.inner.try_get(name).map_err(Error::from)
    }

    pub fn get_optional_string(&self, name: &str) -> Result<Option<String>, Error> {
        self.inner.try_get(name).map_err(Error::from)
    }

    pub fn get_optional_bool(&self, name: &str) -> Result<Option<bool>, Error> {
        self.inner.try_get(name).map_err(Error::from)
    }

    pub fn get_optional_datetime(&self, name: &str) -> Result<Option<DateTime<Utc>>, Error> {
        self.inner.try_get(name).map_err(Error::from)
    }
}

/// A typed value ready to bind to a SQL placeholder.
///
/// `#[non_exhaustive]` because we expect to add variants (`Uuid`, `Json`,
/// `Bytes`, `Decimal`) in later releases. The `bind_value` matcher below
/// must be updated in lockstep with additions here.
#[non_exhaustive]
#[derive(Debug)]
pub enum Value {
    I32(i32),
    I64(i64),
    String(String),
    Bool(bool),
    DateTime(DateTime<Utc>),
    /// NULL. Produced from `None` via the `From<Option<T>>` impls.
    Null,
}

impl From<i32> for Value {
    fn from(v: i32) -> Self {
        Value::I32(v)
    }
}

impl From<i64> for Value {
    fn from(v: i64) -> Self {
        Value::I64(v)
    }
}

impl From<String> for Value {
    fn from(v: String) -> Self {
        Value::String(v)
    }
}

impl From<&str> for Value {
    fn from(v: &str) -> Self {
        Value::String(v.to_owned())
    }
}

impl From<bool> for Value {
    fn from(v: bool) -> Self {
        Value::Bool(v)
    }
}

impl From<DateTime<Utc>> for Value {
    fn from(v: DateTime<Utc>) -> Self {
        Value::DateTime(v)
    }
}

// Blanket `Option<T>` support: any type that converts into `Value` can
// also be wrapped in `Option` for nullable columns. `None` becomes
// `Value::Null`, `Some(x)` becomes whatever `x` converts to.
impl<T> From<Option<T>> for Value
where
    T: Into<Value>,
{
    fn from(v: Option<T>) -> Self {
        match v {
            Some(inner) => inner.into(),
            None => Value::Null,
        }
    }
}

pub trait Model: Sized + Send + Sync + Unpin + 'static {
    const TABLE: &'static str;
    const COLUMNS: &'static [&'static str];
    const INSERT_COLUMNS: &'static [&'static str];

    fn id(&self) -> i64;
    fn from_row(row: Row<'_>) -> Result<Self, Error>;
    fn insert_values(&self) -> Vec<Value>;

    fn find(
        db: &Db,
        id: i64,
    ) -> impl std::future::Future<Output = Result<Option<Self>, Error>> + Send
    where
        Self: Send,
    {
        async move {
            let sql = format!(
                "SELECT {} FROM {} WHERE id = ?",
                Self::COLUMNS.join(", "),
                Self::TABLE,
            );
            let row = sqlx::query(&sql).bind(id).fetch_optional(db.pool()).await?;
            match row {
                Some(r) => Ok(Some(Self::from_row(Row::new(&r))?)),
                None => Ok(None),
            }
        }
    }

    fn all(db: &Db) -> impl std::future::Future<Output = Result<Vec<Self>, Error>> + Send {
        async move {
            let sql = format!("SELECT {} FROM {}", Self::COLUMNS.join(", "), Self::TABLE);
            let rows = sqlx::query(&sql).fetch_all(db.pool()).await?;
            rows.iter().map(|r| Self::from_row(Row::new(r))).collect()
        }
    }

    fn create<'a>(
        &'a self,
        db: &'a Db,
    ) -> impl std::future::Future<Output = Result<i64, Error>> + Send + 'a {
        async move {
            let placeholders = vec!["?"; Self::INSERT_COLUMNS.len()].join(", ");
            let sql = format!(
                "INSERT INTO {} ({}) VALUES ({})",
                Self::TABLE,
                Self::INSERT_COLUMNS.join(", "),
                placeholders,
            );
            let mut query = sqlx::query(&sql);
            for v in self.insert_values() {
                query = bind_value(query, v);
            }
            let result = query.execute(db.pool()).await?;
            Ok(result.last_insert_rowid())
        }
    }

    fn update<'a>(
        &'a self,
        db: &'a Db,
    ) -> impl std::future::Future<Output = Result<(), Error>> + Send + 'a {
        async move {
            let assignments: Vec<String> = Self::INSERT_COLUMNS
                .iter()
                .map(|c| format!("{c} = ?"))
                .collect();
            let sql = format!(
                "UPDATE {} SET {} WHERE id = ?",
                Self::TABLE,
                assignments.join(", "),
            );
            let mut query = sqlx::query(&sql);
            for v in self.insert_values() {
                query = bind_value(query, v);
            }
            query = query.bind(self.id());
            query.execute(db.pool()).await?;
            Ok(())
        }
    }

    fn delete(db: &Db, id: i64) -> impl std::future::Future<Output = Result<(), Error>> + Send {
        async move {
            let sql = format!("DELETE FROM {} WHERE id = ?", Self::TABLE);
            sqlx::query(&sql).bind(id).execute(db.pool()).await?;
            Ok(())
        }
    }
}

fn bind_value<'q>(
    query: sqlx::query::Query<'q, sqlx::Sqlite, sqlx::sqlite::SqliteArguments<'q>>,
    value: Value,
) -> sqlx::query::Query<'q, sqlx::Sqlite, sqlx::sqlite::SqliteArguments<'q>> {
    match value {
        Value::I32(v) => query.bind(v),
        Value::I64(v) => query.bind(v),
        Value::String(v) => query.bind(v),
        Value::Bool(v) => query.bind(v),
        Value::DateTime(v) => query.bind(v),
        Value::Null => query.bind(Option::<i64>::None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone as _;

    #[derive(Debug, PartialEq)]
    struct User {
        id: i64,
        name: String,
        is_admin: bool,
    }

    impl Model for User {
        const TABLE: &'static str = "users";
        const COLUMNS: &'static [&'static str] = &["id", "name", "is_admin"];
        const INSERT_COLUMNS: &'static [&'static str] = &["name", "is_admin"];

        fn id(&self) -> i64 {
            self.id
        }

        fn from_row(row: Row<'_>) -> Result<Self, Error> {
            Ok(Self {
                id: row.get_i64("id")?,
                name: row.get_string("name")?,
                is_admin: row.get_bool("is_admin")?,
            })
        }

        fn insert_values(&self) -> Vec<Value> {
            vec![self.name.clone().into(), self.is_admin.into()]
        }
    }

    async fn setup() -> Db {
        let db = Db::memory().await.unwrap();
        db.execute(
            "CREATE TABLE users (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                is_admin INTEGER NOT NULL
            )",
        )
        .await
        .unwrap();
        db
    }

    #[tokio::test]
    async fn create_assigns_new_id_and_find_reads_it_back() {
        let db = setup().await;
        let u = User {
            id: 0,
            name: "Alice".into(),
            is_admin: false,
        };
        let id = u.create(&db).await.unwrap();
        assert!(id >= 1);
        let back = User::find(&db, id).await.unwrap().unwrap();
        assert_eq!(back.name, "Alice");
        assert!(!back.is_admin);
        assert_eq!(back.id, id);
    }

    #[tokio::test]
    async fn find_missing_returns_none() {
        let db = setup().await;
        assert!(User::find(&db, 42).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn all_returns_every_row() {
        let db = setup().await;
        User {
            id: 0,
            name: "a".into(),
            is_admin: false,
        }
        .create(&db)
        .await
        .unwrap();
        User {
            id: 0,
            name: "b".into(),
            is_admin: true,
        }
        .create(&db)
        .await
        .unwrap();
        User {
            id: 0,
            name: "c".into(),
            is_admin: false,
        }
        .create(&db)
        .await
        .unwrap();
        let rows = User::all(&db).await.unwrap();
        assert_eq!(rows.len(), 3);
        let names: Vec<&str> = rows.iter().map(|u| u.name.as_str()).collect();
        assert_eq!(names, vec!["a", "b", "c"]);
    }

    #[tokio::test]
    async fn update_modifies_row_in_place() {
        let db = setup().await;
        let id = User {
            id: 0,
            name: "old".into(),
            is_admin: false,
        }
        .create(&db)
        .await
        .unwrap();
        let updated = User {
            id,
            name: "new".into(),
            is_admin: true,
        };
        updated.update(&db).await.unwrap();
        let back = User::find(&db, id).await.unwrap().unwrap();
        assert_eq!(back.name, "new");
        assert!(back.is_admin);
    }

    #[tokio::test]
    async fn delete_removes_row() {
        let db = setup().await;
        let id = User {
            id: 0,
            name: "x".into(),
            is_admin: false,
        }
        .create(&db)
        .await
        .unwrap();
        assert!(User::find(&db, id).await.unwrap().is_some());
        User::delete(&db, id).await.unwrap();
        assert!(User::find(&db, id).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn row_getters_handle_wrong_type_gracefully() {
        let db = setup().await;
        User {
            id: 0,
            name: "a".into(),
            is_admin: false,
        }
        .create(&db)
        .await
        .unwrap();
        let row = sqlx::query("SELECT id, name, is_admin FROM users LIMIT 1")
            .fetch_one(db.pool())
            .await
            .unwrap();
        let wrapped = Row::new(&row);
        assert!(wrapped.get_i64("id").is_ok());
        assert!(wrapped.get_string("nonexistent_column").is_err());
    }

    // --- Option<T> NULL ↔ None round-trip ------------------------------
    //
    // Proves that the *whole* chain — `Value::Null`, the nullable getters
    // on `Row`, and the `From<Option<T>>` blanket impl — stays coherent
    // in both directions. If any piece drifts, `Some`/`None` start
    // leaking into each other silently and the admin forms misbehave.

    #[derive(Debug, PartialEq)]
    struct Event {
        id: i64,
        title: String,
        note: Option<String>,
        priority: Option<i32>,
        starts_at: Option<DateTime<Utc>>,
    }

    impl Model for Event {
        const TABLE: &'static str = "events";
        const COLUMNS: &'static [&'static str] = &["id", "title", "note", "priority", "starts_at"];
        const INSERT_COLUMNS: &'static [&'static str] = &["title", "note", "priority", "starts_at"];

        fn id(&self) -> i64 {
            self.id
        }

        fn from_row(row: Row<'_>) -> Result<Self, Error> {
            Ok(Self {
                id: row.get_i64("id")?,
                title: row.get_string("title")?,
                note: row.get_optional_string("note")?,
                priority: row.get_optional_i32("priority")?,
                starts_at: row.get_optional_datetime("starts_at")?,
            })
        }

        fn insert_values(&self) -> Vec<Value> {
            vec![
                self.title.clone().into(),
                self.note.clone().into(),
                self.priority.into(),
                self.starts_at.into(),
            ]
        }
    }

    async fn setup_events() -> Db {
        let db = Db::memory().await.unwrap();
        db.execute(
            "CREATE TABLE events (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                title TEXT NOT NULL,
                note TEXT NULL,
                priority INTEGER NULL,
                starts_at TEXT NULL
            )",
        )
        .await
        .unwrap();
        db
    }

    #[tokio::test]
    async fn option_none_round_trips_as_null() {
        let db = setup_events().await;
        let id = Event {
            id: 0,
            title: "empty".into(),
            note: None,
            priority: None,
            starts_at: None,
        }
        .create(&db)
        .await
        .unwrap();

        let back = Event::find(&db, id).await.unwrap().unwrap();
        assert_eq!(back.note, None);
        assert_eq!(back.priority, None);
        assert_eq!(back.starts_at, None);

        // The raw row must actually be NULL, not the empty string — a
        // string that round-trips as Some("") would silently break the
        // admin's "no value" semantics.
        let row = sqlx::query(
            "SELECT note IS NULL AS note_is_null,
                    priority IS NULL AS priority_is_null,
                    starts_at IS NULL AS starts_is_null
             FROM events WHERE id = ?",
        )
        .bind(id)
        .fetch_one(db.pool())
        .await
        .unwrap();
        assert_eq!(row.get::<i64, _>(0), 1);
        assert_eq!(row.get::<i64, _>(1), 1);
        assert_eq!(row.get::<i64, _>(2), 1);
    }

    #[tokio::test]
    async fn option_some_round_trips_without_data_loss() {
        let db = setup_events().await;
        let when = Utc.with_ymd_and_hms(2026, 4, 18, 10, 12, 33).unwrap();
        let id = Event {
            id: 0,
            title: "full".into(),
            note: Some("hello".into()),
            priority: Some(7),
            starts_at: Some(when),
        }
        .create(&db)
        .await
        .unwrap();

        let back = Event::find(&db, id).await.unwrap().unwrap();
        assert_eq!(back.note.as_deref(), Some("hello"));
        assert_eq!(back.priority, Some(7));
        assert_eq!(back.starts_at, Some(when));
    }

    #[tokio::test]
    async fn option_update_flips_null_to_some_and_back() {
        let db = setup_events().await;
        let id = Event {
            id: 0,
            title: "t".into(),
            note: None,
            priority: None,
            starts_at: None,
        }
        .create(&db)
        .await
        .unwrap();

        Event {
            id,
            title: "t".into(),
            note: Some("filled".into()),
            priority: Some(1),
            starts_at: None,
        }
        .update(&db)
        .await
        .unwrap();
        let mid = Event::find(&db, id).await.unwrap().unwrap();
        assert_eq!(mid.note.as_deref(), Some("filled"));
        assert_eq!(mid.priority, Some(1));
        assert_eq!(mid.starts_at, None);

        Event {
            id,
            title: "t".into(),
            note: None,
            priority: None,
            starts_at: None,
        }
        .update(&db)
        .await
        .unwrap();
        let after = Event::find(&db, id).await.unwrap().unwrap();
        assert_eq!(after.note, None);
        assert_eq!(after.priority, None);
        assert_eq!(after.starts_at, None);
    }
}
