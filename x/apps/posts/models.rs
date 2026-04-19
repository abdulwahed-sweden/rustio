use rustio_core::{Error, Model, Row, RustioAdmin, Value};

/// The Post model.
///
/// This is a starting point — edit freely. Supported field types today
/// are `i32`, `i64`, `String`, and `bool`. To add a field:
///
///   1. Add it to the struct below.
///   2. Append its column name to `COLUMNS` (and `INSERT_COLUMNS` if the
///      DB shouldn't autofill it).
///   3. Read it in `from_row` and emit it in `insert_values`.
///   4. Generate a migration to update the table:
///        rustio migrate generate alter_posts
///      then write the `ALTER TABLE ...` SQL and run `rustio migrate apply`.
#[derive(Debug, RustioAdmin)]
pub struct Post {
    pub id: i64,
    pub title: String,
    pub is_active: bool,
    pub priority: i32,
}

impl Model for Post {
    const TABLE: &'static str = "posts";
    const COLUMNS: &'static [&'static str] = &["id", "title", "is_active", "priority"];
    const INSERT_COLUMNS: &'static [&'static str] = &["title", "is_active", "priority"];

    fn id(&self) -> i64 {
        self.id
    }

    fn from_row(row: Row<'_>) -> Result<Self, Error> {
        Ok(Self {
            id: row.get_i64("id")?,
            title: row.get_string("title")?,
            is_active: row.get_bool("is_active")?,
            priority: row.get_i32("priority")?,
        })
    }

    fn insert_values(&self) -> Vec<Value> {
        vec![
            self.title.clone().into(),
            self.is_active.into(),
            self.priority.into(),
        ]
    }
}
