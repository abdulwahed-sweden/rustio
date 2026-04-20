-- Departments — reference data for doctors.
--
-- `head_doctor_id` is nullable to break the circular FK between
-- departments and doctors: a department may not have a head yet, and
-- the doctor who eventually becomes head does not exist at the moment
-- the department row is created.
PRAGMA foreign_keys = ON;

CREATE TABLE departments (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    name            TEXT NOT NULL,
    code            TEXT NOT NULL,
    is_active       INTEGER NOT NULL DEFAULT 1,
    head_doctor_id  INTEGER,
    created_at      TEXT NOT NULL DEFAULT '1970-01-01 00:00:00'
);

CREATE UNIQUE INDEX idx_departments_code ON departments (code);
CREATE INDEX        idx_departments_active ON departments (is_active);
