-- Freelance: clients table.
-- Mirrors the `Client` model in src/lib.rs. Every column listed
-- here matches a `#[rustio(sql = "...")]` declaration; the
-- validator (rustio doctor --check-schema) will refuse to enable
-- search for any drift between this DDL and the model.
CREATE TABLE clients (
    id         BIGSERIAL PRIMARY KEY,
    name       TEXT NOT NULL,
    email      TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX clients_email_idx ON clients (email);
