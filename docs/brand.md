# RustIO brand

> **Historical spec.** This document captures the rust+teal visual
> language drafted in Phase 6a. Two redesigns later (Phase 11/a teal
> alignment, then 1.8.3 Cobalt Blue migration), the framework default
> is now **Cobalt Blue (`#2563EB`)**.
>
> **Current source of truth:** `docs/design-system.json → themes.light`.
> The layered model (design tokens → `--rio-*` chrome tokens →
> runtime override) is documented in `docs/architecture.md → Theming`.
> Per-project overrides go through `Admin::theme(AdminTheme)` — see
> the README's "Theme it" section.
>
> Preserved as the historical reference for the rust / steel-teal
> palette so future redesign phases have something to measure
> against.

---

Canonical visual brand for the RustIO project. Replaces the Phase 6a
"Django classic + RustIO accent" contract that lives in
`rustio-core/assets/static/css/admin.css`. The Phase 6a admin styling
remains in the codebase until a redesign phase ships; this document
is the spec the redesign will be measured against.

The companion artifact `docs/brand/showcase.html` is the canonical
reference render. Open it in a browser to see every component
together.

---

## Identity

Industrial-grade tools for serious systems. The visual register is
the same one the project's tagline points at: warm paper-white
surfaces, dark-metal headers and code panels, rust-orange accents
that signal action and identity.

Two foreground voices:

- **Rust** (`#aa4422`) — primary CTA, brand mark, hero highlights,
  status indicators that mean "active" or "now".
- **Steel teal** (`#338899`) — secondary accent, "verified" / "synced"
  status, complementary semantic when rust is already in play.

Two background voices:

- **Paper white** (`#f9f8f6`) — page surface, slightly warm so
  rust-orange against it doesn't read as a fire alarm.
- **Dark metal** (`#181a1f`) and surface dark (`#22252b`) — top bar,
  code blocks, eventually a dark-mode pass.

---

## Palette

```css
:root {
  /* Brand */
  --rust:        #aa4422;   /* Primary CTA, mark, "now" */
  --rust-hover:  #88331b;
  --rust-glow:   #d46644;   /* On dark surfaces */

  /* Backgrounds */
  --bg-page:     #f9f8f6;   /* Warm paper white */
  --bg-card:     #ffffff;
  --bg-dark:     #181a1f;   /* Dark metal */
  --bg-surface:  #22252b;   /* Cards on dark */

  /* Text */
  --text-main:   #2c303a;   /* Metal black */
  --text-light:  #e2e8f0;   /* On dark */
  --text-muted:  #8b949e;

  /* Borders */
  --border:      #e2e8f0;

  /* Accent */
  --teal:        #338899;
}
```

### Migration map from Phase 6a admin.css

| Phase 6a token | New token | Notes |
|---|---|---|
| `--accent: #C55A3A` | `--rust: #aa4422` | Slightly darker, more saturated |
| `--accent-hover: #A84A2F` | `--rust-hover: #88331b` | |
| `--primary: #1A202C` | `--bg-dark: #181a1f` | Header background |
| `--secondary: #4A5568` | `--bg-surface: #22252b` | Surface dark |
| `--body-fg: #0F1114` | `--text-main: #2c303a` | Body text — meaningfully lighter |
| `--body-bg: #fff` | `--bg-page: #f9f8f6` | Was bright white; now warm paper |
| `--accent-subtle: #FDF4F0` | (drop) | Selected-row tint covered by `rgba(170,68,34,.08)` |

Three Phase 6a pill colours (`emerald`, `indigo`, `rose`) collapse
to the badge family below.

---

## Typography

```css
font-family: 'Inter', system-ui, sans-serif;
font-feature-settings: "ss01", "tnum";
-webkit-font-smoothing: antialiased;
```

**Font:** Inter (Google Fonts) with `system-ui, sans-serif` fallback.
The two `font-feature-settings` are non-trivial — `ss01` activates
Inter's stylistic alternate set and `tnum` enforces tabular numbers
(load-bearing for the stat-value cells in dashboards).

**Type scale** (from the showcase):

| Role | Size | Weight | Tracking | Line-height |
|---|---:|---:|---:|---:|
| Hero H1 | 48px | 700 | -0.025em | 1.08 |
| Card title | 18px | 700 | -0.015em | inherit |
| Stat value | 28px | 700 | -0.02em (tnum) | 1.1 |
| Hero body | 17px | 400 | normal | 1.6 |
| Body | 15px | 400 | normal | 1.55 |
| Form input | 14.5px | 400 | normal | inherit |
| Card desc | 14px | 400 | normal | 1.6 |
| Small text | 13.5px | 500 | normal | inherit |
| Form label | 13px | 600 | normal | inherit |
| Stat trend / form help | 12.5px | 500 | normal | inherit |
| Eyebrow | 12px | 600 | 0.14em UPPERCASE | inherit |
| Stat label / section label | 11.5px | 600 | 0.14em UPPERCASE | inherit |
| Card eyebrow | 11px | 600 | 0.12em UPPERCASE | inherit |

The eyebrow pattern (uppercase, tight tracking, rust-coloured)
appears at three sizes (12 / 11.5 / 11) — keep them distinct, don't
collapse them to one rule.

---

## Geometry

**Radii** (4 stops, no other values):

```
6px   — small mark inside the top bar
8px   — buttons, form inputs, badges (pill is 14px)
10px  — stat cards, code blocks
12px  — content cards, form cards
```

**Shadows** (4 specific recipes):

```
mark:           0 1px 3px rgba(170,68,34,0.4)
btn-primary:    0 1px 2px rgba(170,68,34,0.3)
btn-primary :hover:  0 4px 10px rgba(170,68,34,0.25)  + translateY(-1px)
card :hover:    0 8px 20px rgba(0,0,0,0.04)           + translateY(-2px)
```

No general-purpose shadow scale. Shadows are reserved for
brand-coloured CTAs and hover lifts on cards. Everything else stays
flat with a 1px `--border` outline.

**Borders:** always 1px solid `--border` (`#e2e8f0`). For dark
surfaces, `#2d3138`.

**Spacing:** the showcase uses an 8-based scale informally
(8/12/14/16/20/22/24/28). Don't introduce 6 or 18 outside what's
already in use.

---

## Components

### Top bar
Dark surface (`--bg-surface`), 14px vertical / 40px horizontal
padding, rust-orange brand mark with `--rust` background and white
"R" inside. Nav links sit muted (`--text-muted`); active state uses
`--rust-glow` so it's legible on dark.

### Buttons
- **Primary:** `--rust` bg, white fg, soft rust shadow. Hover lifts
  1px and deepens the shadow.
- **Ghost:** white bg, `--border` outline, `--text-main` fg. Hover
  swaps to a `--text-muted` border.

Both buttons are 11px/20px padding, 8px radius, 14.5px text,
600 weight.

### Cards
12px radius, white bg, 1px border, 24px padding. On hover: lift 2px
and faint shadow (no border colour change unless the card is
clickable — then border darkens to `--text-muted`).

### Stats
Three-column grid, 14px gap. Each stat is its own 10px-radius card
with: uppercase tracked label, tabular-numeric value, 12.5px trend
line (rust for positive, muted for neutral).

### Forms
Form-card wraps the whole form in a 12px-radius outline. Inputs are
8px radius, 11/13 padding. Focus state: 1px rust border + 3px rust
halo (`box-shadow: 0 0 0 3px rgba(170,68,34,0.1)`). The halo is the
brand's signature — never substitute a default browser focus ring.

### Badges
Pill (14px radius), 4/10 padding, 12px/600 text. Leading dot
(`::before { background: currentColor }`) is the badge's identity.
Three semantic flavours:

- `badge-rust` — "active", "in review", anything currently happening
- `badge-teal` — "verified", "synced", anything green-equivalent that
  shouldn't fight rust for attention
- `badge-muted` — "archived", "draft", anything inactive

The Phase 6a `.rio-pill-{emerald,indigo,rose}` collapses to these
three.

### Code block
Dark surface (`--bg-dark`), monospace stack, 10px radius. Token
colours: comments `--text-muted`, keywords `--rust-glow`, strings
`--teal`, identifiers `--text-light`.

### Eyebrows
Three sizes (see typography). Always uppercase, tight letter-spacing,
rust or muted. Render with a small leading rule (22×2 px) before
hero eyebrows; the rule is what visually anchors the H1 below it.

---

## Implementation considerations (read before adopting)

The Phase 6a admin.css contract had four bullets. Adopting this
brand supersedes three of them:

| Phase 6a contract | This brand |
|---|---|
| Plain CSS only. No Tailwind, no Bootstrap, no JS-driven styles. | **Superseded as of Phase 7a/2** — Tailwind at build time, single minified `admin.css` baked into the binary at deploy. The single-binary invariant is preserved; the "no Tailwind" rule was about avoiding a runtime CDN, which no longer applies. |
| System font stack — no @font-face, no Google Fonts. | **Superseded.** Inter via Google Fonts (or self-hosted woff2). |
| Sharp corners (≤3px radius). Flat (no shadows, no gradients). | **Superseded.** 6–12px radii, four named shadows. No gradients. |
| Single theme. No dark-mode toggle. | **Still true.** Dark surfaces are component-level (top bar, code block), not a theme switch. |

### The Inter / single-binary tension

The framework's first design principle is **single-binary deploy** —
every template and stylesheet is `include_str!`-baked. The showcase
uses `<link href="https://fonts.googleapis.com/...">`, which adds a
runtime dependency on Google's CDN.

Three options when this brand is implemented:

1. **Self-host Inter.** Drop `Inter-{Regular,Medium,SemiBold,Bold,ExtraBold}.woff2`
   into `rustio-core/assets/static/fonts/` and serve via
   `embedded_admin_fonts()` helper, mirroring `embedded_admin_css()`.
   Adds ~150KB to the binary; preserves single-binary purity. **Recommended.**
2. **CDN dependency.** Keep `<link href="...googleapis.com/...">`.
   Smallest change but breaks single-binary deploy and adds a third
   party to the request path.
3. **System-only.** Drop Inter, use `system-ui` for everything. The
   tracking and weights in the showcase are tuned for Inter; system
   fonts will look distinctly different on every OS. Cheapest, ugliest.

### Phase 6a coexistence

Phase 6a admin.css and a new brand stylesheet cannot share selectors
(both target `#header`, `.module`, `.results`, etc.). Migration is
**either**:

- A clean-slate rewrite of `admin.css` in one phase. Browser smoke
  must cover every existing template at the end.
- A parallel stylesheet (`admin-2026.css`) routed only on opted-in
  pages, with the rest still on Phase 6a. Buys time but guarantees
  visual inconsistency for the duration of the transition.

Whichever path: **the redesign is its own phase** with its own spec,
its own browser-smoke matrix per template, and its own commit. This
file is the destination, not the path.

---

## Open questions — resolved by Phase 7a/2

1. ~~**Sidebar?**~~ **Yes.** Top-bar brand mark + collapsible
   left sidebar, mobile drawer toggle. The Phase 6a "no sidebar"
   contract is superseded.
2. ~~**Icons?**~~ **lucide.** 16 stroke icons baked at compile time
   in `admin/icons.rs`; templates use `{{ icon("home", class="w-4 h-4") }}`.
   Adding an icon: drop the lucide inner-SVG fragment into `ICONS`
   and update the unit-test catalogue.
3. ~~**Typography?**~~ **Self-hosted Inter.** Four woff2 weights
   (Regular/Medium/SemiBold/Bold) under
   `rustio-core/assets/static/fonts/`, served by per-weight routes
   from `register_admin_routes`. Adds ~95KB to the binary; preserves
   single-binary purity. (Phase 2's earlier interim — Roboto + Space
   Grotesk + JetBrains Mono via Google Fonts — was retired.)
4. ~~**Token names?**~~ **Tailwind tokens are the source.**
   `docs/design-system.json` is the canonical token sheet; CSS
   custom properties in `assets/css/input.css` and `theme.extend` in
   `tailwind.config.js` mirror it. The Phase 6a `--accent` /
   `--primary` set was retired in the Phase 3 token sweep; the
   design-system tokens replaced them.
5. ~~**Stats cards in admin?**~~ **Login / list / form / view** are
   in scope. The hero + perf-stats grid stays as a brand asset for
   landing pages, not the admin shell.

---

*Adopted: 2026-04-26. **Implemented:** Phase 7a/2 (commits `4d773e0` → `9f00dce`).
Token sweep + design-system pass: Phases 2 → 4. Open questions
above marked resolved as of Phase 7.3.*
