-- Vital signs — point-in-time measurements captured during a visit.
--
-- `temperature_c` and `weight_kg` are TEXT because RustIO has no
-- Float type; store formatted as "36.8" / "72.3". Integer measurements
-- (heart rate, BP, O2 saturation, height) stay as INTEGER.
PRAGMA foreign_keys = ON;

CREATE TABLE vital_signs (
    id                 INTEGER PRIMARY KEY AUTOINCREMENT,
    medical_record_id  INTEGER NOT NULL,
    patient_id         INTEGER NOT NULL,
    heart_rate_bpm     INTEGER NOT NULL DEFAULT 0,
    systolic_bp        INTEGER NOT NULL DEFAULT 0,
    diastolic_bp       INTEGER NOT NULL DEFAULT 0,
    temperature_c      TEXT    NOT NULL DEFAULT '0.0',
    oxygen_saturation  INTEGER NOT NULL DEFAULT 0,
    weight_kg          TEXT    NOT NULL DEFAULT '0.0',
    height_cm          INTEGER NOT NULL DEFAULT 0,
    recorded_at        TEXT    NOT NULL,
    notes              TEXT    NOT NULL DEFAULT '',
    created_at         TEXT    NOT NULL DEFAULT '1970-01-01 00:00:00',
    FOREIGN KEY (medical_record_id) REFERENCES medical_records (id) ON DELETE CASCADE,
    FOREIGN KEY (patient_id)        REFERENCES patients        (id) ON DELETE RESTRICT
);

CREATE INDEX idx_vital_signs_record   ON vital_signs (medical_record_id);
CREATE INDEX idx_vital_signs_patient  ON vital_signs (patient_id, recorded_at);
