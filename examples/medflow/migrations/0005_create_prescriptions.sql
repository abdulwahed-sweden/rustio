-- Prescriptions — attached to an appointment. Patient and doctor are
-- denormalised for fast per-patient / per-doctor lookups without a
-- join back through appointments.
--
-- `ON DELETE CASCADE` on the appointment FK mirrors real-world intent:
-- if an appointment is deleted, its prescriptions disappear with it.
PRAGMA foreign_keys = ON;

CREATE TABLE prescriptions (
    id                 INTEGER PRIMARY KEY AUTOINCREMENT,
    appointment_id     INTEGER NOT NULL,
    patient_id         INTEGER NOT NULL,
    doctor_id          INTEGER NOT NULL,
    medication         TEXT    NOT NULL,
    dosage             TEXT    NOT NULL,
    frequency          TEXT    NOT NULL,
    duration_days      INTEGER NOT NULL DEFAULT 7,
    is_refillable      INTEGER NOT NULL DEFAULT 0,
    refills_remaining  INTEGER NOT NULL DEFAULT 0,
    notes              TEXT    NOT NULL DEFAULT '',
    created_at         TEXT    NOT NULL DEFAULT '1970-01-01 00:00:00',
    FOREIGN KEY (appointment_id) REFERENCES appointments (id) ON DELETE CASCADE,
    FOREIGN KEY (patient_id)     REFERENCES patients     (id) ON DELETE RESTRICT,
    FOREIGN KEY (doctor_id)      REFERENCES doctors      (id) ON DELETE RESTRICT
);

CREATE INDEX idx_prescriptions_appointment ON prescriptions (appointment_id);
CREATE INDEX idx_prescriptions_patient     ON prescriptions (patient_id);
