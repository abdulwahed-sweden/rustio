# Foundation

**Complexity:** ⭐☆☆☆☆
**Models:** 2

## What this domain teaches

The smallest meaningful Rustio schema. Demonstrates every primitive
type the framework supports, one foreign-key relation, and the
auto-managed timestamp convention — nothing else. Read this before
the compiled `examples/blog/` walkthrough.

## Models

| Model    | Key fields                                                                   | Relations                                  |
|----------|------------------------------------------------------------------------------|--------------------------------------------|
| Category | `name`, `description?`, `is_active`, `sort_order`                            | (none)                                     |
| Item     | `category_id`, `title`, `body?`, `is_published`, `view_count`, `published_at?` | belongs_to Category (`category_id`)       |

`?` marks nullable fields. Every model also carries auto-managed
`id`, `created_at`, `updated_at` (not editable).

## Filtering scenarios

* **Published items in a category** — `Item.is_published=true AND Item.category_id=X`, ordered by `published_at DESC`. The standard "feed" query.
* **Recent activity** — `Item.created_at >= now - 7d` ordered by `created_at DESC`. Drives a "this week" panel.
* **Most-read** — `Item.is_published=true` ordered by `view_count DESC`, limit 10. Trending lookup.
* **Active categories with content** — `Category.is_active=true` joined to `Item` with `Item.is_published=true`, grouped, count > 0. The navigation surface.

## SQL type mapping (documentation only)

The framework is **PostgreSQL-only** at v1.0. The schema's
`"type": "String"` is materialised as **`TEXT`** in PostgreSQL —
no length cap. The mapping lives in
`rustio-core/src/ai/executor.rs::sql_type_for`:

| Schema type | PostgreSQL column type |
|-------------|------------------------|
| `i32`       | `INTEGER`              |
| `i64`       | `BIGINT`               |
| `bool`      | `BOOLEAN`              |
| `String`    | `TEXT`                 |
| `DateTime`  | `TIMESTAMPTZ`          |

There is no separate path for SQLite, MySQL, or other engines.
`Item.body` is intentionally `String` rather than introducing a
separate "long text" type — PostgreSQL `TEXT` has no performance
penalty over `VARCHAR(N)` and handles long-form content without
re-platforming. If you ever target a database with hard column
limits (e.g. some MySQL configurations cap indexed `TEXT`), enforce
the length at the application or migration layer; the schema does
not surface this.

## How to use

```
rustio new project myapp --schema schema.json
```

## Why this matters

Every real project starts with one or two domain models and a
relationship. This example shows that minimum without imposing a
domain — pick the names that fit your project, keep the shape.

## Next

→ `examples/blog/` for the compiled walkthrough that wires this same
shape into PostgreSQL + Meilisearch + admin UI, or skip ahead to
`examples/02-healthcare/` for the production-style flagship.
