# Screen specifications

> Sizes quoted below follow the amended type scale in `TOKENS.md`
> (Typography Legibility Amendment, 2026-09-18). Nothing functional is under 13px.

Each section: what the board shows, what changes against today's admin, and where it
lands in the codebase. Open the board beside the section as you read.

The shell — 72px top bar, 240px rail, 1120px content measure, footer in the main
column — is unchanged on every screen.

---

## 1. List — table · `boards/Records-Table.html`

Template: `rustio-core/assets/templates/admin/list.html` · CSS: `.toolbar`, `.layout-switch`, `.pagination`

### The toolbar: two rows, three zones

Today the toolbar mixes searching, layout switching and view editing in one wrapping
row, so *find me a record* and *change how records are drawn* sit side by side.

```
┌──────────────────────────────────────────────────────────────────────────────┐
│ [🔍 search 300px] │ Status ▾  Resource ▾  Clear          8 results · 2 filters │
├──────────────────────────────────────────────────────────────────────────────┤
│ [ Table │ List │ Cards │ Compact ]  Sorted by Accepted at, newest first       │
│                                          [ Set as default ]  [ Edit view ]    │
└──────────────────────────────────────────────────────────────────────────────┘
```

- **Row one — narrowing the result set.** Search field (300px, 36px tall, search
  icon inset at 11px), a vertical `#e1e6ed` hairline, then one labelled `<select>`
  per filter, then a `Clear` link. The result count is pushed right, 14px 500
  `--ink-soft`, with the number in `--ink` 700.
- **Full-width hairline** between the rows: `height:1px; margin:12px -16px; background:#e1e6ed`.
- **Row two — changing how the set is drawn.** The layout switch on the left, the
  current sort stated in words beside it, and the two view tools pushed right as
  small secondary buttons. Nothing in this row writes data.
- **An active filter uses the filled-field treatment** (`#7890af` border, `#f4f8fd`
  fill, value in `--blue` 600) — the same pair `admin.css` already gives a filled
  input. That is the whole affordance: a glance tells you the list is narrowed.
  A filter at its default value stays in the empty-field treatment.

### Sorting

Moves out of the toolbar into the column head. The sorted `<th>` gets
`background:#e9eef5`, its label becomes a borderless `<button>` in `--ink` (not
`--ink-soft`), followed by a 12px chevron. The `<th>` carries `aria-sort`.
Every sortable head is a button; only the active one is darkened.

### Multi-select

A 44px checkbox column at the head of each row (16px box, `accent-color: var(--blue)`).
When anything is selected, a bar appears directly above the column heads:
`--blue-soft` fill, `--blue-line` bottom border, 44px tall, holding the count in
`--blue-dark` 700, the bulk actions as 30px buttons, and a `Clear selection` link
pushed right. Selected rows take `--surface-soft`.

Bulk actions are state-changing, so they are POST + CSRF like the logout form.

### Row actions

Unchanged in language — `Edit` and `Delete` as 30px secondary buttons, right
aligned, delete in `--red` text on the white face. Do not swap them for icon-only
buttons; the text labels are what the rest of the admin uses.

### Pagination

Moves inside the table card as a `--surface-head` footer strip with a top rule:
rows-per-page select on the left, `Showing 1–8 of 8` beside it, page buttons and
prev/next chevrons pushed right. Current page is `--blue-soft` with `--blue-line`.
Disabled prev is `#f8fafc` with `#b9c5d3` glyph — present, not removed.

---

## 2. List — cards · `boards/Records-Cards.html`

Template: the `cards` branch of `list.html` · CSS: `.card-grid`

Today every card repeats the same labelled pairs, so nothing in it is a heading and
the grid reads as a wall. The board gives each card the field **roles**:

```
┌─────────────────────────────────────┐
│ Lastbil Volvo FH16        ● Offered │  ← Title (link, 17/700) + Badge
│ Booking BK-2002 · Nordfrakt AB      │  ← Subtitle (15/500, --ink-soft)
│ ─────────────────────────────────── │
│ WINDOW              LOCATION        │  ← Meta pairs, mono 13 label
│ 19 Jun, 16:00–20:00 Göteborg hamn   │
│                                     │
│ 🕐 Offered 19 Jun 17:00 · no reply  │  ← Timestamp, 14/500, --ink-soft
├─────────────────────────────────────┤
│ [Edit] [Delete]               #2002 │  ← --surface-soft footer
└─────────────────────────────────────┘
```

Three columns at 1120px, 16px gap. The card is the standard card — 14px radius,
`--border`, `--shadow-sm`, 16px padding — with a `--surface-soft` action footer
separated by an `#e1e6ed` rule.

The toolbar is the same two-row toolbar as the table, with one difference: sorting
has no column head to live in, so it becomes a labelled `Sort by` select in row two.

---

## 3. Edit record · `boards/Record-Form.html`

Template: `rustio-core/assets/templates/admin/form.html`

- **Two columns**: the form card, and a 320px side column holding *Record*
  (primary key, created, last change), *Your permissions*, and the danger zone.
  Below 760px the side column stacks under the form.
- **Fields are grouped** under mono section labels — *Links*, *State and timing*,
  *Contact and notes* — with two fields per row. Grouping is presentational only;
  the field order still comes from the model.
- **Errors are stated twice**: an alert at the top of the card body saying nothing
  was written, and the rejected field itself in the rejected treatment with the
  reason under it in `--red` 700. Both are already in `admin.css` (`.alert-error`,
  `input[aria-invalid]`, `.error-text`).
- **The enum renders as a segmented control**, not a select — three or fewer
  choices are faster to hit than a dropdown, and it reuses `.layout-switch`
  geometry exactly (34px tall inside a 1px `--border-strong` box, current choice on
  `--blue-soft`).
- **Sensitive fields keep their marker** (`.field-sensitive`, amber, 13px) beside
  the label, with the reason in the hint — this is `rustio.context.json` surfacing,
  not decoration.
- **The action bar is pinned** to the bottom of the card on a `--surface-soft`
  strip: `Save changes` (primary), `Save and add another` (secondary), `Cancel`
  (link), and an unsaved-changes marker pushed right.

---

## 4. Overview · `boards/Main.html`

Template: `rustio-core/assets/templates/admin/dashboard.html`

Keeps the established hero treatment — the neutral badge, the 33px headline with
the accent word, the sentence under it. Then:

- **Stat grid**, four across, unchanged geometry. Each stat gains one line of
  movement under the label (`+18 this week`, `7 unpaid`) in 14px 600, green or
  amber. The largest model keeps the `--blue-soft` featured face.
- **Recent actions** — a real table (When / Actor / Action / Result) from
  `admin::audit`, with the outcome as a badge. This replaces having no audit
  surface on the dashboard at all.
- **Schema health** — migrations applied, export freshness, fields without a label.
  Comes from `admin::schema_cache`; a poisoned cache degrades to "cache empty" and
  the card says so rather than showing zeros.
- **Suggestion** — the highest-confidence item from `admin::suggestions` in a
  `--blue-soft` card with its confidence as a mono chip, one sentence of *why*, and
  `Review plan` / `Dismiss`. It links to the plan, it never applies anything.

---

## 5. View composer · `boards/View-Editor.html`

Template: `rustio-core/assets/templates/admin/view_editor.html` · plus a route change

Today: one wide table where every field is a row of six controls. It works at six
fields and collapses at thirty, and the instructions are a paragraph above it.

Proposed: **live preview on top, two panes under it.**

```
┌──────────────────────────────────────────────────────────────────────────────┐
│ PREVIEW — TABLE LAYOUT              3 columns · 1 filter · 1 merge · 3 hidden │
│ BOOKING          RESOURCE        ACCEPTED AT      STATUS                      │
│ BK-2002 · Nord…  Volvo FH16      19 Jun 17:00     ● Offered                   │
├────────────────────────┬─────────────────────────────────────────────────────┤
│ FIELDS   drag to order │ STATUS   status · enum(3)         Language [ en ▾ ]  │
│ ⠿ Booking      [Title] │ ROLE IN THE LIST                                     │
│ ⠿ Customer  [Subtitle] │ ( )Title  ( )Subtitle  (•)Badge                      │
│ ⠿ Resource      [Meta] │ ( )Timestamp ( )Meta   ( )Hidden                     │
│ ⠿ Accepted [Timestamp] │ BEHAVIOUR              PLACEMENT                     │
│ ⠿ Status       [Badge] │ [x] list filter        Merge into [ — ▾ ]            │
│ ⠿ Note        [Hidden] │ [x] sortable           Label [ Status ]              │
│ ⠿ ID          [Hidden] │ VALUE LABELS — offered / accepted / declined         │
└────────────────────────┴─────────────────────────────────────────────────────┘
```

- **Left pane (372px)** — the ordered field list. Each row: drag handle, field name
  with `name · type` in mono under it, and the current role as a badge. The
  selected field takes the `--blue-soft` treatment. Hidden fields are dimmed to
  `--ink-soft`. A footer line states the rule: *a view shows at most one Title and
  one Badge.*
- **Right pane** — everything about the selected field. The six roles are option
  cards with a one-line explanation each, so the vocabulary is learned in place
  instead of in a paragraph at the top of the page. Then behaviour (filter,
  sortable), placement (merge target, column label), and the enum's value labels
  with each value's pill colour shown beside it.
- **The preview is the first thing on the page**, not the last: it shows the row
  the current settings produce, with the selected field's column highlighted.
- **Language** is a select in the pane header, and the pane footer says which
  languages still have empty labels for this field.

The save contract is unchanged — the same `ViewSpec` is posted back. What changes is
a selected-field parameter on the GET route and a richer context for one field.

---

## 6. Sign in · `boards/Sign-In.html`

Template: `rustio-core/assets/templates/auth/login.html`

The existing 420px card, unchanged in geometry. Two changes: the brand block moves
*inside* the card so the card is the whole of the page's content, and a failed
attempt is reported twice — an `.alert-error` above the fields naming what happened
and what it costs, plus the password field in the rejected treatment with one line
under it. The `Dev` chip and session note sit under the card, outside it.

---

## 7. States · `boards/Foundations.html`

New patterns, assembled from existing tokens. Each template's empty branch and the
error paths need them.

| State | Shape |
|---|---|
| **Empty** | Centred in a card: 40px `--blue-soft` mark, `h3`, one sentence at a 34ch measure, primary action. The existing `.empty` block, with the copy tightened. |
| **Loading** | Row skeletons — three bars per row at `#e1e6ed` / `#eef1f5`, fading on the last row. The page head and toolbar stay put, so nothing jumps when data lands. |
| **Error** | `.alert-error` with a `Try again` button inside it. Says plainly that nothing was written. |
| **Forbidden** | Amber card naming the role you have and the role the action needs. The control stays **visible and disabled** rather than vanishing, so the rule is legible. This is for in-page actions; entering a URL without permission still returns the framework 403 page. |

## Responsive

Three breakpoints, all already in `admin.css`:

- **≥1041px** — rail beside the work area, toolbar keeps both rows.
- **≤1040px** — the identity cluster in the top bar stacks to two lines; filters
  wrap under the search field.
- **≤760px** — the rail becomes a horizontally scrolling strip above the content,
  the detail grid goes to one column, and **the table falls back to the cards
  layout** instead of scrolling sideways.
- **≤480px** — the top bar stacks to three rows; button rows go full width.
