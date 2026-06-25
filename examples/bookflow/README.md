# bookflow — a general-purpose booking system

`bookflow` is the canonical RustIO example: a **general-purpose booking
system** written entirely in Rust as a real RustIO project. It is
deliberately domain-agnostic. The same seven models — customers,
resources, bookings, locations, schedules, assignments, invoices — fit
any *reserve-a-resource* business:

- **Container logistics** — resources are containers, bookings are slot
  reservations at a depot location.
- **Equipment rental** — resources are vehicles or tools, bookings are
  rental periods, invoices bill the customer.
- **Appointments** — resources are rooms or staff, bookings are
  appointment slots, schedules declare availability.

The point of keeping the names generic is to demonstrate the core RustIO
idea: **the same schema reshapes into different domains purely by editing
the ViewSpec — no structural change.** The data model is fixed; how it
*reads* is a presentation decision.

## The models

| Model        | What it is                              | Key relations                          |
|--------------|-----------------------------------------|----------------------------------------|
| `Location`   | service area / delivery point           | —                                      |
| `Customer`   | the party making a booking              | —                                      |
| `Resource`   | the bookable thing                      | → Location                             |
| `Booking`    | the reservation itself (the heart)      | → Customer, → Resource, → Resource*    |
| `Schedule`   | when a resource is available            | → Resource                             |
| `Assignment` | who fulfilled a booking                 | → Booking, → Resource                  |
| `Invoice`    | billing issued to a customer            | → Customer                             |

\* `Booking.assignee_id` is an optional `Resource` (set when the booking
is assigned).

Each model is one Rust struct deriving `RustioAdmin` + a hand-written
`impl Model`, one migration, and one `admin.model::<T>()` line — see
`apps/<model>/models.rs`.

## Type mapping note

RustIO's schema vocabulary is `i32 / i64 / String / bool / DateTime`.
This example maps the richer field intents onto it deterministically:

- **Enum** fields (`status`, `customer_type`, `resource_type`,
  `service_type`, `weekday`, `mode`) → `String`, with the allowed values
  documented on each field and a sensible `DEFAULT` in the migration.
- **Money** (`rate_cents`, `amount_cents`) → `i64` integer minor units
  (cents/öre) — never floats.
- **Time-of-day** (`Schedule.start_time` / `end_time`) → `String`
  `"HH:MM"` — RustIO has no first-class time type.
- **Relations** → `i64` foreign-key columns with
  `#[rustio(belongs_to = "...", display = "...")]`; the optional
  `assignee_id` is `Option<i64>`.

## Run it

```bash
cd examples/bookflow
rustio migrate apply        # create tables + seed demo rows
rustio user create --email admin@bookflow.local --password demo1234 --role admin
rustio run                  # serve on http://127.0.0.1:8000
# Sign in at /admin: admin@bookflow.local / demo1234
```

## See a view in the terminal

The whole reason the model is generic: render any model's
domain-shaped default view straight from the schema, no web layer
needed. First emit the schema, then view a model. **Model names are
PascalCase singular** (`Booking`, `Customer`, …), matching
`rustio.schema.json`:

```bash
rustio schema                       # emit rustio.schema.json
rustio view Booking --layout list   # the richest model, list layout
rustio view Customer --layout cards
rustio view Resource --layout compact
```

Example output:

```
View: Booking  ·  layout: list  ·  rows: 3  (demo data)

Row 1
  Title      sample booking_number 1
  Timestamp  Jun 25, 2026 · 14:30
  Timestamp  Jun 25, 2026 · 14:30
  Badge      sample status 1
```

`rustio view` derives a sensible default view (Title / Subtitle / Badge /
Timestamp / Meta / Hidden roles) from the schema. Save it with
`rustio view Booking --save` to get `booking.view.json`, then hand-edit
that file to reshape the view for *your* domain — the schema never
changes. Add `--json` to dump the structured `RenderedView` for scripting.

> The demo rows shown by `rustio view` are synthesised placeholders so you
> can see the layout shape; the command does not read the database.
