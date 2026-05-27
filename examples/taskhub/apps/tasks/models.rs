use chrono::{DateTime, Utc};
use rustio_core::{Error, Model, Row, RustioAdmin, Value};

use crate::apps::projects::models::Project;

/// A Task belongs to a Project (`project_id` foreign key). `due_at` is
/// optional — leave it `None` for backlog items with no fixed deadline.
///
/// `status` is a small string vocabulary: "todo" / "in_progress" /
/// "done". The admin renders it as a free-text field; consider an
/// enum + a custom widget once you have a real workflow.
///
/// `priority` is `1` (lowest) to `5` (highest). Inferred as `i32` by
/// the planner — keep it that way; the admin filter expects integer
/// equality.
#[derive(Debug, RustioAdmin)]
pub struct Task {
    pub id: i64,
    pub title: String,
    pub description: String,
    pub status: String,
    pub priority: i32,
    #[rustio(belongs_to = "Project", display = "name")]
    pub project_id: i64,
    pub due_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

impl Model for Task {
    const TABLE: &'static str = "tasks";
    const COLUMNS: &'static [&'static str] = &[
        "id",
        "title",
        "description",
        "status",
        "priority",
        "project_id",
        "due_at",
        "created_at",
    ];
    const INSERT_COLUMNS: &'static [&'static str] = &[
        "title",
        "description",
        "status",
        "priority",
        "project_id",
        "due_at",
        "created_at",
    ];

    fn id(&self) -> i64 {
        self.id
    }

    fn from_row(row: Row<'_>) -> Result<Self, Error> {
        Ok(Self {
            id: row.get_i64("id")?,
            title: row.get_string("title")?,
            description: row.get_string("description")?,
            status: row.get_string("status")?,
            priority: row.get_i32("priority")?,
            project_id: row.get_i64("project_id")?,
            due_at: row.get_optional_datetime("due_at")?,
            created_at: row.get_datetime("created_at")?,
        })
    }

    fn insert_values(&self) -> Vec<Value> {
        vec![
            self.title.clone().into(),
            self.description.clone().into(),
            self.status.clone().into(),
            self.priority.into(),
            self.project_id.into(),
            self.due_at.into(),
            self.created_at.into(),
        ]
    }
}
