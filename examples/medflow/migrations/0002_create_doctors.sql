-- Doctors — each belongs to a department.
--
-- `ON DELETE RESTRICT` on the department FK means a department with
-- doctors attached cannot be silently removed.
PRAGMA foreign_keys = ON;

CREATE TABLE doctors (
    id                INTEGER PRIMARY KEY AUTOINCREMENT,
    full_name         TEXT    NOT NULL,
    specialty         TEXT    NOT NULL,
    department_id     INTEGER NOT NULL,
    license_no        TEXT    NOT NULL,
    email             TEXT    NOT NULL,
    phone             TEXT    NOT NULL,
    years_experience  INTEGER NOT NULL DEFAULT 0,
    is_active         INTEGER NOT NULL DEFAULT 1,
    created_at        TEXT    NOT NULL DEFAULT '1970-01-01 00:00:00',
    FOREIGN KEY (department_id) REFERENCES departments (id) ON DELETE RESTRICT
);

CREATE UNIQUE INDEX idx_doctors_license ON doctors (license_no);
CREATE UNIQUE INDEX idx_doctors_email   ON doctors (email);
CREATE INDEX        idx_doctors_dept    ON doctors (department_id, is_active);
