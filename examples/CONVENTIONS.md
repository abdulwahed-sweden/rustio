# Rustio conventions

This document defines the shared patterns used across all examples.
Each example README focuses on its domain — this file captures the
cross-cutting rules that apply everywhere.

If you adopt a schema from `examples/`, you are expected to adopt these
conventions unless you have a strong reason not to.

---

## 1. Derived fields (staleness contract)

Some fields are **stored, not computed on read**, even though they are
derivable from other data.

Examples:

* `QueueEntry.priority_score`
* `QueueEntry.waiting_days`
* `Course.enrolled_count`
* `Cart.total_amount`
* `Cart.item_count`

### Why

* Faster reads (no runtime aggregation)
* Simpler query layer
* Predictable sorting/filtering

### Trade-off

Stored derived values become **stale** unless explicitly updated.

### Required patterns

Every derived field must follow one of these:

#### (A) Write-time maintenance

Update the field whenever the underlying data changes.

Example:

* increment `enrolled_count` when a student enrolls
* decrement on drop

#### (B) Scheduled recomputation (recommended for aggregates)

Run a background job (e.g. nightly) that:

1. Recomputes the value from source fields
2. Writes it back
3. Updates a timestamp (if present)

Example:

* recompute `priority_score` for all active queue entries

#### (C) Hybrid (best for critical systems)

* maintain on write
* periodically reconcile via job

### Consumer contract

Consumers must treat derived fields as:

> "correct as of last update", not "real-time truth"

If freshness matters, add filters like:

```sql
priority_score_recomputed_at > now - interval '24 hours'
```

---

## 2. Money handling

All monetary values use:

* type: `i64`
* unit: **smallest currency unit**

Examples:

* USD → cents
* SEK → öre

### Why

* Avoid floating-point rounding errors
* Consistent arithmetic
* Database-safe comparisons

### Rules

* Never use floats for money
* Always document currency at the application layer
* Formatting (e.g. `10.50`) is UI responsibility

---

## 3. Status fields (string enums)

All status fields are stored as:

```json
"type": "String"
```

Examples:

* `Appointment.status`
* `Order.status`
* `Task.status`
* `Application.status`

### Why

* Flexible schema (no migration for new values)
* Clear filtering (`WHERE status = 'active'`)
* Language-agnostic

### Rules

* Each example defines a **closed vocabulary** in its README
* Application code must enforce allowed values
* Treat values as case-sensitive constants

---

## 4. Integrity rules (app-layer constraints)

Some constraints cannot be expressed directly in the schema.

Examples:

* Partial unique indexes
* Conditional nullability
* Cross-field validation

### Pattern

Document the rule in README and enforce via:

* application logic, or
* database migrations (advanced users)

### Example: partial uniqueness

```sql
CREATE UNIQUE INDEX queue_entries_applicant_category_global
    ON queue_entries (applicant_id, category)
    WHERE listing_id IS NULL;
```

### Example: mutual exclusivity

```text
Exactly one of:
- customer_id IS NOT NULL
- session_token IS NOT NULL
```

### Rule

If a constraint is not in the schema, it **must** be:

1. documented
2. enforced elsewhere

---

## 5. Tenant isolation (multi-tenancy)

Multi-tenant systems (see `04-saas-core`) rely on:

```text
organization_id
```

### Critical rule

Every query must be scoped by tenant:

```sql
WHERE organization_id = current_org
```

### Why

The schema does **not** enforce isolation.
A missing filter = data leak.

### Enforcement options

* Application-layer filtering (minimum)
* Row-Level Security (recommended for production)
* Middleware guards

---

## 6. Audit responsibility (production gaps)

Some examples intentionally omit audit tables to stay within model limits.

Example:

* `Healthcare` omits `MedicalRecordAccess`

### Rule

Any system handling sensitive data must implement:

* access logs (who viewed what, when)
* immutable audit trail
* actor identity (`viewer_id`)
* timestamp (`viewed_at`)
* action type (`read`, `edit`, `export`)

### Principle

> If data is sensitive, access must be traceable.

---

## 7. Referential integrity

All relationships use explicit foreign keys:

```json
"relation": {
  "model": "X",
  "field": "id",
  "kind": "belongs_to"
}
```

### Rules

* Prefer real FKs over string references
* Do not denormalize unless necessary
* Enforce cascade/delete behavior in application or migrations

---

## 8. Recursive relationships (trees)

Self-referential FKs (e.g. `Category.parent_category_id`) are supported.

### Required safeguards

* Limit depth (recommended: 3–4 levels)
* Prevent cycles at write-time
* Guard recursive queries:

```sql
WHERE depth < N
```

### Principle

> Trees must be bounded and acyclic.

---

## 9. Timestamps

Every model includes:

* `created_at`
* `updated_at`

Both:

* non-nullable
* `editable: false`

### Rules

* Managed automatically by the framework
* Domain-specific timestamps are explicit:

  * `scheduled_at`
  * `placed_at`
  * `recorded_at`

---

## 10. Schema vs behavior

Rustio schemas define:

* structure
* relations
* field types

They do **not** define:

* business logic
* validation rules
* workflows enforcement
* background jobs

### Principle

> Schema describes shape. Code defines behavior.

---

## Final note

These conventions are not theoretical.

They are the **minimum set of patterns required** to move from:

> "data model that works"

to

> "system that survives real-world usage"

Every example in this repository assumes these rules.
