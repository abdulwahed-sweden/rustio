use chrono::{DateTime, Utc};
use rustio_core::{Error, Model, Row, RustioAdmin, Value};

// Relation targets on other apps. Brought into scope so the
// `#[rustio(belongs_to = "Patient")]` / `"Doctor"` compile-time
// checks can resolve `<Target as Model>::TABLE` and `COLUMNS`.
use crate::apps::people::models::{Doctor, Patient};

// ───────────────────────────────────────────────────────────────
// Appointment
// ───────────────────────────────────────────────────────────────

#[derive(Debug, RustioAdmin)]
pub struct Appointment {
    pub id: i64,
    #[rustio(belongs_to = "Patient", display = "full_name")]
    pub patient_id: i64,
    #[rustio(belongs_to = "Doctor", display = "full_name")]
    pub doctor_id: i64,
    pub scheduled_at: DateTime<Utc>,
    pub status: String,
    pub reason: String,
    pub notes: String,
    pub duration_minutes: i32,
    pub priority: i32,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
}

impl Model for Appointment {
    const TABLE: &'static str = "appointments";
    const COLUMNS: &'static [&'static str] = &[
        "id",
        "patient_id",
        "doctor_id",
        "scheduled_at",
        "status",
        "reason",
        "notes",
        "duration_minutes",
        "priority",
        "is_active",
        "created_at",
    ];
    const INSERT_COLUMNS: &'static [&'static str] = &[
        "patient_id",
        "doctor_id",
        "scheduled_at",
        "status",
        "reason",
        "notes",
        "duration_minutes",
        "priority",
        "is_active",
        "created_at",
    ];

    fn id(&self) -> i64 {
        self.id
    }

    fn from_row(row: Row<'_>) -> Result<Self, Error> {
        Ok(Self {
            id: row.get_i64("id")?,
            patient_id: row.get_i64("patient_id")?,
            doctor_id: row.get_i64("doctor_id")?,
            scheduled_at: row.get_datetime("scheduled_at")?,
            status: row.get_string("status")?,
            reason: row.get_string("reason")?,
            notes: row.get_string("notes")?,
            duration_minutes: row.get_i32("duration_minutes")?,
            priority: row.get_i32("priority")?,
            is_active: row.get_bool("is_active")?,
            created_at: row.get_datetime("created_at")?,
        })
    }

    fn insert_values(&self) -> Vec<Value> {
        vec![
            self.patient_id.into(),
            self.doctor_id.into(),
            self.scheduled_at.into(),
            self.status.clone().into(),
            self.reason.clone().into(),
            self.notes.clone().into(),
            self.duration_minutes.into(),
            self.priority.into(),
            self.is_active.into(),
            self.created_at.into(),
        ]
    }
}

// ───────────────────────────────────────────────────────────────
// Prescription
// ───────────────────────────────────────────────────────────────

#[derive(Debug, RustioAdmin)]
pub struct Prescription {
    pub id: i64,
    #[rustio(belongs_to = "Appointment", display = "reason")]
    pub appointment_id: i64,
    #[rustio(belongs_to = "Patient", display = "full_name")]
    pub patient_id: i64,
    #[rustio(belongs_to = "Doctor", display = "full_name")]
    pub doctor_id: i64,
    pub medication: String,
    pub dosage: String,
    pub frequency: String,
    pub duration_days: i32,
    pub is_refillable: bool,
    pub refills_remaining: i32,
    pub notes: String,
    pub created_at: DateTime<Utc>,
}

impl Model for Prescription {
    const TABLE: &'static str = "prescriptions";
    const COLUMNS: &'static [&'static str] = &[
        "id",
        "appointment_id",
        "patient_id",
        "doctor_id",
        "medication",
        "dosage",
        "frequency",
        "duration_days",
        "is_refillable",
        "refills_remaining",
        "notes",
        "created_at",
    ];
    const INSERT_COLUMNS: &'static [&'static str] = &[
        "appointment_id",
        "patient_id",
        "doctor_id",
        "medication",
        "dosage",
        "frequency",
        "duration_days",
        "is_refillable",
        "refills_remaining",
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
            doctor_id: row.get_i64("doctor_id")?,
            medication: row.get_string("medication")?,
            dosage: row.get_string("dosage")?,
            frequency: row.get_string("frequency")?,
            duration_days: row.get_i32("duration_days")?,
            is_refillable: row.get_bool("is_refillable")?,
            refills_remaining: row.get_i32("refills_remaining")?,
            notes: row.get_string("notes")?,
            created_at: row.get_datetime("created_at")?,
        })
    }

    fn insert_values(&self) -> Vec<Value> {
        vec![
            self.appointment_id.into(),
            self.patient_id.into(),
            self.doctor_id.into(),
            self.medication.clone().into(),
            self.dosage.clone().into(),
            self.frequency.clone().into(),
            self.duration_days.into(),
            self.is_refillable.into(),
            self.refills_remaining.into(),
            self.notes.clone().into(),
            self.created_at.into(),
        ]
    }
}
