# Design system

This document is honest about a moving target. The RustIO admin
currently has **two stylesheets in the repo and one of them is
unshipped**. New contributors usually need that fact spelled out
before they spend an hour wondering which one to edit.

## The two files

| Path | Status | What it is |
|---|---|---|
| `rustio-core/assets/static/admin.css` | **Bundled.** Compiled by `build.rs` (Tailwind v4) → `OUT_DIR/admin.css` → `include_bytes!`'d into the binary. | The shipping admin theme. Phase 3 / Tailwind v4 / warm-light Django-feeling system. Templates emit BEM-style classes against this file (`.rio-sidebar__item`, `.rio-card__title`, `.rio-btn--primary`). |
| `rustio-core/assets/admin.css` | **Spec only.** Not referenced by `build.rs`, not `include_str!`'d anywhere. | The v7 design system — operator-scale, density-aware, ink+rust palette, weight-driven hierarchy. Flat-kebab class vocabulary (`.rio-nav-link`, `.rio-card-body`, `.rio-btn-primary`). Authored as a target spec for the next admin generation. |

`assets/admin.css` is intentionally kept in tree so the v7 intent is
discoverable from the framework crate itself, not buried in a Figma
file or an issue thread. It is **not** delivered to user projects in
the current release.

## What ships today

The bundled stylesheet is the one in `assets/static/admin.css`. It
is structured as:

- A `:root` block with passthrough variables that work even when
  Tailwind is bypassed.
- An `@theme {}` block that lets Tailwind v4 generate utility
  classes (`bg-canvas`, `text-primary`, …) from the same tokens.
- An `@layer base` for global resets and default type rules.
- An `@layer components` for the Phase 3 canonical vocabulary
  — `.rio-shell`, `.rio-sidebar`, `.rio-topbar`, `.rio-btn`,
  `.rio-card`, `.rio-stat`, `.rio-table`, `.rio-form*`, etc.
- A second `@layer components` block (added in 0.11) for the new
  Django-admin page primitives — `.rio-page-header`,
  `.rio-breadcrumb`, `.rio-detail-grid`, `.rio-pagination`,
  `.rio-meta-list`, `.rio-timeline`, `.rio-empty`, `.rio-sidebar__count`,
  `.rio-theme-toggle`.
- Bare `[data-theme="light"]` / `[data-theme="dark"]` blocks that
  bind semantic aliases to one of two palettes — light is the
  default; dark is opt-in. The no-FOUC bootstrap script in
  `base.html` reads `localStorage.rio-theme` before any stylesheet
  link.

Hard rules carried from the 0.10.x brief — no pure black, no pure
white (`#F7F6F2` is the lightest surface), all borders `0.5px`,
font weights `400 / 500` only, body text 16 px / line-height 1.7,
accent `#FF6A3D` capped at ≤ 7 elements per viewport.

## What v7 reframes

The v7 file in `assets/admin.css` is a deliberate pivot away from
the "Django-feeling" framing and toward an **operator back-office for
long sessions**. The differences worth knowing:

| Axis | Shipping (Phase 3) | v7 spec |
|---|---|---|
| Identity | Calm, Django-feeling. | Operator-scale. Clinical, scannable, dense. |
| Type scale | 26 / 22 / 18 / 16 / 14 / 13 px (weights 400 / 500 only). | 28 / 22 / 18 / 16 / 15 / 14 / 13 / 12 px (weights 400 → 600 → 700). Display titles are heavier, label rows distinctly different from body rows. |
| Palette | Warm light (`F7F6F2` → `1F1E1B`) + rust accent. | Cool ink scale (`F5F7F9` → `0F141A`) + rust-600 accent. Borders `--rio-ink-200`. |
| Density | Comfortable only. | `.rio-density-compact` on `<body>` flips a set of `--rio-density-*` variables; ~40 % more rows per viewport at the cost of two pixels per cell. Opt-in via `rustio.design.json`. |
| Fonts | Inter via Google Fonts (until 0.11.x — see "Adopted so far" below). | Inter optional via project-supplied self-host; OS native UI font otherwise. No CDN @import at runtime. |
| Class convention | BEM (`.rio-card__title`, `.rio-btn--primary`). | Flat kebab (`.rio-card-title`, `.rio-btn-primary`). |
| Semantic colour | Single accent only. | Triplets (success / warn / danger) — soft bg + border + text per role. |

The two systems can't be swapped one-for-one because the bundled
templates emit BEM modifiers that v7 doesn't define (`.rio-card__title`
exists; `.rio-card-title` is what v7 provides). A class-vocabulary
migration of every template is the prerequisite for fully adopting
v7.

## Adopted so far

Pieces of v7's intent that are already in the bundled file:

- **Single-binary font policy** (0.11.x). `base.html` no longer pulls
  Inter from Google Fonts; `--font-sans` lists Inter first then drops
  to the OS native UI stack. Renders identically offline / behind a
  strict CSP / on an air-gapped network. See the commit message of
  `feat(admin): drop Google Fonts; system stack only (v7 alignment)`.

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
