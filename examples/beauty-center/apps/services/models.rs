use rustio_core::{Error, Model, Row, RustioAdmin, Value};

/// A service offered by the center. `category` is a string enum
/// ("hair" | "skin" | "nails" | "lashes") → Badge + filter. `price_cents`
/// is integer minor units (öre); never a float.
#[derive(Debug, RustioAdmin)]
pub struct Service {
    pub id: i64,
    pub name: String,
    /// enum: "hair" | "skin" | "nails" | "lashes"
    pub category: String,
    pub duration_minutes: i32,
    pub price_cents: i64,
}

impl Model for Service {
    const TABLE: &'static str = "services";
    const COLUMNS: &'static [&'static str] =
        &["id", "name", "category", "duration_minutes", "price_cents"];
    const INSERT_COLUMNS: &'static [&'static str] =
        &["name", "category", "duration_minutes", "price_cents"];

    fn id(&self) -> i64 {
        self.id
    }

    fn from_row(row: Row<'_>) -> Result<Self, Error> {
        Ok(Self {
            id: row.get_i64("id")?,
            name: row.get_string("name")?,
            category: row.get_string("category")?,
            duration_minutes: row.get_i32("duration_minutes")?,
            price_cents: row.get_i64("price_cents")?,
        })
    }

    fn insert_values(&self) -> Vec<Value> {
        vec![
            self.name.clone().into(),
            self.category.clone().into(),
            self.duration_minutes.into(),
            self.price_cents.into(),
        ]
    }
}
