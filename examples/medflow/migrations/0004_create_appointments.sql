-- Appointments — the busy join table.
--
-- Indexed on `scheduled_at` (main calendar lookup) and on the two
-- FK pairs (patient + scheduled_at, doctor + scheduled_at) which cover
-- "all appointments for patient X" and "today's schedule for doctor Y".
PRAGMA foreign_keys = ON;

CREATE TABLE appointments (
    id                INTEGER PRIMARY KEY AUTOINCREMENT,
    patient_id        INTEGER NOT NULL,
    doctor_id         INTEGER NOT NULL,
    scheduled_at      TEXT    NOT NULL,
    status            TEXT    NOT NULL DEFAULT 'scheduled',
                      -- allow-list: scheduled / checked_in / in_progress /
                      --             completed / cancelled / no_show
    reason            TEXT    NOT NULL DEFAULT '',
    notes             TEXT    NOT NULL DEFAULT '',
    duration_minutes  INTEGER NOT NULL DEFAULT 30,
    priority          INTEGER NOT NULL DEFAULT 5,
    is_active         INTEGER NOT NULL DEFAULT 1,
    created_at        TEXT    NOT NULL DEFAULT '1970-01-01 00:00:00',
    FOREIGN KEY (patient_id) REFERENCES patients (id) ON DELETE RESTRICT,
    FOREIGN KEY (doctor_id)  REFERENCES doctors  (id) ON DELETE RESTRICT
);

CREATE INDEX idx_appointments_when           ON appointments (scheduled_at);
CREATE INDEX idx_appointments_patient_when   ON appointments (patient_id, scheduled_at);
CREATE INDEX idx_appointments_doctor_when    ON appointments (doctor_id, scheduled_at);
CREATE INDEX idx_appointments_status         ON appointments (status);
