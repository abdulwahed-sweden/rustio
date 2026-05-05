-- Freelance: projects table.
-- Mirrors the `Project` model in src/lib.rs. The
-- `client_id` column carries the FK that the model declares
-- via `#[rustio(references = "clients(id)")]`.
CREATE TABLE projects (
    id           BIGSERIAL PRIMARY KEY,
    name         TEXT NOT NULL,
    description  TEXT,
    client_id    BIGINT NOT NULL REFERENCES clients(id) ON DELETE CASCADE,
    budget_cents BIGINT,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX projects_client_idx ON projects (client_id);
