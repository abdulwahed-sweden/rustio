-- Prescriptions now also point at the MedicalRecord produced during
-- the visit. The existing `appointment_id` FK is preserved for
-- backward compatibility and for prescriptions issued without a
-- corresponding record on file (phone refills, simple renewals).
--
-- `medical_record_id` is nullable so legacy prescriptions written
-- before the MedicalRecord workflow existed stay valid. New
-- prescriptions issued through the clinical flow should set it.
PRAGMA foreign_keys = ON;

ALTER TABLE prescriptions
    ADD COLUMN medical_record_id INTEGER REFERENCES medical_records (id) ON DELETE SET NULL;

CREATE INDEX idx_prescriptions_record ON prescriptions (medical_record_id);
