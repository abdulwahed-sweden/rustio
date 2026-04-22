-- Canonicalise invoice.status into the three-value payment lifecycle:
--   pending → paid      (happy path)
--   pending → failed    (unrecoverable — card declined, chargeback,
--                        insurance denial)
--
-- Old allow-list (migration 0006) had draft/issued/paid/overdue/void;
-- we normalise onto the new set:
--   draft   → pending
--   issued  → pending
--   overdue → pending   (still owed, just late — business logic, not
--                        a new status)
--   void    → failed
--   paid    → paid
--
-- CHECK constraint added via the recreate-table pattern. Transition
-- validity (pending → paid, pending → failed) is enforced by
-- application code; SQLite CHECK only validates the value, not the
-- delta.
PRAGMA foreign_keys = ON;

UPDATE invoices SET status = 'pending' WHERE status IN ('draft', 'issued', 'overdue');
UPDATE invoices SET status = 'failed'  WHERE status = 'void';

CREATE TABLE invoices_new (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    invoice_number  TEXT    NOT NULL,
    patient_id      INTEGER NOT NULL,
    appointment_id  INTEGER,
    amount_cents    INTEGER NOT NULL DEFAULT 0,
    currency        TEXT    NOT NULL DEFAULT 'USD',
    status          TEXT    NOT NULL DEFAULT 'pending'
                    CHECK (status IN ('pending','paid','failed')),
    issued_at       TEXT    NOT NULL,
    paid_at         TEXT,
    notes           TEXT    NOT NULL DEFAULT '',
    created_at      TEXT    NOT NULL DEFAULT '1970-01-01 00:00:00',
    FOREIGN KEY (patient_id)     REFERENCES patients     (id) ON DELETE RESTRICT,
    FOREIGN KEY (appointment_id) REFERENCES appointments (id) ON DELETE SET NULL
);

INSERT INTO invoices_new (
    id, invoice_number, patient_id, appointment_id, amount_cents,
    currency, status, issued_at, paid_at, notes, created_at
)
SELECT
    id, invoice_number, patient_id, appointment_id, amount_cents,
    currency, status, issued_at, paid_at, notes, created_at
FROM invoices;

DROP TABLE invoices;
ALTER TABLE invoices_new RENAME TO invoices;

CREATE UNIQUE INDEX idx_invoices_number       ON invoices (invoice_number);
CREATE INDEX        idx_invoices_patient_stat ON invoices (patient_id, status);
CREATE INDEX        idx_invoices_issued_at    ON invoices (issued_at);
