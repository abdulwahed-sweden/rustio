# The visual contract

Two kinds of value live here.

**Inherited and locked** — colour, font stack, radii, rail width, control density,
spacing. These already exist in `rustio-core/assets/static/admin.css`. If this file
and `admin.css` disagree about one of them, `admin.css` wins.

**Amended** — the type scale, raised on **2026-09-18** by the Typography Legibility
Amendment. These values do *not* match `admin.css` yet; implementing them is part of
the work. They are the only intentional divergence in this handoff.

A diff that introduces a value on neither list is out of scope by definition.

---

## The floor rule

> **No functional text in RustIO is smaller than 13px.**
> 11px survives only for a marginal technical annotation, never for a label a user
> has to read. A status badge, a column head, a record count and a rail label are all
> functional text.

This rule is what drove most of the amendment: the old scale put four load-bearing
roles — column heads, rail labels, card labels and badges — at 11–12px.

---

## Type — amended

| Role | Was | **Is** | Colour |
|---|---|---|---|
| Dashboard hero `h1` | 33 / 800 | **33 / 800** | `--ink` |
| Page title `h1` | 24 / 800 | **24 / 800** | `--ink` |
| Section heading `h2` | 17 / 700 | **19 / 700** | `--ink` |
| Card title / `h3` | 15 / 700 | **17 / 700** | `--ink` |
| Body, lead | 15 / 400 | **16 / 500** | `--ink` / `--ink-soft` |
| Table cell, record value | 14 / 400 | **15 / 500** | `--ink` |
| Rail item, button, strong UI text | 14 / 600–700 | **15 / 600** | per control |
| Column head — mono, `.12em`, uppercase | 12 / 700 `--ink-soft` | **13 / 700** | `--ink` |
| Rail label, card head, stat label — mono | 11–11.5 / 700 `#596779` | **13 / 700** | `#334052` |
| Form label | 13 / 500 | **14 / 600** | `--ink` |
| Field hint, helper | 14 / 400 `#4b596b` | **14 / 500** | `--ink-soft` |
| Secondary meta, breadcrumb, counts | 13 / 400 | **14 / 500** | `--ink-soft` |
| Small control, segmented item | 13 / 600–700 | **14 / 600** | per control |
| Badge, chip, marker | 12 / 600 | **13 / 600** | per status |
| Inline code, mono meta | 12–13 / 400 | **14 / 400** | `--ink` / `--ink-soft` |
| Mono value in a cell | 14 / 400 | **15 / 500** | `--ink` |
| Stat value | 33 / 800 | **33 / 800** | `--ink` |

Three consequences follow from the amendment rather than being chosen:

1. **`h3` had to move** from 15 to 17 — at 15 it would have been *smaller* than the
   new 16px body, which inverts the hierarchy. `h2` follows to 19 to clear it.
   `h1` (24) and the hero (33) are unchanged, so the step from `h1` to body is now
   tighter than before. Say so if you want `h1` at 26.
2. **Weight 450 is not used.** `admin.css` ships no webfont — it asks for Inter and
   falls back to `system-ui`. Static and fallback faces snap 450 to 400 or 500, so
   the scale specifies **500**. If Inter variable is ever bundled, 450 for body is
   the more refined choice and it is a one-value change.
3. **Small controls grew from 28px to 30px** to hold 14px text without crowding.
   Everything else keeps its height.

**Stack** — unchanged:
`Inter, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif`
Mono: `ui-monospace, SFMono-Regular, "SF Mono", "JetBrains Mono", Menlo, Consolas, monospace`

---

## Colour — inherited, unchanged

| Token | Value | Used for |
|---|---|---|
| `--page` | `#f1f1ee` | Page ground |
| `--surface` | `#ffffff` | Cards, table body, top bar |
| `--surface-soft` | `#f7f9fc` | Row hover, selected row, card footers |
| `--surface-head` | `#f1f4f8` | Card heads, column heads, pagination strip |
| `--rail-bg` | `#f4f6f9` | Module rail |
| `--rail-line` | `#d3dbe5` | Rail's right edge |
| `--blue` | `#1f5797` | Primary button, links, active rail item |
| `--blue-dark` | `#174578` | Hover, headings inside blue-soft |
| `--blue-soft` | `#dfeafb` | Active nav, selection bar, selected option |
| `--blue-line` | `#b7cdea` | Border on blue-soft surfaces |
| `--ink` | `#202733` | Primary text |
| `--ink-soft` | `#4c5868` | Secondary text |
| `--border` | `#d5dce5` | Card and table rules |
| `--border-strong` | `#b9c5d3` | Control borders |
| `--focus` | `#2f7bd6` | Focus ring only — 3px solid, 2px offset |
| `--green` / `--green-soft` | `#19724b` / `#e8f6ee` | Accepted, saved, allowed |
| `--amber` / `--amber-soft` | `#935b0a` / `#fff4df` | Offered, blocked, warning |
| `--red` / `--red-soft` | `#a23f3a` / `#fff0ee` | Declined, destructive, rejected |

One colour moved, as part of the amendment: mono labels went from `#596779` to
**`#334052`**, because a 13px uppercase label with wide tracking needs the extra
weight of colour to read as a label rather than as noise. `#334052` is already in
`admin.css` (`.module-link`).

Other supporting greys the boards use, all already in the file: `#354154`
(pagination), `#465366`, `#4b596b`, `#e1e6ed` (inner dividers), `#f8fafc`
(pagination face), `#7890af` + `#f4f8fd` (filled field), `#f2f4f7` + `#cfd6df`
(neutral badge), `#e3c981` + `#fff8e8` + `#644617` (env chip), `#b4dbc5` /
`#e7c27b` / `#e3b0ab` (badge borders), `#e0a7a2` + `#782f2b` (alert),
`#dfb0ab` + `#fbf3f2` + `#e4bbb7` (danger zone).

---

## Geometry — inherited, unchanged

| Thing | Value |
|---|---|
| Content measure | `min(100% - 32px, 1120px)`, centred |
| Module rail | 240px |
| Top bar | 72px |
| Card radius | 14px |
| Control radius | 8px — 6px for small controls |
| Control height | 36px — **30px small** (was 28), 32px pagination |
| Table row | 44px, head 36px, cell padding `0 16px` |
| Rail item | 36px min-height, 6px radius |
| Spacing steps | 4 · 8 · 12 · 16 · 24 · 32 |
| Card shadow | `0 1px 2px rgba(22,32,56,.06)` |
| Primary button shadow | `0 3px 10px -3px rgba(31,87,151,.6)` |

The 44px row still holds 15px text comfortably, and the 36px control still holds
15px text, so the amendment costs no density. Only the page's vertical rhythm grows
— roughly 7%, which is why the boards are drawn 80–120px taller than before.

---

## Field states — inherited, unchanged

| State | Border | Fill |
|---|---|---|
| Empty | `#b9c5d3` | `#ffffff` |
| Filled — **also how an active filter reads** | `#7890af` | `#f4f8fd` |
| Rejected | `--red` | `--red-soft` |
| Disabled | `#d5dce5` | `#f2f4f7`, text `--ink-soft` |
| Focus | `--blue` + `0 0 0 3px rgba(31,87,151,.12)` | — |

## Breakpoints

`1040px` · `760px` · `480px` — all three already in `admin.css`. See the responsive
panel on `boards/Foundations.html`.
