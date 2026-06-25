use chrono::{DateTime, Utc};
use rustio_core::{Error, Model, Row, RustioAdmin, Value};

use crate::apps::bookings::models::Booking;
use crate::apps::resources::models::Resource;

/// An Assignment connects a Booking to the Resource that fulfilled it,
/// recording the offer/accept handshake.
///
/// `status` is a string enum: `offered` / `accepted` / `declined`.
#[derive(Debug, RustioAdmin)]
pub struct Assignment {
    pub id: i64,
    #[rustio(belongs_to = "Booking", display = "booking_number")]
    pub booking_id: i64,
    #[rustio(belongs_to = "Resource", display = "name")]
    pub resource_id: i64,
    pub accepted_at: DateTime<Utc>,
    /// enum: "offered" | "accepted" | "declined"
    pub status: String,
}

impl Model for Assignment {
    const TABLE: &'static str = "assignments";
    const COLUMNS: &'static [&'static str] =
        &["id", "booking_id", "resource_id", "accepted_at", "status"];
    const INSERT_COLUMNS: &'static [&'static str] =
        &["booking_id", "resource_id", "accepted_at", "status"];

    fn id(&self) -> i64 {
        self.id
    }

    fn from_row(row: Row<'_>) -> Result<Self, Error> {
        Ok(Self {
            id: row.get_i64("id")?,
            booking_id: row.get_i64("booking_id")?,
            resource_id: row.get_i64("resource_id")?,
            accepted_at: row.get_datetime("accepted_at")?,
            status: row.get_string("status")?,
        })
    }

    fn insert_values(&self) -> Vec<Value> {
        vec![
            self.booking_id.into(),
            self.resource_id.into(),
            self.accepted_at.into(),
            self.status.clone().into(),
        ]
    }
}
