-- Richer demo data: +3 projects (5 total) and +22 tasks (27 total)
-- distributed across realistic status / priority / due-date / FK
-- combinations. Includes a few `review` and `blocked` values to
-- exercise the admin's neutral-pill fallback for status strings that
-- don't have a dedicated colour mapping in admin.css.
--
-- Idempotent on a clean DB (run after 0003 once). Re-running this
-- migration on a DB that already has it applied is a no-op because
-- the migration tracker remembers it; running it on a partially-
-- populated DB would duplicate rows. The intent is one-shot seeding.

INSERT INTO projects (name, description, is_active) VALUES
    ('Q3 product launch',  'Cross-functional push for the September launch window.',                1),
    ('API rate-limiter',   'Replace the in-memory limiter with a Redis-backed sliding window.',     1),
    ('Documentation site', 'Static-site rebuild on Astro; consolidate three legacy docs into one.', 0);

INSERT INTO tasks (title, description, status, priority, project_id, due_at) VALUES
    -- Website redesign (project_id = 1)
    ('Replace the marketing typography',  'Switch to Inter for body, drop the Helvetica fallback chain.', 'done',        2, 1, NULL),
    ('Build the pricing calculator',      'Three-tier toggle + per-seat math. No Stripe yet.',            'in_progress', 4, 1, '2026-06-15 17:00:00'),
    ('A/B test the new hero copy',        'Variant B leans into the metric we land at. Ship by Friday.', 'review',      3, 1, '2026-06-12 12:00:00'),
    ('Audit footer link inventory',       '',                                                              'todo',        1, 1, NULL),

    -- Mobile app v2 (project_id = 2)
    ('Storyboard the onboarding',         'Four screens. Lead with the value prop, not the form.',                       'done',        4, 2, NULL),
    ('Wire push-notification opt-in',     'After first session ends, not on app launch.',                                'in_progress', 5, 2, '2026-07-15 12:00:00'),
    ('Decide on local-data encryption',   'Per-record vs. database-level. Review the Apple guidance first.',              'blocked',     5, 2, NULL),
    ('Bench-test offline queue sizes',    '',                                                                             'todo',        2, 2, NULL),

    -- Q3 product launch (project_id = 3)
    ('Lock the launch date',              'Cross-check with marketing, sales-enablement, and infra.',                    'done',        5, 3, NULL),
    ('Draft the changelog narrative',     'One blog post. Three sub-sections. Lead with the demo gif.',                  'in_progress', 4, 3, '2026-09-01 09:00:00'),
    ('Schedule podcast outreach',         'Three target shows; need pitch ready by July 20.',                             'todo',        3, 3, '2026-07-20 17:00:00'),
    ('Run the pricing simulation',        'Two scenarios. Loop in finance.',                                              'todo',        4, 3, '2026-08-05 17:00:00'),
    ('Coordinate the launch-day demo',    '',                                                                             'review',      5, 3, '2026-09-04 14:00:00'),

    -- API rate-limiter (project_id = 4)
    ('Spike the Redis sliding-window',    'Need an answer on Lua-script vs. ZSET-based bookkeeping.',                    'in_progress', 5, 4, '2026-06-20 17:00:00'),
    ('Migrate the integration tests',     'Replace the per-test mock with a real Redis fixture.',                         'todo',        3, 4, NULL),
    ('Decide failure mode',               'Fail-open or fail-closed when Redis times out. The default matters.',         'blocked',     5, 4, NULL),
    ('Add per-tenant quota headers',      'X-RateLimit-* in the response. Match the GitHub API shape.',                   'todo',        2, 4, NULL),

    -- Documentation site (project_id = 5)
    ('Inventory the three legacy docs',   'Tag each page by status: keep, merge, retire.',                                'done',        2, 5, NULL),
    ('Pick the new search backend',       'Algolia DocSearch vs. self-hosted Meilisearch.',                               'review',      3, 5, '2026-06-25 17:00:00'),
    ('Move the API reference',            'Auto-generate from OpenAPI. Was hand-maintained before.',                      'in_progress', 4, 5, '2026-07-10 17:00:00'),
    ('Set up the deploy preview',         'Per-PR Vercel previews. Replace the existing Netlify rig.',                    'todo',        2, 5, NULL),
    ('Write the contributor guide',       '',                                                                             'todo',        1, 5, NULL);
