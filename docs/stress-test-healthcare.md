# RustIO Real-World Stress Test — Healthcare Admin

A deliberately uncomfortable test of RustIO's admin under realistic relational complexity.
The goal was **not** to build a demo — it was to expose where RustIO breaks once you go
past a single flat table.

The full worked example lives at `examples/medflow/`. This document is its lab notebook.

---

## 1. Project goal

Test whether a production-grade operator could use RustIO **today** to run a small
clinic with six related entities, medium data volume, and normal day-to-day tasks:
read the schedule, find a patient's prescriptions, chase an unpaid invoice, delete
a retired department.

Not tested: performance, concurrency, migrations over a live DB, upgrades, SSO.

---

## 2. Reproduction from zero

Every command below was run against RustIO 0.3.1 at `rustio-core` HEAD of April 2026.
If you're using a published version of `rustio-cli`, drop the `RUSTIO_CORE_PATH`
prefix and `cargo run --manifest-path …` dance — it exists only so the example
points at the in-tree `rustio-core`.

| # | Command | Why it exists | What it touches |
|---|---|---|---|
| 1 | `cargo install rustio-cli` | Installs the `rustio` binary into `~/.cargo/bin`. Skip if you're using the in-tree CLI. | `~/.cargo/bin/rustio` |
| 2 | `cd examples && rustio init medflow --preset basic` | Scaffolds a new project with a minimal `main.rs`, auth wiring, and an empty `apps/` module. | Creates `examples/medflow/` with `Cargo.toml`, `main.rs`, `apps/mod.rs`, `static/`, `templates/`. |
| 3 | `cd medflow` | All subsequent commands are project-relative. | — |
| 4 | `rustio new app people` | Scaffolds a new app module for Patient / Doctor / Department. The CLI **edits `apps/mod.rs`** via marker comments to register the new app. | Creates `apps/people/{mod,admin,models,views}.rs` and `migrations/0001_create_peoples.sql`. |
| 5 | `rustio new app care` | Same, for Appointment / Prescription. | Creates `apps/care/…` and `migrations/0002_create_cares.sql`. |
| 6 | `rustio new app billing` | Same, for Invoice. | Creates `apps/billing/…` and `migrations/0003_create_billings.sql`. |
| 7 | *(manual)* `rm migrations/000{1,2,3}_create_{peoples,cares,billings}.sql` | The scaffolded migrations each define a trivial 3-column table (`title`, `is_active`, `priority`). We want real tables with real columns, indexes, and foreign keys, so we delete them and start over. | Leaves `migrations/` empty. |
| 8 | *(manual, one file per table)* Write `migrations/0001_create_departments.sql` through `0006_create_invoices.sql`. | Real schema — see §4 below. Order is FK-dependency, not app-boundary: `departments` before `doctors`, `patients` before `appointments`, etc. | 6 new `.sql` files. |
| 9 | *(manual)* Rewrite `apps/people/models.rs`, `apps/care/models.rs`, `apps/billing/models.rs`. | The scaffolder produces one struct per app matching the app's name. We replace those with the six real structs. `#[derive(RustioAdmin)]` + `impl Model` must match the SQL exactly. | 3 heavily-edited `.rs` files. |
| 10 | *(manual)* Update each `apps/<name>/admin.rs` to register every model the app owns. | `people::admin::install` goes from `admin.model::<People>()` to `admin.model::<Department>().model::<Doctor>().model::<Patient>()`. | 3 small edits. |
| 11 | *(manual)* Empty each `apps/<name>/views.rs` down to a no-op `register`. | The scaffolder emits a welcome HTML page at `/people`, `/care`, `/billing`. This stress test is admin-only — we delete them to keep the surface clean. | 3 small edits. |
| 12 | *(manual)* Add an empty `[workspace]` table at the top of `Cargo.toml`. | `examples/medflow/` sits inside the RustIO workspace tree but must not be part of the workspace, or `cargo build` at the workspace root tries to compile it. The empty `[workspace]` table is cargo's idiomatic "this crate is its own root." | `Cargo.toml` +3 lines. |
| 13 | `cargo build` | Surfaces macro / schema mismatches before touching the DB. Without the `RustioAdmin` derive being happy, the admin can't register the model. | `target/` gets populated; no source changes. |
| 14 | `rustio migrate apply` | Runs every pending migration inside a tracked transaction; then regenerates `rustio.schema.json` from the compiled admin. | Creates `app.db` (SQLite), writes `rustio.schema.json`. |
| 15 | `sqlite3 app.db < seed.sql` | Populates all six tables. Deliberately uses `sqlite3` (not the admin UI) because seeding 303 rows through browser forms is how you test the admin's patience, not your productivity. | `app.db` gains 278 rows across 6 tables. |
| 16 | `rustio user create --email admin@medflow.local --password medflow123 --role admin` | Creates a row in `rustio_users` so you can log into the admin. Auth is not automatic — new DBs have no users. | `app.db` gains 1 `rustio_users` row. |
| 17 | `rustio schema` | Regenerates `rustio.schema.json` from the compiled admin. Redundant right after `migrate apply` (which also runs it), but worth remembering — the schema is the only stable contract external tooling reads. | Rewrites `rustio.schema.json`. |
| 18 | `rustio run` | Boots hyper on `127.0.0.1:8000`. | Binds a port. No file changes. |
| 19 | *(browser)* Open `http://127.0.0.1:8000/admin`, log in. | Everything below §6 was tested in this UI. | — |

Steps 7–12 are the uncomfortable truth of this test: **RustIO's scaffolder produces
toy models**, and building anything real means hand-editing six files the scaffolder
just wrote. The AI-plan grammar (`rustio ai plan "add X as String to Y"`) can
mechanically add one field at a time, but applying it to a 60-field project would
take 60 plan/apply cycles. Hand-editing is both faster and more honest.

---

## 3. Schema design

Six tables, one-to-many everywhere, no join tables (no many-to-many). Field types
are constrained to what RustIO supports: `i32`, `i64`, `String`, `bool`, `chrono::DateTime<Utc>`,
and `Option<T>`. That vocabulary is the first place the test bites (see §7).

### 3.1 Entity relationships

```
Department (8) ──< Doctor (10) ──< Appointment (120) >── Patient (40)
                                          │                   │
                                          ├──< Prescription (60)
                                          └──< Invoice (40)  ──┘
```

- `Department` 1-to-many `Doctor` (every doctor belongs to one department).
- `Department.head_doctor_id` 0-or-1 `Doctor` (nullable; not every department has a head — breaks the circular FK between the two).
- `Doctor` 1-to-many `Appointment`.
- `Patient` 1-to-many `Appointment`, 1-to-many `Invoice`.
- `Appointment` 1-to-many `Prescription`.
- `Appointment` 0-or-1 `Invoice` (some invoices are standalone — membership fees, lab packages).
- `Prescription` also carries denormalised `patient_id` / `doctor_id` so per-patient prescription lookups don't require a join back through appointments.

### 3.2 Field summary per model

| Model | Fields | Field types |
|---|---|---|
| `Department` | `name · code · is_active · head_doctor_id? · created_at` | String · String · bool · Option\<i64\> · DateTime |
| `Doctor` | `full_name · specialty · department_id · license_no · email · phone · years_experience · is_active · created_at` | 5×String · i64 · i32 · bool · DateTime |
| `Patient` | `full_name · date_of_birth · gender · national_id · phone · email · blood_type · allergies · is_active · created_at` | 7×String · 2×DateTime · bool |
| `Appointment` | `patient_id · doctor_id · scheduled_at · status · reason · notes · duration_minutes · priority · is_active · created_at` | 2×i64 · 2×DateTime · 3×String · 2×i32 · bool |
| `Prescription` | `appointment_id · patient_id · doctor_id · medication · dosage · frequency · duration_days · is_refillable · refills_remaining · notes · created_at` | 3×i64 · 4×String · 2×i32 · bool · DateTime |
| `Invoice` | `invoice_number · patient_id · appointment_id? · amount_cents · currency · status · issued_at · paid_at? · notes · created_at` | 4×String · 2×i64 · Option\<i64\> · 2×DateTime · Option\<DateTime\> |

### 3.3 Type-vocabulary compromises

| Intent | Compromise taken | Fallout in §7 |
|---|---|---|
| Money (`Decimal(10,2)`) | Store cents as `i64` | Invoice list shows `45000` where the user wants `$450.00` |
| Date-only (no time) | `DateTime<Utc>` with midnight time | Patient DOB shown as `1985-07-22T00:00` |
| Enum (`AppointmentStatus`) | `String` with comment-documented allow-list | No compile-time exhaustiveness; admin renders as plain text, not a pill |
| `UUID` / natural-key ID | `String` for `national_id`, `license_no`, `invoice_number` | Fine in practice; indexed unique columns. Not a problem. |

---

## 4. Migrations

One migration per table, in FK-dependency order. Full SQL in `examples/medflow/migrations/`.

| # | File | What it creates |
|---|---|---|
| 0001 | `create_departments.sql` | `departments` + `UNIQUE(code)` + index on `is_active` |
| 0002 | `create_doctors.sql` | `doctors` + FK → `departments(id) ON DELETE RESTRICT` + unique indexes on `license_no`, `email` + composite index `(department_id, is_active)` |
| 0003 | `create_patients.sql` | `patients` + unique indexes on `national_id`, `email` + index on `is_active` |
| 0004 | `create_appointments.sql` | `appointments` + FKs → `patients(id)` and `doctors(id)` both `ON DELETE RESTRICT` + indexes on `scheduled_at`, `(patient_id, scheduled_at)`, `(doctor_id, scheduled_at)`, `status` |
| 0005 | `create_prescriptions.sql` | `prescriptions` + FK → `appointments(id) ON DELETE CASCADE` + FKs → `patients(id)` and `doctors(id)` both `ON DELETE RESTRICT` + indexes on `appointment_id`, `patient_id` |
| 0006 | `create_invoices.sql` | `invoices` + FK → `patients(id) ON DELETE RESTRICT` + nullable FK → `appointments(id) ON DELETE SET NULL` + unique index on `invoice_number` + composite index `(patient_id, status)` + index on `issued_at` |

Every file begins with `PRAGMA foreign_keys = ON;`. That statement alone is not
enough — see the finding in §7.

### 4.1 FK-enforcement verification

**RustIO sets `foreign_keys=true` at the connection-pool level** (`rustio-core/src/orm.rs:33`).
Every pooled connection comes up with FK enforcement on *before* any SQL runs, so
per-migration `PRAGMA` statements are technically redundant. They're kept in the
files for readers opening the schema in other tools.

**Proof that runtime enforcement is live:**

```
$ sqlite3 app.db "SELECT COUNT(*) FROM doctors WHERE department_id = 1;"
2

$ curl -b "rustio_session=…" -X POST -d "_csrf=…" \
      http://127.0.0.1:8000/admin/departments/1/delete
HTTP/1.1 500 Internal Server Error
```

The DELETE was refused because `ON DELETE RESTRICT` fired. Cardiology still exists
in the DB after the attempt. **That's the good news.** The bad news is what the
admin does with the refusal — see §7, finding #4.

**Gotcha for anyone reading the DB directly:** the `sqlite3` CLI starts sessions
with `PRAGMA foreign_keys = 0` by default, independent of what's baked into the
schema. An interactive `sqlite3 app.db` session can create orphan rows that
RustIO's own connection pool would reject. If you open the DB in the CLI, run
`PRAGMA foreign_keys = ON;` before any write.

---

## 5. Data seeding

One hand-written `seed.sql` populates all six tables in dependency order inside
a single transaction. Row counts are deterministic; dates use `datetime('now', '-N days')`
so the data ages consistently regardless of when you seed.

| Table | Rows | Notes |
|---|---|---|
| `departments` | 8 | Cardiology, Pediatrics, Neurology, Oncology, Orthopedics, Emergency, Radiology, Pharmacy. Pharmacy has no attached doctor → `head_doctor_id IS NULL`. |
| `doctors` | 10 | 2 per busy department; 1 inactive; years of experience 7–25. |
| `patients` | 40 | Ages 5–82 (DOB spread from 1942–2013), mixed gender (38 female/male + 1 `'other'`), varied blood types, 2 inactive (deceased in the seed narrative), ~20% with non-empty allergies. |
| `appointments` | 120 | 60 `completed` (spread −28 to −1 days), 40 `scheduled` (+1 to +30 days), 10 `cancelled`, 5 `no_show`, 3 `checked_in`, 2 `in_progress`. Mixed priority 2–10. |
| `prescriptions` | 60 | One per completed appointment (1..60). Half refillable. Medications span antibiotics, cardiac, oncology, vaccines, contrast agents. |
| `invoices` | 40 | 30 tied to a completed appointment, 10 standalone. Currencies: USD (33), EUR (1), SAR (1), AED (1), 4 mixed. Statuses: 30 paid, 3 issued, 3 overdue, 2 draft, 1 void, 1 complex. |

Run once against a freshly-migrated DB:

```
sqlite3 app.db < seed.sql
```

**Idempotency:** `seed.sql` has no `DELETE` or `INSERT OR REPLACE`. Running it
twice hits unique-constraint failures on department codes, doctor emails / licenses,
patient national IDs, and invoice numbers. Rebuild the DB first:

```
rm app.db app.db-shm app.db-wal
rustio migrate apply
sqlite3 app.db < seed.sql
```

---

## 6. UI testing notes

Visual tests run in the logged-in admin at `/admin`. Observations here are
post-v7 refresh (operator-scale typography, rust-for-writes-only, sticky thead
on tables, zebra rows).

### 6.1 What works

| Check | Verdict |
|---|---|
| Dashboard model list at `/admin` | All 6 models appear in their three app groups. Row counts next to each model are live. |
| List page rendering, any table | Tables render, zebra alternates ink-50, hover promotes to ink-100, thead sticks under topbar. Visually tight even at 120 rows. |
| CRUD on a flat table | Creating, editing, deleting rows with no FK dependents works end-to-end. |
| Auth + CSRF | Login works, session cookie is `HttpOnly; SameSite=Strict; Max-Age=604800`. CSRF token is required on every write form and enforced. |
| Unique constraint errors on create | Creating a second doctor with the same `email` produces a field-level validation error on the form. |
| Schema freshness | `/admin/schema/reload` rereads `rustio.schema.json` without restart. |

### 6.2 What breaks or hurts

See §7 — that's where the stress test earned its name.

---

## 7. Pain points and limitations

Observed on `examples/medflow/` on RustIO 0.3.1. All claims are backed by
reading the source and/or the admin HTML the server returned.

### 7.1 Critical

These block a clinic operator from using RustIO as a day-to-day tool. They're
not "polish" — they're the reason relational UIs exist.

**1 · FK columns render as raw integers.** On `/admin/appointments`, the
`patient_id` column is literally `<td class="rio-cell-num">11</td>`. No name,
no link. An operator staring at "11 · 8 · 2026-04-16T17:27 · Preventive cardio"
has no idea which patient or which doctor that row belongs to. Every FK column
on every list page has the same problem: you can only read the data if you
memorise the ID↔name mapping, which defeats the purpose of a database.

**2 · No inverse-relation views.** A patient detail page at `/admin/patients/1`
shows only the patient's own columns. It does **not** show that patient's
appointments, prescriptions, or invoices. To answer "what has Ahmed Hassan
been in for?" you have to: memorise his ID (`1`), go to `/admin/appointments`,
scan 120 rows visually for `patient_id` cells containing `1`. There is no
`?patient_id=1` filter in the UI — you'd have to craft the URL yourself and
hope the server parses it (it doesn't; see #5).

**3 · Search doesn't cross relations.** `/admin/appointments` has a search
box. Typing "Ahmed Hassan" returns zero rows. The search field looks at
in-table string columns (`reason`, `notes`) — it does not join to `patients`
or `doctors`. The most common clinic query — "find this patient's next
appointment" — is unanswerable from the appointments list.

**4 · FK-constraint violations return `500 Server error`.** RustIO's pool
correctly enforces `ON DELETE RESTRICT` at the SQL layer (good). But the
admin's response to the violation is a generic 500 page reading *"The admin
could not complete your request"*. No list of blocking rows ("2 doctors
reference this department: Dr. Ahmed Abdelrahman, Dr. Sherif Gamal"), no
offer to reassign or cascade, no hint that the problem is an FK. The
operator now doesn't know whether their click did something, whether the
server is broken, or whether to call IT.

**5 · No facet filtering.** The appointments list has a domain-filter
`<select>` on status (`scheduled / completed / …`). That's it. There is
no built-in facet for `doctor_id`, `patient_id`, or date range. Users
expecting "show me today's schedule for Dr. Saleh" have to go find another
way to answer the question. Query-string overrides would need per-model
admin configuration that isn't exposed.

### 7.2 Hurts

Daily friction. Not a blocker, but will make users tired.

**6 · Money stores cleanly, renders raw.** `amount_cents = 45000` shows as
`45000` with no currency symbol, no thousand separator, no decimal. The
operator has to mentally divide by 100 on every invoice, every row. `currency`
and `amount_cents` are two separate columns that the admin doesn't combine.

**7 · Dates render as ISO-8601 UTC.** Patient DOB `1985-07-22 00:00:00`
renders as `1985-07-22T00:00`. Appointment times are in UTC. There's no
timezone awareness, no `DD/MM/YYYY` localization, no "2 hours from now"
relative rendering. Readable, but not friendly — a ward clerk reading 120
rows of ISO timestamps will blur out.

**8 · Enum-like Strings don't auto-pill.** `Appointment.status` is a String
with a 6-value allow-list. The admin renders it as plain text. The
`rio-pill-emerald` / `rio-pill-amber` / `rio-pill-rose` CSS classes exist
for exactly this, but there's no way to tell the admin "this String is an
enum, colour it by value." The page ends up visually flat: a completed
appointment looks identical to a no-show.

**9 · Tables with 10+ columns overflow.** `Patient` has 11 visible columns
(`id · full_name · date_of_birth · gender · national_id · phone · email ·
blood_type · allergies · is_active · created_at`). On a 1440×900 laptop,
the row doesn't fit — horizontal scroll kicks in around column 7. There is
no per-user column-selection UI, no way to hide columns, no responsive
collapse. Every column the model declares, the admin shows.

**10 · No pagination threshold.** 120 appointments render on one page.
No "Load more", no "Page 1 of 4", no row-limit selector. On a bigger seed
(10k appointments) this would DOS the server's HTML generator before it
DOSes the browser. On medium data it's "fine" but wasteful.

**11 · Allergies as `NOT NULL DEFAULT ''`.** The admin offers no way to
store `NULL` in a `String` field; empty strings are indistinguishable from
"I don't know." Declaring `Option<String>` on the Rust side would expose
the distinction — but every scaffolded model uses bare `String`, and
there's no visible guidance on when to pick which.

**12 · `full_name` is a single column.** Sortable alphabetically, but you
can't sort by last name (because there's no last-name field). Splitting
into `first_name` / `last_name` is a schema change, not an admin option.
Realistic clinical systems need both forms of the name for different
workflows. RustIO gives you one column; pick your poison.

### 7.3 Breaks quietly (low severity, high confusion)

**13 · The scaffolder writes a `People` model you then throw away.** `rustio new app people` creates `apps/people/models.rs` containing a `People` struct with the same `title / is_active / priority` shape every other scaffolded model has. Building anything real means rewriting that file from scratch. The CLI's hints (`→ rustio migrate apply`) point at the auto-generated migration you also just deleted. First-time users will follow the hint, apply the stub, build the admin, then realise the "Patients" table is actually a "Peoples" table with a `title` column and three other fields. The scaffolder should either warn that it's a starting point or split "add empty app" from "add app with demo model."

**14 · `rustio new app <name>` pluralises `name` naively.** `new app care` produced migration `0002_create_cares.sql` (which is not a word). `new app people` produced `0001_create_peoples.sql`. The scaffolder appends an `s` — and since most people who pick clean app names have already picked a plural or a mass noun, they'll get a weird SQL name they have to fix. Low blast radius (we deleted all scaffolded migrations anyway), but unpolished.

**15 · `apps/mod.rs` marker comments are load-bearing.** `rustio new app <n>`
edits this file in place via `// -- modules --` / `// -- end admin installs --` /
`// -- end view registrations --` markers. Those markers are not documented
outside `CLAUDE.md`. A user reformatting the file, reordering siblings, or
running a linter that strips "unused" comments will silently disable the
scaffolder for their project. Every subsequent `rustio new app` will
succeed at the CLI level but won't show up in the admin.

**16 · Example projects and the workspace conflict.** `cargo build` at
the repo root refuses to compile `examples/medflow/` unless the project
carries an empty `[workspace]` table or the root workspace lists it. There
is no hint of this anywhere in the generated `Cargo.toml`. First-time
contributors who try `cargo run -p medflow` from the workspace root get
the "current package believes it's in a workspace when it's not" cargo
error and have to guess.

---

## 8. Suggested improvements

Technical, factual. No roadmap phases, no priority labels — if you own
RustIO, you already know which are cheap.

**On the model/admin surface:**

- Resolve FK columns to the referenced model's display form in every
  list rendering. The admin already has enough metadata (`rustio.schema.json`)
  to know that `Appointment.patient_id` references `patients`; one join
  and one `impl Display` (or a declared `display_field`) turns `11` into
  `"Ahmed Hassan"`. Critical for #1.

- Render the target as a link (`<a href="/admin/patients/11">Ahmed Hassan</a>`)
  so the operator can drill into it. Fixes part of #2.

- On the detail page for a model that's referenced by others, list inverse
  relations as "Related" cards: *"3 appointments · 2 invoices · 6 prescriptions"*
  with per-link drill-through. Fixes the rest of #2. Requires a
  `has_many` / inverse-FK declaration in the `Model` trait or the `RustioAdmin`
  derive.

- Let the admin search traverse declared FKs: `search_fields = ["patients.full_name", "doctors.full_name"]`
  on `Appointment`. Fixes #3.

- When a DELETE is refused by a FK `RESTRICT`, catch the SQLite error at
  the admin layer and render a proper error page listing the blocking rows
  ("Cannot delete Cardiology: 2 doctors reference this department") with
  links to each blocker. Fixes #4.

**On the type system:**

- Add a `Decimal` field type (or `Money`) with declared currency. Alternatively,
  let a model declare `#[admin(render_as = "money(amount_cents, currency)")]`
  so the existing `i64 + String` pair displays as `$450.00`. Fixes #6.

- Let a `DateTime` field declare a render locale and whether time-of-day is
  meaningful. Start with ISO for everything except fields annotated
  `#[admin(render_as = "date")]` / `"datetime"` / `"relative"`. Fixes #7.

- Add a lightweight "enum String" declaration on the model:
  `#[admin(variants = "scheduled,checked_in,in_progress,completed,cancelled,no_show", pill = { completed: "emerald", cancelled: "rose", no_show: "amber" })]`.
  Fixes #8. Avoids shipping a new field type.

- Separate `String` from `Option<String>` more visibly in the scaffolder's
  docstring and in the admin. Today both render identically; the nullable
  case loses information. Fixes #11.

**On the list page:**

- Declare default-visible columns and expose a per-user column-selection
  popover. Fixes #9.

- Paginate by default once a list has >25 rows, exposing a selector for
  50 / 100 / 250 / 500 page size. Fixes #10.

- Add facet filters derived from FKs on the model. `Appointment` → facets
  for `patient_id`, `doctor_id`, `status`, with the same rendering as the
  existing status filter. Fixes #5.

**On the scaffolder:**

- `rustio new app <name>` should offer two modes: `--with-model` (current
  behaviour) and `--empty` (no model, no views, just the module structure
  + marker entries in `apps/mod.rs`). Fixes #13 for real projects.

- Pluralisation should be user-controllable:
  `rustio new app patients --model Patient --table patients` with sensible
  defaults. No more `cares` / `billings`. Fixes #14.

- Document the `apps/mod.rs` marker protocol in the generated file itself
  (a header comment saying "// These markers are consumed by `rustio new app`.
  Removing or renaming them will break the scaffolder."). Fixes #15.

- Generated `Cargo.toml` should include the empty `[workspace]` table by
  default for projects that live inside a larger workspace, or at least
  produce a clearer error when the conflict shows up. Fixes #16.

---

## Appendix — measured numbers

- Fresh `cargo build` of `examples/medflow/` against local `rustio-core`:
  ~60 seconds on a 2021 M1 MacBook Pro.
- `rustio migrate apply` over 6 migrations: <200 ms.
- `sqlite3 app.db < seed.sql`: <50 ms for 278 rows across 6 tables.
- Rendering `/admin/appointments` (120 rows, 11 columns): **208 KB of HTML**
  in one response. No gzip at the framework level. At 1000 rows this
  linearly becomes ~1.7 MB per list request.
- Login POST → session cookie: single round-trip, 303 redirect, cookie
  `HttpOnly; SameSite=Strict; Max-Age=604800`.
