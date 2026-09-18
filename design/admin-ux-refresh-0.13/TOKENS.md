# The locked visual contract

Every value below already exists in `rustio-core/assets/static/admin.css`. This file
is a reading aid, not a new source of truth — if the two disagree, `admin.css` wins.

Nothing in this handoff adds to this list. A diff that introduces a value not on this
page is out of scope by definition.

## Colour

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
| `--ink-soft` | `#4c5868` | Secondary text, column heads |
| `--border` | `#d5dce5` | Card and table rules |
| `--border-strong` | `#b9c5d3` | Control borders |
| `--focus` | `#2f7bd6` | Focus ring only — 3px solid, 2px offset |
| `--green` / `--green-soft` | `#19724b` / `#e8f6ee` | Accepted, saved, allowed |
| `--amber` / `--amber-soft` | `#935b0a` / `#fff4df` | Offered, blocked, warning |
| `--red` / `--red-soft` | `#a23f3a` / `#fff0ee` | Declined, destructive, rejected |

Supporting greys already in the file and used by the boards: `#596779` (rail and
card labels), `#334052` (rail item), `#354154` (pagination), `#4b596b` (field hint),
`#465366` (auth footnote), `#e1e6ed` (inner dividers), `#f8fafc` (pagination face),
`#7890af` + `#f4f8fd` (filled field border and fill), `#f2f4f7` + `#cfd6df` (neutral
badge), `#e3c981` + `#fff8e8` + `#644617` (env chip), `#b4dbc5` / `#e7c27b` /
`#e3b0ab` (badge borders), `#e0a7a2` (alert border), `#782f2b` (alert text),
`#dfb0ab` + `#fbf3f2` + `#e4bbb7` (danger zone).

## Type

Stack: `Inter, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif`
Mono: `ui-monospace, SFMono-Regular, "SF Mono", "JetBrains Mono", Menlo, Consolas, monospace`

| Role | Size / weight | Colour |
|---|---|---|
| Dashboard hero `h1` | 33 / 800, `-0.025em` | `--ink` |
| Page title `h1` | 24 / 800, `-0.02em` | `--ink` |
| `h2` | 17 / 700 | `--ink` |
| `h3`, card title | 15 / 700 | `--ink` |
| Body, `.lead` | 15 / 400, line-height 1.6 | `--ink` / `--ink-soft` |
| Table cell | 14 / 400 | `--ink` |
| Column head (mono, `.12em`, uppercase) | 12 / 700 | `--ink-soft` |
| Card head, rail label (mono, `.12em`–`.16em`, uppercase) | 11–11.5 / 700 | `#596779` |
| Field label | 13 / 500 | `--ink` |
| Field hint | 14 / 400 | `#4b596b` |
| Badge | 12 / 600 | per status |
| Stat value | 33 / 800 | `--ink` |
| Footer, breadcrumb | 14 / 13 | `#596779` / `--ink-soft` |

## Geometry

| Thing | Value |
|---|---|
| Content measure | `min(100% - 32px, 1120px)`, centred |
| Module rail | 240px |
| Top bar | 72px |
| Card radius | 14px |
| Control radius | 8px — 6px for small controls |
| Control height | 36px — 30px small, 32px pagination |
| Table row | 44px, head 36px, cell padding `0 16px` |
| Rail item | 36px min-height, 6px radius |
| Spacing steps | 4 · 8 · 12 · 16 · 24 · 32 |
| Card shadow | `0 1px 2px rgba(22,32,56,.06)` |
| Primary button shadow | `0 3px 10px -3px rgba(31,87,151,.6)` |

## Field states

| State | Border | Fill |
|---|---|---|
| Empty | `#b9c5d3` | `#ffffff` |
| Filled — **also how an active filter reads** | `#7890af` | `#f4f8fd` |
| Rejected | `--red` | `--red-soft` |
| Disabled | `#d5dce5` | `#f2f4f7`, text `--ink-soft` |
| Focus | `--blue` + `0 0 0 3px rgba(31,87,151,.12)` | — |

## Breakpoints

`1040px` · `760px` · `480px` — all three already exist in `admin.css`. See the
responsive panel on `boards/Foundations.html` for what each one does.
