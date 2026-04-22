-- Staff members — non-doctor personnel. Doctors live in their own
-- table under the `people` app.
--
-- `role` is an allow-list: nurse / receptionist / technician /
-- cleaner / security / admin. `department_id` is nullable so
-- hospital-wide roles (night security, central admin) aren't
-- forced to pick one.
PRAGMA foreign_keys = ON;

CREATE TABLE staff_members (
    id             INTEGER PRIMARY KEY AUTOINCREMENT,
    full_name      TEXT    NOT NULL,
    role           TEXT    NOT NULL DEFAULT 'nurse',
                   -- allow-list: nurse / receptionist / technician / cleaner / security / admin
    department_id  INTEGER,
    email          TEXT    NOT NULL,
    phone          TEXT    NOT NULL DEFAULT '',
    is_active      INTEGER NOT NULL DEFAULT 1,
    hired_at       TEXT    NOT NULL,
    created_at     TEXT    NOT NULL DEFAULT '1970-01-01 00:00:00',
    FOREIGN KEY (department_id) REFERENCES departments (id) ON DELETE SET NULL
);

CREATE UNIQUE INDEX idx_staff_members_email      ON staff_members (email);
CREATE INDEX        idx_staff_members_role       ON staff_members (role, is_active);
CREATE INDEX        idx_staff_members_department ON staff_members (department_id);
