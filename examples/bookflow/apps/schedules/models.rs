use rustio_core::{Error, Model, Row, RustioAdmin, Value};

use crate::apps::resources::models::Resource;

/// A Schedule declares when a Resource is available. One row per
/// weekday/window.
///
/// String enums: `weekday` is `mon`…`sun`; `mode` is domain-defined
/// (e.g. `available` / `on_call` / `blocked`). `start_time` / `end_time`
/// are stored as `"HH:MM"` text — RustIO has no first-class time-of-day
/// type, and a wall-clock string is closer to intent than a full
/// timestamp with a meaningless date.
#[derive(Debug, RustioAdmin)]
pub struct Schedule {
    pub id: i64,
    #[rustio(belongs_to = "Resource", display = "name")]
    pub resource_id: i64,
    /// enum: "mon" | "tue" | "wed" | "thu" | "fri" | "sat" | "sun"
    pub weekday: String,
    /// Time of day as "HH:MM" (24h).
    pub start_time: String,
    /// Time of day as "HH:MM" (24h).
    pub end_time: String,
    /// enum: domain-defined availability mode
    pub mode: String,
}

impl Model for Schedule {
    const TABLE: &'static str = "schedules";
    const COLUMNS: &'static [&'static str] = &[
        "id",
        "resource_id",
        "weekday",
        "start_time",
        "end_time",
        "mode",
    ];
    const INSERT_COLUMNS: &'static [&'static str] =
        &["resource_id", "weekday", "start_time", "end_time", "mode"];

    fn id(&self) -> i64 {
        self.id
    }

    fn from_row(row: Row<'_>) -> Result<Self, Error> {
        Ok(Self {
            id: row.get_i64("id")?,
            resource_id: row.get_i64("resource_id")?,
            weekday: row.get_string("weekday")?,
            start_time: row.get_string("start_time")?,
            end_time: row.get_string("end_time")?,
            mode: row.get_string("mode")?,
        })
    }

    fn insert_values(&self) -> Vec<Value> {
        vec![
            self.resource_id.into(),
            self.weekday.clone().into(),
            self.start_time.clone().into(),
            self.end_time.clone().into(),
            self.mode.clone().into(),
        ]
    }
}
