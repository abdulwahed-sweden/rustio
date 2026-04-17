use sqlx::Row as _;
use sqlx::sqlite::{SqlitePool, SqlitePoolOptions, SqliteRow};

use crate::error::Error;

#[derive(Clone)]
pub struct Db {
    pool: SqlitePool,
}

impl Db {
    pub async fn connect(url: &str) -> Result<Self, Error> {
        let pool = SqlitePool::connect(url).await?;
        Ok(Self { pool })
    }

    pub async fn memory() -> Result<Self, Error> {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
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
}

#[derive(Debug)]
pub enum Value {
    I32(i32),
    I64(i64),
    String(String),
    Bool(bool),
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

pub trait Model: Sized + Send + Sync + Unpin + 'static {
    const TABLE: &'static str;
    const COLUMNS: &'static [&'static str];
    const INSERT_COLUMNS: &'static [&'static str];

    fn id(&self) -> i64;
    fn from_row(row: Row<'_>) -> Result<Self, Error>;
    fn insert_values(&self) -> Vec<Value>;

    fn find(db: &Db, id: i64) -> impl std::future::Future<Output = Result<Option<Self>, Error>> + Send
    where
        Self: Send,
    {
        async move {
            let sql = format!(
                "SELECT {} FROM {} WHERE id = ?",
                Self::COLUMNS.join(", "),
                Self::TABLE,
            );
            let row = sqlx::query(&sql)
                .bind(id)
                .fetch_optional(db.pool())
                .await?;
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
            rows.iter()
                .map(|r| Self::from_row(Row::new(r)))
                .collect()
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
        Value::Null => query.bind(Option::<i64>::None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        User { id: 0, name: "a".into(), is_admin: false }.create(&db).await.unwrap();
        User { id: 0, name: "b".into(), is_admin: true }.create(&db).await.unwrap();
        User { id: 0, name: "c".into(), is_admin: false }.create(&db).await.unwrap();
        let rows = User::all(&db).await.unwrap();
        assert_eq!(rows.len(), 3);
        let names: Vec<&str> = rows.iter().map(|u| u.name.as_str()).collect();
        assert_eq!(names, vec!["a", "b", "c"]);
    }

    #[tokio::test]
    async fn update_modifies_row_in_place() {
        let db = setup().await;
        let id = User { id: 0, name: "old".into(), is_admin: false }
            .create(&db)
            .await
            .unwrap();
        let updated = User { id, name: "new".into(), is_admin: true };
        updated.update(&db).await.unwrap();
        let back = User::find(&db, id).await.unwrap().unwrap();
        assert_eq!(back.name, "new");
        assert!(back.is_admin);
    }

    #[tokio::test]
    async fn delete_removes_row() {
        let db = setup().await;
        let id = User { id: 0, name: "x".into(), is_admin: false }
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
        User { id: 0, name: "a".into(), is_admin: false }
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
}
