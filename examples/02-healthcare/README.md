# Healthcare

**Complexity:** ⭐⭐⭐⭐⭐
**Models:** 6

## What this domain teaches

A real clinic / hospital system: patients, doctors, scheduled and
emergency appointments, prescription lifecycle, attached medical
records, and per-doctor availability windows. Status fields and
time-based filters drive every operator workflow.

## Models

| Model               | Key fields                                                                                          | Relations                                                                          |
|---------------------|-----------------------------------------------------------------------------------------------------|------------------------------------------------------------------------------------|
| Patient             | `first_name`, `last_name`, `email`, `phone`, `date_of_birth?`, `blood_type?`, `allergies?`, `is_active` | (none)                                                                          |
| Doctor              | `first_name`, `last_name`, `email`, `specialization`, `license_number`, `department?`, `is_active`  | (none)                                                                             |
| DoctorAvailability  | `doctor_id`, `day_of_week`, `start_time`, `end_time`, `is_active`                                   | belongs_to Doctor                                                                  |
| Appointment         | `patient_id`, `doctor_id`, `scheduled_at`, `status`, `is_emergency`, `visit_reason`, `checked_in_at?`, `completed_at?` | belongs_to Patient, belongs_to Doctor                                  |
| Prescription        | `patient_id`, `prescribing_doctor_id`, `appointment_id?`, `medication_name`, `dosage`, `frequency`, `refills_remaining`, `status`, `issued_at`, `expires_at?` | belongs_to Patient, belongs_to Doctor, belongs_to Appointment (nullable) |
| MedicalRecord       | `patient_id`, `author_id`, `appointment_id?`, `record_type`, `title`, `summary`, `attachment_url?`, `is_confidential`, `recorded_at` | belongs_to Patient, belongs_to Doctor (`author_id`), belongs_to Appointment (nullable) |

`?` marks nullable fields. Every model also carries auto-managed
`id`, `created_at`, `updated_at` (not editable).

## Filtering scenarios

* **Today's clinic schedule for one doctor** — `Appointment.doctor_id=X AND scheduled_at BETWEEN today_start AND today_end AND status IN ('scheduled', 'checked_in')`, ordered by `scheduled_at ASC`. The doctor's daily worklist.
* **Emergency intake right now** — `Appointment.is_emergency=true AND status='scheduled' AND scheduled_at <= now+1h`. Triage view.
* **Expiring prescriptions** — `Prescription.status='active' AND expires_at < now+7d`, grouped by `patient_id`. Drives proactive outreach.
* **Refills exhausted** — `Prescription.status='active' AND refills_remaining=0`. Patients who need a re-evaluation visit.
* **Doctor's weekly availability** — `DoctorAvailability.doctor_id=X AND is_active=true`, grouped by `day_of_week`. Combined with `Appointment` for the same doctor, surfaces available slots.
* **No-show pattern by patient** — `Appointment.patient_id=X AND status='no_show' AND scheduled_at >= now-90d`. Operator decision support.

## Status / lifecycle conventions

`Appointment.status`:
`scheduled` → `checked_in` → `completed`, with terminal branches
`cancelled` and `no_show`.

`Prescription.status`:
`active` → `completed` (course finished) or `cancelled` (revoked early).

`DoctorAvailability.day_of_week` (lowercase only):
`monday`, `tuesday`, `wednesday`, `thursday`, `friday`, `saturday`, `sunday`.

`MedicalRecord.record_type`:
`consultation`, `diagnosis`, `lab_result`, `imaging`, `note`.

## ⚠️ Production gap: access audit

This schema intentionally omits a `MedicalRecordAccess` table.
A real deployment **must** add one with at least:

* `record_id` — FK to MedicalRecord
* `viewer_id` — who looked
* `viewed_at` — when
* `action` — `read`, `edit`, `export`
* `ip_address` — origin

**Reason:** GDPR / HIPAA-class regulations require an immutable
audit trail of every access to a patient's clinical data, not just
edits. Without this table, the system cannot satisfy a "who saw my
record?" subject access request.

## How to use

```
rustio new project clinic --schema schema.json
```

## Why this matters

Healthcare is the canonical "data is responsibility" domain.
Scheduling under availability constraints, prescription expiry,
attached files, and the regulatory audit gap above are all real
problems every clinic must solve. This schema is the minimum
shape that makes all of them visible at once.

## Next

→ `examples/03-school-system/` — structured rosters, terms, grading.
