# Phase 7a/2 — Admin redesign (Tailwind + sidebar)

This is the long-form report for Phase 7a/2 of the RustIO 1.0 port.
It covers the admin UI redesign: a Tailwind build pipeline, self-
hosted Inter, an inline lucide icon system, a left sidebar, and a
ground-up rewrite of all 21 admin templates against the new visual
brand from `docs/brand.md` (adopted Phase 7a/0.5/brand).

The phase is **out of sequence** with the original roadmap — Phase
7a/1 (the tolkhuset crate skeleton) was the documented next step
after `PHASE7a-0_5.md` closed. The user redirected to the redesign
after a design assessment found the Phase 6a admin "ugly *exactly
the way they ordered it*" — Phase 6a's design contract was
intentionally Django-classic, but the bar moved. Tolkhuset is
deferred until after this phase ships.

Built on top of Phase 7a/0.5/list-polish (commit `364112e`) plus
the brand spec adoption commit (`0609f0a`).

---

## Table of contents

1. [Where we were on Phase-7a/2-day](#where-we-were-on-phase-7a2-day)
2. [The five commits](#the-five-commits)
3. [`/a` — Tailwind build pipeline + Inter + icon system](#a--tailwind-build-pipeline--inter--icon-system)
4. [`/b` — sidebar layout shell + base.html rewrite](#b--sidebar-layout-shell--basehtml-rewrite)
5. [`/c` — generic templates rewrite](#c--generic-templates-rewrite)
6. [`/d` — built-in templates rewrite](#d--built-in-templates-rewrite)
7. [`/2` (this commit) — docs + verification](#2-this-commit--docs--verification)
8. [Architectural decisions](#architectural-decisions)
9. [Risks accepted](#risks-accepted)
10. [Verification](#verification)
11. [Phase 7a/1 readiness](#phase-7a1-readiness)

---

## Where we were on Phase-7a/2-day

End of Phase 7a/0.5 (commit `522784f`):

- **CSS**: hand-authored `rustio-core/assets/static/css/admin.css`,
  ~1k LOC, "Django classic + RustIO accent" per the Phase 6a design
  contract. No Tailwind, no JS framework, no icons, no avatars,
  system fonts only. The previous design assessment summarised this
  as *"adequate but joyless"*.
- **Layout**: header + breadcrumbs only, no sidebar (explicit Phase
  6a choice). Single content column, occasional right-rail (Recent
  actions on the dashboard).
- **Templates**: 21 admin templates, all using semantic class names
  (`.module`, `.results`, `.btn-primary`, etc.). Every page
  rendered against the same Phase 6a styling.
- **Brand spec**: `docs/brand.md` adopted yesterday, with palette,
  Inter font, four-stop radius scale, four named shadows. Marked as
  spec-only — implementation deferred to a future phase. That phase
  is this one.
- **Tests**: 325 passing / 0 failing / 41 ignored.

The redesign was also audited against three project specs that the
new direction would supersede:

| Spec | What it said | What it says now |
|---|---|---|
| `README.md` design principles bullet 3 | "Plain HTML + plain CSS. No Bootstrap, no Tailwind, no React." | "Tailwind at build time, single binary at deploy." |
| `admin.css` Phase 6a contract header | "Plain CSS only. No Tailwind, no Bootstrap." + sharp corners + flat | File deleted; `admin.css` is now a generated artifact. |
| `docs/brand.md` migration table | "Plain CSS only — still true" | "Superseded as of Phase 7a/2" |

---

## The five commits

```
4d773e0  phase 7a/2/a: tailwind build pipeline + Inter + icon system
f2937ce  phase 7a/2/b: sidebar layout shell + base.html rewrite
c6527ff  phase 7a/2/c: rewrite generic templates (login/list/form/...)
9f00dce  phase 7a/2/d: rewrite built-in templates (users/groups/views)
TBD       phase 7a/2:   docs + verification + PHASE7a-2.md  ← this commit
```

Each commit is independently rebuildable; if any one is reverted,
the previous one stays viable.

---

## `/a` — Tailwind build pipeline + Inter + icon system

**Commit:** `4d773e0`

### Build pipeline

Three new files at the workspace root:

```
package.json           tailwindcss^3.4 + autoprefixer + postcss as devDeps
tailwind.config.js     theme.extend mirrors docs/brand.md
postcss.config.js      tailwind + autoprefixer plugin chain
```

`rustio-core/assets/css/input.css` becomes the **source**: Tailwind
directives + an `@layer base` block (font-faces, body styles) + an
`@layer components` block defining the public-API class contract
(`.btn-primary`, `.module`, `.results`, `.empty-list`, `.errornote`,
etc.). The compiled output lives at the existing path
`rustio-core/assets/static/css/admin.css` so the `include_str!` in
`server.rs::embedded_admin_css()` keeps working.

Three Makefile targets:

| Target | Purpose |
|---|---|
| `make css` | One-shot minified build. |
| `make css-watch` | Live rebuild during development. |
| `make css-check` | Diffs the committed CSS against a fresh build; fails if drift. |

The compiled `admin.css` IS committed so anyone can `cargo run`
without Node. `make css-check` belongs in CI / pre-commit.

### Inter (self-hosted)

Four woff2 files in `rustio-core/assets/static/fonts/`:

```
Inter-Regular.woff2    24KB
Inter-Medium.woff2     24KB
Inter-SemiBold.woff2   24KB
Inter-Bold.woff2       24KB
                       ───
                       95KB
```

Sourced from `@fontsource/inter@5` Latin subset. Each weight is its
own embedded byte slice in `server.rs` and served by an explicit
route in `register_admin_routes` — **not a path-wildcard**, so the
binary can't be tricked into serving arbitrary files from the
assets dir. `font-display: swap` in the `@font-face` declarations
keeps text readable while the font streams.

### Icon system

`rustio-core/src/admin/icons.rs` catalogues 16 lucide stroke icons
as inner-SVG fragments via `include_str!`-style constants. Around
5KB total.

```
home, table, users, users-2, database, clock, terminal,
plus, pencil, trash, arrow-left, log-out, key,
circle-alert, circle-x, menu
```

`render_inline(name, class)` wraps each fragment in a `<svg>` with
`fill="none"` and `stroke="currentColor"` so colour follows the
rendering context (sidebar link, button, alert banner). Unknown
names return an empty string — a typo can't crash the page.

The function is registered as a custom minijinja function `icon()`
in `Templates::new`. Templates write:

```jinja
{{ icon("home", class="w-4 h-4 text-rust") }}
```

The `Value::from_safe_string` wrapper means the SVG renders
unescaped. Three icon-related sandbox tests; one templates-side
test verifying the minijinja registration.

---

## `/b` — sidebar layout shell + base.html rewrite

**Commit:** `f2937ce`

### Layout shape

```
┌─────────────────────────────────────────────────┐
│ [☰] [R] RustIO          user@x · Logout         │  56px topbar
│  ⚠ DEMO USER (if active)                        │
├─────────────┬───────────────────────────────────┤
│  Manage     │                                   │
│  🏠 Dash    │                                   │
│  📋 Posts   │   {{ block content }}             │
│  👥 Users   │                                   │
│  👫 Groups  │                                   │
│             │                                   │
│  Developer  │                                   │
│  🗄 Schema  │                                   │
│  ⏱ Logs    │                                   │
│  ⌨ SQL     │                                   │
└─────────────┴───────────────────────────────────┘
   240px         flex-1, max-w-7xl
```

Below `md:` (768px) the sidebar collapses; the hamburger button on
the topbar toggles it. ~30 lines of inline JS handles two jobs:

1. **Active link highlighting** — matches `window.location.pathname`
   against each link's `data-active-prefix` attribute. Saves
   threading `current_path` through 25 `BaseContext::new` call
   sites.
2. **Mobile drawer toggle**.

### Sidebar nav

Dynamic per role:

- **Always (when logged in):** Manage section. Dashboard + every
  registered model (iterated via `{% for entry in entries %}`) +
  Users + Groups (the latter two when `identity.is_admin`).
- **Developer-only:** Developer section. Schema, Logs, SQL Console.
  Gated on `identity.is_developer` (new flag on `IdentityCtx`,
  derived from `role.includes(Role::Developer) && is_active`).

`DashboardCtx`, `ComingSoonCtx`, `ForbiddenCtx` all gain
`entries: Vec<SidebarEntry>` so the sidebar partial works
uniformly. `IdentityCtx` gains `is_developer: bool`.

---

## `/c` — generic templates rewrite

**Commit:** `c6527ff`

12 templates rewritten:

| Template | Visual identity |
|---|---|
| `index.html` | Card grid for app/model groups; sticky right-rail Recent activity card on lg+ |
| `list.html` | Heading-row with total count + primary "Add" button; rust-accent search panel from `/list-polish` preserved |
| `login.html` | Branded card with key-icon heading; topbar override keeps brand mark visible pre-auth |
| `form.html` | Heading-row with optional History button; Save / continue / add-another all become buttons; Delete appears as right-aligned red button on edit |
| `confirm_delete.html` | Red alert circle hero, card body wrapping the cascade summary |
| `error.html` | Amber alert circle hero, card-style body |
| `forbidden.html` | Red X circle hero, return-to-dashboard ghost button |
| `coming_soon.html` | Rust clock circle hero, ghost return button |
| `password_change.html` | Rust key icon hero, branded submit |
| `object_history.html` | Clock-icon header, results table |
| `log_entries.html` | Clock-icon header, results table |
| `includes/_field_errors.html` | Errornote with leading circle-alert icon |

Hybrid class strategy throughout: every page reaches for the
existing public-API class names (which resolve through `@layer
components`), with utilities for one-off layout (grid, flex,
max-width).

Zero Rust changes in this commit.

---

## `/d` — built-in templates rewrite

**Commit:** `9f00dce`

9 templates:

| Template | Notable changes |
|---|---|
| `users_list.html` | Heading-row + count + primary Add; `is_active` becomes a badge (was yes/no text); row-clickable Phase 7a/0.5/h pattern intact |
| `user_new.html` | Plus-icon hero, branded primary save + ghost cancel |
| `user_edit.html` | Pencil-icon hero, "Back to profile" ghost button. The `/f` warningnote (last-developer heads-up) preserved with its yellow accent |
| `user_view.html` | Circular-avatar header; 2-column Identity + Timeline grid; group memberships render as badge cloud (was bullet list); direct permissions render as code-pill chips. Action row: Back / Edit (primary) / Delete (red OR disabled span) |
| `user_confirm_delete.html` | Alert-circle hero, card body. `/f-fix2` disabled button + `/f-fix1` registry both preserved |
| `groups_list.html` | Heading-row + Add button; pencil + trash icons inline in actions column |
| `group_new.html` | Plus-icon hero, branded save + cancel |
| `group_edit.html` | Users-2-icon hero with subtitle showing current name; permissions render as toggle-able outline-pill grid (was vertical checkbox list) |
| `group_confirm_delete.html` | Alert-circle hero, card body, branded submit + ghost cancel |

Three sandbox tests had brittle exact-string matches against old
markup. Updated to assert on the **semantic contract** (Delete-when-
guarded must be a `<span>` not an `<a>`, demo badge present iff
`is_demo=true`, submit button has `deletelink-button` class without
`disabled` for deletable users). Extracted shared logic into an
`assert_delete_is_disabled_span` helper.

---

## `/2` (this commit) — docs + verification

This commit:

- **README.md design principles** — bullet 3 rewritten from "No
  Bootstrap, no Tailwind, no React" to a positive description of the
  Tailwind-at-build-time + single-binary-at-deploy contract.
- **docs/architecture.md** — the Templates section gains a
  "Styling pipeline" subsection describing `package.json` /
  `tailwind.config.js` / `Makefile` / Inter delivery / icon system.
- **docs/brand.md** — migration table flipped: "Plain CSS only —
  Still true" becomes "Superseded as of Phase 7a/2". Implementation
  date updated.
- **CLAUDE.md** — the `(file, registry, render-test)` triple
  becomes a quadruple with `make css` as the new step 3. `make
  css-check` joins "what done means" as item #5. New "Build pipeline
  operations" Hard Stops category covers `package.json` /
  `tailwind.config.js` / postcss config / font additions.
- **PHASE7a-2.md** — this report.

---

## Architectural decisions

### 1. Tailwind at build time, output committed

The compiled `admin.css` is committed. Anyone can `cargo run`
without Node. Anyone touching styles needs Node + `npm install`,
and `make css-check` (CI / pre-commit) catches drift between
`input.css` and the committed output.

The alternative — a `build.rs` that invokes Tailwind during
`cargo build` — was rejected because it forces every downstream
consumer to install Node just to compile.

### 2. Hybrid class strategy

Public-API class names (`.btn-primary`, `.module`, `.results`,
`.empty-list`, etc.) preserved as `@layer components` declarations
that `@apply` Tailwind utilities. Templates that override these by
name in a project-local `templates/` directory keep working.
Utilities used inside templates only for one-off layout.

The pure-utility alternative (Tailwind classes everywhere, no
component classes) would break every project that has custom
templates targeting the old class names.

### 3. Active sidebar state via JS, not threaded context

The naive design threads `current_path: String` through
`BaseContext::new` and 25 call sites. The chosen design is ~30
lines of inline JS that reads `window.location.pathname` and
toggles `sidebar-link-active` based on `data-active-prefix`. Same
visual outcome, much smaller diff, no risk of missing a call site.

### 4. Inter self-hosted as woff2

Four weights × ~24KB Latin subset = ~95KB binary growth. Two
alternatives rejected:

- **Google Fonts CDN** — breaks single-binary deploy.
- **System-only** — Inter's tracking, x-height, and weight balance
  are tuned-for; system stacks render the brand spec wrong.

### 5. Icons inline via custom minijinja function

16 lucide icons baked as Rust string constants. The `icon(name)`
minijinja function emits inline `<svg fill="none"
stroke="currentColor">`. No external icon library, no font icon
hack, no `<img src>` round trips. `currentColor` means hover/focus
state ripples through automatically.

### 6. Sidebar shows for any logged-in user

Sidebar visibility gated only on `identity` being set. Within the
sidebar, the Manage section's Users + Groups links are
`identity.is_admin`, and the Developer section is
`identity.is_developer`. A Staff user sees the sidebar with
Dashboard + their accessible models, no auth section, no developer
section. Forbidden + coming-soon pages render with the sidebar so
nav remains visible after the 403.

---

## Risks accepted

- **First clone without Node**: `make css` requires `npm install`.
  Mitigated by committing `admin.css` so `cargo run` works without
  it.
- **Drift between input.css and admin.css**: `make css-check`
  catches it before commit; recommend pre-commit hook adoption per
  project.
- **Doc churn within 48 hours**: `docs/brand.md` adopted on
  2026-04-26 with "Plain CSS only — still true"; implemented in
  this phase, that line flips to "superseded". Honest dating, no
  history rewriting.
- **Accessibility — sidebar mobile drawer**: ~30 lines of JS toggles
  `hidden`/`flex` classes. Works for keyboard + mouse. Screen
  readers see the `aria-current="page"` from active-state JS. Full
  audit (focus trap, ESC to close, etc.) deferred until a real
  accessibility pass.
- **Tailwind major-version churn**: locked to `^3.4`. Tailwind 4 is
  a different language; upgrade is a future phase.

---

## Verification

```text
$ cargo check --workspace
    Finished `dev` profile

$ cargo test -p rustio-core --lib
test result: ok. 329 passed; 0 failed; 41 ignored

$ cargo clippy --workspace --all-targets
    Finished `dev` profile

$ make css-check
css in sync
```

### Live curl spot-check (running blog as `developer@rustio.local`)

| Check | Result |
|---|---|
| `GET /admin/login` | renders, no sidebar (correct, anonymous) |
| `GET /admin` (logged in) | sidebar renders with all 8 links + dashboard |
| Inter font routes (4 weights) | each returns 200 |
| `GET /static/admin.css` | minified Tailwind output, 26KB |
| Each sidebar link has `data-active-prefix` | confirmed |
| Each sidebar icon is inline `<svg>` | confirmed |
| `is_developer` flag flips Developer section visibility | confirmed |

Total Rust LOC delta across the five commits:
- ~150 lines added (icon catalogue, font helpers, IdentityCtx field)
- ~5 lines removed (the dead `has_icon` helper)

CSS: `input.css` is 286 lines; compiled `admin.css` is 28KB minified.

Test count: 325 → **329 passing** (+4: 3 icon tests + 1 minijinja
icon function test). 0 failures. 41 ignored (PG-gated, unchanged).

---

## Phase 7a/1 readiness

Phase 7a/1 (the tolkhuset crate skeleton) was deferred so the
redesign could land first. With the new design system in place,
tolkhuset's first commit is still a single-file affair:

```rust
let admin = Admin::new()
    .site_branding(SiteBranding {
        site_title: "Tolkhuset administration".into(),
        site_header: "Tolkhuset administration".into(),
        index_title: "Tolkhuset interpreter management".into(),
        footer_copyright: "© 2026 Tolkhuset AB. Powered by RustIO.".into(),
        domain: "tolkhuset.test".into(),
    });
```

…and the new sidebar shell, brand palette, Inter font, and 16-icon
catalogue all flow through automatically. No extra wiring needed.

Tolkhuset gets the redesigned admin from day 1, not a re-skin in
two weeks.

---

*Phase 7a/2 closed. Working tree clean. Ready for Phase 7a/1.*
