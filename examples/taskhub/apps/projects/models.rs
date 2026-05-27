use chrono::{DateTime, Utc};
use rustio_core::{Error, Model, Row, RustioAdmin, Value};

/// A Project groups Tasks. Every Task belongs to exactly one Project.
///
/// Shown in the admin sidebar as "Projects". The list page renders
/// `name` as the headline column; the edit form exposes every field
/// except `id` (auto-assigned) and `created_at` (auto-stamped).
#[derive(Debug, RustioAdmin)]
pub struct Project {
    pub id: i64,
    pub name: String,
    pub description: String,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
}

impl Model for Project {
    const TABLE: &'static str = "projects";
    const COLUMNS: &'static [&'static str] =
        &["id", "name", "description", "is_active", "created_at"];
    const INSERT_COLUMNS: &'static [&'static str] =
        &["name", "description", "is_active", "created_at"];

    fn id(&self) -> i64 {
        self.id
    }

    fn from_row(row: Row<'_>) -> Result<Self, Error> {
        Ok(Self {
            id: row.get_i64("id")?,
            name: row.get_string("name")?,
            description: row.get_string("description")?,
            is_active: row.get_bool("is_active")?,
            created_at: row.get_datetime("created_at")?,
        })
    }

    fn insert_values(&self) -> Vec<Value> {
        vec![
            self.name.clone().into(),
            self.description.clone().into(),
            self.is_active.into(),
            self.created_at.into(),
        ]
    }
}
