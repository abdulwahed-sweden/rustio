-- Two demo projects + a few tasks so /admin renders non-empty lists
-- on the first run. Safe to delete this migration if you want a
-- pristine database for your own data.
INSERT INTO projects (name, description, is_active) VALUES
    ('Website redesign', 'Migrate marketing site to the new template system.', 1),
    ('Mobile app v2',    'iOS + Android rewrite on a shared backend.',         1);

INSERT INTO tasks (title, description, status, priority, project_id, due_at) VALUES
    ('Pick a typography stack',
     'Decide on body / display fonts and self-host them.',
     'in_progress', 4, 1, '2026-06-10 17:00:00'),
    ('Wire the contact form',
     'Hook the form up to the new lead-capture endpoint.',
     'todo',        3, 1, NULL),
    ('Draft the privacy policy',
     'Align with GDPR + the existing data-retention doc.',
     'done',        2, 1, NULL),
    ('Spec the offline-sync engine',
     'Conflict resolution strategy + worst-case storage budget.',
     'todo',        5, 2, '2026-07-01 12:00:00'),
    ('Set up CI for the iOS build',
     'TestFlight upload on every tagged release.',
     'in_progress', 4, 2, NULL);
