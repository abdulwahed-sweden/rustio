-- Swedish demo data — a fuller, realistic dataset on top of 0008 so the admin
-- shows lifelike Swedish content (names, cities, addresses, kronor amounts).
-- Foreign keys are resolved by name/code subqueries, so this is independent of
-- auto-increment ordering and safe to apply after 0008 on a fresh or live DB.

-- Locations across Sweden (region_code = ISO 3166-2 SE county).
INSERT INTO locations (name, region_code, address, active) VALUES
    ('Stockholm Frihamnen', 'SE-AB', 'Frihamnsgatan 8, 115 56 Stockholm', 1),
    ('Göteborg Hamn',       'SE-O',  'Hamngatan 2, 411 06 Göteborg',      1),
    ('Malmö Central',       'SE-M',  'Centralplan 5, 211 20 Malmö',       1),
    ('Uppsala Depå',        'SE-C',  'Kungsgatan 12, 753 21 Uppsala',     1);

-- Customers — Swedish businesses (AB) and individuals.
INSERT INTO customers (name, customer_type, email, phone, address, created_at) VALUES
    ('Svensk Logistik AB',          'business',   'order@svensklogistik.se',    '+46 8 555 0102',  'Sveavägen 44, 111 34 Stockholm',        '2026-01-12T08:15:00Z'),
    ('Göteborgs Transport AB',      'business',   'info@gbgtransport.se',       '+46 31 555 0144', 'Avenyn 21, 411 36 Göteborg',            '2026-01-20T10:00:00Z'),
    ('Nordic Bygg & Anläggning AB', 'business',   'kontakt@nordicbygg.se',      '+46 40 555 0188', 'Storgatan 7, 211 42 Malmö',             '2026-02-05T09:30:00Z'),
    ('Erik Lindqvist',              'individual', 'erik.lindqvist@example.se',  '+46 70 123 4567', 'Vasagatan 10, 753 30 Uppsala',          '2026-02-18T14:20:00Z'),
    ('Anna Bergström',              'individual', 'anna.bergstrom@example.se',  '+46 73 234 5678', 'Drottninggatan 5, 411 14 Göteborg',     '2026-03-03T11:05:00Z'),
    ('Johan Nilsson',               'individual', 'johan.nilsson@example.se',   '+46 76 345 6789', 'Kungsholmsgatan 3, 112 27 Stockholm',   '2026-03-22T16:40:00Z'),
    ('Karin Andersson',             'individual', 'karin.andersson@example.se', '+46 70 456 7890', 'Söder Mälarstrand 27, 118 25 Stockholm','2026-04-09T09:00:00Z');

-- Resources — containers and vehicles, parked at the Swedish locations.
INSERT INTO resources (name, resource_type, code, location_id, rate_cents, active, created_at) VALUES
    ('Container 40ft #S22',        'container', 'CON-S22',   (SELECT id FROM locations WHERE name='Stockholm Frihamnen'), 13500, 1, '2026-01-13T09:00:00Z'),
    ('Container 20ft #S23',        'container', 'CON-S23',   (SELECT id FROM locations WHERE name='Göteborg Hamn'),       9500,  1, '2026-01-21T09:00:00Z'),
    ('Lastbil Volvo FH16',         'vehicle',   'LB-VFH-01', (SELECT id FROM locations WHERE name='Malmö Central'),       18000, 1, '2026-02-06T09:00:00Z'),
    ('Skåpbil Mercedes Sprinter',  'vehicle',   'SKB-MS-02', (SELECT id FROM locations WHERE name='Stockholm Frihamnen'), 9000,  1, '2026-02-10T09:00:00Z'),
    ('Lastbil Scania R450',        'vehicle',   'LB-SCR-03', (SELECT id FROM locations WHERE name='Uppsala Depå'),        17000, 1, '2026-03-01T09:00:00Z');

-- Bookings — a spread of statuses and Swedish service notes.
INSERT INTO bookings
    (booking_number, customer_id, resource_id, service_type, scheduled_at, duration_minutes, status, assignee_id, notes, created_at)
VALUES
    ('BK-2001', (SELECT id FROM customers WHERE name='Svensk Logistik AB'),          (SELECT id FROM resources WHERE code='CON-S22'),   'delivery',  '2026-06-26T08:00:00Z', 180, 'assigned',  (SELECT id FROM resources WHERE code='LB-VFH-01'), 'Lastning vid kaj 4',           '2026-06-18T10:00:00Z'),
    ('BK-2002', (SELECT id FROM customers WHERE name='Göteborgs Transport AB'),      (SELECT id FROM resources WHERE code='LB-VFH-01'), 'transport', '2026-06-27T09:30:00Z', 240, 'new',       NULL,                                              'Transport Göteborg–Malmö',     '2026-06-19T16:45:00Z'),
    ('BK-2003', (SELECT id FROM customers WHERE name='Erik Lindqvist'),              (SELECT id FROM resources WHERE code='SKB-MS-02'), 'rental',    '2026-06-28T07:00:00Z', 480, 'assigned',  (SELECT id FROM resources WHERE code='SKB-MS-02'), 'Flytthjälp i Uppsala',         '2026-06-21T08:30:00Z'),
    ('BK-2004', (SELECT id FROM customers WHERE name='Anna Bergström'),              (SELECT id FROM resources WHERE code='CON-S23'),   'rental',    '2026-07-01T10:00:00Z', 120, 'completed', NULL,                                              'Förvaring i en vecka',         '2026-06-22T13:15:00Z'),
    ('BK-2005', (SELECT id FROM customers WHERE name='Nordic Bygg & Anläggning AB'), (SELECT id FROM resources WHERE code='LB-SCR-03'), 'delivery',  '2026-07-03T06:30:00Z', 300, 'new',       NULL,                                              'Leverans av byggmaterial',     '2026-06-24T09:50:00Z'),
    ('BK-2006', (SELECT id FROM customers WHERE name='Johan Nilsson'),               (SELECT id FROM resources WHERE code='SKB-MS-02'), 'rental',    '2026-07-05T12:00:00Z', 180, 'cancelled', NULL,                                              'Avbokad av kund',              '2026-06-25T18:05:00Z');

-- Schedules — weekly availability per resource.
INSERT INTO schedules (resource_id, weekday, start_time, end_time, mode) VALUES
    ((SELECT id FROM resources WHERE code='CON-S22'),   'mon', '07:00', '19:00', 'available'),
    ((SELECT id FROM resources WHERE code='LB-VFH-01'), 'tue', '06:00', '15:00', 'available'),
    ((SELECT id FROM resources WHERE code='SKB-MS-02'), 'wed', '08:00', '17:00', 'available'),
    ((SELECT id FROM resources WHERE code='LB-SCR-03'), 'thu', '06:30', '16:30', 'available'),
    ((SELECT id FROM resources WHERE code='CON-S23'),   'fri', '08:00', '16:00', 'maintenance');

-- Assignments — which resource accepted which booking.
INSERT INTO assignments (booking_id, resource_id, accepted_at, status) VALUES
    ((SELECT id FROM bookings WHERE booking_number='BK-2001'), (SELECT id FROM resources WHERE code='LB-VFH-01'), '2026-06-18T11:15:00Z', 'accepted'),
    ((SELECT id FROM bookings WHERE booking_number='BK-2003'), (SELECT id FROM resources WHERE code='SKB-MS-02'), '2026-06-21T09:10:00Z', 'accepted'),
    ((SELECT id FROM bookings WHERE booking_number='BK-2002'), (SELECT id FROM resources WHERE code='LB-VFH-01'), '2026-06-19T17:00:00Z', 'offered');

-- Invoices — amounts in öre (minor units); a spread of statuses.
INSERT INTO invoices (invoice_number, customer_id, amount_cents, status, issued_at) VALUES
    ('INV-6001', (SELECT id FROM customers WHERE name='Svensk Logistik AB'),          40500, 'sent',    '2026-06-26T09:00:00Z'),
    ('INV-6002', (SELECT id FROM customers WHERE name='Göteborgs Transport AB'),      72000, 'draft',   '2026-06-27T09:00:00Z'),
    ('INV-6003', (SELECT id FROM customers WHERE name='Erik Lindqvist'),              36000, 'paid',    '2026-06-22T09:00:00Z'),
    ('INV-6004', (SELECT id FROM customers WHERE name='Anna Bergström'),              19000, 'paid',    '2026-07-01T09:00:00Z'),
    ('INV-6005', (SELECT id FROM customers WHERE name='Nordic Bygg & Anläggning AB'), 85000, 'overdue', '2026-06-15T09:00:00Z');
