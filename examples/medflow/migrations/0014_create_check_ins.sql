-- Check-ins — the front-desk event tying an arriving patient to
-- their appointment, the staff member who handled the check-in,
-- and the room they were sent to.
--
-- `staff_id` and `room_id` are nullable so a check-in can be
-- recorded before a staff member picks it up or before a room is
-- assigned (triage / waiting). `status` is an allow-list:
-- waiting / in_room / with_doctor / done / left_without_seen.
PRAGMA foreign_keys = ON;

CREATE TABLE check_ins (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    appointment_id  INTEGER NOT NULL,
    patient_id      INTEGER NOT NULL,
    staff_id        INTEGER,
    room_id         INTEGER,
    checked_in_at   TEXT    NOT NULL,
    status          TEXT    NOT NULL DEFAULT 'waiting',
                    -- allow-list: waiting / in_room / with_doctor / done / left_without_seen
    priority        INTEGER NOT NULL DEFAULT 5,
    notes           TEXT    NOT NULL DEFAULT '',
    created_at      TEXT    NOT NULL DEFAULT '1970-01-01 00:00:00',
    FOREIGN KEY (appointment_id) REFERENCES appointments  (id) ON DELETE CASCADE,
    FOREIGN KEY (patient_id)     REFERENCES patients      (id) ON DELETE RESTRICT,
    FOREIGN KEY (staff_id)       REFERENCES staff_members (id) ON DELETE SET NULL,
    FOREIGN KEY (room_id)        REFERENCES rooms         (id) ON DELETE SET NULL
);

CREATE INDEX idx_check_ins_appointment ON check_ins (appointment_id);
CREATE INDEX idx_check_ins_patient     ON check_ins (patient_id, checked_in_at);
CREATE INDEX idx_check_ins_status      ON check_ins (status, checked_in_at);
CREATE INDEX idx_check_ins_room        ON check_ins (room_id);
