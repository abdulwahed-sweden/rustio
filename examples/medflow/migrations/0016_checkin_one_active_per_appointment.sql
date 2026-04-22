-- One appointment may have at most one ACTIVE check-in at a time.
--
-- A check-in is "active" while its status is `waiting`, `in_room`, or
-- `with_doctor`. Once it reaches `done` or `left_without_seen`, a new
-- check-in for the same appointment is allowed (rebookings, re-arrivals).
--
-- Enforcement is a unique partial index — SQLite enforces INSERT and
-- UPDATE against it without any application-layer code.
PRAGMA foreign_keys = ON;

CREATE UNIQUE INDEX idx_check_ins_one_active_per_appointment
    ON check_ins (appointment_id)
    WHERE status IN ('waiting', 'in_room', 'with_doctor');
