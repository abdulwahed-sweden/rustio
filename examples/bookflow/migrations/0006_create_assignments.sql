-- status enum: 'offered' | 'accepted' | 'declined'
CREATE TABLE assignments (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    booking_id INTEGER NOT NULL REFERENCES bookings(id) ON DELETE RESTRICT,
    resource_id INTEGER NOT NULL REFERENCES resources(id) ON DELETE RESTRICT,
    accepted_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    status TEXT NOT NULL DEFAULT 'offered'
);

CREATE INDEX idx_assignments_booking_id ON assignments(booking_id);
CREATE INDEX idx_assignments_resource_id ON assignments(resource_id);
CREATE INDEX idx_assignments_status ON assignments(status);
