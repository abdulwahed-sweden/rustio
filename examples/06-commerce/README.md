# Commerce

**Complexity:** ⭐⭐⭐⭐☆
**Models:** 5

## What this domain teaches

A storefront's spine: hierarchical categories (self-referential FK),
product catalogue with stock tracking, customers, carts (including
guest carts), and the order lifecycle from placement to delivery.
Light enough to read in five minutes, real enough to surface the
production gaps every commerce build hits.

## Models

| Model    | Key fields                                                                                              | Relations                                                                          |
|----------|---------------------------------------------------------------------------------------------------------|------------------------------------------------------------------------------------|
| Category | `parent_category_id?`, `name`, `slug`, `description?`, `sort_order`, `is_active`                        | belongs_to Category (`parent_category_id`, nullable — self-referential)            |
| Product  | `category_id`, `sku`, `name`, `description?`, `price`, `stock_quantity`, `reorder_threshold`, `weight_grams?`, `image_url?`, `is_active` | belongs_to Category                                       |
| Customer | `email`, `first_name`, `last_name`, `phone?`, `address?`, `city?`, `region?`, `postal_code?`, `country?`, `is_active` | (none)                                                                |
| Cart     | `customer_id?`, `session_token?`, `status`, `item_count`, `total_amount`, `abandoned_at?`, `converted_at?` | belongs_to Customer (nullable — guest carts allowed)                              |
| Order    | `customer_id`, `order_number`, `status`, `subtotal_amount`, `shipping_amount`, `tax_amount`, `total_amount`, `shipping_address`, `billing_address`, `payment_method`, `tracking_number?`, `placed_at`, `paid_at?`, `shipped_at?`, `delivered_at?`, `canceled_at?` | belongs_to Customer                          |

`?` marks nullable fields. Every model also carries auto-managed
`id`, `created_at`, `updated_at` (not editable).

## Filtering scenarios

* **Catalogue browse** — `Product.is_active=true AND category_id IN (X, ...)`, ordered by `sort_order` then `name`. The shop view.
* **Low stock alert** — `Product.is_active=true AND stock_quantity <= reorder_threshold`. Operator dashboard for purchasing.
* **Abandoned carts ready for outreach** — `Cart.status='abandoned' AND abandoned_at BETWEEN now-7d AND now-1d AND customer_id IS NOT NULL`. The marketing recovery list.
* **Orders in fulfillment** — `Order.status='paid' AND shipped_at IS NULL`, ordered by `placed_at ASC`. The warehouse picking queue.
* **Late deliveries** — `Order.status='shipped' AND delivered_at IS NULL AND shipped_at < now-14d`. Customer-service follow-up.
* **Recent customer activity** — `Order.customer_id=X` ordered by `placed_at DESC`. Account history view.
* **Revenue this period** — `Order.status IN ('paid','shipped','delivered') AND placed_at BETWEEN start AND end`, sum of `total_amount`. Reporting.

## Status / lifecycle conventions

`Cart.status`: `active` / `abandoned` / `converted`.

`Order.status`:
`pending` (created) → `paid` → `shipped` → `delivered`,
plus terminal `cancelled` and `refunded`.

`Order.payment_method`: `card` / `paypal` / `bank_transfer` / `invoice`.

## Cart integrity rule (app layer)

Exactly one of:

* `customer_id IS NOT NULL` (signed-in cart)
* `session_token IS NOT NULL` (guest cart)

The schema does not enforce this constraint. The application is
responsible for rejecting Cart writes that violate it, and a
periodic sweep should flag rows that drifted (both null, or both
set) for cleanup.

## Currency

All money fields (`Product.price`, `Cart.total_amount`,
`Order.subtotal_amount`, `shipping_amount`, `tax_amount`,
`total_amount`) are `i64` in **smallest unit** (cents for USD, öre
for SEK). UI formatting is out of scope.

## Category trees (recursive CTE)

`Category.parent_category_id` is a self-referential nullable FK —
top-level categories have `NULL`, children point at their parent.
Common queries use a recursive CTE:

```sql
-- All descendants of a given category, with their depth.
WITH RECURSIVE category_tree AS (
    SELECT id, parent_category_id, name, 0 AS depth
        FROM categories
        WHERE id = $root_id
    UNION ALL
    SELECT c.id, c.parent_category_id, c.name, ct.depth + 1
        FROM categories c
        JOIN category_tree ct ON c.parent_category_id = ct.id
        WHERE ct.depth < 4   -- guard against pathological depth
)
SELECT * FROM category_tree;
```

**Recommended max depth: 3–4.** Deeper trees become unmanageable
in admin breadcrumbs and slow down faceted filtering. Enforce the
cap when creating or moving a category.

**Cycle prevention.** The schema does not prevent cycles
(category A → B → A). Two layers of defense at the application:

1. On `parent_category_id` write, walk up from the proposed parent
   and reject if the current category appears in the chain.
2. The `WHERE ct.depth < N` guard above terminates queries even if
   a cycle slips through, so dashboards never freeze.

## ⚠️ Production gap: no line items

This is the most important caveat in the example. **Orders do not
have line items.** `subtotal_amount` and `total_amount` on `Order`
are stored aggregates with no underlying breakdown. Likewise
`Cart.total_amount` and `Cart.item_count` are aggregates without
detail rows.

A real production system **must** introduce two additional tables:

* `OrderItem` — `order_id`, `product_id`, `quantity`, `unit_price`,
  `line_total`, plus snapshot fields (`product_name`, `sku`) so
  later catalogue edits don't rewrite history.
* `CartItem` — `cart_id`, `product_id`, `quantity`, `added_at`.

Without them you cannot:

* Show the customer what they're buying.
* Calculate per-product VAT.
* Process partial refunds.
* Inspect why a total is what it is.

This omission is **intentional** to keep the example within the
5-model constraint requested for the catalogue. Treat it as the
first thing you add when adapting this schema for real use.

## How to use

```
rustio new project shop --schema schema.json
```

## Why this matters

Every commerce platform — from indie e-shops to multi-region
marketplaces — rebuilds this exact spine. The interesting work is
in promotions, recommendations, fraud, fulfilment routing — none of
which makes sense until the order-cart-customer-product core is
already correct. This is that core.

## Next

→ end of catalogue. Loop back to the gallery (`examples/README.md`)
or the compiled walkthrough at `examples/blog/`.
