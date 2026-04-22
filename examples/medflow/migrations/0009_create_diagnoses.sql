-- Diagnoses — many per medical record. `code` is free-form so the
-- project can plug in ICD-10, ICD-11, SNOMED, or a local code set
-- without schema changes. One primary diagnosis per record by
-- convention; `is_primary` is enforced in the admin, not the DB.
--
-- `patient_id` is denormalised from the parent medical record so
-- "every diagnosis ever recorded for patient X" is a single-table
-- query without a join back through medical_records.
PRAGMA foreign_keys = ON;

CREATE TABLE diagnoses (
    id                 INTEGER PRIMARY KEY AUTOINCREMENT,
    medical_record_id  INTEGER NOT NULL,
    patient_id         INTEGER NOT NULL,
    code               TEXT    NOT NULL,
    description        TEXT    NOT NULL,
    severity           TEXT    NOT NULL DEFAULT 'moderate',
                       -- allow-list: mild / moderate / severe / critical
    is_primary         INTEGER NOT NULL DEFAULT 0,
    is_chronic         INTEGER NOT NULL DEFAULT 0,
    noted_at           TEXT    NOT NULL,
    notes              TEXT    NOT NULL DEFAULT '',
    created_at         TEXT    NOT NULL DEFAULT '1970-01-01 00:00:00',
    FOREIGN KEY (medical_record_id) REFERENCES medical_records (id) ON DELETE CASCADE,
    FOREIGN KEY (patient_id)        REFERENCES patients        (id) ON DELETE RESTRICT
);

CREATE INDEX idx_diagnoses_record   ON diagnoses (medical_record_id);
CREATE INDEX idx_diagnoses_patient  ON diagnoses (patient_id, noted_at);
CREATE INDEX idx_diagnoses_code     ON diagnoses (code);
