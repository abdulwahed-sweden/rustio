use rustio_core::{Error, Model, Row, RustioAdmin, Value};

/// A staff member. `is_active` toggles whether they take new appointments.
#[derive(Debug, RustioAdmin)]
pub struct Staff {
    pub id: i64,
    pub name: String,
    pub specialty: String,
    pub phone: String,
    pub is_active: bool,
}

impl Model for Staff {
    const TABLE: &'static str = "staff";
    const COLUMNS: &'static [&'static str] = &["id", "name", "specialty", "phone", "is_active"];
    const INSERT_COLUMNS: &'static [&'static str] = &["name", "specialty", "phone", "is_active"];

    fn id(&self) -> i64 {
        self.id
    }

    fn from_row(row: Row<'_>) -> Result<Self, Error> {
        Ok(Self {
            id: row.get_i64("id")?,
            name: row.get_string("name")?,
            specialty: row.get_string("specialty")?,
            phone: row.get_string("phone")?,
            is_active: row.get_bool("is_active")?,
        })
    }

    fn insert_values(&self) -> Vec<Value> {
        vec![
            self.name.clone().into(),
            self.specialty.clone().into(),
            self.phone.clone().into(),
            self.is_active.into(),
        ]
    }
}
