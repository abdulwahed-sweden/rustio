-- Realistic Swedish demo data. Backend values stay English (the iron rule);
-- the data (names, items) is lifelike so lists, pills, filters, and relations
-- show something. FKs resolve by name subquery — order-independent.

INSERT INTO clients (name, phone, email, notes, joined_at) VALUES
    ('Anna Bergström',    '+46 70 111 2233', 'anna.bergstrom@example.se',   'Allergic to fragrance.',        '2025-09-12T10:00:00Z'),
    ('Johan Lindqvist',   '+46 73 222 3344', 'johan.lindqvist@example.se',  'Prefers morning slots.',        '2025-10-03T09:30:00Z'),
    ('Sara Nilsson',      '+46 76 333 4455', 'sara.nilsson@example.se',     '',                              '2025-10-21T14:15:00Z'),
    ('Erik Andersson',    '+46 70 444 5566', 'erik.andersson@example.se',   'VIP — regular colour client.',  '2025-11-08T11:00:00Z'),
    ('Maria Karlsson',    '+46 73 555 6677', 'maria.karlsson@example.se',   '',                              '2025-11-25T16:45:00Z'),
    ('Olof Persson',      '+46 76 666 7788', 'olof.persson@example.se',     'Beard trim every 3 weeks.',     '2025-12-10T08:50:00Z'),
    ('Linnea Johansson',  '+46 70 777 8899', 'linnea.johansson@example.se', '',                              '2026-01-14T13:20:00Z'),
    ('Karin Svensson',    '+46 73 888 9900', 'karin.svensson@example.se',   'Sensitive skin.',               '2026-02-02T10:10:00Z'),
    ('Per Eriksson',      '+46 76 999 0011', 'per.eriksson@example.se',     '',                              '2026-02-19T15:30:00Z'),
    ('Emma Larsson',      '+46 70 121 3141', 'emma.larsson@example.se',     'Bridal package interest.',      '2026-03-09T09:00:00Z'),
    ('Lars Olsson',       '+46 73 151 6171', 'lars.olsson@example.se',      '',                              '2026-04-01T12:40:00Z'),
    ('Sofia Gustafsson',  '+46 76 181 9202', 'sofia.gustafsson@example.se', 'Lash refill every 4 weeks.',    '2026-05-16T11:25:00Z');

INSERT INTO services (name, category, duration_minutes, price_cents) VALUES
    ('Haircut & Style',     'hair',   45,  55000),
    ('Hair Coloring',       'hair',   90, 120000),
    ('Beard Trim',          'hair',   20,  25000),
    ('Classic Facial',      'skin',   60,  80000),
    ('Deep Cleanse Facial', 'skin',   75,  95000),
    ('Gel Manicure',        'nails',  45,  45000),
    ('Spa Pedicure',        'nails',  50,  50000),
    ('Lash Extensions',     'lashes', 120, 140000);

INSERT INTO staff (name, specialty, phone, is_active) VALUES
    ('Frida Holm',     'Hair Stylist',    '+46 70 900 0001', 1),
    ('Sven Ek',        'Barber',          '+46 70 900 0002', 1),
    ('Nadia Aziz',     'Skin Therapist',  '+46 70 900 0003', 1),
    ('Camilla Berg',   'Nail Technician', '+46 70 900 0004', 1),
    ('Tobias Falk',    'Lash Artist',     '+46 70 900 0005', 0);

INSERT INTO appointments (client_id, service_id, staff_id, scheduled_at, status, notes) VALUES
    -- past (completed)
    ((SELECT id FROM clients WHERE name='Anna Bergström'),   (SELECT id FROM services WHERE name='Haircut & Style'),     (SELECT id FROM staff WHERE name='Frida Holm'),   '2026-05-12T09:00:00Z', 'completed', 'Trim + blow dry'),
    ((SELECT id FROM clients WHERE name='Erik Andersson'),   (SELECT id FROM services WHERE name='Hair Coloring'),       (SELECT id FROM staff WHERE name='Frida Holm'),   '2026-05-18T13:00:00Z', 'completed', 'Full colour'),
    ((SELECT id FROM clients WHERE name='Olof Persson'),     (SELECT id FROM services WHERE name='Beard Trim'),          (SELECT id FROM staff WHERE name='Sven Ek'),      '2026-05-22T10:30:00Z', 'completed', ''),
    ((SELECT id FROM clients WHERE name='Karin Svensson'),   (SELECT id FROM services WHERE name='Classic Facial'),      (SELECT id FROM staff WHERE name='Nadia Aziz'),   '2026-06-02T15:00:00Z', 'completed', 'Sensitive-skin products'),
    ((SELECT id FROM clients WHERE name='Sofia Gustafsson'), (SELECT id FROM services WHERE name='Lash Extensions'),     (SELECT id FROM staff WHERE name='Tobias Falk'),  '2026-06-05T11:00:00Z', 'completed', 'Full set'),
    ((SELECT id FROM clients WHERE name='Maria Karlsson'),   (SELECT id FROM services WHERE name='Gel Manicure'),        (SELECT id FROM staff WHERE name='Camilla Berg'), '2026-06-10T14:00:00Z', 'completed', ''),
    ((SELECT id FROM clients WHERE name='Sara Nilsson'),     (SELECT id FROM services WHERE name='Deep Cleanse Facial'), (SELECT id FROM staff WHERE name='Nadia Aziz'),   '2026-06-14T16:00:00Z', 'completed', ''),
    -- cancelled / no_show
    ((SELECT id FROM clients WHERE name='Per Eriksson'),     (SELECT id FROM services WHERE name='Haircut & Style'),     (SELECT id FROM staff WHERE name='Frida Holm'),   '2026-06-15T09:30:00Z', 'cancelled', 'Client rescheduled'),
    ((SELECT id FROM clients WHERE name='Lars Olsson'),      (SELECT id FROM services WHERE name='Beard Trim'),          (SELECT id FROM staff WHERE name='Sven Ek'),      '2026-06-17T12:00:00Z', 'no_show',   ''),
    ((SELECT id FROM clients WHERE name='Linnea Johansson'), (SELECT id FROM services WHERE name='Spa Pedicure'),        (SELECT id FROM staff WHERE name='Camilla Berg'), '2026-06-20T13:30:00Z', 'cancelled', ''),
    -- today (2026-06-28)
    ((SELECT id FROM clients WHERE name='Anna Bergström'),   (SELECT id FROM services WHERE name='Classic Facial'),      (SELECT id FROM staff WHERE name='Nadia Aziz'),   '2026-06-28T10:00:00Z', 'booked',    'Repeat client'),
    ((SELECT id FROM clients WHERE name='Johan Lindqvist'),  (SELECT id FROM services WHERE name='Haircut & Style'),     (SELECT id FROM staff WHERE name='Frida Holm'),   '2026-06-28T11:30:00Z', 'booked',    'Morning slot'),
    ((SELECT id FROM clients WHERE name='Olof Persson'),     (SELECT id FROM services WHERE name='Beard Trim'),          (SELECT id FROM staff WHERE name='Sven Ek'),      '2026-06-28T15:00:00Z', 'booked',    ''),
    -- upcoming
    ((SELECT id FROM clients WHERE name='Emma Larsson'),     (SELECT id FROM services WHERE name='Hair Coloring'),       (SELECT id FROM staff WHERE name='Frida Holm'),   '2026-06-29T09:00:00Z', 'booked',    'Balayage'),
    ((SELECT id FROM clients WHERE name='Sofia Gustafsson'), (SELECT id FROM services WHERE name='Lash Extensions'),     (SELECT id FROM staff WHERE name='Tobias Falk'),  '2026-07-01T10:30:00Z', 'booked',    'Refill'),
    ((SELECT id FROM clients WHERE name='Karin Svensson'),   (SELECT id FROM services WHERE name='Deep Cleanse Facial'), (SELECT id FROM staff WHERE name='Nadia Aziz'),   '2026-07-02T16:00:00Z', 'booked',    ''),
    ((SELECT id FROM clients WHERE name='Maria Karlsson'),   (SELECT id FROM services WHERE name='Gel Manicure'),        (SELECT id FROM staff WHERE name='Camilla Berg'), '2026-07-03T13:00:00Z', 'booked',    ''),
    ((SELECT id FROM clients WHERE name='Erik Andersson'),   (SELECT id FROM services WHERE name='Haircut & Style'),     (SELECT id FROM staff WHERE name='Frida Holm'),   '2026-07-04T11:00:00Z', 'booked',    ''),
    ((SELECT id FROM clients WHERE name='Per Eriksson'),     (SELECT id FROM services WHERE name='Spa Pedicure'),        (SELECT id FROM staff WHERE name='Camilla Berg'), '2026-07-05T14:30:00Z', 'booked',    ''),
    ((SELECT id FROM clients WHERE name='Sara Nilsson'),     (SELECT id FROM services WHERE name='Classic Facial'),      (SELECT id FROM staff WHERE name='Nadia Aziz'),   '2026-07-06T15:30:00Z', 'booked',    '');

INSERT INTO orders (client_id, item_name, quantity, price_cents, status, ordered_at) VALUES
    ((SELECT id FROM clients WHERE name='Anna Bergström'),   'Shampoo 250ml',         1, 18000, 'fulfilled', '2026-05-12T09:45:00Z'),
    ((SELECT id FROM clients WHERE name='Erik Andersson'),   'Hair Serum 50ml',       1, 32000, 'paid',      '2026-05-18T13:50:00Z'),
    ((SELECT id FROM clients WHERE name='Olof Persson'),     'Beard Oil 30ml',        2, 24000, 'fulfilled', '2026-05-22T11:00:00Z'),
    ((SELECT id FROM clients WHERE name='Karin Svensson'),   'Face Moisturizer',      1, 28000, 'paid',      '2026-06-02T15:40:00Z'),
    ((SELECT id FROM clients WHERE name='Sofia Gustafsson'), 'Lash Cleanser',         1, 15000, 'fulfilled', '2026-06-05T11:45:00Z'),
    ((SELECT id FROM clients WHERE name='Maria Karlsson'),   'Nail Polish – Red',     3, 13500, 'new',       '2026-06-10T14:30:00Z'),
    ((SELECT id FROM clients WHERE name='Sara Nilsson'),     'Vitamin C Serum',       1, 39000, 'paid',      '2026-06-14T16:30:00Z'),
    ((SELECT id FROM clients WHERE name='Linnea Johansson'), 'Hair Mask 200ml',       1, 22000, 'refunded',  '2026-06-16T10:00:00Z'),
    ((SELECT id FROM clients WHERE name='Emma Larsson'),     'Dry Shampoo',           2, 17000, 'new',       '2026-06-20T12:15:00Z'),
    ((SELECT id FROM clients WHERE name='Lars Olsson'),      'Beard Balm',            1, 19000, 'paid',      '2026-06-22T09:20:00Z'),
    ((SELECT id FROM clients WHERE name='Per Eriksson'),     'Sunscreen SPF50',       1, 21000, 'new',       '2026-06-24T13:10:00Z'),
    ((SELECT id FROM clients WHERE name='Johan Lindqvist'),  'Styling Wax',           1, 16000, 'fulfilled', '2026-06-25T17:00:00Z'),
    ((SELECT id FROM clients WHERE name='Anna Bergström'),   'Hand Cream',            2, 12000, 'new',       '2026-06-26T10:30:00Z'),
    ((SELECT id FROM clients WHERE name='Sofia Gustafsson'), 'Gift Card 500 kr',      1, 50000, 'paid',      '2026-06-27T11:00:00Z'),
    ((SELECT id FROM clients WHERE name='Maria Karlsson'),   'Cuticle Oil',           1, 11000, 'refunded',  '2026-06-27T14:45:00Z');
