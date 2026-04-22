use chrono::{DateTime, Utc};
use rustio_core::{Error, Model, Row, RustioAdmin, Value};

// Relation targets on other apps.
use crate::apps::care::models::Appointment;
use crate::apps::people::models::{Department, Patient};

// ───────────────────────────────────────────────────────────────
// Staff — non-doctor personnel: nurses, receptionists,
// technicians, cleaners, security, admin. Doctors live in the
// `people` app and have their own table.
// ───────────────────────────────────────────────────────────────

#[derive(Debug, RustioAdmin)]
pub struct Staff {
    pub id: i64,
    pub full_name: String,
    pub role: String,
    #[rustio(belongs_to = "Department", display = "name")]
    pub department_id: Option<i64>,
    pub email: String,
    pub phone: String,
    pub is_active: bool,
    pub hired_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

impl Model for Staff {
    const TABLE: &'static str = "staff_members";
    const COLUMNS: &'static [&'static str] = &[
        "id",
        "full_name",
        "role",
        "department_id",
        "email",
        "phone",
        "is_active",
        "hired_at",
        "created_at",
    ];
    const INSERT_COLUMNS: &'static [&'static str] = &[
        "full_name",
        "role",
        "department_id",
        "email",
        "phone",
        "is_active",
        "hired_at",
        "created_at",
    ];

    fn id(&self) -> i64 {
        self.id
    }

    fn from_row(row: Row<'_>) -> Result<Self, Error> {
        Ok(Self {
            id: row.get_i64("id")?,
            full_name: row.get_string("full_name")?,
            role: row.get_string("role")?,
            department_id: row.get_optional_i64("department_id")?,
            email: row.get_string("email")?,
            phone: row.get_string("phone")?,
            is_active: row.get_bool("is_active")?,
            hired_at: row.get_datetime("hired_at")?,
            created_at: row.get_datetime("created_at")?,
        })
    }

    fn insert_values(&self) -> Vec<Value> {
        vec![
            self.full_name.clone().into(),
            self.role.clone().into(),
            self.department_id.into(),
            self.email.clone().into(),
            self.phone.clone().into(),
            self.is_active.into(),
            self.hired_at.into(),
            self.created_at.into(),
        ]
    }
}

// ───────────────────────────────────────────────────────────────
// Room — a physical room in the hospital (exam, surgery, ward,
// ICU, waiting, office). `room_number` is a short identifier like
// `"3A-112"`; `room_type` is the allow-list category.
// ───────────────────────────────────────────────────────────────

#[derive(Debug, RustioAdmin)]
pub struct Room {
    pub id: i64,
    pub room_number: String,
    pub floor: i32,
    #[rustio(belongs_to = "Department", display = "name")]
    pub department_id: Option<i64>,
    pub room_type: String,
    pub capacity: i32,
    pub is_available: bool,
    pub notes: String,
    pub created_at: DateTime<Utc>,
}

impl Model for Room {
    const TABLE: &'static str = "rooms";
    const COLUMNS: &'static [&'static str] = &[
        "id",
        "room_number",
        "floor",
        "department_id",
        "room_type",
        "capacity",
        "is_available",
        "notes",
        "created_at",
    ];
    const INSERT_COLUMNS: &'static [&'static str] = &[
        "room_number",
        "floor",
        "department_id",
        "room_type",
        "capacity",
        "is_available",
        "notes",
        "created_at",
    ];

    fn id(&self) -> i64 {
        self.id
    }

    fn from_row(row: Row<'_>) -> Result<Self, Error> {
        Ok(Self {
            id: row.get_i64("id")?,
            room_number: row.get_string("room_number")?,
            floor: row.get_i32("floor")?,
            department_id: row.get_optional_i64("department_id")?,
            room_type: row.get_string("room_type")?,
            capacity: row.get_i32("capacity")?,
            is_available: row.get_bool("is_available")?,
            notes: row.get_string("notes")?,
            created_at: row.get_datetime("created_at")?,
        })
    }

    fn insert_values(&self) -> Vec<Value> {
        vec![
            self.room_number.clone().into(),
            self.floor.into(),
            self.department_id.into(),
            self.room_type.clone().into(),
            self.capacity.into(),
            self.is_available.into(),
            self.notes.clone().into(),
            self.created_at.into(),
        ]
    }
}

// ───────────────────────────────────────────────────────────────
// CheckIn — the front-desk event that ties everything together:
// which patient walked in for which appointment, who handled it,
// and which room they were sent to.
// ───────────────────────────────────────────────────────────────

#[derive(Debug, RustioAdmin)]
pub struct CheckIn {
    pub id: i64,
    #[rustio(belongs_to = "Appointment", display = "reason")]
    pub appointment_id: i64,
    #[rustio(belongs_to = "Patient", display = "full_name")]
    pub patient_id: i64,
    #[rustio(belongs_to = "Staff", display = "full_name")]
    pub staff_id: Option<i64>,
    #[rustio(belongs_to = "Room", display = "room_number")]
    pub room_id: Option<i64>,
    pub checked_in_at: DateTime<Utc>,
    pub status: String,
    pub priority: i32,
    pub notes: String,
    pub created_at: DateTime<Utc>,
}

impl Model for CheckIn {
    const TABLE: &'static str = "check_ins";
    const COLUMNS: &'static [&'static str] = &[
        "id",
        "appointment_id",
        "patient_id",
        "staff_id",
        "room_id",
        "checked_in_at",
        "status",
        "priority",
        "notes",
        "created_at",
    ];
    const INSERT_COLUMNS: &'static [&'static str] = &[
        "appointment_id",
        "patient_id",
        "staff_id",
        "room_id",
        "checked_in_at",
        "status",
        "priority",
        "notes",
        "created_at",
    ];

    fn id(&self) -> i64 {
        self.id
    }

    fn from_row(row: Row<'_>) -> Result<Self, Error> {
        Ok(Self {
            id: row.get_i64("id")?,
            appointment_id: row.get_i64("appointment_id")?,
            patient_id: row.get_i64("patient_id")?,
            staff_id: row.get_optional_i64("staff_id")?,
            room_id: row.get_optional_i64("room_id")?,
            checked_in_at: row.get_datetime("checked_in_at")?,
            status: row.get_string("status")?,
            priority: row.get_i32("priority")?,
            notes: row.get_string("notes")?,
            created_at: row.get_datetime("created_at")?,
        })
    }

    fn insert_values(&self) -> Vec<Value> {
        vec![
            self.appointment_id.into(),
            self.patient_id.into(),
            self.staff_id.into(),
            self.room_id.into(),
            self.checked_in_at.into(),
            self.status.clone().into(),
            self.priority.into(),
            self.notes.clone().into(),
            self.created_at.into(),
        ]
    }
}

/// Wire `CheckIn` up to the `Appointment::has_active_checkin` query
/// defined in the `care` app. The "active" statuses must mirror the
/// partial unique index in migration 0016 — if one changes, change
/// both.
impl crate::apps::care::models::CheckInLike for CheckIn {
    fn appointment_id(&self) -> i64 {
        self.appointment_id
    }
    fn is_active(&self) -> bool {
        matches!(self.status.as_str(), "waiting" | "in_room" | "with_doctor")
    }
}
