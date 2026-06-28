use chrono::{DateTime, Utc};
use rustio_core::{Error, Model, Row, RustioAdmin, Value};

use crate::apps::clients::models::Client;
use crate::apps::services::models::Service;
use crate::apps::staff::models::Staff;

/// An appointment ties a Client to a Service with a Staff member at a time.
/// `status` cycles "booked" → "completed" / "cancelled" / "no_show".
#[derive(Debug, RustioAdmin)]
pub struct Appointment {
    pub id: i64,
    #[rustio(belongs_to = "Client", display = "name")]
    pub client_id: i64,
    #[rustio(belongs_to = "Service", display = "name")]
    pub service_id: i64,
    #[rustio(belongs_to = "Staff", display = "name")]
    pub staff_id: i64,
    pub scheduled_at: DateTime<Utc>,
    /// enum: "booked" | "completed" | "cancelled" | "no_show"
    pub status: String,
    pub notes: String,
}

impl Model for Appointment {
    const TABLE: &'static str = "appointments";
    const COLUMNS: &'static [&'static str] = &[
        "id",
        "client_id",
        "service_id",
        "staff_id",
        "scheduled_at",
        "status",
        "notes",
    ];
    const INSERT_COLUMNS: &'static [&'static str] = &[
        "client_id",
        "service_id",
        "staff_id",
        "scheduled_at",
        "status",
        "notes",
    ];

    fn id(&self) -> i64 {
        self.id
    }

    fn from_row(row: Row<'_>) -> Result<Self, Error> {
        Ok(Self {
            id: row.get_i64("id")?,
            client_id: row.get_i64("client_id")?,
            service_id: row.get_i64("service_id")?,
            staff_id: row.get_i64("staff_id")?,
            scheduled_at: row.get_datetime("scheduled_at")?,
            status: row.get_string("status")?,
            notes: row.get_string("notes")?,
        })
    }

    fn insert_values(&self) -> Vec<Value> {
        vec![
            self.client_id.into(),
            self.service_id.into(),
            self.staff_id.into(),
            self.scheduled_at.into(),
            self.status.clone().into(),
            self.notes.clone().into(),
        ]
    }
}
