-- category enum: 'hair' | 'skin' | 'nails' | 'lashes' (domain-defined)
CREATE TABLE services (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    category TEXT NOT NULL DEFAULT 'hair',
    duration_minutes INTEGER NOT NULL DEFAULT 30,
    price_cents INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX idx_services_category ON services(category);
