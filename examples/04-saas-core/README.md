# SaaS core

**Complexity:** ⭐⭐⭐⭐☆
**Models:** 5

## What this domain teaches

The minimum shape every B2B SaaS rebuilds: tenants, members with
roles, scoped projects and tasks, and a billing subscription tied
to the tenant. Tenant isolation is the spine — every queryable
model carries `organization_id` directly or reaches it through
`Project`.

## Models

| Model        | Key fields                                                                                              | Relations                                                                |
|--------------|---------------------------------------------------------------------------------------------------------|--------------------------------------------------------------------------|
| Organization | `name`, `slug`, `description?`, `billing_email`, `is_active`                                            | (none — the tenant root)                                                 |
| Member       | `organization_id`, `email`, `full_name`, `role`, `invited_at`, `joined_at?`, `last_active_at?`, `is_active` | belongs_to Organization                                              |
| Project      | `organization_id`, `lead_id?`, `name`, `key`, `description?`, `status`, `start_date?`, `target_end_date?`, `archived_at?` | belongs_to Organization, belongs_to Member (`lead_id`, nullable) |
| Task         | `project_id`, `assignee_id?`, `reporter_id`, `title`, `description?`, `status`, `priority`, `estimate_hours?`, `due_date?`, `completed_at?` | belongs_to Project, belongs_to Member ×2 (assignee, reporter) |
| Subscription | `organization_id`, `plan`, `status`, `billing_cycle`, `seats`, `monthly_amount`, `current_period_start`, `current_period_end`, `trial_ends_at?`, `canceled_at?` | belongs_to Organization                          |

`?` marks nullable fields. Every model also carries auto-managed
`id`, `created_at`, `updated_at` (not editable).

## Filtering scenarios

* **Tenant dashboard** — for a given `organization_id`: count of `Project.status='active'`, count of `Task.status IN ('backlog','in_progress','in_review')`, count of `Member.is_active=true`. Three numbers above the fold.
* **My open work** — `Task.assignee_id=current_member AND project.organization_id=current_org AND status NOT IN ('done','cancelled')`, ordered by `priority DESC, due_date ASC`. The personal queue.
* **Overdue tasks per project** — `Task.project_id=X AND due_date < now AND completed_at IS NULL AND status != 'cancelled'`. Project-manager early-warning view.
* **Billing renewal soon** — `Subscription.status='active' AND current_period_end < now+7d`. Operator dashboard for the billing team.
* **Trials expiring** — `Subscription.status='trialing' AND trial_ends_at < now+3d`. Hand-off list for sales / customer success.
* **Inactive members** — `Member.organization_id=X AND last_active_at < now-30d AND is_active=true`. Seat-reclamation candidates.

## Status / lifecycle conventions

`Member.role`: `owner` / `admin` / `member` / `viewer`.
`Project.status`: `planning` / `active` / `on_hold` / `completed` / `archived`.
`Task.status`: `backlog` / `in_progress` / `in_review` / `done` / `cancelled`.
`Task.priority`: `low` / `medium` / `high` / `urgent`.
`Subscription.plan`: `free` / `starter` / `pro` / `enterprise`.
`Subscription.status`: `trialing` / `active` / `past_due` / `canceled`.
`Subscription.billing_cycle`: `monthly` / `annual`.

## Currency

`Subscription.monthly_amount` is `i64` in **smallest unit** (cents
for USD, öre for SEK). Display formatting is the project's
responsibility. Same convention applies to every money field across
the example catalogue (rent, prices, totals, income).

## ⚠️ Tenant-isolation gap

This schema describes shape, not enforcement. Every query against
`Member`, `Project`, `Task`, or `Subscription` **must** be scoped
by `organization_id` at the application or row-level-security
layer. A missing filter that lets a user see another tenant's data
is the canonical SaaS data leak — assume the schema does NOT
prevent it and design your query layer accordingly.

## Deferred extensions

Two pieces of real-world surface intentionally omitted to honour
the 5-model cap:

* **Comment** — `task_id` + `member_id` + `body` + `created_at`.
  Activity discussion belongs on tasks; this example skips it.
  Source `playground/schema-tasks.json` includes a working `Comment`
  shape for reference.
* **Invoice** — `subscription_id` + `period_start` + `period_end` +
  `total_amount` + `status` + `paid_at?`. Required for any tenant
  that needs receipts, dunning, or accountant exports. The current
  `Subscription` carries enough state to know a period exists, but
  not to bill against history.

Both are additive — they don't modify any of the five existing
models. Add them when the budget allows.

## How to use

```
rustio new project saas --schema schema.json
```

## Why this matters

Most B2B products rebuild this exact shape from scratch and get
some part of it subtly wrong — usually tenant isolation, or the
trial-vs-active billing transition, or seat counting. Starting from
a complete-but-minimal version means the obvious shape exists and
you can spend the design budget on what makes your product
different.

## Next

→ `examples/05-queue-system/` — priority scoring, eligibility, ranking lists.
