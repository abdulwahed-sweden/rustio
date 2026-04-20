-- Invoices — patient billing. The link to an appointment is optional
-- because standalone invoices (membership fees, lab packages) exist.
--
-- Money is stored in cents as i64 because RustIO has no Decimal type.
-- Currency is a String allow-list.
PRAGMA foreign_keys = ON;

CREATE TABLE invoices (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    invoice_number  TEXT    NOT NULL,
    patient_id      INTEGER NOT NULL,
    appointment_id  INTEGER,
    amount_cents    INTEGER NOT NULL DEFAULT 0,
    currency        TEXT    NOT NULL DEFAULT 'USD',  -- USD / EUR / SAR / AED
    status          TEXT    NOT NULL DEFAULT 'draft',
                    -- allow-list: draft / issued / paid / overdue / void
    issued_at       TEXT    NOT NULL,
    paid_at         TEXT,
    notes           TEXT    NOT NULL DEFAULT '',
    created_at      TEXT    NOT NULL DEFAULT '1970-01-01 00:00:00',
    FOREIGN KEY (patient_id)     REFERENCES patients     (id) ON DELETE RESTRICT,
    FOREIGN KEY (appointment_id) REFERENCES appointments (id) ON DELETE SET NULL
);

CREATE UNIQUE INDEX idx_invoices_number        ON invoices (invoice_number);
CREATE INDEX        idx_invoices_patient_stat  ON invoices (patient_id, status);
CREATE INDEX        idx_invoices_issued_at     ON invoices (issued_at);
