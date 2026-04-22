-- Add `department_id` to appointments so the scheduler can filter by
-- department directly, without going through the appointment's doctor.
-- Nullable: existing rows predate the column, and walk-in / triage
-- appointments can legitimately have no department yet.
PRAGMA foreign_keys = ON;

ALTER TABLE appointments ADD COLUMN department_id INTEGER REFERENCES departments (id) ON DELETE SET NULL;

CREATE INDEX idx_appointments_department ON appointments (department_id);
