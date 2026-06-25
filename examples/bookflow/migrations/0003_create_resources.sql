-- resource_type enum: 'container' | 'room' | 'vehicle' | 'person' (domain-defined)
CREATE TABLE resources (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    resource_type TEXT NOT NULL DEFAULT 'container',
    code TEXT NOT NULL DEFAULT '',
    location_id INTEGER NOT NULL REFERENCES locations(id) ON DELETE RESTRICT,
    rate_cents INTEGER NOT NULL DEFAULT 0,
    active INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX idx_resources_location_id ON resources(location_id);
CREATE INDEX idx_resources_resource_type ON resources(resource_type);
