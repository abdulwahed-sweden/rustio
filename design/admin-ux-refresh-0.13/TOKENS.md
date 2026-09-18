# Tokens — UX Refresh 0.13

Every value the boards are built from. Colour, geometry and spacing come from
the stabilised baseline; the **text ramp and type scale are revised for 0.13**
and supersede the shipped values.

Light only. `color-scheme: light`, no `prefers-color-scheme` block, no theme
attribute, no toggle.

---

## 1. Colour

### Surfaces

| Token | Hex |
|---|---|
| `--page` | `#F1F1EE` |
| `--surface` | `#FFFFFF` |
| `--surface-soft` | `#F7F9FC` |
| `--surface-head` | `#F1F4F8` |
| `--rail-bg` | `#F4F6F9` |

### Primary

| Token | Hex | Note |
|---|---|---|
| `--blue` | `#1F5797` | |
| `--blue-dark` | `#174578` | Hover |
| `--blue-soft` | `#DFEAFB` | Current segment, current page, selected row |
| `--blue-line` | `#B7CDEA` | Hover borders |
| `--focus` | `#2F7BD6` | Focus ring only |

`--focus` is deliberately **not** `--blue`. A ring sharing the primary-button
fill made a focused control read as a button. The ring is solid, 3px, offset
2px — never a tint, and always drawn on the surface behind a control rather
than on its fill.

### Text ramp — revised for 0.13

| Token | Hex | Used for |
|---|---|---|
| `--ink` | `#171B22` | Primary: titles, table cells, form labels, input values, record identity |
| `--ink-body` | `#202733` | Standard body, lead, column heads, count numerals |
| `--ink-2` | `#3F4A59` | Secondary: breadcrumb, result count, pagination, in-cell detail |
| `--ink-hint` | `#4C5868` | Field hints and helper text |
| `--ink-mono` | `#334052` | Mono micro-labels and rail section labels |
| `--ink-3` | `#596779` | **Tertiary technical metadata only** |

**Hierarchy comes from size and weight first.** `#596779` is never used for a
field label, a column head, a control, or any operational information. Nothing
an operator acts on is pale.

### Lines

| Token | Hex |
|---|---|
| `--border` | `#D5DCE5` |
| `--border-strong` | `#B9C5D3` |
| `--rail-line` | `#D3DBE5` |

### Status — meaning, never decoration

| Token | Hex |
|---|---|
| `--green` / `--green-soft` | `#19724B` / `#E8F6EE` |
| `--amber` / `--amber-soft` | `#935B0A` / `#FFF4DF` |
| `--red` / `--red-soft` | `#A23F3A` / `#FFF0EE` |

| Stored value | Badge | Meaning |
|---|---|---|
| `accepted` · `completed` · `active` · `paid` | green | Settled, successful |
| `offered` · `new` · `pending` · `scheduled` | amber | Awaiting something |
| `cancelled` · `declined` · `failed` · `expired` | red | Ended without success |
| anything else | grey | No opinion |

### Active-filter treatment

| Property | Value |
|---|---|
| Border | `#7890AF` |
| Fill | `#F4F8FD` |
| Text | `#174578`, weight 600 |

Three signals, so colour is never the only one.

---

## 2. Type

Inter first, then the OS native UI stack — no webfont fetched at runtime. The
mono stack is reserved for uppercase micro-labels.

| Role | Size / weight | Line-height | Colour |
|---|---|---|---|
| Dashboard hero | 33 / 800 | 1.15 | `#171B22` |
| Page h1 | 24 / 800 | 1.2 | `#171B22` |
| h2 | 18 / 700 | 1.3 | `#171B22` |
| h3 | 16 / 700 | 1.35 | `#171B22` |
| Body · lead | 16 / 500 | 1.55 | `#202733` |
| Table cell | 15 / 500 | 1.4 | `#171B22` |
| Table identity / primary linked value | 15 / 680 | 1.4 | `#171B22` |
| In-cell detail | 14 / 500 | 1.35 | `#3F4A59` |
| Column head | 13 / 700 mono, uppercase, .08em | — | `#202733`; sorted `#171B22` |
| Rail section label | 13 / 700 mono, uppercase | — | `#334052` |
| Card micro-label | 13 / 700 mono, uppercase, .08em | — | `#334052` |
| Form label | 14 / 600 | — | `#171B22` |
| Input · select value | 15 / 500 | — | `#171B22` |
| Hint · helper | 14 / 500 | 1.45 | `#4C5868` |
| Badge | 13 / 600 | 1.3 | per status |
| Normal button | 14 / 680 | 1 | per face |
| Small button | 13 / 650 | 1 | per face |
| Breadcrumb | 13 / 600 | — | `#3F4A59` |
| Pagination · result count | 14 / 600 | — | `#3F4A59`; numerals `#202733` / 700 |
| Tertiary metadata | 12 / 600 | — | `#596779` |

### Hard rules

- **No functional product text below 13px.**
- 12px is reserved for genuinely tertiary technical metadata and small non-text
  glyphs such as the sort chevron.
- 11px does not appear in operational UI at all.
- **Type sizes do not shrink at any breakpoint.** Responsive design changes
  layout, never readability.

Sizes in use: 12 · 13 · 14 · 15 · 16 · 18 · 24 · 33.
Weights in use: 500 · 600 · 650 · 680 · 700 · 800.

Adding a size or weight is a change to the design system, not a use of it.

---

## 3. Geometry

### Shell

| Token | Value |
|---|---|
| Masthead | 56px |
| Desktop rail | 240px |
| Focused content max | 1120px |
| Wide data workspace max | 1600px |

Records — Table, Records — Cards, View Editor and Overview are wide-data
screens and use the **wide workspace** measure. Record Form and the auth
screens use the **focused** measure.

### Controls

| Element | Height |
|---|---|
| Input, select, standard button | 38px |
| Compact masthead utility control | 32px |
| Small action button | ~31px |
| Segmented-control segment | ~31px |
| Table header | 40px |
| Table row | 48px minimum |
| Rail item | 38px, 4px apart |

**Compact masthead utility controls are 32px, not 38px.** The masthead is
56px, and a 38px control leaves 9px of clearance above and below — too tight
beside the 31px log-out button it sits next to. The exception covers utility
controls in the masthead only: the language switcher is the one in the product
today. Every control inside the page — every form input, every select, every
standard button — is 38px. No board renders the masthead's utility controls,
which is why this needed stating explicitly.

| Element | Value |
|---|---|
| Table cell horizontal padding | 16px |
| Search field | 340px preferred, 220px minimum |
| Icon in button | 16px |
| Icon in small button | 14px |
| Icon in search field | 17px |
| Sort chevron | 12px |

### Shape

| Token | Value | Use |
|---|---|---|
| Card radius | 14px | Cards, panels, empty states |
| Control radius | 8px | Buttons, inputs, selects |
| Small radius | 6px | Badges, small buttons, rail items, page links |

### Spacing

`4 / 8 / 12 / 16 / 24 / 32`

Every gap and every pad. No other value.

### Shadow

| Token | Value | Use |
|---|---|---|
| `--shadow-sm` | `0 1px 2px rgba(22,32,56,.06)` | Cards in a grid |
| `--shadow` | three-layer panel shadow | Raised: stat cards, auth card, empty states |

---

## 4. Components

| Component | Geometry | Rule |
|---|---|---|
| Primary / secondary button | 38px, radius 8, 14px / 680 | **One blue button per page**, and it is that page's primary action |
| Small button | ~31px, radius 6, 13px / 650 | Row actions and toolbar tools; always a text label, never icon-only |
| Danger button | Secondary geometry, red pair | A **semantic**, not a third style. Delete confirmations, a record's own “Delete record”, and the row Delete that leads to them |
| Badge | 13px / 600, 6px dot, radius 6 | Non-interactive. Colour from the status map |
| Card | 24px pad, radius 14, 1px `--border` | Tables and card heads bleed out through the padding |
| Focus ring | 3px solid `--focus`, offset 2px | Never a tint; never drawn on a fill |

---

## 5. Breakpoints

| Query | What changes |
|---|---|
| `min-width: 761px` | The wide measure applies on wide-workspace pages |
| `max-width: 1040px` | Masthead goes two-column; identity cluster becomes a wrapping flex row. Masthead stays 56px. |
| `max-width: 760px` | Rail becomes a horizontal scrolling strip; content becomes one column; two-column detail layouts collapse |
| `max-width: 480px` | Masthead stacks to two rows; page-head actions and form action rows go full width |
| `prefers-reduced-motion` | All transitions and smooth scrolling disabled |

No breakpoint reduces a font size.
