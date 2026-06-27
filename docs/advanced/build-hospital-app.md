> **Advanced docs.** This file goes deep — APIs, internals, gotchas.
> If you'''re new to RustIO, start at the [main README](../../README.md) first.
> It walks you from zero to a running admin in 5 minutes.

# Build a hospital management system

A full four-model CRUD admin — **Departments · Doctors · Patients · Appointments** — in under an hour. Every feature shown is what RustIO generates from your Rust structs. You write models and migrations. RustIO writes the UI.

---

## The final result

When you're done, signing in at `http://127.0.0.1:8000/admin` lands you on a dashboard listing four models. The hero page is **Appointments** — everything a scheduler needs, generated from one struct.

**Appointments list page:**

- A small set of default columns: `id`, patient (as a clickable name linked to that patient's page), doctor (same), `scheduled_at`, `status`. The other six fields — `reason`, `notes`, `duration_minutes`, `priority`, `is_active`, `created_at` — are *not* in the table by default. They live behind the row-expansion chevron.
- A toolbar with: full-text search, a **Patient** dropdown (the first FK becomes the primary filter), a **Sort** control, a **Search** button, a **More filters** button, a **Reset** link when anything is active, and a **Columns** menu. A count label ("Showing 12 of 84") sits below.
- Clicking **More filters** drops a panel of secondary controls: **Status** (`scheduled`, `checked_in`, `in_progress`, `completed`, `cancelled`, `no_show`), **Priority**, and the **Doctor** dropdown.
- Every active filter becomes a chip under the toolbar: `Patient: Erik Nilsson ×`, `Status: Scheduled ×`. The `×` is a plain link that rewrites the URL without that one filter; a trailing **Clear all** strips everything.
- Toggle **Columns → notes** off and the column disappears instantly, no reload.
- Click any row's chevron and a panel slides out — actually not slides; there are no animations — revealing `reason`, `notes`, `duration_minutes`, `priority`, `is_active`, `created_at`, each as a read-only field with the same pill/link rendering the table uses.
- Patient and Doctor cells render as `<name> #<id>`, where the name comes from the `display_field` you declared on the FK.

That's the page. Below is how to build it.

---

## 1. Create the project

```bash
cargo install rustio-cli       # skip if you did the quickstart
rustio init clinic --preset basic
cd clinic
```

`--preset basic` gives you an empty project (`apps/mod.rs` with markers, no apps yet). You'll scaffold the apps one at a time.

## 2. Scaffold two apps

We'll group the models by concern:

```bash
rustio new app people     # will hold Department, Doctor, Patient
rustio new app care       # will hold Appointment
```

Each command creates `apps/<name>/{mod.rs, models.rs, admin.rs, views.rs}`, registers the module via the markers in `apps/mod.rs`, and drops a template migration at `migrations/000N_create_<plural>.sql`. The scaffolded model and migration are placeholders — we'll replace them.

⚠️ **Common issues**

- *"not inside a RustIO project"* — run from the project root, not a subdirectory.
- *"app already exists"* — you ran `new app` twice with the same name. Remove `apps/<name>/` and re-run, or pick a different name.

## 3. Write the models

Open `apps/people/models.rs` and replace everything with the three `people` models. The framework has a **strict contract between `#[derive(RustioAdmin)]` and `impl Model`** — the lists of columns must match the struct fields exactly, or you'll get a compile or runtime error.

### `apps/people/models.rs`

```rust
use chrono::{DateTime, Utc};
use rustio_core::{Error, Model, Row, RustioAdmin, Value};

#[derive(Debug, RustioAdmin)]
pub struct Department {
    pub id: i64,
    pub name: String,
    pub code: String,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
}

impl Model for Department {
    const TABLE: &'static str = "departments";
    const COLUMNS: &'static [&'static str] =
        &["id", "name", "code", "is_active", "created_at"];
    const INSERT_COLUMNS: &'static [&'static str] =
        &["name", "code", "is_active", "created_at"];

    fn id(&self) -> i64 { self.id }

    fn from_row(row: Row<'_>) -> Result<Self, Error> {
        Ok(Self {
            id: row.get_i64("id")?,
            name: row.get_string("name")?,
            code: row.get_string("code")?,
            is_active: row.get_bool("is_active")?,
            created_at: row.get_datetime("created_at")?,
        })
    }

    fn insert_values(&self) -> Vec<Value> {
        vec![
            self.name.clone().into(),
            self.code.clone().into(),
            self.is_active.into(),
            self.created_at.into(),
        ]
    }
}

#[derive(Debug, RustioAdmin)]
pub struct Doctor {
    pub id: i64,
    pub full_name: String,
    pub specialty: String,
    #[rustio(belongs_to = "Department", display = "name")]
    pub department_id: i64,
    pub license_no: String,
    pub email: String,
    pub phone: String,
    pub years_experience: i32,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
}

impl Model for Doctor {
    const TABLE: &'static str = "doctors";
    const COLUMNS: &'static [&'static str] = &[
        "id", "full_name", "specialty", "department_id", "license_no",
        "email", "phone", "years_experience", "is_active", "created_at",
    ];
    const INSERT_COLUMNS: &'static [&'static str] = &[
        "full_name", "specialty", "department_id", "license_no",
        "email", "phone", "years_experience", "is_active", "created_at",
    ];

    fn id(&self) -> i64 { self.id }

    fn from_row(row: Row<'_>) -> Result<Self, Error> {
        Ok(Self {
            id: row.get_i64("id")?,
            full_name: row.get_string("full_name")?,
            specialty: row.get_string("specialty")?,
            department_id: row.get_i64("department_id")?,
            license_no: row.get_string("license_no")?,
            email: row.get_string("email")?,
            phone: row.get_string("phone")?,
            years_experience: row.get_i32("years_experience")?,
            is_active: row.get_bool("is_active")?,
            created_at: row.get_datetime("created_at")?,
        })
    }

    fn insert_values(&self) -> Vec<Value> {
        vec![
            self.full_name.clone().into(),
            self.specialty.clone().into(),
            self.department_id.into(),
            self.license_no.clone().into(),
            self.email.clone().into(),
            self.phone.clone().into(),
            self.years_experience.into(),
            self.is_active.into(),
            self.created_at.into(),
        ]
    }
}

#[derive(Debug, RustioAdmin)]
pub struct Patient {
    pub id: i64,
    pub full_name: String,
    pub date_of_birth: DateTime<Utc>,
    pub national_id: String,
    pub phone: String,
    pub email: String,
    pub blood_type: String,
    pub allergies: String,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
}

impl Model for Patient {
    const TABLE: &'static str = "patients";
    const COLUMNS: &'static [&'static str] = &[
        "id", "full_name", "date_of_birth", "national_id", "phone", "email",
        "blood_type", "allergies", "is_active", "created_at",
    ];
    const INSERT_COLUMNS: &'static [&'static str] = &[
        "full_name", "date_of_birth", "national_id", "phone", "email",
        "blood_type", "allergies", "is_active", "created_at",
    ];

    fn id(&self) -> i64 { self.id }

    fn from_row(row: Row<'_>) -> Result<Self, Error> {
        Ok(Self {
            id: row.get_i64("id")?,
            full_name: row.get_string("full_name")?,
            date_of_birth: row.get_datetime("date_of_birth")?,
            national_id: row.get_string("national_id")?,
            phone: row.get_string("phone")?,
            email: row.get_string("email")?,
            blood_type: row.get_string("blood_type")?,
            allergies: row.get_string("allergies")?,
            is_active: row.get_bool("is_active")?,
            created_at: row.get_datetime("created_at")?,
        })
    }

    fn insert_values(&self) -> Vec<Value> {
        vec![
            self.full_name.clone().into(),
            self.date_of_birth.into(),
            self.national_id.clone().into(),
            self.phone.clone().into(),
            self.email.clone().into(),
            self.blood_type.clone().into(),
            self.allergies.clone().into(),
            self.is_active.into(),
            self.created_at.into(),
        ]
    }
}
```

Update `apps/people/admin.rs` to install all three:

```rust
use rustio_core::admin::Admin;
use super::models::{Department, Doctor, Patient};

pub fn install(admin: Admin) -> Admin {
    admin
        .model::<Department>()
        .model::<Doctor>()
        .model::<Patient>()
}
```

### `apps/care/models.rs` — the hero model

The `#[rustio(belongs_to)]` targets must be in scope at the derive site, so bring them up from the `people` app:

```rust
use chrono::{DateTime, Utc};
use rustio_core::{Error, Model, Row, RustioAdmin, Value};

use crate::apps::people::models::{Doctor, Patient};

#[derive(Debug, RustioAdmin)]
pub struct Appointment {
    pub id: i64,
    #[rustio(belongs_to = "Patient", display = "full_name")]
    pub patient_id: i64,
    #[rustio(belongs_to = "Doctor", display = "full_name")]
    pub doctor_id: i64,
    pub scheduled_at: DateTime<Utc>,
    pub status: String,
    pub reason: String,
    pub notes: String,
    pub duration_minutes: i32,
    pub priority: i32,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
}

impl Model for Appointment {
    const TABLE: &'static str = "appointments";
    const COLUMNS: &'static [&'static str] = &[
        "id", "patient_id", "doctor_id", "scheduled_at", "status",
        "reason", "notes", "duration_minutes", "priority",
        "is_active", "created_at",
    ];
    const INSERT_COLUMNS: &'static [&'static str] = &[
        "patient_id", "doctor_id", "scheduled_at", "status",
        "reason", "notes", "duration_minutes", "priority",
        "is_active", "created_at",
    ];

    fn id(&self) -> i64 { self.id }

    fn from_row(row: Row<'_>) -> Result<Self, Error> {
        Ok(Self {
            id: row.get_i64("id")?,
            patient_id: row.get_i64("patient_id")?,
            doctor_id: row.get_i64("doctor_id")?,
            scheduled_at: row.get_datetime("scheduled_at")?,
            status: row.get_string("status")?,
            reason: row.get_string("reason")?,
            notes: row.get_string("notes")?,
            duration_minutes: row.get_i32("duration_minutes")?,
            priority: row.get_i32("priority")?,
            is_active: row.get_bool("is_active")?,
            created_at: row.get_datetime("created_at")?,
        })
    }

    fn insert_values(&self) -> Vec<Value> {
        vec![
            self.patient_id.into(),
            self.doctor_id.into(),
            self.scheduled_at.into(),
            self.status.clone().into(),
            self.reason.clone().into(),
            self.notes.clone().into(),
            self.duration_minutes.into(),
            self.priority.into(),
            self.is_active.into(),
            self.created_at.into(),
        ]
    }
}
```

And `apps/care/admin.rs`:

```rust
use rustio_core::admin::Admin;
use super::models::Appointment;

pub fn install(admin: Admin) -> Admin {
    admin.model::<Appointment>()
}
```

**Relationship notes.** The order of `belongs_to` fields matters. RustIO promotes the *first* `belongs_to` to the primary toolbar filter and parks the rest in the **More filters** panel. Here `patient_id` comes first, so the primary dropdown on `/admin/appointments` is **Patient**; **Doctor** appears inside More filters. Swap the two fields if you want the opposite.

`display = "full_name"` tells the admin which column to read from the target table when rendering the FK in list cells and in dropdowns. If you omit `display`, cells render as `#<id>` and the filter falls back to a numeric input.

⚠️ **Common issues**

- *`cannot find type 'Patient' in this scope`* — missing `use crate::apps::people::models::{Doctor, Patient};` at the top of `apps/care/models.rs`.
- *`the trait bound 'Doctor: Model' is not satisfied`* — you derived `RustioAdmin` but didn't write `impl Model for Doctor`. Both are required. The derive handles the UI; the `Model` impl handles the DB round-trip.
- *Column list out of sync* — `COLUMNS` must list every struct field; `INSERT_COLUMNS` lists every field except `id`. A mismatch is the most common source of runtime SELECT/INSERT errors.

## 4. Write the migrations

`rustio new app <name>` already created placeholder migrations. Replace their contents — **file names stay**. Column names and types must match what `from_row` expects.

### `migrations/0001_create_people.sql`

```sql
PRAGMA foreign_keys = ON;

CREATE TABLE departments (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    name        TEXT    NOT NULL,
    code        TEXT    NOT NULL,
    is_active   INTEGER NOT NULL DEFAULT 1,
    created_at  TEXT    NOT NULL DEFAULT '1970-01-01 00:00:00'
);
CREATE UNIQUE INDEX idx_departments_code ON departments (code);

CREATE TABLE doctors (
    id                INTEGER PRIMARY KEY AUTOINCREMENT,
    full_name         TEXT    NOT NULL,
    specialty         TEXT    NOT NULL,
    department_id     INTEGER NOT NULL,
    license_no        TEXT    NOT NULL,
    email             TEXT    NOT NULL,
    phone             TEXT    NOT NULL,
    years_experience  INTEGER NOT NULL DEFAULT 0,
    is_active         INTEGER NOT NULL DEFAULT 1,
    created_at        TEXT    NOT NULL DEFAULT '1970-01-01 00:00:00',
    FOREIGN KEY (department_id) REFERENCES departments (id) ON DELETE RESTRICT
);
CREATE UNIQUE INDEX idx_doctors_license ON doctors (license_no);
CREATE INDEX        idx_doctors_dept    ON doctors (department_id, is_active);

CREATE TABLE patients (
    id             INTEGER PRIMARY KEY AUTOINCREMENT,
    full_name      TEXT    NOT NULL,
    date_of_birth  TEXT    NOT NULL,
    national_id    TEXT    NOT NULL,
    phone          TEXT    NOT NULL,
    email          TEXT    NOT NULL,
    blood_type     TEXT    NOT NULL,
    allergies      TEXT    NOT NULL DEFAULT '',
    is_active      INTEGER NOT NULL DEFAULT 1,
    created_at     TEXT    NOT NULL DEFAULT '1970-01-01 00:00:00'
);
CREATE UNIQUE INDEX idx_patients_national_id ON patients (national_id);
```

### `migrations/0002_create_care.sql`

```sql
PRAGMA foreign_keys = ON;

CREATE TABLE appointments (
    id                INTEGER PRIMARY KEY AUTOINCREMENT,
    patient_id        INTEGER NOT NULL,
    doctor_id         INTEGER NOT NULL,
    scheduled_at      TEXT    NOT NULL,
    status            TEXT    NOT NULL DEFAULT 'scheduled',
    reason            TEXT    NOT NULL DEFAULT '',
    notes             TEXT    NOT NULL DEFAULT '',
    duration_minutes  INTEGER NOT NULL DEFAULT 30,
    priority          INTEGER NOT NULL DEFAULT 5,
    is_active         INTEGER NOT NULL DEFAULT 1,
    created_at        TEXT    NOT NULL DEFAULT '1970-01-01 00:00:00',
    FOREIGN KEY (patient_id) REFERENCES patients (id) ON DELETE RESTRICT,
    FOREIGN KEY (doctor_id)  REFERENCES doctors  (id) ON DELETE RESTRICT
);
CREATE INDEX idx_appointments_when     ON appointments (scheduled_at);
CREATE INDEX idx_appointments_status   ON appointments (status);
```

`status` is kept as a plain `TEXT` column. RustIO's admin reads the distinct values at request time and builds the filter dropdown from whatever statuses are in the table right now — so the first time you load `/admin/appointments` with no data, the status filter is absent; it appears the moment you save an appointment with any status value.

## 5. Apply migrations, create a user, run the server

```bash
rustio migrate apply
rustio user create --email you@example.com --password secret --role admin
rustio run
```

⚠️ **Common issues**

- *`no such table: appointments`* — `rustio migrate apply` didn't run, or it ran before you updated the migration SQL. If you edited a migration *after* applying it, the run is already recorded. Delete `app.db`, then re-run `migrate apply`.
- *`FOREIGN KEY constraint failed`* — you inserted an appointment with a `patient_id` or `doctor_id` that doesn't exist. Create patients and doctors first.
- *Server prints `serving on http://127.0.0.1:8000` but `/admin` 404s* — `apps/mod.rs` lost its marker comments or its `admin.model::<T>()` calls. The markers (`// -- modules --`, `// -- end modules --`, etc.) must stay exactly as the CLI wrote them; the `register_app_in_mod` rewriter looks for them literally.

## 6. Open the admin

Go to `http://127.0.0.1:8000/admin`. Sign in. You'll see four cards: Departments, Doctors, Patients, Appointments.

Click into **Departments** and add a couple (e.g. `Cardiology` / `CARD`, `Radiology` / `RAD`). Then **Doctors** — the `department_id` form field renders as a dropdown populated from the `Department` rows you just created, showing their `name`. Then **Patients**. Then **Appointments**: both `patient_id` and `doctor_id` are FK dropdowns, populated from the live data and labelled by `full_name`.

## 7. Interact with the hero page

Open `/admin/appointments`. Create six or seven records with mixed statuses (`scheduled`, `checked_in`, `completed`, `cancelled`) and varied doctors/patients. Now play:

### FK dropdown + primary filter

The top-of-toolbar **Patient** dropdown lists every patient by `full_name`. Pick one → the list filters to that patient's appointments and a chip `Patient: <name> ×` appears below the toolbar. Remove the chip → full list returns, sort preserved.

### Secondary filters (More filters panel)

Click **More filters**. The panel contains **Status** (populated from the distinct values you've saved), **Priority**, and **Doctor**. Pick `Status: Scheduled` and click **Search**. Two chips now: `Patient: …` and `Status: Scheduled`. The count label updates: "Showing 3 of 18".

### Columns toggle (no reload)

Click **Columns**. You'll see every field listed — `id`, `patient_id`, `doctor_id`, `scheduled_at`, `status` are checked; `reason`, `notes`, `duration_minutes`, `priority`, `is_active`, `created_at` are unchecked. Uncheck `status` → the column vanishes instantly. The filter chip for Status stays visible — hiding a column is a view concern, not a filter one.

### Row expansion

Click the chevron (▸) in the first column of any row. A panel opens inline showing the six fields that aren't in the default columns: `reason`, `notes`, `duration_minutes`, `priority`, `is_active`, `created_at`. Each value uses the same rendering the table would — timestamps formatted, booleans as pills, numbers right-aligned. Click the chevron (now ▾) to close. Expansion state doesn't persist across page loads — that's intentional.

### URL is the source of truth

Copy the URL while filters are active:

```
/admin/appointments?q=&patient_id=3&status=scheduled&sort=newest
```

Share it, bookmark it, paste it into a runbook. The page reloads the exact same filtered view. **Clear all** in the chip row strips every filter and jumps to `/admin/appointments`.

---

## What you wrote vs what RustIO generated

**You wrote:** ~220 lines across `apps/people/models.rs` and `apps/care/models.rs`, plus two SQL migrations (~55 lines each), plus four lines of `admin.model::<T>()`.

**RustIO generated:**

- 8 routes per model (list, create GET/POST, detail, edit GET/POST, delete GET/POST, bulk) = 32 routes
- Every form, with correct input types per field (text, checkbox, number, `datetime-local`, FK dropdown)
- CSRF tokens on every mutating form
- Search, primary + secondary filters, chips, columns toggle, sort, pagination counts, bulk delete
- Row expansion for every field you didn't need in the default columns
- FK-aware `ON DELETE RESTRICT` protection surfaced as a 409-style "can't delete — referenced by X" in the admin
- `rustio.schema.json` — run `rustio schema` and hand it to the AI layer: `rustio ai plan "add diagnosis text to appointments" --save p.json`

---

## What's next

The bookflow example in `examples/bookflow/` extends this same pattern across several apps — customers, bookings, resources, schedules, invoices, and more — each adding its own models. The shape is identical — one Rust struct, one `Model` impl, one migration, one `admin.model::<T>()` line. You've already seen everything the admin layer does; adding models is additive.

When you're ready to evolve the schema without hand-editing: `rustio ai plan "..." --save p.json && rustio ai review p.json && rustio ai apply p.json --yes`. See `demo-walkthrough.md` for why the planner/executor split exists and what it refuses to do.
