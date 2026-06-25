use chrono::{DateTime, Utc};
use rustio_core::{Error, Model, Row, RustioAdmin, Value};

use crate::apps::locations::models::Location;

/// A Resource is the bookable thing — a container, a room, a vehicle, a
/// technician. It lives at one Location and carries a rate.
///
/// `resource_type` is a string enum (e.g. `container` / `room` /
/// `vehicle` / `person`). `rate_cents` is an integer minor-unit amount
/// (cents/öre) — never a float.
#[derive(Debug, RustioAdmin)]
pub struct Resource {
    pub id: i64,
    pub name: String,
    /// enum: e.g. "container" | "room" | "vehicle" | "person"
    pub resource_type: String,
    pub code: String,
    #[rustio(belongs_to = "Location", display = "name")]
    pub location_id: i64,
    /// Integer minor units (cents). Never use floats for money.
    pub rate_cents: i64,
    pub active: bool,
    pub created_at: DateTime<Utc>,
}

impl Model for Resource {
    const TABLE: &'static str = "resources";
    const COLUMNS: &'static [&'static str] = &[
        "id",
        "name",
        "resource_type",
        "code",
        "location_id",
        "rate_cents",
        "active",
        "created_at",
    ];
    const INSERT_COLUMNS: &'static [&'static str] = &[
        "name",
        "resource_type",
        "code",
        "location_id",
        "rate_cents",
        "active",
        "created_at",
    ];

    fn id(&self) -> i64 {
        self.id
    }

    fn from_row(row: Row<'_>) -> Result<Self, Error> {
        Ok(Self {
            id: row.get_i64("id")?,
            name: row.get_string("name")?,
            resource_type: row.get_string("resource_type")?,
            code: row.get_string("code")?,
            location_id: row.get_i64("location_id")?,
            rate_cents: row.get_i64("rate_cents")?,
            active: row.get_bool("active")?,
            created_at: row.get_datetime("created_at")?,
        })
    }

    fn insert_values(&self) -> Vec<Value> {
        vec![
            self.name.clone().into(),
            self.resource_type.clone().into(),
            self.code.clone().into(),
            self.location_id.into(),
            self.rate_cents.into(),
            self.active.into(),
            self.created_at.into(),
        ]
    }
}
