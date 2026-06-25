-- status enum: 'new' | 'assigned' | 'completed' | 'cancelled'
-- service_type enum: domain-defined service category
CREATE TABLE bookings (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    booking_number TEXT NOT NULL,
    customer_id INTEGER NOT NULL REFERENCES customers(id) ON DELETE RESTRICT,
    resource_id INTEGER NOT NULL REFERENCES resources(id) ON DELETE RESTRICT,
    service_type TEXT NOT NULL DEFAULT 'standard',
    scheduled_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    duration_minutes INTEGER NOT NULL DEFAULT 60,
    status TEXT NOT NULL DEFAULT 'new',
    assignee_id INTEGER REFERENCES resources(id) ON DELETE SET NULL,
    notes TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX idx_bookings_customer_id ON bookings(customer_id);
CREATE INDEX idx_bookings_resource_id ON bookings(resource_id);
CREATE INDEX idx_bookings_assignee_id ON bookings(assignee_id);
CREATE INDEX idx_bookings_status ON bookings(status);
