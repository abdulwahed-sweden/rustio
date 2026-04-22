-- Medical records — one clinical note per encounter.
--
-- `appointment_id` is nullable: retroactively imported records and
-- standalone notes (phone consults, chart corrections) don't always
-- tie to a scheduled appointment. `is_confidential` flags records
-- that should not be shared cross-department (HR, occupational health).
PRAGMA foreign_keys = ON;

CREATE TABLE medical_records (
    id               INTEGER PRIMARY KEY AUTOINCREMENT,
    patient_id       INTEGER NOT NULL,
    appointment_id   INTEGER,
    doctor_id        INTEGER NOT NULL,
    summary          TEXT    NOT NULL,
    chief_complaint  TEXT    NOT NULL DEFAULT '',
    assessment       TEXT    NOT NULL DEFAULT '',
    plan             TEXT    NOT NULL DEFAULT '',
    is_confidential  INTEGER NOT NULL DEFAULT 0,
    recorded_at      TEXT    NOT NULL,
    created_at       TEXT    NOT NULL DEFAULT '1970-01-01 00:00:00',
    FOREIGN KEY (patient_id)     REFERENCES patients     (id) ON DELETE RESTRICT,
    FOREIGN KEY (appointment_id) REFERENCES appointments (id) ON DELETE SET NULL,
    FOREIGN KEY (doctor_id)      REFERENCES doctors      (id) ON DELETE RESTRICT
);

CREATE INDEX idx_medical_records_patient     ON medical_records (patient_id, recorded_at);
CREATE INDEX idx_medical_records_appointment ON medical_records (appointment_id);
CREATE INDEX idx_medical_records_doctor      ON medical_records (doctor_id, recorded_at);
