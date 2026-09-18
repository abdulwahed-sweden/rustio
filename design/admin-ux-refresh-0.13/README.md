# RustIO Admin — UX Refresh 0.13

A design handoff for the RustIO admin. **Seven screens, redrawn inside the existing
visual system.** Nothing here proposes a new look: every colour, font, size, radius
and control height is read straight out of `rustio-core/assets/static/admin.css`.
What changed is layout, interaction and information architecture.

Source of truth for the drawings: `boards/` — open `boards/index.html` in a browser.
Live canvas (owner only): <https://claude.ai/artifact/NdGYxF8v2kK439p3vDRwW8>

---

## The four layers

Read this before touching anything. It is the whole contract of this handoff.

| Layer | Status | What it covers |
|---|---|---|
| **Foundation** | 🔒 **Locked** | Palette, font stack, type and heading scale, radii, 240px rail, control density, status colours. Inherited from `admin.css`, not up for discussion. See `TOKENS.md`. |
| **Component** | Inherited | Buttons, fields, badges, cards, tables, pagination stay as they are. Four additions, assembled from existing parts: selection bar, sortable column head, row skeleton, role option card. |
| **Layout** | ✅ Redesigned | Toolbar in two rows / three zones; pagination gains rows-per-page; cards carry Title / Subtitle / Badge / Timestamp; the edit form groups fields and pins its actions. |
| **Interaction** | ✅ Redesigned | Active filters render as filled fields; sorting moves into the column head; multi-select with bulk actions; live preview in the view composer. |
| **Architecture** | ✅ Redesigned | The view composer becomes a two-pane editor instead of a grid of selects. |

**Do not** introduce a new font, a new accent colour, a new neutral, a larger type
scale, a wider rail, or taller rows. If a change seems to need one of those, it is
out of scope — raise it instead of shipping it.

---

## What is in here

```
design/admin-ux-refresh-0.13/
├── README.md          ← this file: scope, handoff prompt, implementation map
├── SPEC.md            ← screen by screen, what to build and where
├── TOKENS.md          ← the locked visual contract, with the admin.css line it comes from
└── boards/
    ├── index.html     ← contact sheet — start here
    ├── Main.html            Overview (dashboard)
    ├── Records-Table.html   List — table layout
    ├── Records-Cards.html   List — cards layout
    ├── Record-Form.html     Edit record
    ├── View-Editor.html     View composer
    ├── Sign-In.html         Sign in
    └── Foundations.html     The inherited system + states + responsive behaviour
```

The boards are plain static HTML with inline styles — no build step, no framework,
no JavaScript. They are **mockups, not components**: read them for structure,
spacing and copy, then express the result in `minijinja` templates and `admin.css`
the way the rest of the admin is written. Do not paste inline styles into the
templates; the admin renders data-only contexts and keeps its CSS in one file.

The boards use the Bookflow sample project (bookings, assignments, resources) for
realistic content. The field names are illustrative, not a schema to implement.

---

## Handing this to Claude Code

Open a session at the repository root and paste:

> Read `design/admin-ux-refresh-0.13/README.md`, then `TOKENS.md` and `SPEC.md`,
> then open the boards under `design/admin-ux-refresh-0.13/boards/`.
> Implement the **List — table** screen first: the two-row toolbar, the active-filter
> treatment, the sortable column head, multi-select with the bulk bar, and the new
> pagination footer. Work in `rustio-core/assets/templates/admin/list.html` and
> `rustio-core/assets/static/admin.css` only. Every value must already exist in
> `TOKENS.md` — add no new colour, size, radius or font. Run
> `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`
> and `cargo test --workspace --all-targets` before you finish, and show me the diff
> for the CSS separately from the template.

Then go screen by screen in the order under **Implementation map**. One screen per
change is deliberate: each is independently reviewable and independently revertable.

---

## Implementation map

| # | Screen | Board | Touches | Notes |
|---|---|---|---|---|
| 1 | List — table | `Records-Table.html` | `templates/admin/list.html`, `admin.css` | Largest win, no Rust changes needed for the toolbar and pagination. Multi-select needs a bulk-action POST route. |
| 2 | List — cards | `Records-Cards.html` | `templates/admin/list.html` (the `cards` branch), `admin.css` | Needs the field *roles* below to reach the template context. |
| 3 | Edit record | `Record-Form.html` | `templates/admin/form.html`, `admin.css` | Field grouping is presentational; the sensitive/PII marker already exists in `admin::intelligence`. |
| 4 | Overview | `Main.html` | `templates/admin/dashboard.html`, `admin.css` | Recent-actions table comes from `admin::audit`; schema-health numbers from `admin::schema_cache`. |
| 5 | View composer | `View-Editor.html` | `templates/admin/view_editor.html`, `admin/mod.rs`, `admin.css` | **The only item needing real Rust work** — see below. |
| 6 | Sign in | `Sign-In.html` | `templates/auth/login.html` | Small: inline error under the field, and the card owning the brand block. |
| 7 | States | `Foundations.html` | `admin.css`, plus each template's empty branch | Empty / loading / error / forbidden. |

### The one item that is not just templates

The **view composer** (5) currently renders every field as a row of selects. The
board proposes a two-pane editor: the ordered field list on the left, the settings
of the *selected* field on the right. That needs a selected-field parameter on
`GET /admin/<model>/view` and a context that carries one field's full settings
(role, filter, sort, merge target, per-language label, enum value labels) instead of
a flat list. The save contract does not change — the same `ViewSpec` comes back.

If that is more than you want to take on now, ship 1–4, 6 and 7 first. They are
independent of it.

### Field roles

The composer names six roles. `Title`, `Subtitle` and `Timestamp` are new to the
vocabulary; `Badge`, `Meta` and `Hidden` exist today. Adding them means touching
`admin::suggestions` and `admin::intelligence`, which pattern-match on roles — see
the update-site list in `CLAUDE.md` under *The macro ↔ core contract*.

| Role | Renders as |
|---|---|
| `Title` | First column, and the link that opens the record. |
| `Subtitle` | Second line under the title in cards and lists. |
| `Badge` | A short enum, drawn as a coloured pill. |
| `Timestamp` | A date or time, formatted for the reader's locale. |
| `Meta` | Any other value, in a column of its own. |
| `Hidden` | Never rendered; cannot filter or merge. |

---

## Acceptance

A screen is done when:

- It matches its board in structure, spacing and copy at 1440px.
- Every colour, size and radius in the diff appears in `TOKENS.md`.
- `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`
  and `cargo test --workspace --all-targets` are clean.
- The RBAC rules still hold: a missing permission hides the control, and entering
  the URL directly still returns the framework 403 page.
- It degrades the way `Foundations.html` documents at 1040px, 760px and 480px.
- A project overriding that template under its own `templates/` directory still
  renders — the override seams in `CLAUDE.md` are unchanged.
