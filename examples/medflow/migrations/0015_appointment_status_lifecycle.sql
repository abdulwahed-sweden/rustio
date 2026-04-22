-- Tighten appointment.status into a proper lifecycle:
--   scheduled → confirmed → in_progress → completed
--   any status may transition to cancelled
--
-- Old allow-list (migration 0004) included `checked_in` and `no_show`;
-- we normalise both onto the new lifecycle:
--   checked_in → confirmed   (the patient has checked in, so the
--                             appointment is confirmed from the
--                             scheduling system's perspective)
--   no_show    → cancelled   (a no-show is the clinical equivalent
--                             of a cancellation from the workflow side)
--
-- Enforcement is via a CHECK constraint on the column; state-transition
-- validity is enforced in the Rust model layer (see
-- `Appointment::can_transition_to` in apps/care/models.rs), called by
-- any code that mutates `status`. SQLite triggers would be more robust
-- but the migrations driver's statement splitter does not understand
-- `BEGIN … END` trigger bodies.
PRAGMA foreign_keys = ON;

UPDATE appointments SET status = 'confirmed' WHERE status = 'checked_in';
UPDATE appointments SET status = 'cancelled' WHERE status = 'no_show';

CREATE TABLE appointments_new (
    id                INTEGER PRIMARY KEY AUTOINCREMENT,
    patient_id        INTEGER NOT NULL,
    doctor_id         INTEGER NOT NULL,
    department_id     INTEGER,
    scheduled_at      TEXT    NOT NULL,
    status            TEXT    NOT NULL DEFAULT 'scheduled'
                      CHECK (status IN ('scheduled','confirmed','in_progress','completed','cancelled')),
    reason            TEXT    NOT NULL DEFAULT '',
    notes             TEXT    NOT NULL DEFAULT '',
    duration_minutes  INTEGER NOT NULL DEFAULT 30,
    priority          INTEGER NOT NULL DEFAULT 5,
    is_active         INTEGER NOT NULL DEFAULT 1,
    created_at        TEXT    NOT NULL DEFAULT '1970-01-01 00:00:00',
    FOREIGN KEY (patient_id)    REFERENCES patients    (id) ON DELETE RESTRICT,
    FOREIGN KEY (doctor_id)     REFERENCES doctors     (id) ON DELETE RESTRICT,
    FOREIGN KEY (department_id) REFERENCES departments (id) ON DELETE SET NULL
);

INSERT INTO appointments_new (
    id, patient_id, doctor_id, department_id, scheduled_at, status,
    reason, notes, duration_minutes, priority, is_active, created_at
)
SELECT
    id, patient_id, doctor_id, department_id, scheduled_at, status,
    reason, notes, duration_minutes, priority, is_active, created_at
FROM appointments;

DROP TABLE appointments;
ALTER TABLE appointments_new RENAME TO appointments;

CREATE INDEX idx_appointments_when         ON appointments (scheduled_at);
CREATE INDEX idx_appointments_patient_when ON appointments (patient_id, scheduled_at);
CREATE INDEX idx_appointments_doctor_when  ON appointments (doctor_id, scheduled_at);
CREATE INDEX idx_appointments_status       ON appointments (status);
CREATE INDEX idx_appointments_department   ON appointments (department_id);
