-- Payments — one row per payment received against an invoice.
--
-- Partial payments, insurance + patient splits, and refunds
-- (negative `amount_cents`) all live in this table. Paid totals are
-- computed at read time; we never denormalise onto the invoice row.
--
-- `reference` is the external identifier for the payment: card auth
-- code, bank transfer reference, insurance claim number.
PRAGMA foreign_keys = ON;

CREATE TABLE payments (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    invoice_id    INTEGER NOT NULL,
    patient_id    INTEGER NOT NULL,
    amount_cents  INTEGER NOT NULL,
    currency      TEXT    NOT NULL DEFAULT 'USD',  -- USD / EUR / SAR / AED
    method        TEXT    NOT NULL DEFAULT 'card',
                  -- allow-list: cash / card / transfer / insurance / other
    reference     TEXT    NOT NULL DEFAULT '',
    received_at   TEXT    NOT NULL,
    notes         TEXT    NOT NULL DEFAULT '',
    created_at    TEXT    NOT NULL DEFAULT '1970-01-01 00:00:00',
    FOREIGN KEY (invoice_id) REFERENCES invoices (id) ON DELETE CASCADE,
    FOREIGN KEY (patient_id) REFERENCES patients (id) ON DELETE RESTRICT
);

CREATE INDEX idx_payments_invoice       ON payments (invoice_id);
CREATE INDEX idx_payments_patient       ON payments (patient_id, received_at);
CREATE INDEX idx_payments_received_at   ON payments (received_at);
