# MedFlow — Design Export Package

Standalone, product-grade HTML pages for the hospital operations console. This package is meant to be handed to a designer for external refinement; when the files come back, they map 1:1 to live routes in the Rust backend.

- **No framework**: plain HTML + one shared CSS file + one tiny JS file.
- **Theme**: light + dark via `data-theme="dark"` on `<html>`. A toggle button lives in the sidebar / login screen and persists the choice in `localStorage`.
- **Palette**: carried over from the Smarty / HuntKits theme at `/Users/mansour/Documents/smarty/frontend/examples/frontend`.
  - Primary `#574fec` (indigo-purple)
  - Text `#1c0950`, bluish gray scale
  - Shadows deliberately **toned down** versus the source (user brief).
- **Font**: Inter (loaded from Google Fonts).

---

## File map

Each file is **standalone** (sidebar + topbar + content duplicated on each page so the designer can open any one in a browser without needing a server).

| File                         | Live route                              | Backend handler                                      |
| ---------------------------- | --------------------------------------- | ---------------------------------------------------- |
| `dashboard.html`             | `GET /` or `GET /ops/dashboard` (new)   | *not yet wired — stub for the home screen*           |
| `appointments-list.html`     | `GET /ops/appointments`                 | `apps/care/views.rs::ops_list`                       |
| `appointment-new.html`       | `GET /ops/appointments/new`             | `apps/care/views.rs::ops_new`                        |
| `appointment-edit.html`      | `GET /ops/appointments/:id/edit`        | `apps/care/views.rs::ops_detail`                     |
| `patients-list.html`         | `GET /ops/patients` *(new route)*       | to be added alongside `care::views`                  |
| `patient-new.html`           | `GET /ops/patients/new` *(new route)*   | to be added                                          |
| `patient-edit.html`          | `GET /ops/patients/:id/edit` *(new)*    | to be added                                          |
| `doctors-list.html`          | `GET /ops/doctors` *(new)*              | to be added                                          |
| `doctor-edit.html`           | `GET /ops/doctors/:id/edit` *(new)*     | to be added                                          |
| `invoices-list.html`         | `GET /ops/invoices` *(new)*             | to be added                                          |
| `invoice-detail.html`        | `GET /ops/invoices/:id` *(new)*         | to be added                                          |
| `medical-record-detail.html` | `GET /ops/records/:id` *(new)*          | to be added                                          |
| `login.html`                 | `GET /admin/login`                      | `rustio_core::admin` (framework-owned, themes via CSS only) |

Supporting files:

- `styles.css` — the whole design system (tokens, layout, components, responsive, dark mode).
- `app.js` — tiny utilities (theme toggle, character counters, duration preset/custom reveal).

---

## Which pages are critical vs optional

| Tier        | Files |
| ----------- | ----- |
| **Critical** (live today on the Rust backend) | `appointments-list.html`, `appointment-new.html`, `appointment-edit.html` |
| **Critical** (front door — always the first impression) | `login.html`, `dashboard.html` |
| **Important** (models exist, routes yet to wire) | `patients-list.html`, `patient-edit.html`, `invoices-list.html`, `invoice-detail.html`, `medical-record-detail.html` |
| **Supporting** (round out the workflow) | `patient-new.html`, `doctors-list.html`, `doctor-edit.html` |

---

## What's static layout vs dynamic data

Every page contains an **HTML comment** above every region that the real backend will inject. Example:

```html
<!-- dynamic: appointment rows -->
<tbody>
  <tr>…</tr>
  …
</tbody>
```

### Dynamic zones by page

| Page                         | Dynamic zones                                                                              |
| ---------------------------- | ------------------------------------------------------------------------------------------ |
| `dashboard.html`             | stat numbers, today's schedule rows, recent activity feed, user greeting                   |
| `appointments-list.html`     | stat numbers, row list, active filter chips, pagination                                    |
| `appointment-new.html`       | patient / doctor / department `<select>` options, preview panel (all fields)               |
| `appointment-edit.html`      | lifecycle action buttons (role-dependent), info grid values, timeline events                |
| `patients-list.html`         | stat numbers, patient rows, pagination                                                     |
| `patient-new.html`           | summary preview panel                                                                       |
| `patient-edit.html`          | summary banner, clinical snapshot, upcoming appointment rows, recent records, contacts     |
| `doctors-list.html`          | stat numbers, doctor rows                                                                  |
| `doctor-edit.html`           | profile fields, today's schedule rows, weekly stats                                        |
| `invoices-list.html`         | stat numbers, invoice rows, pagination                                                     |
| `invoice-detail.html`        | line items, payments list (or empty state), summary fields, activity timeline              |
| `medical-record-detail.html` | SOAP sections, diagnoses list, prescriptions list, vitals grid, attachments                |
| `login.html`                 | optional error banner                                                                      |

Anything **not** tagged as dynamic is pure layout — the designer may rearrange, re-style, or replace it.

---

## Expected action buttons / states

### Appointment lifecycle (visible on `appointments-list.html`, `appointment-edit.html`, `dashboard.html`)

The backend enforces a strict state machine:

```
scheduled → confirmed → in_progress → completed
     │          │            │
     └──────────┴────────────┴──→ cancelled
```

Each row / detail page shows **only the buttons the current role is allowed to trigger** for the current status:

| Status        | Receptionist    | Doctor           | Admin (super-role) |
| ------------- | --------------- | ---------------- | ------------------ |
| `scheduled`   | Confirm, Cancel | Cancel           | Confirm, Cancel    |
| `confirmed`   | Check-in, Cancel| Cancel           | Check-in, Cancel   |
| `in_progress` | Cancel          | Complete, Cancel | Complete, Cancel   |
| `completed`   | —               | —                | —                  |
| `cancelled`   | —               | —                | —                  |

### Other role gates

- Only **receptionist** + **admin** see "+ New appointment" and "Register patient".
- Only **billing** + **admin** see "Issue invoice" and "Record payment".

### Empty states

Every table-bearing page has an empty state layout available (search returns nothing or no records at all). For `appointments-list.html` the empty state markup is commented out at the bottom of the file — the designer can surface it by swapping it in.

---

## Reintegration workflow

1. The designer receives `design-export/` as a folder.
2. They refine `styles.css` (tokens, spacing, shadows), tweak markup in any HTML file, and return the same files back.
3. Engineering reads the **updated `styles.css`** and ports it verbatim into `examples/medflow/apps/ui.rs`'s `STYLES` const (it's a single Rust string literal with the CSS body).
4. For each **refined HTML file**, the engineer copies the body structure into the matching Rust renderer in `examples/medflow/apps/care/views.rs` (or the future `patients.rs` / `doctors.rs` / `invoices.rs` modules). The dynamic-zone comments in the HTML mark exactly where a `format!()` insertion goes.
5. The JS file is copied into the shell's inline `<script>` block.

The backend never changes — routing, services, auth, migrations all stay. Only the rendered HTML and its stylesheet are swapped.

---

## How to preview locally

Open any `.html` file directly in a browser. No server needed.

```bash
open design-export/dashboard.html
# or
open design-export/appointments-list.html
```

Navigate between pages via the sidebar (all hrefs are relative, same folder).

Toggle the theme via the button in the sidebar footer (or top-right on the login screen). The choice persists across pages via `localStorage`.

---

## Sample data used

| Entity      | Sample values |
| ----------- | ------------- |
| Patients    | Anna Lindberg, Bengt Johansson, Cecilia Berg, Daniel Ek, Eva Sorensen, Freja Holm, Gustav Lind |
| Doctors     | Dr. Erik Nilsson, Dr. Anna Lindqvist, Dr. Omar Haddad, Dr. Maria Costa, Dr. Peter Andersson |
| Departments | Cardiology, Pediatrics, Radiology, Orthopedics |
| Diagnoses   | I20.9 (angina), E78.5 (hyperlipidaemia), I10 (hypertension) |
| Medications | Metoprolol 50 mg, Aspirin 75 mg |
| Currency    | SEK |
| Locale      | en-GB dates (`YYYY-MM-DD`), 24-hour UTC times |

All data is fictional and chosen to look realistic for a Swedish cardiology / pediatric clinic.

---

## Is this sufficient for a designer?

**Yes.** The package covers:

- Every primary hospital workflow screen (dashboard, appointments, patients, doctors, billing, clinical).
- Both a list layout and a detail/form layout per domain.
- All the common UI primitives — cards, tables, filter bars, stat strips, timelines, avatars, pills, empty states, banners, previews, login panel.
- Light + dark mode out of the box.
- Explicit dynamic zones so the designer never wonders what's static copy vs live data.
- A reintegration recipe so their updated files map back to the Rust backend with no backend change required.

If a screen is missing that the designer wants to cover (e.g. a check-in kiosk board, an availability calendar view, a settings page), they can add it alongside the others using the same shell and the same token set.
