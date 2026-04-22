-- Appointment event log — append-only audit of every status
-- transition executed through `Appointment::transition_to`.
--
-- The log row is written in the SAME transaction as the UPDATE on
-- `appointments.status`. If the two ever diverge (an appointment's
-- status disagrees with the last event row for it), someone
-- bypassed `transition_to` with a direct assignment — that's a bug
-- and should be surfaced by a reconciliation check, not silently
-- healed here.
--
-- The table has no UNIQUE index on `(appointment_id, to_status)`
-- because the lifecycle permits self-edges to `cancelled` from any
-- state, and re-entering `scheduled` after a cancellation-and-new-
-- booking cycle is expected for rebookings.
PRAGMA foreign_keys = ON;

CREATE TABLE appointment_events (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    appointment_id  INTEGER NOT NULL,
    from_status     TEXT    NOT NULL,
    to_status       TEXT    NOT NULL,
    created_at      TEXT    NOT NULL DEFAULT '1970-01-01 00:00:00',
    FOREIGN KEY (appointment_id) REFERENCES appointments (id) ON DELETE CASCADE
);

CREATE INDEX idx_appointment_events_appointment ON appointment_events (appointment_id, created_at);
CREATE INDEX idx_appointment_events_created_at  ON appointment_events (created_at);
