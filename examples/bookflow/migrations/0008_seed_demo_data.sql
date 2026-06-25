-- Minimal demo data so the admin + list pages aren't empty on first run.
-- Inserted in FK-dependency order. Generic values: the same rows read as
-- container logistics, equipment rental, or appointments depending on how
-- you reshape the view.

INSERT INTO locations (name, region_code, address, active) VALUES
    ('North Depot', 'SE-STO', '1 Harbor Road, Stockholm', 1),
    ('West Hub',    'US-WEST', '200 Bay Street, Oakland',  1);

INSERT INTO customers (name, customer_type, email, phone, address, created_at) VALUES
    ('Nordic Freight AB', 'business',   'ops@nordicfreight.example',   '+46 8 123 456', 'Stockholm', '2026-01-10T08:00:00Z'),
    ('Dana Olsen',        'individual', 'dana.olsen@example.com',      '+1 510 555 0142', 'Oakland',  '2026-02-02T13:30:00Z');

INSERT INTO resources (name, resource_type, code, location_id, rate_cents, active, created_at) VALUES
    ('Container 40ft #A12', 'container', 'CON-A12', 1, 12000, 1, '2026-01-11T09:00:00Z'),
    ('Cargo Van 02',        'vehicle',   'VAN-02',  2,  8000, 1, '2026-01-12T09:00:00Z');

INSERT INTO bookings
    (booking_number, customer_id, resource_id, service_type, scheduled_at, duration_minutes, status, assignee_id, notes, created_at)
VALUES
    ('BK-1001', 1, 1, 'delivery', '2026-06-25T14:30:00Z', 120, 'assigned', 2, 'Dock 3 pickup',        '2026-06-20T10:00:00Z'),
    ('BK-1002', 2, 2, 'rental',   '2026-07-02T09:00:00Z',  90, 'new',      NULL, 'Half-day rental',    '2026-06-21T16:45:00Z');

INSERT INTO schedules (resource_id, weekday, start_time, end_time, mode) VALUES
    (1, 'mon', '08:00', '16:00', 'available'),
    (2, 'tue', '09:00', '17:00', 'available');

INSERT INTO assignments (booking_id, resource_id, accepted_at, status) VALUES
    (1, 2, '2026-06-20T11:15:00Z', 'accepted');

INSERT INTO invoices (invoice_number, customer_id, amount_cents, status, issued_at) VALUES
    ('INV-5001', 1, 24000, 'sent',  '2026-06-26T09:00:00Z'),
    ('INV-5002', 2, 12000, 'draft', '2026-07-03T09:00:00Z');
