use chrono::{DateTime, Utc};
use rustio_core::{Error, Model, Row, RustioAdmin, Value};

/// A salon client. `notes` is free text (a natural Hidden / detail-only field);
/// `name` + `phone` make a nice merge demo in the composition editor.
#[derive(Debug, RustioAdmin)]
pub struct Client {
    pub id: i64,
    pub name: String,
    pub phone: String,
    pub email: String,
    pub notes: String,
    pub joined_at: DateTime<Utc>,
}

impl Model for Client {
    const TABLE: &'static str = "clients";
    const COLUMNS: &'static [&'static str] =
        &["id", "name", "phone", "email", "notes", "joined_at"];
    const INSERT_COLUMNS: &'static [&'static str] =
        &["name", "phone", "email", "notes", "joined_at"];

    fn id(&self) -> i64 {
        self.id
    }

    fn from_row(row: Row<'_>) -> Result<Self, Error> {
        Ok(Self {
            id: row.get_i64("id")?,
            name: row.get_string("name")?,
            phone: row.get_string("phone")?,
            email: row.get_string("email")?,
            notes: row.get_string("notes")?,
            joined_at: row.get_datetime("joined_at")?,
        })
    }

    fn insert_values(&self) -> Vec<Value> {
        vec![
            self.name.clone().into(),
            self.phone.clone().into(),
            self.email.clone().into(),
            self.notes.clone().into(),
            self.joined_at.into(),
        ]
    }
}
