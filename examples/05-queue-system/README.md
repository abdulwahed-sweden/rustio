# Queue system

**Complexity:** ⭐⭐⭐⭐☆
**Models:** 4

## What this domain teaches

A HomeQ-style housing queue: applicants accumulate priority over
time, listings open and close on schedules, applications draw from
a ranked queue. Demonstrates priority scoring with explainable
component fields, listing-specific vs global queue patterns, and
the staleness problem every materialised-score system hits.

## Models

| Model       | Key fields                                                                                                                   | Relations                                                                      |
|-------------|------------------------------------------------------------------------------------------------------------------------------|--------------------------------------------------------------------------------|
| Applicant   | `first_name`, `last_name`, `email`, `phone`, `date_of_birth`, `city`, `region`, `household_size`, `employment_status`, `monthly_income?`, `is_verified`, `verified_at?`, `registered_at`, `is_active` | (none)                                              |
| Listing     | `title`, `description?`, `address`, `city`, `region`, `postal_code`, `monthly_rent`, `area_sqm`, `num_rooms`, `max_household_size`, `available_from`, `application_deadline`, `queue_category`, `minimum_queue_days`, `requires_employment_verification`, `status` | (none)            |
| Application | `applicant_id`, `listing_id`, `submitted_at`, `status`, `rank_at_submission?`, `household_size_declared`, `motivation?`, `reviewed_at?`, `decision_reason?`, `withdrawn_at?` | belongs_to Applicant, belongs_to Listing                                |
| QueueEntry  | `applicant_id`, `listing_id?`, `category`, `joined_at`, `waiting_days`, `boost_points`, `income_score?`, `household_score?`, `priority_score`, `priority_score_recomputed_at?`, `is_eligible`, `ineligibility_reason?`, `is_active`, `frozen_until?` | belongs_to Applicant, belongs_to Listing (nullable)         |

`?` marks nullable fields. Every model also carries auto-managed
`id`, `created_at`, `updated_at` (not editable).

## Filtering scenarios

* **Open listings near me** — `Listing.status='open' AND city=X AND monthly_rent<=Y AND num_rooms>=N AND application_deadline>now`, ordered by `application_deadline ASC`. Applicant-facing search.
* **Listing-specific ranking** — `QueueEntry.listing_id=X AND is_eligible=true AND is_active=true`, ordered by `priority_score DESC`. The actual draw order when an operator runs the lottery.
* **Global category ranking** — `QueueEntry.listing_id IS NULL AND category=X AND is_eligible=true`, ordered by `priority_score DESC LIMIT 100`. The persistent leaderboard view per category.
* **Verification backlog** — `Applicant.is_verified=false AND registered_at < now-30d AND is_active=true`. Operator triage queue.
* **Score-component audit** — `QueueEntry.income_score IS NULL AND priority_score > threshold AND is_eligible=true`. High-rank entries with missing income data; verification staff fix these before the next draw.
* **SLA breaches on review** — `Application.status='under_review' AND submitted_at < now-14d`. Operations dashboard.
* **Stale priority scores** — `QueueEntry.is_active=true AND (priority_score_recomputed_at IS NULL OR priority_score_recomputed_at < now-24h)`. Health check for the recompute job.

## Status / lifecycle conventions

`Application.status`:
`submitted` → `under_review` → `accepted` or `rejected`, plus
`withdrawn` as an applicant-driven exit.

`Listing.status`:
`draft` / `open` / `closed` / `awarded`.

`QueueEntry.category`:
`general` / `senior` / `youth` / `student` (project-defined; lock
the set in your application layer).

## Priority scoring (stored, recomputed)

`priority_score` is **stored**, not derived on read. The schema
stores the components AND the final figure; consumers read the
final figure for ranking.

Components:

* `waiting_days` — derived from `joined_at`. Should be recomputed
  whenever the score is.
* `boost_points` — operator-set, e.g. `+30` for verified employment,
  `+20` for documented disability, `+50` for repatriation programs.
* `income_score` (nullable) — populated when income data is
  verified, else `NULL` (different from zero).
* `household_score` (nullable) — non-zero when household size or
  composition affects the queue (family-only buildings,
  single-occupancy preferences).
* `priority_score` — recommended formula:
  `waiting_days + boost_points + COALESCE(income_score, 0) + COALESCE(household_score, 0)`.
  Schema does not enforce the formula; document the chosen weighting
  in your application code.

**Staleness contract.** `priority_score` becomes stale unless
recomputed. The system **assumes a background job runs nightly (or
on demand) over every `is_active=true` row** and:

1. Updates `waiting_days` from `joined_at`.
2. Recomputes `priority_score` from the components.
3. Stamps `priority_score_recomputed_at = now()`.

The "stale priority scores" filter above is the canary — alert when
any active entry hasn't been recomputed in the last 24 hours.

## Integrity rules (enforced at application layer)

The schema does **not** support partial unique indexes. These rules
must be enforced in application logic or via DB migrations that add
`UNIQUE INDEX ... WHERE ...` constructs (PostgreSQL syntax shown).

**1. One global queue entry per category per applicant.**

```sql
CREATE UNIQUE INDEX queue_entries_applicant_category_global
    ON queue_entries (applicant_id, category)
    WHERE listing_id IS NULL;
```

**2. One listing-specific queue entry per applicant + listing.**

```sql
CREATE UNIQUE INDEX queue_entries_applicant_listing
    ON queue_entries (applicant_id, listing_id)
    WHERE listing_id IS NOT NULL;
```

Without these, the same applicant can show up multiple times in the
same draw.

## How to use

```
rustio new project queue --schema schema.json
```

## Why this matters

Public-housing queues, waiting lists for clinics, scarce-resource
allocation, lottery-based admission — all of these depend on a
materialised priority score that's auditable, recomputable, and
explainable to the applicant who didn't make the cut. The component
fields (`income_score`, `household_score`, `boost_points`,
`waiting_days`) are how you answer "why was I ranked there?"
without a courtroom.

## Next

→ `examples/06-commerce/` — products, orders, payment lifecycle.
