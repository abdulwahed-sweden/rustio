# Spec — UX Refresh 0.13

Per-screen structure and the product-scope classification for every proposal.
Values live in `TOKENS.md`; the visual references live in `boards/`.

---

## Behavioural boundary

**Production code and its tests are authoritative for behaviour.** This
document is the visual and UX contract only. It does not define, and must not
be read as redefining, any of the following:

| Contract | Stays as shipped |
|---|---|
| Canonical form route | `GET/POST /admin/<model>/new` |
| Compatibility alias | `GET/POST /admin/<model>/create` — kept for projects generated before the templated engine; its GET renders the legacy form and that form posts back to it |
| Other routes | `/admin/<model>`, `/admin/<model>/<id>/edit`, `/<id>/delete`, `/view`, `/layout` |
| Auth and sessions | Login, logout, session cookie |
| CSRF | Hidden `_csrf` on every mutating form; a POST without it is rejected |
| RBAC | No view permission → the model never appears. No create → no Add button and 403 by URL. No edit/delete → row actions absent. A 403 is never softened into a 404. |
| Database, migrations | Unchanged |
| PRG | POST → 303 → list, on create and on edit |
| Unknown record | 404 on both GET and POST |
| ViewSpec semantics | The six roles, the per-layout role sets, the never-empty-row fallback, the declared-relation identity derivation, the save path |
| Page size | Fixed page-size contract |
| Template seams | A project may still override any file under `templates/admin/…`, `auth/…`, `includes/…` and the two base templates |

**If a board appears to conflict with current production behaviour, the code
wins and the conflict is documented here rather than resolved by changing the
code.** No conflict is known at the time of freezing.

---

## Product-scope classification

| Status | Meaning |
|---|---|
| **EXISTING BEHAVIOUR** | Already true of the shipped admin |
| **VISUAL / UX CHANGE ONLY** | Presentation only; no handler, context, route or query change |
| **REQUIRES EXISTING BACKEND WIRING** | Uses a capability the runtime already has; needs the context or template wired to it |
| **FUTURE PRODUCT FEATURE** | Not in this implementation |

### Explicit classifications

| Item | Status | Note |
|---|---|---|
| Sortable column headers | **REQUIRES EXISTING BACKEND WIRING** | Uses the **existing `sort` / `dir` contract** and `ColumnView.sortable`. The header builds the same URL the current control builds. A second presentation of the existing sort — **not a second sorting system**. Columns with `sortable = false`, including every merged column, render as plain text. |
| Individually removable active filters | **REQUIRES EXISTING BACKEND WIRING** | Where needed. The active filter state and `clear_filters_href` already exist; each constraint needs one href that drops a single parameter and keeps the rest, from the same query-string builder that produces the layout links. |
| Dashboard activity panel | **REQUIRES EXISTING BACKEND WIRING** | Audit log, its query and `/admin/actions` already exist; the dashboard context needs the latest N entries. |
| Form field grouping | **REQUIRES EXISTING BACKEND WIRING** | Derived from the readonly flag, the primary key, and declared `belongs_to`. A group key per field; the template renders the bands. |
| Validation summary count | **REQUIRES EXISTING BACKEND WIRING** | Counted from errors the renderer already attaches per field. |
| View Editor live preview | **REQUIRES EXISTING BACKEND WIRING** | One sample row in the page context plus the role sets; the client re-renders from form state. No new endpoint, no ViewSpec change, nothing persisted until Save view. |
| Bulk selection | **FUTURE PRODUCT FEATURE** | No multi-id endpoint, no CSRF-protected bulk route, no audit shape for a batch. |
| Bulk actions | **FUTURE PRODUCT FEATURE** | As above. |
| Rows-per-page | **FUTURE PRODUCT FEATURE** | The runtime has a fixed page-size contract. A selector needs a page-size parameter threaded through the query, the pagination builder and every layout href. |
| Per-user saved views | **FUTURE PRODUCT FEATURE** | Needs a per-user ViewSpec store and a resolution order. |
| Inline relation creation | **FUTURE PRODUCT FEATURE** | Needs a nested create contract and a way to return the new id to the parent form. |
| Drag-to-reorder | **FUTURE PRODUCT FEATURE** | ▲▼ stay regardless — they are the keyboard-accessible path. |
| Unsaved-changes guard | **FUTURE PRODUCT FEATURE** | Needs client state tracking and a definition of dirty. |

Future features appear only in clearly separated **Exploration — not approved**
blocks, drawn dashed and desaturated. None appears as a current implementation
target on any approved board.

---

## Records — Table

The approved hierarchy. Five stages, never mixed.

```
1  PAGE IDENTITY   breadcrumb · page title · short description │ Add (alone, only blue)
   ───────────────────────────────────────────────────────────────────────────────
2  FIND / NARROW   Search · Status filter · Resource filter · Clear filters → count right
   ───────────────────────── full-width 1px divider ─────────────────────────────
3  VIEW            Table | List | Cards | Compact · "Sorted by …" → Set as default · Edit view
   ───────────────────────────────────────────────────────────────────────────────
4  DATA            BOOKING · RESOURCE · STATUS · ACCEPTED AT · ACTIONS
5  PAGINATION      Showing 1–20 of N │ Previous · page numbers · Next
```

### 1 · Page identity

Breadcrumb 13/600, 24px title, then a 16px description line. The right side
holds the **primary record action and nothing else** — no layout control, no
filter, no view-editor action stands beside Add. It is the only strong blue on
the page.

### 2 · Find / narrow

Left to right: search, filters, clear, then the result count pushed right by
`margin-left: auto` so it stays attached to the group it describes and never
sits near Add.

- Search 340px preferred (flexes 220–340), 38px, 15/500, 17px leading icon, and
  a clear control that appears only when the query is non-empty and submits the
  same form with `q` emptied.
- Filters 38px, values 15px. Active filter: border `#7890AF`, fill `#F4F8FD`,
  text `#174578`/600.
- “Clear filters” renders only when at least one constraint is active.
- Result count 14/600 secondary with numerals at `#202733`/700.

### 3 · View / presentation

Layout switch hard left. A passive statement — “Sorted by Accepted at · newest
first”, 14/500 secondary — beside it. View tools pushed right.

**In Table mode sorting lives in the column headers.** There is deliberately no
sort-by dropdown; the sentence reports the state the headers set and must not
compete with the layout switch.

Rows 2 and 3 share one card, separated by a full-width 1px `--border`. Row 3
sits on `--surface-soft` because presentation is the rarer job.

### 4 · Data

Column order tells the record's story left to right: identity → primary related
context → state → time → actions.

For Assignment: **BOOKING · RESOURCE · STATUS · ACCEPTED AT · ACTIONS**.

**Identity leads the row. A technical database id does not.** Service type and
duration ride under the resource as in-cell detail at 14/500 `#3F4A59`, keeping
five columns instead of seven.

- Header 40px, 13/700 mono uppercase `#202733`. Sorted header: background
  `#e9eef6`, label `#171B22`, solid 12px chevron, `aria-sort` set.
- Rows 48px minimum, cells 15/500 `#171B22`, 16px horizontal padding, 1px
  dividers, hover `--surface-soft`, no zebra.
- Identity cell 15/680, blue link, underlined.
- Actions right-aligned and visually subordinate: Edit small secondary, Delete
  small danger. Both carry text labels; neither is icon-only.

### 5 · Pagination

Below the data, inside the table card, on `--surface-head`. Range left;
Previous, page numbers, Next right. 14/600 with 38px targets. Current page takes
the blue-soft treatment. Disabled arrows stay visible and disabled with
`aria-disabled` so the control does not change width between pages.

**No rows-per-page control.**

### Cross-layout consistency

The surrounding hierarchy is identical across Table, List, Cards and Compact:
search and filters, then view controls, then records, then pagination. Only the
record presentation changes. Rearranging the surrounding controls on every
layout switch would make the switch feel like a different page.

There is **no automatic Table → Cards behaviour** at any width. The layout is
whatever the operator chose or the view's stored default; the runtime does not
swap it for them and this design does not claim it does.

---

## Overview

Hero — eyebrow badge, 33/800 headline, one sentence — then a 2.1 : 1 split:
model grid left, activity panel right, 24px gutter. Model grid is a **fixed 3
columns** inside the split (4 without the activity panel); a fixed count instead
of `auto-fit` is what stops a card stranding alone on the last row.

| Proposal | Status |
|---|---|
| Drop the “featured” stat card — magnitude is not priority | VISUAL / UX ONLY |
| Fixed column count for the model grid | VISUAL / UX ONLY |
| Stat number loses the inherited link underline | VISUAL / UX ONLY |
| Recent activity panel | REQUIRES EXISTING BACKEND WIRING |
| Per-card “New record” shortcut | FUTURE |

---

## Records — Cards

Five zones, each a ViewSpec role — no new vocabulary:

| Zone | Role | Treatment |
|---|---|---|
| Headline | `Title` | 16/700 `#171B22`, the link to the record |
| Supporting line | `Subtitle` | 15/500 `#3F4A59`, no label |
| State, top-right | `Badge` | 13/600, `flex:none` so a long title never crushes it |
| Detail pairs | `Meta` | Two-column `<dl>`; 13/700 mono term, 15/500 value |
| Footer | `Timestamp` + actions | 14/500 tabular on `--surface-soft`, actions right |

Grid `repeat(auto-fill, minmax(320px, 1fr))` at 16px. Cards in a row are equal
height; the footer is pinned with `margin-top: auto` so timestamps align.

This is a rearrangement of cells the renderer already emits — the Cards layout
already receives all five roles and currently prints them as identical
label-value rows. **No new data is requested.** Status: VISUAL / UX ONLY.

---

## Record Form

Focused measure; the form card is capped at **728px** (680 field measure + 24px
padding each side). At 1920 the workspace grows around the form — the fields do
not.

Sections are bands inside one card, separated by a full-bleed 1px `--border`,
each headed by a 13/700 mono label: **Identity · Relationships · Details ·
System**. System sits on `--surface-soft`. Rhythm 16px between fields, 24px
section padding, 4px label-to-control. Paired short fields share a two-column
row at desktop and stack below 600px. The action bar is pinned to the bottom of
the card: primary, secondary, then a quiet validation summary pushed right. The
page-head Cancel / Save pair is small secondary so the bottom Save is the only
blue button.

| Proposal | Status |
|---|---|
| Field grouping from readonly / primary-key / declared relation | REQUIRES EXISTING BACKEND WIRING |
| Validation summary count | REQUIRES EXISTING BACKEND WIRING |
| Sticky action bar | VISUAL / UX ONLY |
| Two-column rows for short paired fields | VISUAL / UX ONLY |
| Readonly fields read as readonly | VISUAL / UX ONLY |
| Relation fields render their display value | EXISTING BEHAVIOUR |
| CSRF · PRG · RBAC · unknown-record 404 | EXISTING BEHAVIOUR |
| Unsaved-changes guard · inline relation creation | FUTURE |

---

## View Editor

Two columns, 1.35 : 1, 24px gutter, on the wide measure. Composer left, result
right; the preview column is sticky so it stays in view down a long field list.

The field list is a stack of rows, not a spreadsheet: Order, Field, Role,
Display label. Field identity is two lines — human label 15/680 above the
backend column name 13px mono. Hidden rows render at 55% opacity, still
reorderable. The selected row takes `--blue-soft`. Value labels collapse into
one `<details>` per field. A role × layout matrix documents the renderer's fixed
role sets.

| Proposal | Status |
|---|---|
| Two-column composer with sticky result column | VISUAL / UX ONLY |
| Label over backend name; hidden dimmed; selected tinted | VISUAL / UX ONLY |
| Value labels in collapsible sections | VISUAL / UX ONLY |
| Role × layout matrix | EXISTING BEHAVIOUR (documents `roles_for_layout`) |
| Live preview | REQUIRES EXISTING BACKEND WIRING |
| Filter checkbox gated by role | REQUIRES EXISTING BACKEND WIRING |
| Drag-to-reorder · per-user saved views | FUTURE |

---

## Sign In

Card `min(100%, 420px)`, centred on both axes, 24px padding, 14px radius, full
panel shadow. Brand block left-aligned with the form — one alignment per card.
Fields 16px apart; the submit is full width and the only blue on the page. The
alert sits between the brand block and the form.

| Proposal | Status |
|---|---|
| Error copy names the fix; email preserved; password carries the error | VISUAL / UX ONLY |
| Signed-out confirmation reuses the same card | VISUAL / UX ONLY |
| Autocomplete and input types | EXISTING BEHAVIOUR |
| Caps-lock hint · rate-limit feedback | FUTURE |

---

## Responsive contract

Verified at 1920, 1440, 900 and 390.

| Width | Rule |
|---|---|
| 1920 | Wide-data screens use the 1600px workspace naturally; controls do not stretch to fill it. Search keeps 340px. |
| 1440 | Full desktop composition. |
| 900 | Search and filter controls may wrap; the result count stays visually attached to the filtering group. View-management actions may move to their own line. |
| 390 | Operational groups stack vertically in the same order. Buttons may become full width. Tables scroll inside their own card with the identity column first. |

Global rules:

- No horizontal document overflow at any width.
- **Typography never shrinks to make things fit.** Layout absorbs the
  constraint.
- Table, List, Cards and Compact keep the same surrounding hierarchy.
- No automatic layout switching is claimed, because the runtime does not
  implement it.

---

## Implementation order

Each step is verifiable by screenshot and leaves the admin shippable.

| # | Step | Verify by |
|---|---|---|
| 1 | Typography and control geometry — tokens and component sizes only | Screenshot diff at 1440; computed-style diff elsewhere |
| 2 | CSS-only wins: readonly treatment, dimmed pagination arrows, stat underline, card hover borders | Screenshot diff |
| 3 | List: five-stage order, two operational rows, search icon and clear | List at 1920 / 1440 / 900 / 390 |
| 4 | List: semantic column order, in-cell detail, row action faces | Records — Table at four widths |
| 5 | Two empty states, skeleton loading, table error alert | Force each state |
| 6 | Cards: role-driven hierarchy and footer | `?layout=cards` at four widths |
| 7 | Overview: drop featured, fix the grid | Dashboard at 1920 |
| 8 | Sign-in states | Default, rejected, signed-out |
| 9 | Sortable headers *(first wiring step)* | Click a header; the URL matches the existing sort URL exactly |
| 10 | Individually removable active filters | Apply two, remove one, the other survives |
| 11 | Form grouping, sticky bar, validation summary | Edit form at four widths; an invalid submit |
| 12 | View Editor layout and collapsed value labels | Composer at 1440 and 390 |
| 13 | View Editor live preview *(last, highest risk)* | Change a role; Save view writes byte-identically to before |
| 14 | Dashboard activity panel | With and without audit rows |

Steps 1–8 are presentation only. Steps 9–14 each touch a context builder and
should land separately, each extending the navigation-level test so the new
affordance is covered by something that follows a rendered link rather than a
typed URL.
