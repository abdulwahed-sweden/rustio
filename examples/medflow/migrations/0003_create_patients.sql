-- Patients — the core entity that appointments and invoices point at.
--
-- `national_id` and `email` are unique. `allergies` is TEXT (possibly
-- empty) rather than nullable — RustIO does not currently surface a
-- NULL/empty distinction in the admin for String fields.
PRAGMA foreign_keys = ON;

CREATE TABLE patients (
    id             INTEGER PRIMARY KEY AUTOINCREMENT,
    full_name      TEXT    NOT NULL,
    date_of_birth  TEXT    NOT NULL,
    gender         TEXT    NOT NULL,   -- allow-list: male / female / other
    national_id    TEXT    NOT NULL,
    phone          TEXT    NOT NULL,
    email          TEXT    NOT NULL,
    blood_type     TEXT    NOT NULL,   -- allow-list: A+ A- B+ B- AB+ AB- O+ O-
    allergies      TEXT    NOT NULL DEFAULT '',
    is_active      INTEGER NOT NULL DEFAULT 1,
    created_at     TEXT    NOT NULL DEFAULT '1970-01-01 00:00:00'
);

CREATE UNIQUE INDEX idx_patients_national_id ON patients (national_id);
CREATE UNIQUE INDEX idx_patients_email       ON patients (email);
CREATE INDEX        idx_patients_active      ON patients (is_active);
