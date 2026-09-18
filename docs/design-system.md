# Design system

This document is honest about a moving target. The RustIO admin
currently has **two stylesheets in the repo and one of them is
unshipped**. New contributors usually need that fact spelled out
before they spend an hour wondering which one to edit.

## The two files

| Path | Status | What it is |
|---|---|---|
| `rustio-core/assets/static/admin.css` | **Bundled.** Compiled by `build.rs` (Tailwind v4) → `OUT_DIR/admin.css` → `include_bytes!`'d into the binary. | The shipping admin theme: cool institutional blue ("Bureau", 2.0.5). Templates emit BEM-style classes against this file (`.rio-sidebar__item`, `.rio-card__title`, `.rio-btn--primary`). |
| `rustio-core/assets/admin.css` | **Spec only.** Not referenced by `build.rs`, not `include_str!`'d anywhere. | The v7 design system — operator-scale, density-aware, ink+rust palette, weight-driven hierarchy. Flat-kebab class vocabulary (`.rio-nav-link`, `.rio-card-body`, `.rio-btn-primary`). Authored as a target spec for the next admin generation. |

`assets/admin.css` is intentionally kept in tree so the v7 intent is
discoverable from the framework crate itself, not buried in a Figma
file or an issue thread. It is **not** delivered to user projects in
the current release.

## What ships today

The bundled stylesheet is the one in `assets/static/admin.css`. It is
structured as:

- A `:root` block with passthrough variables that work even when
  Tailwind is bypassed.
- An `@theme {}` block that lets Tailwind v4 generate utility
  classes (`bg-canvas`, `text-primary`, …) from the same tokens.
- An `@layer base` for global resets and default type rules.
- An `@layer components` for the canonical vocabulary — `.rio-shell`,
  `.rio-sidebar`, `.rio-topbar`, `.rio-btn`, `.rio-card`, `.rio-stat`,
  `.rio-table`, `.rio-form*`, etc.
- A second `@layer components` block for the page primitives —
  `.rio-page-header`, `.rio-breadcrumb`, `.rio-detail-grid`,
  `.rio-pagination`, `.rio-meta-list`, `.rio-empty`,
  `.rio-sidebar__count`, `.rio-theme-toggle`, `.rio-table-wrap`.
- `[data-theme="light"]` / `[data-theme="dark"]` blocks binding the
  semantic aliases to one of two palettes — light is the default, dark
  is opt-in, and roughly 20 component rules have a dark-specific
  override. The no-FOUC bootstrap in `base.html` reads
  `localStorage.rio-theme` before any stylesheet link.

### The tokens that ship

Read off the served stylesheet, not from memory. The identity is a
**cool, institutional** one: a blue-grey canvas, white surfaces, a
confident blue accent.

| Group | Values |
|---|---|
| Surfaces | canvas `#E9EDF3` · surface `#FFFFFF` · elevated `#F6F8FC` |
| Borders | `#DCE2EC`, strong `#C3CCDA` |
| Text | primary `#151A23` · secondary `#4E5A6E` · tertiary `#2A3342` · muted `#818D9F` |
| Accent | `#2B54E0`, deep `#1E3FB8`, soft `#E7ECFD`, on-accent `#FFFFFF` |
| Semantic triplets | ok `#0C6B49` / `#E2F4EB` / `#B2E0C8` · warn `#9A5B0B` / `#FBF0DA` / `#ECCF8E` · bad `#B0271A` / `#FBEAE7` / `#F0BDB4`, each with a matching dot |
| Sidebar | its own dark scale — `#12161F` / `#0E121A`, text `#C4CCDA`, accent `#7C97FF` |
| Type scale | hero 34 · stat 30 · h2 22 · h3 17 · **body 15** · meta 14 · small 13 · mono 12.5 · micro 11.5 px |
| Spacing | 4 · 8 · 12 · 16 · 22 · 28 · 38 · 52 px |
| Radius | 5 · 8 · 12 px |
| Shadows | three levels, all cool-tinted `rgba(20,29,45,…)` |

### The rules it actually follows

- **White surfaces are used.** `--color-surface` is `#FFFFFF`; the
  canvas is what carries the tint.
- **Weight carries hierarchy.** Seven weights are in play — 450, 500,
  550, 600, 650, 700, 800 — with 700 by far the most common (39 rules)
  for titles, table captions and buttons.
- **Borders are 1px**, not hairlines. `999px` appears for pills, `3px`
  for the occasional emphasis rule.
- **Body text is 15px.** Line height is set per component rather than
  globally.
- **The accent is not rationed.** It carries links, focus rings,
  primary buttons and the active sidebar state.

If you are changing the theme, change these values and update this
table in the same commit. The previous version of this document
described a warm, rust-accented system with 400/500 weights and
0.5px borders; none of that had been true since the 2.0.5 "Bureau"
restyle, and the gap cost more than the doc was worth.

## What v7 reframes

`assets/admin.css` is a deliberate pivot toward an **operator back-office
for long sessions**. Now that the shipping theme is itself cool and
institutional, the two are closer than this document used to claim — the
remaining differences are these:

| Axis | Shipping | v7 spec |
|---|---|---|
| Palette | Blue-grey canvas `#E9EDF3`, white surfaces, blue accent `#2B54E0`. | Cool ink scale `#F5F7F9` → `#0F141A` with a **rust** accent (`--rio-rust-500 #D2501E`). Same temperature, different accent family. |
| Type scale | 34 / 30 / 22 / 17 / 15 / 14 / 13 / 12.5 / 11.5 px. | 28 / 22 / 18 / 16 / 15 / 14 / 13 / 12 px — shorter, with a smaller display size. |
| Weights | Seven in play (450 → 800), 700 dominant. | Three deliberate steps: 400 / 600 / 700. |
| Density | Comfortable only; no density variables. | `.rio-density-compact` on `<body>` flips `--rio-density-*`; ~40 % more rows per viewport. Opt-in via `rustio.design.json`. |
| Semantic colour | Triplets already ship (ok / warn / bad — background, border, text and dot). | Same idea, ink-scale-derived values. |
| Class convention | BEM (`.rio-card__title`, `.rio-btn--primary`). | Flat kebab (`.rio-card-title`, `.rio-btn-primary`). |
| Fonts | System stack, Inter first, no CDN. | Same — this one is **adopted**, see below. |

The two systems still can't be swapped one-for-one: the bundled templates
emit BEM modifiers that v7 doesn't define (`.rio-card__title` exists;
`.rio-card-title` is what v7 provides). A class-vocabulary migration of
every template remains the prerequisite for adopting v7 wholesale.

**The v7 file has not been re-checked against the shipped theme since the
Bureau restyle.** Its accent family (rust) and weight policy (400/600/700)
now differ from what ships for reasons that may be intent or may be drift.
This document does not assume which, and the file was not rewritten.

## Adopted so far

Pieces of v7's intent that are already in the bundled file:

- **Single-binary font policy** (0.11.x). `base.html` no longer pulls
  Inter from Google Fonts; `--font-sans` lists Inter first then drops
  to the OS native UI stack. Renders identically offline / behind a
  strict CSP / on an air-gapped network. See the commit message of
  `feat(admin): drop Google Fonts; system stack only (v7 alignment)`.
- **Semantic colour triplets.** ok / warn / bad each ship with a
  background, border, text and dot value, which v7 lists as one of its
  reframes. The values are the shipping palette's, not v7's.
- **A cool, institutional temperature.** The Bureau restyle moved the
  canvas and text scale off warm neutrals, which was v7's other headline
  argument. What remains different is the accent family and the weight
  policy.

## Migration path

There's no rush to flip the bundle to v7. The steps below are the
*order* a migration should follow if and when it is taken on — each
step is a separately-shippable PR.

1. **Token migration.** Introduce v7's `--rio-ink-*` / `--rio-rust-*`
   / `--rio-fs-*` / `--rio-s-*` variables into the shipping file as a
   parallel set, mapped onto the existing semantic aliases. No visible
   change to users; the BEM components still consume the semantic
   names.
2. **Density variable wiring.** Add the `--rio-density-*` variables
   and the `.rio-density-compact` body-class flip behind a
   `rustio.design.json` field. Defaults stay comfortable. Templates
   read the densities from semantic aliases as today.
3. **Typography weight pass.** Introduce the weight-driven hierarchy
   (400 → 600 → 700) on `h1` / `h2` / labels / buttons / table caps.
   Visible change; needs a release note. Lock the new type scale
   under `[data-theme="light"]` and `[data-theme="dark"]` so both
   themes stay coherent.
4. **Class vocabulary migration, template by template.** For each
   template that currently emits `.rio-card__title` / `.rio-btn--*` /
   `.rio-form__field`, add the corresponding flat-kebab class
   alongside (`<h3 class="rio-card__title rio-card-title">`). Once
   every template is dual-classed, the BEM selectors can be retired
   from the bundle in a single removal commit.
5. **Bundle swap.** Make `build.rs` source from `assets/admin.css`
   directly (rather than `assets/static/admin.css`). The two files
   merge into one canonical source.
6. **Density opt-out cleanup.** Once compact density is verified
   against real operator workloads, decide whether comfortable
   becomes opt-in instead of the default.

Steps 1–3 are non-breaking and can be cherry-picked at any release
cadence. Step 4 is the long pole — it touches every Rust handler
that emits admin HTML (the relations dropdowns in
`admin::layout::list_render`, the form builder in `admin::form`, the
audit and suggestion templates, the password-change and 404/403
pages, every admin sub-page). Step 5 is mechanical once 4 is done.

## Pointers

- `rustio-core/assets/admin.css` — the v7 spec, read it first.
- `rustio-core/assets/static/admin.css` — what ships today.
- `rustio-core/build.rs` — Tailwind compile step + the
  passthrough fallback (when Tailwind isn't on PATH the file is
  served verbatim after stripping `@theme {}` and the Tailwind
  import).
- `rustio-core/src/admin/design.rs` — `rustio.design.json` parsing
  + `Design::global()`. The natural surface for wiring the v7
  density toggle.
- `rustio-core/src/admin/templating.rs` — the minijinja environment
  setup; user projects override any admin template by placing a
  file of the same relative path under their `templates/`.
