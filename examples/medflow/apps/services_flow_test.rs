//! End-to-end hospital workflow verification.
//!
//! Runs the full happy-path flow through the application service
//! layer on a fresh in-memory SQLite database, asserting observable
//! state after each step. Negative checks are embedded inline so
//! one `cargo test` run proves both the primary workflow AND the
//! constraint-enforcement layer.
//!
//! Run: `cargo test --manifest-path examples/medflow/Cargo.toml`

use chrono::{TimeZone, Utc};
use rustio_core::{migrations, Db, Error, Model};

use super::billing::models::{Invoice, Payment};
use super::care::models::{Appointment, AppointmentEvent, Diagnosis, MedicalRecord, Prescription};
use super::people::models::{Department, Doctor, Patient};
use super::services::*;
use super::workflow::models::CheckIn;

/// Spin up an in-memory DB and apply every migration in the project.
/// Used by each `#[tokio::test]` for total isolation.
async fn setup_db() -> Db {
    let db = Db::memory().await.expect("in-memory db");
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations");
    migrations::apply_with(&db, &dir, migrations::ApplyOptions::default())
        .await
        .expect("migrations apply");
    db
}

/// Seed the three root entities (Department → Doctor → Patient) and
/// return their ids. Order matters: `Doctor.department_id` and
/// `Appointment.*` FKs require the parents to exist first.
async fn seed_people(db: &Db) -> (i64, i64, i64) {
    let dept_id = Department {
        id: 0,
        name: "Cardiology".into(),
        code: "CARD".into(),
        is_active: true,
        head_doctor_id: None,
        created_at: Utc::now(),
    }
    .create(db)
    .await
    .expect("create Department");

    let doctor_id = Doctor {
        id: 0,
        full_name: "Dr. Erik Nilsson".into(),
        specialty: "Cardiology".into(),
        department_id: dept_id,
        license_no: "SE-CARD-0001".into(),
        email: "e.nilsson@medflow.test".into(),
        phone: "+46-70-100-0001".into(),
        years_experience: 12,
        is_active: true,
        created_at: Utc::now(),
    }
    .create(db)
    .await
    .expect("create Doctor");

    let patient_id = Patient {
        id: 0,
        full_name: "Anna Lindberg".into(),
        date_of_birth: Utc.with_ymd_and_hms(1985, 6, 1, 0, 0, 0).unwrap(),
        gender: "female".into(),
        national_id: "19850601-1234".into(),
        phone: "+46-70-200-0001".into(),
        email: "anna.lindberg@example.com".into(),
        blood_type: "A+".into(),
        allergies: "penicillin".into(),
        is_active: true,
        created_at: Utc::now(),
    }
    .create(db)
    .await
    .expect("create Patient");

    (dept_id, doctor_id, patient_id)
}

// ═══════════════════════════════════════════════════════════════
// MAIN TEST — the full 13-step hospital workflow
// ═══════════════════════════════════════════════════════════════

#[tokio::test]
async fn hospital_flow_end_to_end() {
    let db = setup_db().await;

    // ── Steps 1-3: seed root entities ────────────────────────
    let (dept_id, doctor_id, patient_id) = seed_people(&db).await;
    assert!(dept_id > 0 && doctor_id > 0 && patient_id > 0);

    // ── Step 4: schedule_appointment ─────────────────────────
    let scheduled_at = Utc.with_ymd_and_hms(2026, 5, 10, 10, 0, 0).unwrap();
    let appt_id = schedule_appointment(
        &db,
        ScheduleAppointmentInput {
            patient_id,
            doctor_id,
            department_id: Some(dept_id),
            scheduled_at,
            reason: "Chest pain follow-up".into(),
            notes: "".into(),
            duration_minutes: 30,
            priority: 5,
        },
    )
    .await
    .expect("schedule_appointment");

    let appt = Appointment::find(&db, appt_id).await.unwrap().unwrap();
    assert_eq!(appt.status, "scheduled");
    assert_eq!(appt.patient_id, patient_id);
    assert_eq!(appt.doctor_id, doctor_id);
    assert_eq!(appt.department_id, Some(dept_id));
    assert_eq!(appt.duration_minutes, 30);
    // Creation is NOT a transition — no event written yet.
    assert_eq!(AppointmentEvent::all(&db).await.unwrap().len(), 0);

    // Negative: non-positive duration is rejected by the service.
    let err = schedule_appointment(
        &db,
        ScheduleAppointmentInput {
            patient_id,
            doctor_id,
            department_id: None,
            scheduled_at,
            reason: "".into(),
            notes: "".into(),
            duration_minutes: 0,
            priority: 5,
        },
    )
    .await
    .unwrap_err();
    assert!(
        matches!(err, Error::BadRequest(_)),
        "expected BadRequest for zero duration, got {err:?}"
    );

    // ── Step 5: confirm_appointment ──────────────────────────
    confirm_appointment(&db, appt_id)
        .await
        .expect("confirm_appointment");

    let appt = Appointment::find(&db, appt_id).await.unwrap().unwrap();
    assert_eq!(appt.status, "confirmed");

    let events = AppointmentEvent::all(&db).await.unwrap();
    assert_eq!(events.len(), 1, "one event after confirm");
    assert_eq!(events[0].appointment_id, appt_id);
    assert_eq!(events[0].from_status, "scheduled");
    assert_eq!(events[0].to_status, "confirmed");

    // ── Step 6: check_in_appointment ─────────────────────────
    let checkin_id = check_in_appointment(
        &db,
        CheckInInput {
            appointment_id: appt_id,
            staff_id: None,
            room_id: None,
            priority: 5,
            notes: "Arrived 5 min early".into(),
        },
    )
    .await
    .expect("check_in_appointment");

    let checkin = CheckIn::find(&db, checkin_id).await.unwrap().unwrap();
    assert_eq!(checkin.appointment_id, appt_id);
    assert_eq!(checkin.patient_id, patient_id);
    assert_eq!(checkin.status, "waiting");

    // Negative: a second active check-in for the same appointment
    // trips the partial unique index from migration 0016.
    let dup = check_in_appointment(
        &db,
        CheckInInput {
            appointment_id: appt_id,
            staff_id: None,
            room_id: None,
            priority: 5,
            notes: "".into(),
        },
    )
    .await
    .unwrap_err();
    match dup {
        Error::Internal(msg) => assert!(
            msg.contains("UNIQUE") || msg.contains("unique"),
            "expected UNIQUE violation, got: {msg}"
        ),
        other => panic!("expected Internal UNIQUE error, got {other:?}"),
    }

    // ── Step 7: start_consultation ───────────────────────────
    start_consultation(&db, appt_id)
        .await
        .expect("start_consultation");

    let appt = Appointment::find(&db, appt_id).await.unwrap().unwrap();
    assert_eq!(appt.status, "in_progress");
    assert!(!appt.is_completed());
    assert!(!appt.is_cancelled());
    assert!(!appt.is_billable());

    // Check-in should have been promoted as a side effect.
    let checkin = CheckIn::find(&db, checkin_id).await.unwrap().unwrap();
    assert_eq!(checkin.status, "with_doctor");

    assert_eq!(AppointmentEvent::all(&db).await.unwrap().len(), 2);

    // has_active_checkin via the CheckInLike bridge.
    let all_checkins = CheckIn::all(&db).await.unwrap();
    assert!(
        appt.has_active_checkin(&all_checkins),
        "active checkin should be detected"
    );

    // ── Step 8: create_medical_record_for_appointment ────────
    let record_id = create_medical_record_for_appointment(
        &db,
        MedicalRecordInput {
            appointment_id: appt_id,
            summary: "Routine cardio consult".into(),
            chief_complaint: "Chest tightness on exertion".into(),
            assessment: "Stable angina, not acute.".into(),
            plan: "Continue statin. Add beta blocker. Follow-up 4w.".into(),
            is_confidential: false,
        },
    )
    .await
    .expect("create_medical_record_for_appointment");

    let record = MedicalRecord::find(&db, record_id).await.unwrap().unwrap();
    assert_eq!(record.patient_id, patient_id);
    assert_eq!(record.doctor_id, doctor_id);
    assert_eq!(record.appointment_id, Some(appt_id));

    // Negative: one record per appointment is enforced at the service layer.
    let dup = create_medical_record_for_appointment(
        &db,
        MedicalRecordInput {
            appointment_id: appt_id,
            summary: "dup".into(),
            chief_complaint: "".into(),
            assessment: "".into(),
            plan: "".into(),
            is_confidential: false,
        },
    )
    .await
    .unwrap_err();
    assert!(matches!(dup, Error::BadRequest(_)));

    // ── Step 9: add_diagnosis_to_record ──────────────────────
    let diag1_id = add_diagnosis_to_record(
        &db,
        DiagnosisInput {
            medical_record_id: record_id,
            code: "I20.9".into(),
            description: "Angina pectoris, unspecified".into(),
            severity: "moderate".into(),
            is_primary: true,
            is_chronic: true,
            notes: "".into(),
        },
    )
    .await
    .expect("primary diagnosis");

    // A second primary diagnosis demotes the first.
    let diag2_id = add_diagnosis_to_record(
        &db,
        DiagnosisInput {
            medical_record_id: record_id,
            code: "E78.5".into(),
            description: "Hyperlipidemia, unspecified".into(),
            severity: "mild".into(),
            is_primary: true,
            is_chronic: true,
            notes: "".into(),
        },
    )
    .await
    .expect("secondary primary diagnosis");

    let diag1 = Diagnosis::find(&db, diag1_id).await.unwrap().unwrap();
    let diag2 = Diagnosis::find(&db, diag2_id).await.unwrap().unwrap();
    assert!(!diag1.is_primary, "first primary should be demoted");
    assert!(diag2.is_primary, "second should remain primary");

    // Negative: unknown severity rejected.
    let bad = add_diagnosis_to_record(
        &db,
        DiagnosisInput {
            medical_record_id: record_id,
            code: "X".into(),
            description: "x".into(),
            severity: "catastrophic".into(),
            is_primary: false,
            is_chronic: false,
            notes: "".into(),
        },
    )
    .await
    .unwrap_err();
    assert!(matches!(bad, Error::BadRequest(_)));

    // ── Step 10: add_prescription_to_record ──────────────────
    let rx_id = add_prescription_to_record(
        &db,
        PrescriptionInput {
            medical_record_id: record_id,
            medication: "Metoprolol".into(),
            dosage: "50 mg".into(),
            frequency: "twice daily".into(),
            duration_days: 30,
            is_refillable: true,
            refills_remaining: 2,
            notes: "".into(),
        },
    )
    .await
    .expect("add_prescription_to_record");

    let rx = Prescription::find(&db, rx_id).await.unwrap().unwrap();
    assert_eq!(rx.appointment_id, appt_id);
    assert_eq!(rx.medical_record_id, Some(record_id));
    assert_eq!(rx.patient_id, patient_id);
    assert_eq!(rx.doctor_id, doctor_id);

    // Negative: non-positive duration_days rejected.
    let bad = add_prescription_to_record(
        &db,
        PrescriptionInput {
            medical_record_id: record_id,
            medication: "X".into(),
            dosage: "1".into(),
            frequency: "daily".into(),
            duration_days: 0,
            is_refillable: false,
            refills_remaining: 0,
            notes: "".into(),
        },
    )
    .await
    .unwrap_err();
    assert!(matches!(bad, Error::BadRequest(_)));

    // ── Step 11: complete_appointment ────────────────────────
    complete_appointment(&db, appt_id)
        .await
        .expect("complete_appointment");

    let appt = Appointment::find(&db, appt_id).await.unwrap().unwrap();
    assert_eq!(appt.status, "completed");
    assert!(appt.is_completed());
    assert!(appt.is_billable(), "completed appointment must be billable");

    // Side effect: active check-in closed.
    let checkin = CheckIn::find(&db, checkin_id).await.unwrap().unwrap();
    assert_eq!(checkin.status, "done");

    // Event chain: scheduled → confirmed → in_progress → completed.
    let mut events = AppointmentEvent::all(&db).await.unwrap();
    events.sort_by_key(|e| e.id);
    let chain: Vec<(String, String)> = events
        .iter()
        .map(|e| (e.from_status.clone(), e.to_status.clone()))
        .collect();
    assert_eq!(
        chain,
        vec![
            ("scheduled".into(), "confirmed".into()),
            ("confirmed".into(), "in_progress".into()),
            ("in_progress".into(), "completed".into()),
        ]
    );

    // ── Step 12: issue_invoice_for_completed_appointment ─────
    let invoice_id = issue_invoice_for_completed_appointment(
        &db,
        IssueInvoiceInput {
            appointment_id: appt_id,
            invoice_number: "INV-2026-0001".into(),
            amount_cents: 12_500,
            currency: "SEK".into(),
            notes: "Cardio consult + beta blocker prescription".into(),
        },
    )
    .await
    .expect("issue_invoice_for_completed_appointment");

    let invoice = Invoice::find(&db, invoice_id).await.unwrap().unwrap();
    assert_eq!(invoice.status, "pending");
    assert_eq!(invoice.amount_cents, 12_500);
    assert_eq!(invoice.currency, "SEK");
    assert_eq!(invoice.patient_id, patient_id);
    assert_eq!(invoice.appointment_id, Some(appt_id));

    // ── Step 13: record_payment ──────────────────────────────
    let payment_id = record_payment(
        &db,
        PaymentInput {
            invoice_id,
            amount_cents: 12_500,
            currency: "SEK".into(),
            method: "card".into(),
            reference: "AUTH-ABC123".into(),
            received_at: Utc::now(),
            notes: "".into(),
        },
    )
    .await
    .expect("record_payment");

    let payment = Payment::find(&db, payment_id).await.unwrap().unwrap();
    assert_eq!(payment.invoice_id, invoice_id);
    assert_eq!(payment.amount_cents, 12_500);
    assert_eq!(payment.method, "card");
    assert_eq!(payment.patient_id, patient_id);

    // Automation is deferred: invoice stays `pending` after full payment.
    let invoice = Invoice::find(&db, invoice_id).await.unwrap().unwrap();
    assert_eq!(
        invoice.status, "pending",
        "record_payment must not auto-transition the invoice"
    );

    // Negative: unknown payment method rejected.
    let bad = record_payment(
        &db,
        PaymentInput {
            invoice_id,
            amount_cents: 0,
            currency: "SEK".into(),
            method: "bitcoin".into(),
            reference: "".into(),
            received_at: Utc::now(),
            notes: "".into(),
        },
    )
    .await
    .unwrap_err();
    assert!(matches!(bad, Error::BadRequest(_)));

    // ── Final cross-cutting invariants ───────────────────────
    assert_eq!(Appointment::all(&db).await.unwrap().len(), 1);
    assert_eq!(MedicalRecord::all(&db).await.unwrap().len(), 1);
    assert_eq!(Diagnosis::all(&db).await.unwrap().len(), 2);
    assert_eq!(Prescription::all(&db).await.unwrap().len(), 1);
    assert_eq!(Invoice::all(&db).await.unwrap().len(), 1);
    assert_eq!(Payment::all(&db).await.unwrap().len(), 1);
    assert_eq!(CheckIn::all(&db).await.unwrap().len(), 1);
    assert_eq!(AppointmentEvent::all(&db).await.unwrap().len(), 3);
}

// ═══════════════════════════════════════════════════════════════
// BILLING GUARD — invoice cannot be issued before completion
// ═══════════════════════════════════════════════════════════════

#[tokio::test]
async fn cannot_invoice_before_appointment_is_completed() {
    let db = setup_db().await;
    let (dept_id, doctor_id, patient_id) = seed_people(&db).await;

    let appt_id = schedule_appointment(
        &db,
        ScheduleAppointmentInput {
            patient_id,
            doctor_id,
            department_id: Some(dept_id),
            scheduled_at: Utc::now(),
            reason: "".into(),
            notes: "".into(),
            duration_minutes: 30,
            priority: 5,
        },
    )
    .await
    .unwrap();

    // status = "scheduled"
    let err = issue_invoice_for_completed_appointment(
        &db,
        IssueInvoiceInput {
            appointment_id: appt_id,
            invoice_number: "INV-PREMATURE".into(),
            amount_cents: 100,
            currency: "SEK".into(),
            notes: "".into(),
        },
    )
    .await
    .unwrap_err();
    assert!(matches!(err, Error::BadRequest(_)));

    // status = "confirmed" — still not billable
    confirm_appointment(&db, appt_id).await.unwrap();
    let err = issue_invoice_for_completed_appointment(
        &db,
        IssueInvoiceInput {
            appointment_id: appt_id,
            invoice_number: "INV-PREMATURE-2".into(),
            amount_cents: 100,
            currency: "SEK".into(),
            notes: "".into(),
        },
    )
    .await
    .unwrap_err();
    assert!(matches!(err, Error::BadRequest(_)));

    // Nothing slipped through.
    assert_eq!(Invoice::all(&db).await.unwrap().len(), 0);
}

// ═══════════════════════════════════════════════════════════════
// LIFECYCLE GUARD — invalid transition rejected by transition_to
// ═══════════════════════════════════════════════════════════════

#[tokio::test]
async fn cannot_skip_lifecycle_stages() {
    let db = setup_db().await;
    let (dept_id, doctor_id, patient_id) = seed_people(&db).await;

    let appt_id = schedule_appointment(
        &db,
        ScheduleAppointmentInput {
            patient_id,
            doctor_id,
            department_id: Some(dept_id),
            scheduled_at: Utc::now(),
            reason: "".into(),
            notes: "".into(),
            duration_minutes: 30,
            priority: 5,
        },
    )
    .await
    .unwrap();

    // scheduled → in_progress is NOT in the transition table.
    let err = start_consultation(&db, appt_id).await.unwrap_err();
    assert!(
        matches!(err, Error::BadRequest(_)),
        "expected BadRequest for invalid delta, got {err:?}"
    );

    // scheduled → completed also rejected.
    let err = complete_appointment(&db, appt_id).await.unwrap_err();
    assert!(matches!(err, Error::BadRequest(_)));

    // Confirm that no spurious events or status drift happened.
    let appt = Appointment::find(&db, appt_id).await.unwrap().unwrap();
    assert_eq!(appt.status, "scheduled");
    assert_eq!(AppointmentEvent::all(&db).await.unwrap().len(), 0);
}

// ═══════════════════════════════════════════════════════════════
// CANCEL FROM ANY STAGE — terminal transition is always allowed
// ═══════════════════════════════════════════════════════════════

#[tokio::test]
async fn cancel_is_always_allowed() {
    let db = setup_db().await;
    let (dept_id, doctor_id, patient_id) = seed_people(&db).await;

    let appt_id = schedule_appointment(
        &db,
        ScheduleAppointmentInput {
            patient_id,
            doctor_id,
            department_id: Some(dept_id),
            scheduled_at: Utc::now(),
            reason: "".into(),
            notes: "".into(),
            duration_minutes: 30,
            priority: 5,
        },
    )
    .await
    .unwrap();

    confirm_appointment(&db, appt_id).await.unwrap();
    start_consultation(&db, appt_id).await.unwrap();
    complete_appointment(&db, appt_id).await.unwrap();

    let appt = Appointment::find(&db, appt_id).await.unwrap().unwrap();
    assert!(appt.is_billable());

    // Post-hoc cancel after completion is allowed by the lifecycle.
    cancel_appointment(&db, appt_id).await.unwrap();

    let appt = Appointment::find(&db, appt_id).await.unwrap().unwrap();
    assert_eq!(appt.status, "cancelled");
    assert!(appt.is_cancelled());
    assert!(
        !appt.is_billable(),
        "a cancelled appointment — even one previously completed — must not be billable"
    );
}
