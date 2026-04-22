-- Rooms — physical rooms in the hospital. `room_type` is an
-- allow-list: `exam`, `surgery`, `ward`, `icu`, `waiting`, `office`.
-- `is_available` is the scheduler's quick filter for assignment;
-- detailed occupancy lives in `check_ins`.
PRAGMA foreign_keys = ON;

CREATE TABLE rooms (
    id             INTEGER PRIMARY KEY AUTOINCREMENT,
    room_number    TEXT    NOT NULL,
    floor          INTEGER NOT NULL DEFAULT 0,
    department_id  INTEGER,
    room_type      TEXT    NOT NULL DEFAULT 'exam',
                   -- allow-list: exam / surgery / ward / icu / waiting / office
    capacity       INTEGER NOT NULL DEFAULT 1,
    is_available   INTEGER NOT NULL DEFAULT 1,
    notes          TEXT    NOT NULL DEFAULT '',
    created_at     TEXT    NOT NULL DEFAULT '1970-01-01 00:00:00',
    FOREIGN KEY (department_id) REFERENCES departments (id) ON DELETE SET NULL
);

CREATE UNIQUE INDEX idx_rooms_number     ON rooms (room_number);
CREATE INDEX        idx_rooms_department ON rooms (department_id);
CREATE INDEX        idx_rooms_type_avail ON rooms (room_type, is_available);
