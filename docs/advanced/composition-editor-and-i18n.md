> **Advanced docs.** This file goes deep — APIs, internals, gotchas.
> If you're new to RustIO, start at the [main README](../../README.md) first.
> It walks you from zero to a running admin in 5 minutes.

# Composition editor & i18n

The admin list isn't fixed. Every model has a **ViewSpec** — a small presentation
document that decides which fields show, in what order, what role each plays,
which are filters, and what they're called in each language. You edit it from a
single page, and none of it touches your data.

## The ViewSpec, briefly

A ViewSpec lives next to the schema as `<model_snake_case>.view.json`. It controls
how a model's *list* renders: field display order, the role each field plays,
which fields are filters, field merging, the default layout, and i18n labels.

It never changes data or schema. The iron rule: **the backend stays English** —
sources, columns, stored data, and sorting are never translated. The ViewSpec is
presentation only.

The list renders *through* the ViewSpec. A `?layout=` switcher picks
**Table**, **List**, **Cards**, or **Compact**; the chosen layout can be saved as
the model's default via **Set as default** (CSRF-protected).

A minimal `task.view.json` looks like this:

```json
{
  "default_language": "en",
  "filters": ["status", "assignee"],
  "labels": {
    "title":  { "sv": "Titel" },
    "status": { "sv": "Status" }
  },
  "value_labels": {
    "status": {
      "assigned": { "sv": "Tilldelad" },
      "done":     { "sv": "Klar" }
    }
  }
}
```

## The composition editor

Open `/admin/<model>/view`. The page is edit-gated (you need `edit` on the
model). It's one page with one **Save** — the save runs through validate → atomic
write, so a bad edit never half-applies.

### Roles

Each field has a role in the list: **Title**, **Subtitle**, **Badge**,
**Timestamp**, **Meta**, or **Hidden**. Set `title` to Title, `status` to Badge,
`created` to Timestamp, and so on. **Hidden** fields never render.

Open `/admin/task/view`, set `notes` to Hidden, **Save** — the `notes` column
disappears from every layout.

### Order

Reorder fields with the ▲▼ controls. Order in the editor *is* display order in the
list. Move `status` above `title`, **Save**, and `status` leads the row.

### Filters

A checkbox per field marks it as a list filter. Tick `status` and `assignee`,
**Save**, and both gain a control in the list's filter bar (see
[List filtering](#list-filtering) below). **Hidden** fields can't be filters — the
checkbox is unavailable for them.

### Merge

Combine fields into one cell with the **Merge into** select. Point `email` at
`name`, **Save**, and the list shows a single cell joining the values with ` · `
(e.g. `Ada Lovelace · ada@example.com`). Non-anchor members (`email` here) are
removed from the field list while merged; unmerging restores them.

## Internationalisation (i18n)

i18n in RustIO is **display labels only**. Stored data, sorting, and tokens stay
English; only the text a reader sees changes.

### Field labels (the header)

Per-language display labels, keyed by the English field source:
`ViewSpec.labels` is `source → lang → label`. The list **header** renders through
these for the active language. A field with no label falls back to the admin's
humanised English name — so a label-less view looks exactly as it did before.

Edit these in the editor's **Display label** column. With editing language `sv`,
type `Titel` against `title`, **Save**, switch a viewer to Swedish, and the column
header reads "Titel" while the data underneath is untouched.

```json
"labels": {
  "title":    { "sv": "Titel" },
  "assignee": { "sv": "Tilldelad till" }
}
```

### Value labels (enum-like values)

Per-language labels for a field's **stored values**: `ViewSpec.value_labels` is
`source → value → lang → label`. For example status `assigned` → "Tilldelad" in
Swedish. The stored value, the sorting, and the status-pill **colour** all stay
English — only the shown text translates.

The editor auto-discovers candidate values for **status-shaped** fields and for
low-cardinality **String** fields (≤12 distinct values), and offers a label input
per value.

```json
"value_labels": {
  "status": {
    "assigned":    { "sv": "Tilldelad" },
    "in_progress": { "sv": "Pågår" },
    "done":        { "sv": "Klar" }
  }
}
```

### Choosing the editing language

In the editor, a `?lang=<code>` selector switches the language whose labels you're
editing. It's a **GET reload, never a save** — switching languages can never
overwrite another language's labels. A separate **Set as default** checkbox makes
the editing language the view's stored `default_language`.

So: open `/admin/task/view?lang=sv`, fill in Swedish labels, **Save**; switch to
`?lang=en` and the English labels are exactly as you left them.

### Per-user language preference (L4)

Each admin user picks their own UI language. The active render language resolves
as **user preference → the view's `default_language` → `"en"`**. Picking a
preference never mutates any ViewSpec.

A reusable language switcher — showing endonyms like "English" / "Svenska" and
storing ISO codes (`en` / `sv`) — appears in the topbar and at the bottom of the
sidebar. Two users can read the same list in different languages at the same time.

Language codes are ISO 639-1 strings and an open set: `en` and `sv` to start,
extensible from there.

## List filtering

The list toolbar renders one control per field in `ViewSpec.filters`, chosen by
the field's shape:

- **boolean** → a tri-state select (any / true / false)
- **enum-like** (low-cardinality) → a dropdown of distinct values; option labels
  are translated via value labels, but the submitted value is the English token
- **foreign key** → a dropdown of related rows, each shown by the target's display
  label
- **high-cardinality** → a free-text box

Controls **auto-submit on change**, and a **Clear** link resets them. Filters
compose with search, sort, and the layout switcher — they all read from the URL,
so any combination is a shareable link.

## Per-model RBAC + PII masking

List actions are gated on the signed-in user's role: **SuperAdmin** / **Admin**
get full CRUD on app models, **Editor** loses delete, **Viewer** is view-only.
Framework tables are stricter.

PII masking keys off `rustio.context.json`. When the project declares one, shown
sensitive cells (email / phone / personal-id) are masked — a short prefix, the
rest as `•`. Without that context file, **nothing is masked** (no surprises).
Hidden fields are always omitted, masked or not.

## See also

- [`README.md`](../../README.md) — the beginner entry point and the "What's
  shipped" table.
- [`ROADMAP.md`](../../ROADMAP.md) — the three phases and where each release fits.
- [`CHANGELOG.md`](../../CHANGELOG.md) — every visible change, version by version.
