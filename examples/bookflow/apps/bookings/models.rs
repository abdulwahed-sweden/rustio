use chrono::{DateTime, Utc};
use rustio_core::{Error, Model, Row, RustioAdmin, Value};

use crate::apps::customers::models::Customer;
use crate::apps::resources::models::Resource;

/// A Booking is the heart of the system: a Customer reserves a Resource
/// for a window of time. Everything else (schedules, assignments,
/// invoices) hangs off this.
///
/// String enums: `service_type` is domain-defined; `status` cycles
/// `new` → `assigned` → `completed` / `cancelled`. `assignee_id` is the
/// Resource that actually fulfils the booking and is optional until the
/// booking is assigned.
#[derive(Debug, RustioAdmin)]
pub struct Booking {
    pub id: i64,
    pub booking_number: String,
    #[rustio(belongs_to = "Customer", display = "name")]
    pub customer_id: i64,
    #[rustio(belongs_to = "Resource", display = "name")]
    pub resource_id: i64,
    /// enum: domain-defined service category
    pub service_type: String,
    pub scheduled_at: DateTime<Utc>,
    pub duration_minutes: i32,
    /// enum: "new" | "assigned" | "completed" | "cancelled"
    pub status: String,
    /// Optional Resource that fulfils the booking (set on assignment).
    #[rustio(belongs_to = "Resource", display = "name")]
    pub assignee_id: Option<i64>,
    pub notes: String,
    pub created_at: DateTime<Utc>,
}

impl Model for Booking {
    const TABLE: &'static str = "bookings";
    const COLUMNS: &'static [&'static str] = &[
        "id",
        "booking_number",
        "customer_id",
        "resource_id",
        "service_type",
        "scheduled_at",
        "duration_minutes",
        "status",
        "assignee_id",
        "notes",
        "created_at",
    ];
    const INSERT_COLUMNS: &'static [&'static str] = &[
        "booking_number",
        "customer_id",
        "resource_id",
        "service_type",
        "scheduled_at",
        "duration_minutes",
        "status",
        "assignee_id",
        "notes",
        "created_at",
    ];

    fn id(&self) -> i64 {
        self.id
    }

    fn from_row(row: Row<'_>) -> Result<Self, Error> {
        Ok(Self {
            id: row.get_i64("id")?,
            booking_number: row.get_string("booking_number")?,
            customer_id: row.get_i64("customer_id")?,
            resource_id: row.get_i64("resource_id")?,
            service_type: row.get_string("service_type")?,
            scheduled_at: row.get_datetime("scheduled_at")?,
            duration_minutes: row.get_i32("duration_minutes")?,
            status: row.get_string("status")?,
            assignee_id: row.get_optional_i64("assignee_id")?,
            notes: row.get_string("notes")?,
            created_at: row.get_datetime("created_at")?,
        })
    }

    fn insert_values(&self) -> Vec<Value> {
        vec![
            self.booking_number.clone().into(),
            self.customer_id.into(),
            self.resource_id.into(),
            self.service_type.clone().into(),
            self.scheduled_at.into(),
            self.duration_minutes.into(),
            self.status.clone().into(),
            self.assignee_id.into(),
            self.notes.clone().into(),
            self.created_at.into(),
        ]
    }
}
