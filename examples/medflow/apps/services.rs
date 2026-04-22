//! Application service layer — workflow orchestration for the hospital
//! system.
//!
//! Services are free async functions that take a `&Db` plus a typed
//! input struct, validate preconditions, call the appropriate
//! lifecycle methods on the domain models, persist changes, and
//! write audit rows. They are the only sanctioned place in the
//! project to mutate workflow state: models stay pure, the admin UI
//! round-trips fields directly through [`rustio_core::Model`], and
//! every workflow-aware change goes through a service.
//!
//! ## Two-step writes — no cross-row transaction
//!
//! `rustio_core::Db` does not yet expose a transaction API to the
//! project layer (the `pool()` accessor is crate-private). Services
//! that need to write more than one row do so step-by-step. Under
//! failure, a later step may be missing. We mitigate by:
//!
//!   * ordering writes so the authoritative row is written FIRST
//!     (e.g. [`transition_appointment`] UPDATEs `appointments`, then
//!     appends the [`AppointmentEvent`] — the log never describes a
//!     transition that didn't land on the appointment row);
//!   * returning `Err` at the first failure so the caller sees the
//!     partial state.
//!
//! Full atomicity arrives when core exposes `Db::transaction()`.
//!
//! ## Dead code
//!
//! These functions have no call sites yet — there is no HTTP layer.
//! They are the intended target of integration tests, seed scripts,
//! and the eventual workflow API. `#[allow(dead_code)]` at the
//! module level keeps `cargo check` quiet until callers arrive.

#![allow(dead_code)]

use chrono::{DateTime, Utc};
use rustio_core::{Db, Error, Model};

use crate::apps::billing::models::{Invoice, Payment};
use crate::apps::care::models::{
    Appointment, AppointmentEvent, Diagnosis, MedicalRecord, Prescription,
};
use crate::apps::workflow::models::CheckIn;

// ═══════════════════════════════════════════════════════════════
// Shared helpers
// ═══════════════════════════════════════════════════════════════

async fn load_appointment(db: &Db, id: i64) -> Result<Appointment, Error> {
    Appointment::find(db, id)
        .await?
        .ok_or_else(|| Error::BadRequest(format!("appointment #{id} not found")))
}

async fn load_record(db: &Db, id: i64) -> Result<MedicalRecord, Error> {
    MedicalRecord::find(db, id)
        .await?
        .ok_or_else(|| Error::BadRequest(format!("medical record #{id} not found")))
}

async fn load_invoice(db: &Db, id: i64) -> Result<Invoice, Error> {
    Invoice::find(db, id)
        .await?
        .ok_or_else(|| Error::BadRequest(format!("invoice #{id} not found")))
}

/// Find the currently-active check-in for an appointment, if any.
/// Active = `waiting` / `in_room` / `with_doctor` (mirrors the
/// partial unique index in migration 0016). Implemented as a full
/// table scan; acceptable at demo scale. A per-appointment query
/// helper belongs on `workflow::models::CheckIn` once there is a
/// real call-site pressure.
async fn find_active_checkin(db: &Db, appointment_id: i64) -> Result<Option<CheckIn>, Error> {
    Ok(CheckIn::all(db).await?.into_iter().find(|c| {
        c.appointment_id == appointment_id
            && matches!(c.status.as_str(), "waiting" | "in_room" | "with_doctor")
    }))
}

/// Pair an appointment status transition with an `AppointmentEvent`
/// audit row. Order of writes:
///
///   1. `Appointment::transition_to` — validates the delta in-memory
///      and mutates `status` on the in-scope `&mut Appointment`.
///   2. `Appointment::update` — UPDATE appointments SET … WHERE id.
///      The DB CHECK constraint rejects any value outside the
///      canonical vocabulary here.
///   3. `AppointmentEvent::create` — INSERT INTO appointment_events.
///      Ordered last so the log never describes a transition that
///      did not land on the source row. A failure here means the
///      appointment moved but the event is missing; surface this as
///      `Err` and let the caller reconcile.
async fn transition_appointment(db: &Db, id: i64, next: &str) -> Result<(), Error> {
    let mut appt = load_appointment(db, id).await?;
    let from_status = appt.status.clone();
    appt.transition_to(next)?; // delta check (unknown value / invalid transition)
    appt.update(db).await?; // CHECK-enforced value write
    let event = AppointmentEvent {
        id: 0,
        appointment_id: id,
        from_status,
        to_status: next.to_string(),
        created_at: Utc::now(),
    };
    event.create(db).await?;
    Ok(())
}

// ═══════════════════════════════════════════════════════════════
// 1. schedule_appointment
//
// Precondition: inputs are internally consistent (positive duration,
// non-empty reason optional). Patient / Doctor / Department existence
// is enforced by the FK constraints on INSERT — if an id is wrong,
// SQLite returns `FOREIGN KEY constraint failed` and this function
// propagates it as `Error`.
//
// Not a transition: creation starts in `scheduled`. The first event
// row is written by the first `transition_to` call, typically via
// [`confirm_appointment`].
// ═══════════════════════════════════════════════════════════════

pub struct ScheduleAppointmentInput {
    pub patient_id: i64,
    pub doctor_id: i64,
    pub department_id: Option<i64>,
    pub scheduled_at: DateTime<Utc>,
    pub reason: String,
    pub notes: String,
    pub duration_minutes: i32,
    pub priority: i32,
}

pub async fn schedule_appointment(
    db: &Db,
    input: ScheduleAppointmentInput,
) -> Result<i64, Error> {
    if input.duration_minutes <= 0 {
        return Err(Error::BadRequest(
            "duration_minutes must be positive".into(),
        ));
    }
    let appt = Appointment {
        id: 0,
        patient_id: input.patient_id,
        doctor_id: input.doctor_id,
        department_id: input.department_id,
        scheduled_at: input.scheduled_at,
        status: "scheduled".to_string(),
        reason: input.reason,
        notes: input.notes,
        duration_minutes: input.duration_minutes,
        priority: input.priority,
        is_active: true,
        created_at: Utc::now(),
    };
    appt.create(db).await
}

// ═══════════════════════════════════════════════════════════════
// 2. confirm_appointment — scheduled → confirmed
// ═══════════════════════════════════════════════════════════════

pub async fn confirm_appointment(db: &Db, appointment_id: i64) -> Result<(), Error> {
    transition_appointment(db, appointment_id, "confirmed").await
}

// ═══════════════════════════════════════════════════════════════
// 3. check_in_appointment
//
// Requires the appointment to be in `scheduled` or `confirmed`. If
// still `scheduled`, the act of checking in also confirms it (one
// transition + event row), because a patient physically arriving is
// confirmation. The subsequent INSERT into `check_ins` is subject to
// the partial unique index from migration 0016 — a second active
// check-in for the same appointment raises
// `UNIQUE constraint failed: check_ins.appointment_id`.
// ═══════════════════════════════════════════════════════════════

pub struct CheckInInput {
    pub appointment_id: i64,
    pub staff_id: Option<i64>,
    pub room_id: Option<i64>,
    pub priority: i32,
    pub notes: String,
}

pub async fn check_in_appointment(db: &Db, input: CheckInInput) -> Result<i64, Error> {
    let appt = load_appointment(db, input.appointment_id).await?;
    match appt.status.as_str() {
        "scheduled" | "confirmed" => {}
        other => {
            return Err(Error::BadRequest(format!(
                "cannot check in an appointment with status `{other}`"
            )))
        }
    }
    if appt.status == "scheduled" {
        transition_appointment(db, appt.id, "confirmed").await?;
    }
    let now = Utc::now();
    let checkin = CheckIn {
        id: 0,
        appointment_id: appt.id,
        patient_id: appt.patient_id,
        staff_id: input.staff_id,
        room_id: input.room_id,
        checked_in_at: now,
        status: "waiting".to_string(),
        priority: input.priority,
        notes: input.notes,
        created_at: now,
    };
    checkin.create(db).await
}

// ═══════════════════════════════════════════════════════════════
// 4. start_consultation — confirmed → in_progress
//
// Side effect: if there is an active check-in for this appointment,
// advance its status to `with_doctor` so the front-desk board
// reflects reality. Absent check-ins are tolerated — simple clinics
// may skip the check-in step entirely.
// ═══════════════════════════════════════════════════════════════

pub async fn start_consultation(db: &Db, appointment_id: i64) -> Result<(), Error> {
    transition_appointment(db, appointment_id, "in_progress").await?;
    if let Some(mut checkin) = find_active_checkin(db, appointment_id).await? {
        if checkin.status != "with_doctor" {
            checkin.status = "with_doctor".to_string();
            checkin.update(db).await?;
        }
    }
    Ok(())
}

// ═══════════════════════════════════════════════════════════════
// 5. complete_appointment — in_progress → completed
//
// Side effect: close the active check-in (status = `done`). The
// appointment becomes billable at this point — see
// [`issue_invoice_for_completed_appointment`].
// ═══════════════════════════════════════════════════════════════

pub async fn complete_appointment(db: &Db, appointment_id: i64) -> Result<(), Error> {
    transition_appointment(db, appointment_id, "completed").await?;
    if let Some(mut checkin) = find_active_checkin(db, appointment_id).await? {
        checkin.status = "done".to_string();
        checkin.update(db).await?;
    }
    Ok(())
}

// ═══════════════════════════════════════════════════════════════
// 6. cancel_appointment — any → cancelled
//
// Side effect: close the active check-in as `left_without_seen`.
// Cancelled appointments are never billable; a previously-drafted
// invoice needs separate handling (void / refund — out of scope
// for this service).
// ═══════════════════════════════════════════════════════════════

pub async fn cancel_appointment(db: &Db, appointment_id: i64) -> Result<(), Error> {
    transition_appointment(db, appointment_id, "cancelled").await?;
    if let Some(mut checkin) = find_active_checkin(db, appointment_id).await? {
        checkin.status = "left_without_seen".to_string();
        checkin.update(db).await?;
    }
    Ok(())
}

// ═══════════════════════════════════════════════════════════════
// 7. create_medical_record_for_appointment
//
// Preconditions:
//   * Appointment exists and is `in_progress` or `completed`
//     (records are written during or after the consultation).
//   * No existing record for this appointment (business rule: one
//     record per appointment). Enforced via table scan since we
//     don't have a UNIQUE INDEX on medical_records.appointment_id.
//
// `patient_id` and `doctor_id` are derived from the appointment, so
// the caller cannot accidentally attach a record to the wrong
// patient.
// ═══════════════════════════════════════════════════════════════

pub struct MedicalRecordInput {
    pub appointment_id: i64,
    pub summary: String,
    pub chief_complaint: String,
    pub assessment: String,
    pub plan: String,
    pub is_confidential: bool,
}

pub async fn create_medical_record_for_appointment(
    db: &Db,
    input: MedicalRecordInput,
) -> Result<i64, Error> {
    let appt = load_appointment(db, input.appointment_id).await?;
    match appt.status.as_str() {
        "in_progress" | "completed" => {}
        other => {
            return Err(Error::BadRequest(format!(
                "medical records are written during / after consultation; status is `{other}`"
            )))
        }
    }
    if input.summary.trim().is_empty() {
        return Err(Error::BadRequest("summary is required".into()));
    }
    let existing = MedicalRecord::all(db)
        .await?
        .into_iter()
        .find(|r| r.appointment_id == Some(appt.id));
    if existing.is_some() {
        return Err(Error::BadRequest(format!(
            "appointment #{} already has a medical record",
            appt.id
        )));
    }
    let now = Utc::now();
    let record = MedicalRecord {
        id: 0,
        patient_id: appt.patient_id,
        appointment_id: Some(appt.id),
        doctor_id: appt.doctor_id,
        summary: input.summary,
        chief_complaint: input.chief_complaint,
        assessment: input.assessment,
        plan: input.plan,
        is_confidential: input.is_confidential,
        recorded_at: now,
        created_at: now,
    };
    record.create(db).await
}

// ═══════════════════════════════════════════════════════════════
// 8. add_diagnosis_to_record
//
// Preconditions:
//   * Record exists.
//   * `severity` is in the allow-list {mild, moderate, severe, critical}.
//
// Side effect: if `is_primary = true` and a prior primary diagnosis
// exists on the same record, demote the prior one. A record may
// have at most one primary diagnosis by convention (not enforced by
// the DB).
// ═══════════════════════════════════════════════════════════════

pub struct DiagnosisInput {
    pub medical_record_id: i64,
    pub code: String,
    pub description: String,
    pub severity: String,
    pub is_primary: bool,
    pub is_chronic: bool,
    pub notes: String,
}

pub async fn add_diagnosis_to_record(db: &Db, input: DiagnosisInput) -> Result<i64, Error> {
    let record = load_record(db, input.medical_record_id).await?;
    if !matches!(
        input.severity.as_str(),
        "mild" | "moderate" | "severe" | "critical"
    ) {
        return Err(Error::BadRequest(format!(
            "unknown severity: `{}`",
            input.severity
        )));
    }
    if input.code.trim().is_empty() {
        return Err(Error::BadRequest("diagnosis code is required".into()));
    }
    if input.is_primary {
        let prior_primaries: Vec<Diagnosis> = Diagnosis::all(db)
            .await?
            .into_iter()
            .filter(|d| d.medical_record_id == input.medical_record_id && d.is_primary)
            .collect();
        for mut prior in prior_primaries {
            prior.is_primary = false;
            prior.update(db).await?;
        }
    }
    let now = Utc::now();
    let diag = Diagnosis {
        id: 0,
        medical_record_id: record.id,
        patient_id: record.patient_id,
        code: input.code,
        description: input.description,
        severity: input.severity,
        is_primary: input.is_primary,
        is_chronic: input.is_chronic,
        noted_at: now,
        notes: input.notes,
        created_at: now,
    };
    diag.create(db).await
}

// ═══════════════════════════════════════════════════════════════
// 9. add_prescription_to_record
//
// Preconditions:
//   * Record exists and carries a non-NULL `appointment_id` (a
//     prescription needs an appointment FK; the column on the
//     `prescriptions` table is NOT NULL by migration 0005).
//   * `duration_days > 0`, `refills_remaining >= 0`.
// ═══════════════════════════════════════════════════════════════

pub struct PrescriptionInput {
    pub medical_record_id: i64,
    pub medication: String,
    pub dosage: String,
    pub frequency: String,
    pub duration_days: i32,
    pub is_refillable: bool,
    pub refills_remaining: i32,
    pub notes: String,
}

pub async fn add_prescription_to_record(
    db: &Db,
    input: PrescriptionInput,
) -> Result<i64, Error> {
    let record = load_record(db, input.medical_record_id).await?;
    let appointment_id = record.appointment_id.ok_or_else(|| {
        Error::BadRequest(format!(
            "medical record #{} has no appointment — prescription requires an appointment FK",
            record.id
        ))
    })?;
    if input.duration_days <= 0 {
        return Err(Error::BadRequest("duration_days must be positive".into()));
    }
    if input.refills_remaining < 0 {
        return Err(Error::BadRequest(
            "refills_remaining cannot be negative".into(),
        ));
    }
    if input.medication.trim().is_empty() {
        return Err(Error::BadRequest("medication is required".into()));
    }
    let rx = Prescription {
        id: 0,
        appointment_id,
        medical_record_id: Some(record.id),
        patient_id: record.patient_id,
        doctor_id: record.doctor_id,
        medication: input.medication,
        dosage: input.dosage,
        frequency: input.frequency,
        duration_days: input.duration_days,
        is_refillable: input.is_refillable,
        refills_remaining: input.refills_remaining,
        notes: input.notes,
        created_at: Utc::now(),
    };
    rx.create(db).await
}

// ═══════════════════════════════════════════════════════════════
// 10. issue_invoice_for_completed_appointment
//
// Preconditions:
//   * Appointment exists and `is_billable()` returns true. The
//     predicate is the authoritative reader for the billing rule:
//     only `status == "completed"` is billable. A previously-
//     completed but since-cancelled appointment returns false here.
//   * `invoice_number` non-empty, `amount_cents >= 0`.
//
// Issues the invoice in `pending` state. The payment lifecycle
// (`pending → paid` / `pending → failed`) is driven manually by the
// billing team — no automation in this service.
// ═══════════════════════════════════════════════════════════════

pub struct IssueInvoiceInput {
    pub appointment_id: i64,
    pub invoice_number: String,
    pub amount_cents: i64,
    pub currency: String,
    pub notes: String,
}

pub async fn issue_invoice_for_completed_appointment(
    db: &Db,
    input: IssueInvoiceInput,
) -> Result<i64, Error> {
    let appt = load_appointment(db, input.appointment_id).await?;
    if !appt.is_billable() {
        return Err(Error::BadRequest(format!(
            "appointment #{} is not billable (status = `{}`)",
            appt.id, appt.status
        )));
    }
    if input.amount_cents < 0 {
        return Err(Error::BadRequest(
            "amount_cents must be non-negative".into(),
        ));
    }
    if input.invoice_number.trim().is_empty() {
        return Err(Error::BadRequest("invoice_number is required".into()));
    }
    let now = Utc::now();
    let invoice = Invoice {
        id: 0,
        invoice_number: input.invoice_number,
        patient_id: appt.patient_id,
        appointment_id: Some(appt.id),
        amount_cents: input.amount_cents,
        currency: input.currency,
        status: "pending".to_string(),
        issued_at: now,
        paid_at: None,
        notes: input.notes,
        created_at: now,
    };
    invoice.create(db).await
}

// ═══════════════════════════════════════════════════════════════
// 11. record_payment
//
// Appends one payment row against an invoice. Partial payments,
// insurance splits, and refunds (negative `amount_cents`) are all
// permitted.
//
// We deliberately do NOT auto-flip `invoice.status` to `paid` when
// the running total hits `invoice.amount_cents` — that is
// automation, explicitly deferred in the spec. The billing team
// drives invoice status transitions manually for now.
// ═══════════════════════════════════════════════════════════════

pub struct PaymentInput {
    pub invoice_id: i64,
    pub amount_cents: i64,
    pub currency: String,
    pub method: String,
    pub reference: String,
    pub received_at: DateTime<Utc>,
    pub notes: String,
}

pub async fn record_payment(db: &Db, input: PaymentInput) -> Result<i64, Error> {
    let invoice = load_invoice(db, input.invoice_id).await?;
    if !matches!(
        input.method.as_str(),
        "cash" | "card" | "transfer" | "insurance" | "other"
    ) {
        return Err(Error::BadRequest(format!(
            "unknown payment method: `{}`",
            input.method
        )));
    }
    let payment = Payment {
        id: 0,
        invoice_id: invoice.id,
        patient_id: invoice.patient_id,
        amount_cents: input.amount_cents,
        currency: input.currency,
        method: input.method,
        reference: input.reference,
        received_at: input.received_at,
        notes: input.notes,
        created_at: Utc::now(),
    };
    payment.create(db).await
}
