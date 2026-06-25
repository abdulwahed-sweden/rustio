-- weekday enum: 'mon'..'sun'; mode enum: domain-defined availability mode.
-- start_time / end_time stored as "HH:MM" text (no first-class time type).
CREATE TABLE schedules (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    resource_id INTEGER NOT NULL REFERENCES resources(id) ON DELETE RESTRICT,
    weekday TEXT NOT NULL DEFAULT 'mon',
    start_time TEXT NOT NULL DEFAULT '09:00',
    end_time TEXT NOT NULL DEFAULT '17:00',
    mode TEXT NOT NULL DEFAULT 'available'
);

CREATE INDEX idx_schedules_resource_id ON schedules(resource_id);
