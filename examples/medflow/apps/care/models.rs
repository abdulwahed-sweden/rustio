use chrono::{DateTime, Utc};
use rustio_core::{Error, Model, Row, RustioAdmin, Value};

// Relation targets on other apps. Brought into scope so the
// `#[rustio(belongs_to = "Patient")]` / `"Doctor"` / `"Department"`
// compile-time checks can resolve `<Target as Model>::TABLE` and
// `COLUMNS`.
use crate::apps::people::models::{Department, Doctor, Patient};

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
    #[rustio(belongs_to = "Department", display = "name")]
    pub department_id: Option<i64>,
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
        "department_id",
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
        "department_id",
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
            department_id: row.get_optional_i64("department_id")?,
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
            self.department_id.into(),
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

// ═══════════════════════════════════════════════════════════════
// Appointment — lifecycle
// ═══════════════════════════════════════════════════════════════
//
// All lifecycle logic lives here: the status vocabulary, the
// transition table, the mutating entry-point (`transition_to`),
// the state predicates (`is_completed`, `is_cancelled`), the
// billing rule (`is_billable`), and the check-in query
// (`has_active_checkin`).
//
// Enforcement is three-layer:
//
//   1. `appointments.status` has a CHECK constraint (migration 0015)
//      that rejects any value outside the vocabulary. SQLite catches
//      drift from the DB side; cannot be bypassed.
//   2. `can_transition_to` validates the delta *before* a write.
//      The CHECK only validates the value, it does not reject an
//      invalid delta like `completed` → `scheduled`.
//   3. `transition_to` is the single sanctioned mutator: it runs
//      `can_transition_to` and then mutates `self.status`. It is
//      the only place in the codebase that should set `self.status`.
//
//          ┌─────────────────────────────────────────────────────┐
//          │ ⚠️  DO NOT assign to `appt.status` directly.        │
//          │                                                     │
//          │ Direct assignment (`appt.status = "completed"`)     │
//          │ bypasses the transition check AND the event log.   │
//          │ The `pub` field is there for the admin derive; it   │
//          │ is a technical requirement, not an invitation.      │
//          │ If you need to change status, call `transition_to`. │
//          └─────────────────────────────────────────────────────┘
//
// Transition table (self-edges are allowed no-ops):
//   scheduled   → confirmed   | cancelled
//   confirmed   → in_progress | cancelled
//   in_progress → completed   | cancelled
//   completed   → cancelled
//   cancelled   → (terminal)
//
// Every successful `transition_to` must be paired with an
// `appointment_events` row in the same transaction (see the struct
// at the bottom of this file and migration 0019).
// ═══════════════════════════════════════════════════════════════

/// Minimal interface a check-in must expose so
/// [`Appointment::has_active_checkin`] can reason about it without
/// pulling the concrete `CheckIn` type in from the `workflow` app
/// (which would create a reverse dependency: `care` → `workflow`).
///
/// Implemented for `workflow::models::CheckIn`.
pub trait CheckInLike {
    /// The `appointment_id` this check-in belongs to.
    fn appointment_id(&self) -> i64;
    /// Whether this check-in is currently active — i.e. the patient
    /// is somewhere in the `waiting` / `in_room` / `with_doctor`
    /// arc, and has not yet reached a terminal status.
    fn is_active(&self) -> bool;
}

#[allow(dead_code)] // Public lifecycle API — called by future workflow integrations, tests, and import scripts.
impl Appointment {
    /// The canonical status vocabulary. Must match the CHECK
    /// constraint in migration 0015 — if one changes, change both.
    pub const ALLOWED_STATUSES: &'static [&'static str] = &[
        "scheduled",
        "confirmed",
        "in_progress",
        "completed",
        "cancelled",
    ];

    // ────────────────────────────────────────────────────────────
    // Transition validation + execution
    // ────────────────────────────────────────────────────────────

    /// Check if this appointment can transition from its current
    /// status to `next`. Returns `Ok(())` for valid transitions
    /// (including no-op self-edges); `Err(Error::BadRequest)` for
    /// invalid deltas or unknown status values.
    ///
    /// This is a pure query — it does not mutate `self`. Use
    /// [`Self::transition_to`] to actually change status.
    pub fn can_transition_to(&self, next: &str) -> Result<(), Error> {
        if !Self::ALLOWED_STATUSES.contains(&next) {
            return Err(Error::BadRequest(format!(
                "unknown appointment status: `{next}`"
            )));
        }
        let current = self.status.as_str();
        // No-op update is always fine.
        if current == next {
            return Ok(());
        }
        // Any status → cancelled is always allowed.
        if next == "cancelled" {
            return Ok(());
        }
        let ok = matches!(
            (current, next),
            ("scheduled", "confirmed")
                | ("confirmed", "in_progress")
                | ("in_progress", "completed")
        );
        if ok {
            Ok(())
        } else {
            Err(Error::BadRequest(format!(
                "invalid appointment status transition: `{current}` → `{next}`"
            )))
        }
    }

    /// Transition this appointment's status to `next`. This is the
    /// **only** sanctioned way to change `status` — direct assignment
    /// to the field bypasses this check and the event-log contract.
    ///
    /// On success, the caller **must** insert an
    /// [`AppointmentEvent`] row carrying the `from` / `to` values
    /// in the same transaction as the UPDATE on `appointments`.
    /// Snapshot `self.status` **before** calling this method so you
    /// still have the `from` value when constructing the event row.
    ///
    /// Recommended usage:
    ///
    /// ```text
    /// let from = appt.status.clone();
    /// appt.transition_to(new_status)?;
    /// // Inside one tx:
    /// //   UPDATE appointments SET status = ? WHERE id = ?
    /// //   INSERT INTO appointment_events (appointment_id, from_status,
    /// //                                   to_status, created_at) VALUES ...
    /// ```
    pub fn transition_to(&mut self, next: &str) -> Result<(), Error> {
        self.can_transition_to(next)?;
        self.status = next.to_string();
        Ok(())
    }

    // ────────────────────────────────────────────────────────────
    // State predicates — pure, no DB, no allocation
    // ────────────────────────────────────────────────────────────

    /// True once the clinical encounter is finished and the record
    /// is expected to be finalised. See also [`Self::is_billable`]
    /// for the billing-domain interpretation of this state.
    pub fn is_completed(&self) -> bool {
        self.status == "completed"
    }

    /// True once the appointment has been cancelled. Terminal state:
    /// a cancelled appointment never reopens; a rebooking creates a
    /// fresh row.
    pub fn is_cancelled(&self) -> bool {
        self.status == "cancelled"
    }

    /// True if any of the supplied check-ins belongs to this
    /// appointment AND is currently active (`waiting` / `in_room` /
    /// `with_doctor`). Uniqueness of the active check-in for a given
    /// appointment is enforced at the DB level by migration 0016;
    /// this method lets callers ask the question without a DB trip
    /// when the slice is already in hand (e.g. during a bulk
    /// schedule-view render).
    pub fn has_active_checkin<C: CheckInLike>(&self, check_ins: &[C]) -> bool {
        check_ins
            .iter()
            .any(|c| c.appointment_id() == self.id && c.is_active())
    }

    // ────────────────────────────────────────────────────────────
    // Billing linkage rule
    // ────────────────────────────────────────────────────────────

    /// A completed appointment is billable; anything else is not.
    ///
    /// Note that `completed → cancelled` is a valid transition (the
    /// lifecycle permits post-hoc cancellation, e.g. insurance
    /// rejection, clinical void). A previously-completed appointment
    /// that has since been cancelled returns `false` here — its
    /// current status is `cancelled`, not `completed`, and the
    /// billing layer must treat it as non-billable even if an
    /// invoice was already drafted.
    ///
    /// This is the authoritative reader for the rule. Automated
    /// invoice generation on transition is deferred to a later
    /// integration — for now, billing is a manual workflow driven
    /// by this predicate.
    pub fn is_billable(&self) -> bool {
        self.status == "completed"
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
    #[rustio(belongs_to = "MedicalRecord", display = "summary")]
    pub medical_record_id: Option<i64>,
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
        "medical_record_id",
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
        "medical_record_id",
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
            medical_record_id: row.get_optional_i64("medical_record_id")?,
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
            self.medical_record_id.into(),
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

// ───────────────────────────────────────────────────────────────
// MedicalRecord — the clinical note attached to a visit.
//
// One record per completed encounter. `appointment_id` is optional
// so records imported from another system (or typed retroactively)
// don't have to be tied to a scheduled appointment.
// ───────────────────────────────────────────────────────────────

#[derive(Debug, RustioAdmin)]
pub struct MedicalRecord {
    pub id: i64,
    #[rustio(belongs_to = "Patient", display = "full_name")]
    pub patient_id: i64,
    #[rustio(belongs_to = "Appointment", display = "reason")]
    pub appointment_id: Option<i64>,
    #[rustio(belongs_to = "Doctor", display = "full_name")]
    pub doctor_id: i64,
    pub summary: String,
    pub chief_complaint: String,
    pub assessment: String,
    pub plan: String,
    pub is_confidential: bool,
    pub recorded_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

impl Model for MedicalRecord {
    const TABLE: &'static str = "medical_records";
    const COLUMNS: &'static [&'static str] = &[
        "id",
        "patient_id",
        "appointment_id",
        "doctor_id",
        "summary",
        "chief_complaint",
        "assessment",
        "plan",
        "is_confidential",
        "recorded_at",
        "created_at",
    ];
    const INSERT_COLUMNS: &'static [&'static str] = &[
        "patient_id",
        "appointment_id",
        "doctor_id",
        "summary",
        "chief_complaint",
        "assessment",
        "plan",
        "is_confidential",
        "recorded_at",
        "created_at",
    ];

    fn id(&self) -> i64 {
        self.id
    }

    fn from_row(row: Row<'_>) -> Result<Self, Error> {
        Ok(Self {
            id: row.get_i64("id")?,
            patient_id: row.get_i64("patient_id")?,
            appointment_id: row.get_optional_i64("appointment_id")?,
            doctor_id: row.get_i64("doctor_id")?,
            summary: row.get_string("summary")?,
            chief_complaint: row.get_string("chief_complaint")?,
            assessment: row.get_string("assessment")?,
            plan: row.get_string("plan")?,
            is_confidential: row.get_bool("is_confidential")?,
            recorded_at: row.get_datetime("recorded_at")?,
            created_at: row.get_datetime("created_at")?,
        })
    }

    fn insert_values(&self) -> Vec<Value> {
        vec![
            self.patient_id.into(),
            self.appointment_id.into(),
            self.doctor_id.into(),
            self.summary.clone().into(),
            self.chief_complaint.clone().into(),
            self.assessment.clone().into(),
            self.plan.clone().into(),
            self.is_confidential.into(),
            self.recorded_at.into(),
            self.created_at.into(),
        ]
    }
}

// ───────────────────────────────────────────────────────────────
// Diagnosis — one or more per MedicalRecord.
//
// `code` is a free-form String so the project can adopt ICD-10,
// ICD-11, SNOMED, or a local code set without touching the model.
// ───────────────────────────────────────────────────────────────

#[derive(Debug, RustioAdmin)]
pub struct Diagnosis {
    pub id: i64,
    #[rustio(belongs_to = "MedicalRecord", display = "summary")]
    pub medical_record_id: i64,
    #[rustio(belongs_to = "Patient", display = "full_name")]
    pub patient_id: i64,
    pub code: String,
    pub description: String,
    pub severity: String,
    pub is_primary: bool,
    pub is_chronic: bool,
    pub noted_at: DateTime<Utc>,
    pub notes: String,
    pub created_at: DateTime<Utc>,
}

impl Model for Diagnosis {
    const TABLE: &'static str = "diagnoses";
    const COLUMNS: &'static [&'static str] = &[
        "id",
        "medical_record_id",
        "patient_id",
        "code",
        "description",
        "severity",
        "is_primary",
        "is_chronic",
        "noted_at",
        "notes",
        "created_at",
    ];
    const INSERT_COLUMNS: &'static [&'static str] = &[
        "medical_record_id",
        "patient_id",
        "code",
        "description",
        "severity",
        "is_primary",
        "is_chronic",
        "noted_at",
        "notes",
        "created_at",
    ];

    fn id(&self) -> i64 {
        self.id
    }

    fn from_row(row: Row<'_>) -> Result<Self, Error> {
        Ok(Self {
            id: row.get_i64("id")?,
            medical_record_id: row.get_i64("medical_record_id")?,
            patient_id: row.get_i64("patient_id")?,
            code: row.get_string("code")?,
            description: row.get_string("description")?,
            severity: row.get_string("severity")?,
            is_primary: row.get_bool("is_primary")?,
            is_chronic: row.get_bool("is_chronic")?,
            noted_at: row.get_datetime("noted_at")?,
            notes: row.get_string("notes")?,
            created_at: row.get_datetime("created_at")?,
        })
    }

    fn insert_values(&self) -> Vec<Value> {
        vec![
            self.medical_record_id.into(),
            self.patient_id.into(),
            self.code.clone().into(),
            self.description.clone().into(),
            self.severity.clone().into(),
            self.is_primary.into(),
            self.is_chronic.into(),
            self.noted_at.into(),
            self.notes.clone().into(),
            self.created_at.into(),
        ]
    }
}

// ───────────────────────────────────────────────────────────────
// VitalSigns — point-in-time measurements captured during a visit.
//
// `temperature_c` and `weight_kg` are Strings because RustIO has
// no Float type; store as `"36.8"` / `"72.3"`. Integer measurements
// (heart rate, BP, O2) stay as `i32`.
// ───────────────────────────────────────────────────────────────

#[derive(Debug, RustioAdmin)]
pub struct VitalSigns {
    pub id: i64,
    #[rustio(belongs_to = "MedicalRecord", display = "summary")]
    pub medical_record_id: i64,
    #[rustio(belongs_to = "Patient", display = "full_name")]
    pub patient_id: i64,
    pub heart_rate_bpm: i32,
    pub systolic_bp: i32,
    pub diastolic_bp: i32,
    pub temperature_c: String,
    pub oxygen_saturation: i32,
    pub weight_kg: String,
    pub height_cm: i32,
    pub recorded_at: DateTime<Utc>,
    pub notes: String,
    pub created_at: DateTime<Utc>,
}

impl Model for VitalSigns {
    const TABLE: &'static str = "vital_signs";
    const COLUMNS: &'static [&'static str] = &[
        "id",
        "medical_record_id",
        "patient_id",
        "heart_rate_bpm",
        "systolic_bp",
        "diastolic_bp",
        "temperature_c",
        "oxygen_saturation",
        "weight_kg",
        "height_cm",
        "recorded_at",
        "notes",
        "created_at",
    ];
    const INSERT_COLUMNS: &'static [&'static str] = &[
        "medical_record_id",
        "patient_id",
        "heart_rate_bpm",
        "systolic_bp",
        "diastolic_bp",
        "temperature_c",
        "oxygen_saturation",
        "weight_kg",
        "height_cm",
        "recorded_at",
        "notes",
        "created_at",
    ];

    fn id(&self) -> i64 {
        self.id
    }

    fn from_row(row: Row<'_>) -> Result<Self, Error> {
        Ok(Self {
            id: row.get_i64("id")?,
            medical_record_id: row.get_i64("medical_record_id")?,
            patient_id: row.get_i64("patient_id")?,
            heart_rate_bpm: row.get_i32("heart_rate_bpm")?,
            systolic_bp: row.get_i32("systolic_bp")?,
            diastolic_bp: row.get_i32("diastolic_bp")?,
            temperature_c: row.get_string("temperature_c")?,
            oxygen_saturation: row.get_i32("oxygen_saturation")?,
            weight_kg: row.get_string("weight_kg")?,
            height_cm: row.get_i32("height_cm")?,
            recorded_at: row.get_datetime("recorded_at")?,
            notes: row.get_string("notes")?,
            created_at: row.get_datetime("created_at")?,
        })
    }

    fn insert_values(&self) -> Vec<Value> {
        vec![
            self.medical_record_id.into(),
            self.patient_id.into(),
            self.heart_rate_bpm.into(),
            self.systolic_bp.into(),
            self.diastolic_bp.into(),
            self.temperature_c.clone().into(),
            self.oxygen_saturation.into(),
            self.weight_kg.clone().into(),
            self.height_cm.into(),
            self.recorded_at.into(),
            self.notes.clone().into(),
            self.created_at.into(),
        ]
    }
}

// ═══════════════════════════════════════════════════════════════
// AppointmentEvent — append-only audit of every status transition
//
// One row is inserted by the caller of `Appointment::transition_to`
// in the same DB transaction as the status UPDATE. The log is
// forward-only; rows are never edited or deleted by application
// code (the ON DELETE CASCADE on `appointment_id` is there so
// deleting a whole appointment leaves no dangling events, not as a
// channel for general deletion).
// ═══════════════════════════════════════════════════════════════

#[derive(Debug, RustioAdmin)]
pub struct AppointmentEvent {
    pub id: i64,
    #[rustio(belongs_to = "Appointment", display = "reason")]
    pub appointment_id: i64,
    pub from_status: String,
    pub to_status: String,
    pub created_at: DateTime<Utc>,
}

impl Model for AppointmentEvent {
    const TABLE: &'static str = "appointment_events";
    const COLUMNS: &'static [&'static str] = &[
        "id",
        "appointment_id",
        "from_status",
        "to_status",
        "created_at",
    ];
    const INSERT_COLUMNS: &'static [&'static str] = &[
        "appointment_id",
        "from_status",
        "to_status",
        "created_at",
    ];

    fn id(&self) -> i64 {
        self.id
    }

    fn from_row(row: Row<'_>) -> Result<Self, Error> {
        Ok(Self {
            id: row.get_i64("id")?,
            appointment_id: row.get_i64("appointment_id")?,
            from_status: row.get_string("from_status")?,
            to_status: row.get_string("to_status")?,
            created_at: row.get_datetime("created_at")?,
        })
    }

    fn insert_values(&self) -> Vec<Value> {
        vec![
            self.appointment_id.into(),
            self.from_status.clone().into(),
            self.to_status.clone().into(),
            self.created_at.into(),
        ]
    }
}
