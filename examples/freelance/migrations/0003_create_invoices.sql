-- Freelance: invoices table.
-- Mirrors the `Invoice` model in src/lib.rs. Money is stored
-- as `BIGINT` cents per Type Rule #3 (the alternative is
-- `NUMERIC(12,2)` paired with `rust_decimal::Decimal` on the
-- Rust side; this example uses cents to avoid the extra crate
-- dependency).
CREATE TABLE invoices (
    id           BIGSERIAL PRIMARY KEY,
    project_id   BIGINT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    amount_cents BIGINT NOT NULL,
    paid         BOOLEAN NOT NULL DEFAULT FALSE,
    issued_at    TIMESTAMPTZ NOT NULL,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX invoices_project_idx ON invoices (project_id);
CREATE INDEX invoices_paid_issued_idx ON invoices (paid, issued_at DESC);
