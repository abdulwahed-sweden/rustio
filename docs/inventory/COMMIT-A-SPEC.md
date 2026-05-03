# Commit A — implementation spec

`style(admin): teal brand migration + Windmill design discipline`

Pre-implementation review document. Awaiting authorization. **No edits
performed yet.**

---

## Pre-implementation addendum (post-approval notes)

### Note A — Blue focus ring is an active behavior change

`<html>` carries no theme class on either admin or public templates,
so the `.theme-brand` / `.theme-rust` overrides at `input.css:112–125`
are dormant. The bare `:root` rule is what every page resolves
against today. That means `:root --ds-color-accent: 59 130 246`
(`#3B82F6` — blue) is the active accent value, and the keyboard
`:focus-visible` ring (`input.css:182`: `@apply ring-2 ring-accent/30
outline-none`) currently renders **blue at 30% opacity** on every
focused input / link / button. The user wasn't seeing this because
focus-visible only fires on keyboard navigation, not mouse clicks.

After Edit 2.1 flips `:root --ds-color-accent` to `13 148 136`, the
keyboard focus ring will render brand teal at 30% opacity — a real
visible change for keyboard-navigation users. This must be called
out in the commit message body.

**WCAG AA contrast verification** (computed via Python sRGB→linear
→relative-luminance pipeline; `(L1+0.05)/(L2+0.05)` formula):

| Foreground | Background | Ratio | WCAG AA non-text (≥3:1) |
|---|---|---|---|
| `#0d9488` (brand-600) | `#ffffff` (page surface) | **3.744:1** | PASS |
| `#0d9488` | `#f9fafb` (gray-50, hover bg) | **3.583:1** | PASS |
| `#0d9488` | `#f3f4f6` (gray-100, avatar bg) | **3.402:1** | PASS |
| `#0d9488` | `#f0fdfa` (brand-50, selection bg) | **3.590:1** | PASS |

All four pass WCAG AA non-text-component (3:1). No accessibility
regression vs the old rust `#B84318` baseline (which would have had
similar luminance, ~5.6:1 against white — slightly higher because
deep-orange is darker than mid-teal, but the post-migration ratio
is still comfortably above the 3:1 floor).

The 30% opacity used by `:focus-visible` softens the visible ring but
does not change the WCAG calculation, which is performed on the
solid color of the indicator against its adjacent unfocused state.
The 12% opacity `--rio-ring` used by `.field input:focus` is a
softer halo on top of the existing 1.5px gray-200 input border —
the focus indicator's visual weight comes from the border-color
swap to `var(--rio-accent)` (the solid teal), not the rgba halo.

### Note B — `input.css` lines 67 and 110: deferred-with-reason

Decision #3 (comment scrub) explicitly excluded these two lines
from the scrub list. They describe the `.theme-rust` *selector*,
not the rust visual:

- Line 67: `Layer values for light/rust themes are intentionally
  empty` — refers to the `.theme-rust` selector at line 113 by name.
- Line 110: `brand-accented theme. .theme-rust is kept as a
  backward-compatible alias` — documents the bw-compat alias
  mechanism for the selector.

The `.theme-rust` selector itself is preserved (Decision #3 defer);
its accent value flips to teal (Edit 2.2). The comments remain
factually accurate: there is still a `.theme-rust` selector, and
its layer values are still intentionally empty. Scrubbing these
comments while the selector still exists would create a worse
inconsistency than the one we're solving — a future reader would
see the selector and have no documented reason for it.

When Phase 11/b deletes the `.theme-rust` selector (along with
sweeping `text-rust` template references), the matching comments
get scrubbed in the same commit. **Skip both for Commit A.**

---

## Pre-flight safety check results

### Check 1 — Decision #3 scrub-safety: every "rust" reference classified

**`rustio-core/assets/templates/admin/base.html`** — 12 matches:

| Line | String | Classification | Action |
|---|---|---|---|
| 15 | `* RustIO admin design system` | Framework name (proper noun) | **DO NOT TOUCH** |
| 28 | `compiled from rustio-core/assets/css/` | File path | **DO NOT TOUCH** |
| 297 | `KEEP the 3px rust left-stripe (improvement #2)` | Color comment | **SCRUB** |
| 440 | `Buttons — primary now uses rust accent (was solid black).` | Color comment | **SCRUB** |
| 441 | `In an 8-hour-a-day admin, the rust anchor on Save/Add gives` | Color comment | **SCRUB** |
| 832 | `rust-tinted circle. Used on user_view.html. */` | Color comment | **SCRUB** |
| 867 | `uses the rust accent so it reads as the same affordance` | Color comment | **SCRUB** |
| 1568 | `RustIO {{ footer_copyright|replace('RustIO ', '')|replace('rustio ', '') }}` | Framework name + jinja logic | **DO NOT TOUCH** |
| 1594 | `· powered by RustIO {{...}}` | Framework name | **DO NOT TOUCH** |
| 1596 | `https://github.com/abdulwahed-sweden/rustio#readme` | URL | **DO NOT TOUCH** |
| 1597 | `https://github.com/abdulwahed-sweden/rustio/blob/main/CHANGELOG.md` | URL | **DO NOT TOUCH** |
| 1598 | `https://github.com/abdulwahed-sweden/rustio` | URL | **DO NOT TOUCH** |

**`rustio-core/assets/templates/admin/login.html`** — 1 match:

| Line | String | Classification | Action |
|---|---|---|---|
| 6 | `of the admin (white card, rust-accent submit, light topbar brand).` | Color comment | **SCRUB** |

**`rustio-core/assets/css/input.css`** — 11 matches:

| Line | String | Classification | Action |
|---|---|---|---|
| 1 | `RustIO admin — Tailwind input.` | Framework name | **DO NOT TOUCH** |
| 4 | `rustio-core/assets/static/css/admin.css` | File path | **DO NOT TOUCH** |
| 11 | `rustio-core/assets/templates/admin/base.html` | File path | **DO NOT TOUCH** |
| 67 | `Layer values for light/rust themes are intentionally empty` | Comment describing the `.theme-rust` selector (selector kept per Decision #3 defer; comment factually still accurate) | **DO NOT TOUCH** |
| 110 | `brand-accented theme. .theme-rust is kept as a backward-compatible` | Comment documenting the bw-compat alias mechanism (load-bearing — explains why the selector is kept) | **DO NOT TOUCH** |
| 113 | `.theme-rust {` | CSS selector (deferred per Decision #3) | **DO NOT TOUCH** |
| 172 | `a { @apply text-rust hover:text-rust-hover transition-colors; }` | CSS rule using the `text-rust` Tailwind utility | **REWRITE** under principle 10 / Decision #1 (using `text-brand-700` instead of `text-rust`) — not a "scrub", a principle edit |
| 173 | `::selection { @apply bg-rust text-white; }` | CSS rule | **REWRITE** to use `bg-brand-600` (selection IS an affordance — principle 10) |
| 198 | `.breadcrumbs a { ... hover:text-rust ... }` | CSS rule | **REWRITE** under principle 10 (breadcrumbs decoration → `hover:text-gray-900`) |
| 216 | `.results tr:hover td { @apply bg-rust/5; }` | CSS rule | **REWRITE** under principle 10 (`bg-gray-50`) |
| 241 | `.action-checkbox input ... { ... text-rust focus:ring-accent ... }` | CSS rule | **REWRITE** to `text-brand-600` (checkbox accent IS an affordance — selection state) |
| 246 | `.required { @apply text-rust font-semibold ml-0.5; }` | CSS rule | **REWRITE** to `text-brand-700` (Decision #1 — required marker stays branded) |

**Net scrub set: 6 comment lines** (5 in `base.html` + 1 in `login.html`). The 7 input.css rule edits are not "scrubs" — they are principle edits that happen to also remove `text-rust` / `bg-rust` from those rules. The `text-rust` / `bg-rust` Tailwind utilities themselves remain registered in `tailwind.config.js` (aliased to brand teal per Decision #3 defer), so the 4 templates that still call `text-rust` keep working.

### Check 2 — Decision #7 selector-specificity audit

**`.results` rule sites:**

- `rustio-core/assets/css/input.css` — 12 references at lines 205, 209, 212, 215, 216, 217, 218, 221, 222, 223. Sole authoritative definition site.
- `rustio-core/assets/templates/admin/base.html:67` — comment only (`.results and .row-clickable are legacy bridges in...`), no rule.
- `rustio-core/assets/static/css/admin.css` — compiled output, regenerated automatically.
- No other source files define `.results` rules.

**Specificity calculation for the split:**

| Selector | Specificity | Source line (post-edit) |
|---|---|---|
| `.results th, .results td` | (0,0,1,1) per branch | line 209 (combined: padding + alignment, **no border**) |
| `.results th` | (0,0,1,1) | line 212 (existing — bg + text + new `border-b border-gray-200`) |
| `.results td` | (0,0,1,1) | NEW line — text-small + `border-b border-gray-100` |
| `.results.row-clickable td` | (0,0,2,1) | line 217 (unchanged — `p-0` zeroes padding only, doesn't touch border) |
| `.results tr:hover td` | (0,0,1,2) | line 216 (rewritten to `bg-gray-50`) |

All `.results th` and `.results td` rules share specificity (0,0,1,1); source order resolves. The new `.results td { ... border-b border-gray-100 }` must land **after** the combined `.results th, .results td` rule. Recommendation: insert immediately after the existing `.results th { ... }` (line 212) so the order reads: combined-shared → th-specific → td-specific.

**Consumer impact (verified):**

- `users_list.html:36` — `<table class="results row-clickable">`. The `.results.row-clickable td { @apply p-0 }` rule (line 217, specificity 0,0,2,1) overrides padding only. New `border-b border-gray-100` from the td-specific rule still applies. ✓
- `log_entries.html:18` — `<table class="results">`. New rules apply uniformly. ✓
- `object_history.html:21` — `<table class="results">`. Same. ✓

**No conflict found. Split is safe.**

---

## Files in edit order

5 files. Build pipeline regenerates the 5th automatically.

1. `tailwind.config.js` — palette swap (brand added, rust aliased, teal-cyan dropped, shadow rgbs updated).
2. `rustio-core/assets/css/input.css` — token flip + principle 1–11 component edits.
3. `rustio-core/assets/templates/admin/base.html` — token flip + principle 1–11 inline-`<style>` edits + 5 comment scrubs.
4. `rustio-core/assets/templates/admin/login.html` — 1 comment scrub.
5. `rustio-core/assets/static/css/rustio.css` — public-site `--accent` flip.
6. `rustio-core/assets/static/css/admin.css` — **regenerated** by `make css` after edits 1–5.

---

## File 1: `tailwind.config.js`

### Edit 1.1 — Add brand palette + drop legacy `teal` key + alias `rust` to brand teal

**Before** (lines 32–77):

```js
      colors: {
        // ... semantic tokens unchanged ...
        bg:             "rgb(var(--ds-color-bg) / <alpha-value>)",
        // ... (lines 39–56 unchanged) ...
        "layer-4":      "rgb(var(--ds-color-layer-4) / <alpha-value>)",

        // ---------------------------------------------------------------
        // Legacy brand colors. Phase 7a/2 templates still reference
        // these (text-rust, bg-paper, bg-metal-surface, badge-teal, …).
        // Kept intact so Phase 2 lands without forcing a same-commit
        // template refactor; the next sub-phase moves call sites to
        // the semantic tokens above and prunes this block.
        // ---------------------------------------------------------------
        rust: {
          DEFAULT: "#aa4422",
          hover:   "#88331b",
          glow:    "#d46644",
        },
        paper: "#f9f8f6",
        metal: {
          DEFAULT: "#2c303a",
          dark:    "#181a1f",
          surface: "#22252b",
        },
        teal: "#338899",
      },
```

**After:**

```js
      colors: {
        // ... semantic tokens unchanged ...
        bg:             "rgb(var(--ds-color-bg) / <alpha-value>)",
        // ... (lines 39–56 unchanged) ...
        "layer-4":      "rgb(var(--ds-color-layer-4) / <alpha-value>)",

        // ---------------------------------------------------------------
        // Brand palette — teal #0d9488 (Phase 11/a, replaces rust orange).
        // The 50/100/600/700 scale is what every brand-anchored component
        // pulls from. Soft tints at 50/100 are reserved for backgrounds
        // (active-row tint, soft chip), 600 for primary affordances
        // (button, active sidebar, focus ring), 700 for hover/active
        // states + body-text links.
        // ---------------------------------------------------------------
        brand: {
          50:  "#f0fdfa",
          100: "#ccfbf1",
          600: "#0d9488",
          700: "#0f766e",
        },

        // ---------------------------------------------------------------
        // Legacy aliases — the `rust` palette key resolves to brand teal
        // for backward compatibility. Four templates still reference
        // `text-rust` / `hover:text-rust-hover` (log_entries.html,
        // object_history.html, confirm_delete.html); they keep rendering
        // through this alias until a follow-up commit sweeps them to
        // `text-brand-700`.
        // ---------------------------------------------------------------
        rust: {
          DEFAULT: "#0d9488",  // = brand-600
          hover:   "#0f766e",  // = brand-700
          glow:    "#5eead4",  // = teal-300 (lighter accent)
        },
        paper: "#f9f8f6",
        metal: {
          DEFAULT: "#2c303a",
          dark:    "#181a1f",
          surface: "#22252b",
        },
        // (legacy `teal: "#338899"` removed — that was the Phase 7a/2
        //  cyan-teal, unused in current admin templates and would clash
        //  semantically with the new brand teal.)
      },
```

**Rationale:** Adds the canonical `brand-{50,100,600,700}` palette per spec. Aliases the legacy `rust.*` keys to brand teal (Decision #3 defer). Removes the unused legacy `teal: "#338899"` (Decision #2).

### Edit 1.2 — Update legacy `boxShadow` rgba values from rust to brand teal

**Before** (lines 146–155):

```js
      boxShadow: {
        card:     "0 2px 6px rgba(0,0,0,0.08)",
        dropdown: "0 12px 24px rgba(0,0,0,0.12)",
        modal:    "0 24px 48px rgba(0,0,0,0.18)",
        // Legacy
        mark:        "0 1px 3px rgba(170,68,34,0.4)",
        btn:         "0 1px 2px rgba(170,68,34,0.3)",
        "btn-hover": "0 4px 10px rgba(170,68,34,0.25)",
        "card-hover":"0 8px 20px rgba(0,0,0,0.04)",
      },
```

**After:**

```js
      boxShadow: {
        card:     "0 2px 6px rgba(0,0,0,0.08)",
        dropdown: "0 12px 24px rgba(0,0,0,0.12)",
        modal:    "0 24px 48px rgba(0,0,0,0.18)",
        // Legacy — brand-tinted (rgb(13,148,136) = brand-600).
        mark:        "0 1px 3px rgba(13,148,136,0.4)",
        btn:         "0 1px 2px rgba(13,148,136,0.3)",
        "btn-hover": "0 4px 10px rgba(13,148,136,0.25)",
        "card-hover":"0 8px 20px rgba(0,0,0,0.04)",
      },
```

**Rationale:** Three legacy shadow utilities (`shadow-mark`, `shadow-btn`, `shadow-btn-hover`) currently emit a rust-orange rgba glow. Flip to brand teal rgba so any future call site produces a brand-consistent shadow. (Note: these utilities are not currently consumed by `assets/templates/admin/`; verified by grep. Updating defensively.)

---

## File 2: `rustio-core/assets/css/input.css`

### Edit 2.1 — Flip `:root --ds-color-accent` from blue placeholder to brand teal (load-bearing)

**Before** (lines 71–87):

```css
  :root {
    /* themes.light from docs/design-system.json */
    --ds-color-bg:             244 246 251; /* #F4F6FB */
    /* ... */
    --ds-color-accent:          59 130 246; /* #3B82F6 */
    /* ... */
  }
```

**After:**

```css
  :root {
    /* themes.light from docs/design-system.json */
    --ds-color-bg:             244 246 251; /* #F4F6FB */
    /* ... */
    --ds-color-accent:          13 148 136; /* #0D9488 — brand-600 */
    /* ... */
  }
```

**Rationale:** Active because `<html>` carries no theme class — the bare `:root` is what every admin page resolves against. The `:focus-visible` keyboard ring (line 182) and `.action-checkbox input` focus state (line 241) read this token. Flipping it makes keyboard focus rings render in brand teal across the admin (principle 10 affordance).

### Edit 2.2 — Flip `.theme-brand` / `.theme-rust` accent to brand teal

**Before** (lines 112–125):

```css
  .theme-brand,
  .theme-rust {
    --ds-color-bg:             255 250 248; /* #FFFAF8 */
    /* ... */
    --ds-color-accent:         194  65  12; /* #C2410C */
    /* ... */
  }
```

**After:**

```css
  .theme-brand,
  .theme-rust {
    --ds-color-bg:             255 250 248; /* #FFFAF8 */
    /* ... */
    --ds-color-accent:          13 148 136; /* #0D9488 — brand-600 */
    /* ... */
  }
```

**Rationale:** The `.theme-rust` selector is preserved (Decision #3 defer); flip the accent value so any consumer that toggles the theme class gets brand teal. The optional brand-themed surface tints (`--ds-color-bg`, `--ds-color-surface-muted`, `--ds-color-border`) stay as the warm pinkish-cream `#FFFAF8` family — those are surface-tone choices independent of the accent. Acceptable until a future commit revisits theme surfaces.

### Edit 2.3 — Global `a` rule: brand teal links + plain hover (Decision #1 conservative)

**Before** (line 172):

```css
  a { @apply text-rust hover:text-rust-hover transition-colors; }
```

**After:**

```css
  a { @apply text-brand-700 hover:text-brand-700 hover:underline transition-colors; }
```

**Rationale:** Decision #1 — body links read teal-700 for scannability; hover keeps the same color but adds an underline (no fill, no color shift). This makes the link discoverable without using brand-100 as a hover background.

### Edit 2.4 — `::selection` keeps brand (selection IS affordance)

**Before** (line 173):

```css
  ::selection { @apply bg-rust text-white; }
```

**After:**

```css
  ::selection { @apply bg-brand-600 text-white; }
```

**Rationale:** Selection highlight is an affordance ("this text is selected"). Principle 10 keeps brand on selection states.

### Edit 2.5 — Breadcrumbs hover (decoration → neutral)

**Before** (line 198):

```css
  .breadcrumbs a { @apply text-gray-600 hover:text-rust no-underline transition-colors; }
```

**After:**

```css
  .breadcrumbs a { @apply text-gray-600 hover:text-gray-900 no-underline transition-colors; }
```

**Rationale:** Breadcrumbs are navigation chrome, not action links. Hover should darken (gray-900), not adopt brand color. Principle 10 — remove brand-600 from non-affordance link contexts.

### Edit 2.6 — `.results` table: drop outer border (principle 9), set padding/text-align

**Before** (lines 205–208):

```css
  .results {
    @apply w-full border-collapse bg-surface border border-border rounded-md text-body overflow-hidden;
    table-layout: fixed;
  }
```

**After:**

```css
  .results {
    @apply w-full border-collapse bg-surface rounded-md text-body overflow-hidden;
    table-layout: fixed;
  }
```

**Rationale:** Principle 9 — table wrapper has no outer border, only internal dividers + shadow-sm. The `bg-surface` + `rounded-md` keep the card-like silhouette; the parent template provides shadow if needed (legacy `.results` is rendered raw, but its container in `users_list.html` and `log_entries.html` doesn't have a wrapper — accept that as part of the legacy class's minimal styling).

### Edit 2.7 — Split `.results th, td` border declaration (Decision #7)

**Before** (lines 209–214):

```css
  .results th, .results td {
    @apply px-4 py-3.5 text-left border-b border-gray-100;
  }
  .results th {
    @apply bg-surface-muted text-metal font-semibold text-caption uppercase tracking-wider;
  }
```

**After:**

```css
  .results th, .results td {
    @apply px-4 py-4 text-left;
  }
  .results th {
    @apply bg-surface-muted text-gray-500 font-semibold text-caption uppercase tracking-wider border-b border-gray-200;
  }
  .results td {
    @apply text-small border-b border-gray-100;
  }
```

**Rationale:**

- `py-3.5` (14px) → `py-4` (16px) — principle 4 (table row padding 16–20px, target ~64px).
- Header color `text-metal` (`#2c303a`, near-black) → `text-gray-500` (`#6b7280`) — principle 2.
- Header divider `border-gray-100` → `border-gray-200` — Decision #7 (heavier line at header→body junction).
- New `td` rule: `text-small` (14px) — principle 3 (primary cell 14px). Body divider `border-gray-100` retained.
- Specificity safe (audited above).

### Edit 2.8 — `.results` row-hover: brand → neutral gray (principle 10)

**Before** (line 216):

```css
  .results tr:hover td { @apply bg-rust/5; }
```

**After:**

```css
  .results tr:hover td { @apply bg-gray-50; }
```

**Rationale:** Principle 10 — table row hover is decoration, not affordance. Replace rust 5% tint with `bg-gray-50` (`#f9fafb`).

### Edit 2.9 — `.results.row-clickable .row-link` padding bump (principle 4)

**Before** (lines 218–220):

```css
  .results.row-clickable .row-link {
    @apply block px-4 py-3.5 text-metal no-underline transition-colors;
  }
```

**After:**

```css
  .results.row-clickable .row-link {
    @apply block px-4 py-4 text-metal no-underline transition-colors;
  }
```

**Rationale:** Principle 4 — clickable rows put padding on the inner anchor (since the parent `td` is `p-0`). Same 14→16px bump as the th/td rule above.

### Edit 2.10 — `.results.row-clickable .row-link.help` font-size (principle 3)

**Before** (line 223):

```css
  .results.row-clickable .row-link.help { @apply text-gray-600 text-small; }
```

**After:**

```css
  .results.row-clickable .row-link.help { @apply text-gray-600 text-[13px] leading-[1.5]; }
```

**Rationale:** Principle 3 — secondary cell 13px (currently 14px via `text-small`). Tailwind has no `text-13` utility in the current config; arbitrary value `text-[13px]` is the cleanest path. `leading-[1.5]` preserves the existing line-height.

### Edit 2.11 — `.action-checkbox input`: brand checkbox accent

**Before** (line 241):

```css
  .action-checkbox input[type=checkbox] { @apply w-4 h-4 rounded border-gray-300 text-rust focus:ring-accent; }
```

**After:**

```css
  .action-checkbox input[type=checkbox] { @apply w-4 h-4 rounded border-gray-300 text-brand-600 focus:ring-accent; }
```

**Rationale:** Checkbox accent (the inner check tick color) is an affordance — selection state. Principle 10 keeps brand here. `focus:ring-accent` already pulls from `--ds-color-accent` (Edit 2.1) so no change needed there.

### Edit 2.12 — `.required` marker: brand-700 (Decision #1)

**Before** (line 246):

```css
  .required { @apply text-rust font-semibold ml-0.5; }
```

**After:**

```css
  .required { @apply text-brand-700 font-semibold ml-0.5; }
```

**Rationale:** Decision #1 — required marker is an affordance (which fields *must* be filled), keep brand color, use 700 weight to match link color.

### Edit 2.13 — `.message-success` flash banner: emerald → green (principle 11)

**Before** (line 257):

```css
  .message-success { @apply bg-emerald-50 text-emerald-800 border-emerald-200; }
```

**After:**

```css
  .message-success { @apply bg-green-50 text-green-800 border-green-200; }
```

**Rationale:** Principle 11 + the user's "no emerald in source" rule. Success-flash messages are semantically GREEN (not the brand teal). Tailwind's `green-50/200/800` are `#f0fdf4` / `#bbf7d0` / `#166534` — the same hexes already used by `base.html` `.alert--success` (lines 775) and `.badge-yes-v14` (line 674), so the rendered color matches the existing v14 success palette. Removes the only literal `emerald` reference from the input.css source.

---

## File 3: `rustio-core/assets/templates/admin/base.html`

The inline `<style>` block edits, in source order. 21 edits.

### Edit 3.1 — Brand tokens (§1)

**Before** (lines 117–121):

```css
            /* Brand (unchanged) */
            --rio-accent:          #b8431a;
            --rio-accent-bg:       #fff4ed;
            --rio-accent-border:   #fed7aa;
            --rio-ring:            0 0 0 3px rgba(184,67,26,0.12);
```

**After:**

```css
            /* Brand — teal #0d9488 (Phase 11/a). */
            --rio-accent:          #0d9488;
            --rio-accent-bg:       #f0fdfa;
            --rio-accent-border:   #ccfbf1;
            --rio-ring:            0 0 0 3px rgba(13,148,136,0.12);
```

**Rationale:** Single-point re-anchor. Every component below that uses `var(--rio-accent)` / `--rio-accent-bg` / `--rio-accent-border` / `--rio-ring` flips to brand teal in one stroke.

### Edit 3.2 — Rename `--emerald-*` aliases to `--brand-*` (§1.5)

**Before** (lines 144–150):

```css
            --emerald-50:     var(--rio-accent-bg);
            --emerald-100:    var(--rio-accent-border);
            --emerald-200:    var(--rio-accent-border);
            --emerald-500:    var(--rio-accent);
            --emerald-600:    var(--rio-accent);
            --emerald-700:    var(--rio-accent);
            --emerald-800:    var(--rio-accent);
```

**After:**

```css
            --brand-50:       var(--rio-accent-bg);
            --brand-100:      var(--rio-accent-border);
            --brand-200:      var(--rio-accent-border);
            --brand-500:      var(--rio-accent);
            --brand-600:      var(--rio-accent);
            --brand-700:      var(--rio-accent);
            --brand-800:      var(--rio-accent);
```

**Rationale:** Removes the only literal "emerald" identifier from `base.html`. The alias-to-rio-accent mechanism is preserved exactly; only the names change. All consumers of `var(--emerald-XXX)` (lines 1153, 1180, 1182, 1308, 1309, 1328, 1329, 1377, 1381, 1449, 1453) get updated below.

### Edit 3.3 — Update §1.5 prelude comment (lines 123–132)

**Before** (lines 123–132):

```css
            /* §1.5 — Phase 10/b — Tailwind-UI palette aliases ================
             * The splitview / tabs / timeline / show-grid / stat-strip
             * vocabulary was authored against Tailwind v3's `--gray-*` /
             * `--emerald-*` / status / `--shadow-*` / `--ring-*` / `--rounded-*`
             * tokens. Preserving those names keeps the structural CSS
             * verbatim from the original Tailwind UI authoring; emerald
             * shades alias to `--rio-accent` so the new components inherit
             * the framework brand instead of bleeding emerald in. Existing
             * pages don't touch these — they keep using `--rio-*` directly.
             */
```

**After:**

```css
            /* §1.5 — Tailwind-UI palette aliases ================
             * The splitview / tabs / timeline / show-grid / stat-strip
             * vocabulary was authored against Tailwind v3's `--gray-*` /
             * `--shadow-*` / `--ring-*` / `--rounded-*` tokens. Preserving
             * those names keeps the structural CSS verbatim from the
             * original Tailwind UI authoring. The brand-* shades alias to
             * `--rio-accent` so the new components inherit the framework
             * brand. Existing pages don't touch these — they keep using
             * `--rio-*` directly.
             *
             * Phase 11/a: renamed `--emerald-*` → `--brand-*` to match the
             * teal brand migration; the alias mechanism is unchanged.
             */
```

**Rationale:** The current comment names "emerald" as the vocabulary; post-rename it lies. Updates accurately.

### Edit 3.4 — `--ring-emerald` → `--ring-brand` (line 174)

**Before** (line 174):

```css
            --ring-emerald:   inset 0 0 0 2px var(--rio-accent);
```

**After:**

```css
            --ring-brand:     inset 0 0 0 2px var(--rio-accent);
```

**Rationale:** Naming consistency with the `--brand-*` rename. Sole consumer at line 1153 will be updated below.

### Edit 3.5 — Sidebar comment scrub (line 297)

**Before** (line 297):

```css
        /* Active item — KEEP the 3px rust left-stripe (improvement #2) */
```

**After:**

```css
        /* Active item — KEEP the 3px brand left-stripe (improvement #2) */
```

**Rationale:** Decision #3 scrub.

### Edit 3.6 — Page-head h1 letter-spacing (principle 1)

**Before** (lines 370–374):

```css
        .page-head h1 {
            font-size: 32px; font-weight: var(--rio-weight-heading); margin: 0 0 8px;
            color: var(--rio-text); letter-spacing: -0.02em; line-height: 1.15;
            font-family: var(--rio-font-heading);
        }
```

**After:**

```css
        .page-head h1 {
            font-size: 32px; font-weight: var(--rio-weight-heading); margin: 0 0 8px;
            color: var(--rio-text); letter-spacing: -0.018em; line-height: 1.15;
            font-family: var(--rio-font-heading);
        }
```

**Rationale:** Principle 1 — letter-spacing exact value `-0.018em`.

### Edit 3.7 — Buttons comment scrub + `.btn-primary` brand updates (lines 440–452)

**Before** (lines 440–452):

```css
        /* Buttons — primary now uses rust accent (was solid black).
           In an 8-hour-a-day admin, the rust anchor on Save/Add gives
           the eye a faster scan target than black-on-white. */
        .btn-primary {
            display: inline-flex; align-items: center; gap: 8px;
            padding: 11px 18px; background: var(--rio-accent);
            border: 1px solid var(--rio-accent); border-radius: 8px;
            color: #fff; font-size: 15px; font-weight: 600;
            cursor: pointer; box-shadow: 0 1px 2px rgba(184,67,26,0.18);
            font-family: inherit; text-decoration: none;
        }
        .btn-primary:hover { background: #9c3815; border-color: #9c3815; }
        .btn-primary:focus-visible { outline: 3px solid rgba(184,67,26,0.30); outline-offset: 2px; }
```

**After:**

```css
        /* Buttons — primary now uses brand accent (was solid black).
           In an 8-hour-a-day admin, the brand anchor on Save/Add gives
           the eye a faster scan target than black-on-white. */
        .btn-primary {
            display: inline-flex; align-items: center; gap: 8px;
            padding: 11px 18px; background: var(--rio-accent);
            border: 1px solid var(--rio-accent); border-radius: 8px;
            color: #fff; font-size: 15px; font-weight: 600;
            cursor: pointer; box-shadow: 0 1px 2px rgba(13,148,136,0.18);
            font-family: inherit; text-decoration: none;
        }
        .btn-primary:hover { background: #0f766e; border-color: #0f766e; }
        .btn-primary:focus-visible { outline: 3px solid rgba(13,148,136,0.30); outline-offset: 2px; }
```

**Rationale:**

- Comment scrub (Decision #3) — "rust" → "brand" twice.
- `box-shadow` rgba(184,67,26) (rust) → rgba(13,148,136) (brand teal).
- `:hover` background+border `#9c3815` → `#0f766e` (brand-700).
- `:focus-visible` outline rgba(184,67,26,0.30) → rgba(13,148,136,0.30).

### Edit 3.8 — `.tip .tip-head svg` icon color (principle 10)

**Before** (line 529):

```css
        .tip .tip-head svg { color: var(--rio-accent); }
```

**After:**

```css
        .tip .tip-head svg { color: var(--rio-text-muted); }
```

**Rationale:** Principle 10 — tip-card icon is decoration (visual hint), not an action affordance. Mute it to gray-600 (`--rio-text-muted: #4b5563`).

### Edit 3.9 — `.table thead th` color + tracking (principle 2) + padding (principle 4)

**Before** (lines 594–600):

```css
        .table thead th {
            background: #f3f4f6; font-size: 12px; font-weight: 600;
            color: var(--rio-text-secondary); text-align: left; padding: 14px 18px;
            border-bottom: 1px solid var(--rio-border);
            letter-spacing: 0.04em; text-transform: uppercase;
            position: sticky; top: 0; z-index: 1;
        }
```

**After:**

```css
        .table thead th {
            background: #f3f4f6; font-size: 12px; font-weight: 600;
            color: var(--rio-text-faint); text-align: left; padding: 16px 18px;
            border-bottom: 1px solid var(--rio-border);
            letter-spacing: 0.05em; text-transform: uppercase;
            position: sticky; top: 0; z-index: 1;
        }
```

**Rationale:**

- `color: --rio-text-secondary` (#1f2937 — very dark) → `--rio-text-faint` (#6b7280 ≈ gray-500) — principle 2.
- `padding: 14px 18px` → `padding: 16px 18px` — principle 4 (vertical padding).
- `letter-spacing: 0.04em` → `0.05em` — principle 2 exact tracking.

### Edit 3.10 — `.table tbody td` font-size + color + padding (principles 3, 4)

**Before** (lines 601–605):

```css
        .table tbody td {
            padding: 14px 18px; border-bottom: 1px solid var(--rio-border-faint);
            font-size: 15px; color: var(--rio-text-secondary); vertical-align: middle;
            transition: background 120ms ease;
        }
```

**After:**

```css
        .table tbody td {
            padding: 16px 18px; border-bottom: 1px solid var(--rio-border-faint);
            font-size: 14px; color: var(--rio-text); vertical-align: middle;
            transition: background 120ms ease;
        }
```

**Rationale:**

- `padding: 14px` → `16px` — principle 4.
- `font-size: 15px` → `14px` — principle 3 (primary cell 14px).
- `color: --rio-text-secondary` (#1f2937) → `--rio-text` (#111827, ≈ gray-900) — principle 3 (primary cell gray-900).

### Edit 3.11 — `.table-wrap` outer border drop (principle 9)

**Before** (lines 587–592):

```css
        /* Table */
        .table-wrap {
            background: var(--rio-bg-surface-1);
            border: 1px solid var(--rio-border); border-radius: 10px;
            box-shadow: var(--rio-shadow-card); overflow: hidden;
        }
```

**After:**

```css
        /* Table */
        .table-wrap {
            background: var(--rio-bg-surface-1);
            border-radius: 10px;
            box-shadow: var(--rio-shadow-card); overflow: hidden;
        }
```

**Rationale:** Principle 9 — drop outer border, keep shadow + radius.

### Edit 3.12 — `.table .row-link:hover` (decoration → underline only)

**Before** (lines 632–636):

```css
        .table .row-link {
            color: var(--rio-text); font-weight: 600; text-decoration: none;
            cursor: pointer;
        }
        .table .row-link:hover { color: var(--rio-accent); text-decoration: underline; }
```

**After:**

```css
        .table .row-link {
            color: var(--rio-text); font-weight: 600; text-decoration: none;
            cursor: pointer;
        }
        .table .row-link:hover { color: var(--rio-text); text-decoration: underline; }
```

**Rationale:** Principle 10 — table row link hover. The link itself is the affordance (the bold weight); hover should add an underline, not shift color. Avoid the brand-on-hover decoration. (Note: the row-link primary cell text stays brand-anchored via Decision #1's "links inside table cells: teal-700" — this rule keeps the cell at `var(--rio-text)` because the entire row is clickable; the user's mental model is "click the row" not "click the email". Defer color decision to a follow-up if data shows users click the email.)

### Edit 3.13 — `.main` padding (principle 5)

**Before** (line 318–321):

```css
        .main {
            flex: 1; padding: 32px 24px; background: var(--rio-bg); overflow-y: auto;
            display: flex; justify-content: center;
        }
```

**After:**

```css
        .main {
            flex: 1; padding: 32px 32px; background: var(--rio-bg); overflow-y: auto;
            display: flex; justify-content: center;
        }
```

**Rationale:** Principle 5 — main content padding 32–40px desktop. 32px hits the floor; conservative.

### Edit 3.14 — `.perm-grid` gap (principle 6)

**Before** (lines 869–874):

```css
        .perm-grid {
            display: grid; gap: 8px; grid-template-columns: 1fr;
        }
        @media (min-width: 640px) {
            .perm-grid { grid-template-columns: repeat(2, 1fr); }
        }
```

**After:**

```css
        .perm-grid {
            display: grid; gap: 24px; grid-template-columns: 1fr;
        }
        @media (min-width: 640px) {
            .perm-grid { grid-template-columns: repeat(2, 1fr); }
        }
```

**Rationale:** Principle 6 — card grid gap 24px (gap-6).

### Edit 3.15 — `.dashboard-models` gap (principle 6)

**Before** (lines 898–900):

```css
        .dashboard-models {
            display: grid; grid-template-columns: 1fr; gap: 8px;
        }
```

**After:**

```css
        .dashboard-models {
            display: grid; grid-template-columns: 1fr; gap: 24px;
        }
```

**Rationale:** Principle 6.

### Edit 3.16 — `.stat-strip` gap (principle 6)

**Before** (lines 1456–1461):

```css
        .stat-strip {
            display: grid;
            grid-template-columns: repeat(auto-fit, minmax(160px, 1fr));
            gap: 16px;
            margin: 0 0 24px;
        }
```

**After:**

```css
        .stat-strip {
            display: grid;
            grid-template-columns: repeat(auto-fit, minmax(160px, 1fr));
            gap: 24px;
            margin: 0 0 24px;
        }
```

**Rationale:** Principle 6.

### Edit 3.17 — `.stat-card` shadow + bg (principle 7)

**Before** (lines 1462–1467):

```css
        .stat-card {
            background: var(--gray-50);
            border-radius: var(--rounded-lg);
            padding: 14px 16px;
            box-shadow: var(--ring-black-5);
        }
```

**After:**

```css
        .stat-card {
            background: #ffffff;
            border-radius: var(--rounded-lg);
            padding: 14px 16px;
            box-shadow: var(--shadow-sm);
        }
```

**Rationale:** Principle 7 — stat card has shadow-sm only, no border, no ring. The ring-black-5 was an inset 1px-equivalent ring — drop it. Background flips to white so the card reads against the page bg via shadow alone (gray-50 + shadow-sm would lose the shadow's separation against the gray-50 page bg).

### Edit 3.18 — `.avatar` neutral palette (principle 10 decoration)

**Before** (lines 832–839):

```css
        /* Avatar disc — first letter of an email/identifier on a
           rust-tinted circle. Used on user_view.html. */
        .avatar {
            display: inline-flex; align-items: center; justify-content: center;
            width: 56px; height: 56px; border-radius: 50%;
            background: var(--rio-accent-bg); color: var(--rio-accent);
            font-weight: 700; font-size: 22px; font-family: var(--rio-font-heading);
            border: 1px solid var(--rio-accent-border);
        }
```

**After:**

```css
        /* Avatar disc — first letter of an email/identifier on a
           soft-gray circle. Used on user_view.html. */
        .avatar {
            display: inline-flex; align-items: center; justify-content: center;
            width: 56px; height: 56px; border-radius: 50%;
            background: var(--gray-100); color: var(--gray-700);
            font-weight: 700; font-size: 22px; font-family: var(--rio-font-heading);
        }
```

**Rationale:**

- Comment scrub (Decision #3) — "rust-tinted" → "soft-gray".
- Background `--rio-accent-bg` (brand-50) → `--gray-100` (#f3f4f6) — principle 10 (decoration uses neutral gray, not brand). Brand soft tint is reserved for selection states (active row, active tab).
- Color `--rio-accent` → `--gray-700` (#374151) — readable on gray-100 bg.
- Border line removed — principle 7-style minimalism, the soft-gray circle is enough silhouette.

### Edit 3.19 — `.perm-tile:hover` (decoration → neutral)

**Before** (lines 875–893):

```css
        .perm-tile {
            display: inline-flex; align-items: center; gap: 10px;
            padding: 8px 12px; border: 1px solid var(--rio-border);
            border-radius: 6px; background: var(--rio-bg-surface-1);
            cursor: pointer; font-size: 13px;
            transition: border-color 0.12s ease, background 0.12s ease;
        }
        .perm-tile:hover {
            border-color: var(--rio-accent-border);
            background: var(--rio-accent-bg);
        }
        .perm-tile input[type="checkbox"] {
            width: 16px; height: 16px; flex-shrink: 0; cursor: pointer;
            accent-color: var(--rio-accent);
        }
```

**After:**

```css
        .perm-tile {
            display: inline-flex; align-items: center; gap: 10px;
            padding: 8px 12px; border: 1px solid var(--rio-border);
            border-radius: 6px; background: var(--rio-bg-surface-1);
            cursor: pointer; font-size: 13px;
            transition: border-color 0.12s ease, background 0.12s ease;
        }
        .perm-tile:hover {
            border-color: var(--gray-300);
            background: var(--gray-50);
        }
        .perm-tile input[type="checkbox"] {
            width: 16px; height: 16px; flex-shrink: 0; cursor: pointer;
            accent-color: var(--rio-accent);
        }
```

**Rationale:**

- Hover bg/border swap to neutral — principle 10 (decoration → gray-50).
- The `accent-color: var(--rio-accent)` on the checkbox itself stays brand — checkbox tick IS an affordance (selection state).

### Edit 3.20 — `.dashboard-model:hover` + link hover (principle 10)

**Before** (lines 904–927):

```css
        .dashboard-model {
            display: flex; align-items: center; justify-content: space-between;
            border: 1px solid var(--rio-border); border-radius: 8px;
            padding: 10px 14px; background: var(--rio-bg-surface-1);
            transition: border-color 0.12s ease, background 0.12s ease;
        }
        .dashboard-model:hover {
            border-color: var(--rio-accent-border); background: var(--rio-accent-bg);
        }
        .dashboard-model-link {
            display: inline-flex; align-items: center; gap: 10px;
            color: var(--rio-text); font-weight: 600; text-decoration: none;
            font-size: 15px;
        }
        .dashboard-model-link:hover { color: var(--rio-accent); }
        .dashboard-model-link svg { color: var(--rio-text-faint); }
        .dashboard-model:hover .dashboard-model-link svg { color: var(--rio-accent); }
        .dashboard-model-actions {
            display: inline-flex; gap: 12px; font-size: 13px;
        }
        .dashboard-model-actions a {
            color: var(--rio-text-muted); text-decoration: none; font-weight: 500;
        }
        .dashboard-model-actions a:hover { color: var(--rio-accent); }
```

**After:**

```css
        .dashboard-model {
            display: flex; align-items: center; justify-content: space-between;
            border: 1px solid var(--rio-border); border-radius: 8px;
            padding: 10px 14px; background: var(--rio-bg-surface-1);
            transition: border-color 0.12s ease, background 0.12s ease;
        }
        .dashboard-model:hover {
            border-color: var(--gray-300); background: var(--gray-50);
        }
        .dashboard-model-link {
            display: inline-flex; align-items: center; gap: 10px;
            color: var(--rio-text); font-weight: 600; text-decoration: none;
            font-size: 15px;
        }
        .dashboard-model-link:hover { color: var(--rio-text); text-decoration: underline; }
        .dashboard-model-link svg { color: var(--rio-text-faint); }
        .dashboard-model:hover .dashboard-model-link svg { color: var(--rio-text); }
        .dashboard-model-actions {
            display: inline-flex; gap: 12px; font-size: 13px;
        }
        .dashboard-model-actions a {
            color: var(--rio-text-muted); text-decoration: none; font-weight: 500;
        }
        .dashboard-model-actions a:hover { color: var(--rio-text); text-decoration: underline; }
```

**Rationale:** Principle 10 — model rows are the dashboard's primary clickable content. Hover should darken neutrally + underline; brand-on-hover felt over-accented and competed with the active sidebar indicator. The model name link itself stays at `var(--rio-text)` (gray-900) — Decision #1 conservative reading: links INSIDE row hover targets read as part of the row, not as standalone teal links.

### Edit 3.21 — `.dashboard-recent` link hover (principle 10)

**Before** (lines 938–942):

```css
        .dashboard-recent li a {
            color: var(--rio-text); font-weight: 500;
            text-decoration: none; margin-left: 6px;
        }
        .dashboard-recent li a:hover { color: var(--rio-accent); }
```

**After:**

```css
        .dashboard-recent li a {
            color: var(--rio-text); font-weight: 500;
            text-decoration: none; margin-left: 6px;
        }
        .dashboard-recent li a:hover { color: var(--rio-text); text-decoration: underline; }
```

**Rationale:** Principle 10 — recent-activity feed link hover. Underline on hover, no color shift.

### Edit 3.22 — `.pane-list` search input focus ring rename

**Before** (line 1153):

```css
        .pane-list .pane-search input:focus { box-shadow: var(--ring-emerald); }
```

**After:**

```css
        .pane-list .pane-search input:focus { box-shadow: var(--ring-brand); }
```

**Rationale:** Token rename consistent with Edit 3.4. Behavior unchanged.

### Edit 3.23 — `.pane-list .row.is-active` brand selection state

**Before** (lines 1178–1182):

```css
        .pane-list .row.is-active {
            background: var(--emerald-50);
            box-shadow: inset 3px 0 0 0 var(--emerald-600);
        }
        .pane-list .row.is-active .row-name { color: var(--emerald-700); }
```

**After:**

```css
        .pane-list .row.is-active {
            background: var(--brand-50);
            box-shadow: inset 3px 0 0 0 var(--brand-600);
        }
        .pane-list .row.is-active .row-name { color: var(--brand-700); }
```

**Rationale:** Token rename. Visually unchanged (the alias points at `--rio-accent` which now = `#0d9488`). KEEP per principle 10 (selection IS affordance).

### Edit 3.24 — `.tabs > a.is-active` + `.tab-count` brand selection state

**Before** (lines 1306–1330):

```css
        .tabs > a.is-active, .tabs > .tab.is-active,
        .tabs > a[aria-current="page"], .tabs > .tab[aria-current="page"] {
            color: var(--emerald-600);
            border-bottom-color: var(--emerald-600);
            font-weight: 600;
        }
        .tabs .tab-count {
            display: inline-flex;
            /* ... unchanged ... */
            background: var(--gray-100);
            color: var(--gray-700);
            /* ... unchanged ... */
        }
        .tabs > .is-active .tab-count,
        .tabs > [aria-current="page"] .tab-count {
            background: var(--emerald-50);
            color: var(--emerald-700);
        }
```

**After:**

```css
        .tabs > a.is-active, .tabs > .tab.is-active,
        .tabs > a[aria-current="page"], .tabs > .tab[aria-current="page"] {
            color: var(--brand-600);
            border-bottom-color: var(--brand-600);
            font-weight: 600;
        }
        .tabs .tab-count {
            display: inline-flex;
            /* ... unchanged ... */
            background: var(--gray-100);
            color: var(--gray-700);
            /* ... unchanged ... */
        }
        .tabs > .is-active .tab-count,
        .tabs > [aria-current="page"] .tab-count {
            background: var(--brand-50);
            color: var(--brand-700);
        }
```

**Rationale:** Token rename. Active tab + active tab-count chip stay brand-anchored — selection affordance.

### Edit 3.25 — `.timeline > li.tl-success::before` (semantic green, NOT brand)

**Before** (line 1362):

```css
        .timeline > li.tl-success::before { box-shadow: inset 0 0 0 2px var(--emerald-600); }
```

**After:**

```css
        .timeline > li.tl-success::before { box-shadow: inset 0 0 0 2px var(--green-600); }
```

**Rationale:** Principle 11 — success is GREEN, not brand teal. The `--green-600` token is already defined at line 154 (`--green-600: #16a34a`). Other timeline kinds (`.tl-info` blue, `.tl-warning` yellow, `.tl-error` red) already use semantic colors; success was the outlier. Fix.

### Edit 3.26 — `.timeline .tl-text a` brand links

**Before** (lines 1376–1381):

```css
        .timeline .tl-text a {
            color: var(--emerald-600);
            text-decoration: none;
            font-weight: 500;
        }
        .timeline .tl-text a:hover { color: var(--emerald-700); }
```

**After:**

```css
        .timeline .tl-text a {
            color: var(--brand-700);
            text-decoration: none;
            font-weight: 500;
        }
        .timeline .tl-text a:hover { color: var(--brand-700); text-decoration: underline; }
```

**Rationale:**

- Decision #1 (links): teal-700 base color, hover-to-darker-no-fill (here we keep teal-700 and just add the underline since we don't have a brand-800).
- Token rename and decision-1 style applied together.

### Edit 3.27 — `.show-row > .show-v a` brand links (Decision #1)

**Before** (lines 1448–1453):

```css
        .show-row > .show-v a {
            color: var(--emerald-600);
            text-decoration: none;
            font-weight: 500;
        }
        .show-row > .show-v a:hover { color: var(--emerald-700); }
```

**After:**

```css
        .show-row > .show-v a {
            color: var(--brand-700);
            text-decoration: none;
            font-weight: 500;
        }
        .show-row > .show-v a:hover { color: var(--brand-700); text-decoration: underline; }
```

**Rationale:** Decision #1 — links inside show-grids: teal-700, underline-on-hover.

### Edit 3.28 — `.dashboard-recent` gap unchanged (Decision #6)

(Decision #6 confirmed: leave at `gap: 14px` — no edit.)

### Edit 3.29 — `.status-card h1` size unchanged (Decision #4)

(Decision #4 confirmed: leave at 22px — no edit.)

### Edit 3.30 — `.detail-name` size unchanged (Decision #5)

(Decision #5 confirmed: leave at 22px — no edit.)

### Edit 3.31 — Avatar `rust-tinted` comment + perm-tile + dashboard comments

(Already covered in Edits 3.18, 3.5, 3.7 above. The remaining comment scrubs:)

**Edit 3.31a — line 832 already covered in Edit 3.18.**

**Edit 3.31b — line 867 (perm-tile comment):**

**Before** (lines 866–868):

```css
        /* Permission tiles — used by group_edit.html. A 2-column
           responsive grid of checkbox tiles, each carrying a
           permission name in monospace. Tile hover/active state
           uses the rust accent so it reads as the same affordance
           as `.dashboard-model`. */
```

**After:**

```css
        /* Permission tiles — used by group_edit.html. A 2-column
           responsive grid of checkbox tiles, each carrying a
           permission name in monospace. Tile hover uses neutral
           gray; the checkbox `accent-color` carries the brand. */
```

**Rationale:** Comment scrub (Decision #3) + the comment now correctly describes the post-edit behavior (Edit 3.19 made the hover gray, the checkbox stays brand).

---

## File 4: `rustio-core/assets/templates/admin/login.html`

### Edit 4.1 — Line 6 comment scrub

**Before** (line 6):

```html
   of the admin (white card, rust-accent submit, light topbar brand).
```

**After:**

```html
   of the admin (white card, brand-accent submit, light topbar brand).
```

**Rationale:** Decision #3 scrub. The `submit` button on the login form uses `.btn-primary` which now renders teal — comment matches reality.

---

## File 5: `rustio-core/assets/static/css/rustio.css`

### Edit 5.1 — Public-site accent token flip

**Before** (lines 5–6):

```css
    --accent: #B84318;
    --accent-soft: #fff1ec;
```

**After:**

```css
    --accent: #0d9488;
    --accent-soft: #f0fdfa;
```

**Rationale:** The public-site stylesheet (used by non-admin shell `templates/base.html`) still resolves `--accent` against rust hex literals at every consumer site (lines 30, 50, 83, 84, 116, 118, 138, 225, 226, 294, 439, 440, 450, 511, 531, 532, 562). Flipping the two token values cascades brand teal across the public site without touching any of the consumer rules. Removes `#B84318` and `#fff1ec` from the codebase.

---

## File 6: `rustio-core/assets/static/css/admin.css`

**Regenerated** by `make css`. The committed `admin.css` must match a fresh Tailwind build from `input.css` (per `make css-check`). Run `make css` after edits 1–5 land; commit the regenerated file alongside.

---

## Estimated diff size (source files only — excludes regenerated `admin.css`)

| File | Lines added | Lines removed | Net delta |
|---|---|---|---|
| `tailwind.config.js` | ~24 | ~6 | +18 |
| `rustio-core/assets/css/input.css` | ~14 | ~12 | +2 |
| `rustio-core/assets/templates/admin/base.html` | ~30 | ~30 | ~0 |
| `rustio-core/assets/templates/admin/login.html` | 1 | 1 | 0 |
| `rustio-core/assets/static/css/rustio.css` | 2 | 2 | 0 |
| **Source total** | **~71** | **~51** | **+20** |
| `admin.css` (regenerated) | varies | varies | (≈ small — palette swap, token rename, decoration-color flips) |

---

## Acceptance criteria

Same shape as Phase 10/a:

- [ ] `cargo check --workspace` clean.
- [ ] `cargo test --workspace --lib` green — sandbox count 469 (matches snapshot).
- [ ] `cargo clippy --workspace --all-targets` clean.
- [ ] `RUSTIO_TEST_DB=1 cargo test --lib -- --ignored` green — PG-gated count 47.
- [ ] `make css-check` clean post-`make css` (committed `admin.css` matches fresh Tailwind build from `input.css`).
- [ ] Browser smoke matrix:
  - [ ] `/login` — submit button renders teal #0d9488, focus ring teal/30.
  - [ ] `/admin/` (dashboard) — sidebar active item shows brand teal 3px stripe + brand-50 bg + brand-700 text. `.dashboard-model:hover` shows `bg-gray-50` (NOT teal). Recent-activity link hover underlines without color shift.
  - [ ] `/admin/users` — table header text is gray-500 (not metal/dark), uppercase, tracking 0.05em. Row hover is `bg-gray-50` (NOT teal/5%). Each row 64px-ish height (16px py + content). Pager `.is-active` page renders teal.
  - [ ] `/admin/users/new` — primary "Save" button teal, secondary cancel button neutral, focus ring around inputs is brand teal at 30% opacity.
  - [ ] `/admin/users/<id>` — splitview active row in left pane shows `--brand-50` bg + 3px brand-600 inset stripe + brand-700 row name. Tabs active state teal underline + teal text + brand-50 count chip. Stat cards (Sessions / Activity / Last seen) render with shadow-sm only, white bg, no border, no ring (ring-black-5 gone).
  - [ ] `/admin/groups/<id>` — perm-tile hover shows `bg-gray-50` (NOT teal-tint).
  - [ ] `/admin/log-entries` — `.results` table headers gray-500, row hover gray-50.
  - [ ] `/error` and `/forbidden` — status-card `<h1>` 22px (unchanged).
  - [ ] Public-site (any non-admin page rendered with `templates/base.html`) — links and accent tokens render brand teal.
- [ ] **Negative grep verification (must return zero):**
  - [ ] `grep -rn "emerald" rustio-core/assets/` (excluding compiled `admin.css`).
  - [ ] `grep -rn "#059669\|#10b981\|#047857\|#a7f3d0\|#d1fae5\|#ecfdf5\|#6ee7b7\|#34d399" rustio-core/assets/css/ rustio-core/assets/templates/ rustio-core/assets/static/css/rustio.css`.
  - [ ] `grep -rEn "#B84318|#b8431a|#aa4422|#88331b|#9c3815|#fff1ec|#fff4ed|#fed7aa|rgba\(184|rgba\(170" rustio-core/assets/css/ rustio-core/assets/templates/ rustio-core/assets/static/css/rustio.css tailwind.config.js`.
  - [ ] `grep -in "rust" rustio-core/assets/templates/admin/base.html | grep -v RUSTIO_ | grep -v "powered by RustIO\|RustIO admin\|RustIO {{\|github.com/abdulwahed-sweden/rustio\|rustio-core/assets"` → expected zero (catches any stray un-scrubbed color comment).
- [ ] **Positive grep verification:**
  - [ ] `grep -n "#0d9488\|--brand-" rustio-core/assets/css/input.css rustio-core/assets/templates/admin/base.html` returns multiple hits.

---

## Commit message draft

```
phase 11/a: teal brand migration + Windmill design discipline

Re-anchor RustIO admin's brand color from rust orange (#B84318) to
teal #0d9488, and apply Windmill-style typography / spacing / border
discipline across the admin UI. Pure CSS / token / palette commit;
no template logic changes.

Why touching four-plus files for "one color change":

The admin's design tokens are spread across three CSS surfaces plus
a public-site stylesheet:

  1. tailwind.config.js          palette + utility class names
  2. rustio-core/assets/css/
       input.css                  semantic tokens (--ds-color-*),
                                  Tailwind-input + survivor
                                  components
  3. rustio-core/assets/
       templates/admin/base.html  the inline <style> block (1472 LOC,
                                  the v14 component design system,
                                  --rio-* tokens + every component
                                  rule)
  4. rustio-core/assets/static/
       css/rustio.css             public-site CSS (--accent token)

A clean migration re-anchors the brand at every entry point:

  - tailwind.config.js: add brand-{50,100,600,700} palette; alias the
    legacy `rust.*` keys to brand teal so the four templates that
    still reference `text-rust` keep working (sweep deferred to a
    follow-up commit). Drop unused legacy `teal: "#338899"` to avoid
    naming collision with the new brand.
  - input.css: flip `:root --ds-color-accent` from blue placeholder
    (#3B82F6) to teal — load-bearing because <html> carries no theme
    class, so :root is the active state and drives :focus-visible
    rings + .action-checkbox accent. Same flip on .theme-brand /
    .theme-rust (kept as bw-compat alias selectors).
  - base.html: rewrite --rio-accent / --rio-accent-bg / --rio-accent-
    border / --rio-ring at the §1 token block. Rename §1.5
    --emerald-* aliases to --brand-* (single grep+replace; alias
    mechanism unchanged). Update .btn-primary's hardcoded hover
    `#9c3815` and rgba(184,67,26,*) shadow / outline colors to brand
    teal hexes / rgba.
  - rustio.css: flip --accent / --accent-soft tokens; cascades to all
    17 consumer rules without per-site edits.

Windmill design discipline applied (12 principles):

  - typography: bump page-h1 letter-spacing to -0.018em; table
    headers move from text-metal to text-gray-500; primary cells
    drop from 15px to 14px and gain gray-900 color
  - spacing: table rows py-3.5 -> py-4 (target ~64px height); .main
    horizontal padding 24px -> 32px; .perm-grid / .dashboard-models
    / .stat-strip gap 8-16px -> 24px (gap-6)
  - borders: drop outer border on .table-wrap and .results
    (principle 9); .stat-card switches from inset ring to shadow-sm
    + white bg (principle 7); split .results th/td so header divider
    is gray-200 (heavier) and body dividers stay gray-100 (Decision #7)
  - color discipline (principle 10): brand-600 stays only on
    affordances (active-sidebar 3px stripe, .btn-primary, focus rings,
    .pager .is-active, .filters .pill.is-active, .pane-list .row.is-
    active, .tabs > .is-active, .show-grid action links). Removed
    from decoration sites: .results tr:hover (-> bg-gray-50), .table
    .row-link:hover (-> underline only), .tip svg (-> gray-600),
    .avatar (-> soft gray), .perm-tile:hover, .dashboard-model:hover,
    .dashboard-recent link hover, breadcrumbs hover.
  - global anchor color: links read brand-700 (Decision #1
    conservative — admin scannability over Stripe-style
    everything-gray); hover keeps the same color and adds underline,
    no fill.
  - status palette (principle 11): .message-success flips from
    Tailwind emerald to green (`bg-green-50/text-green-800/
    border-green-200`); .timeline > li.tl-success switches from
    --emerald-600 (which was aliased to brand) to --green-600
    (semantic — success is GREEN, not brand teal).

Note for keyboard users: the :focus-visible ring on form fields
and links was rendering blue (#3B82F6) prior to this commit due to
a dormant :root accent variable. <html> carries no theme class, so
the .theme-brand / .theme-rust overrides at input.css:112-125 were
inactive, and the :root default (#3B82F6, a Phase 2 placeholder)
was driving every keyboard focus ring. After this migration the
ring renders teal (#0d9488) consistently with the rest of the
brand. No accessibility regression — measured WCAG contrast ratios
of #0d9488 against page surfaces:

  vs #ffffff (white):           3.744:1   AA non-text
  vs #f9fafb (gray-50 hover):   3.583:1   AA non-text
  vs #f3f4f6 (gray-100):        3.402:1   AA non-text
  vs #f0fdfa (brand-50):        3.590:1   AA non-text

All four ratios pass WCAG AA's 3:1 threshold for non-text UI
components.

Comments updated where they described the old rust visual (6 lines
in base.html / login.html). Two comments in input.css (lines 67,
110) describe the .theme-rust *selector* rather than the rust
visual; left untouched because the selector is preserved here as a
bw-compat alias and Phase 11/b will sweep both selector and
comments together. The framework name "RustIO" in proper-noun and
URL contexts left untouched.

Tests:
  cargo test --workspace --lib                            469 → 469
  RUSTIO_TEST_DB=1 cargo test --lib -- --ignored           47 →  47
  cargo clippy --workspace --all-targets                  clean
  make css-check                                          clean

Phase 11/b will sweep the four `text-rust` template references in
log_entries.html, object_history.html, confirm_delete.html to
`text-brand-700` and add the optional principle 12 stat-card icon
spans + 32px avatar circles in users_list rows.
```

---

## Implementation discipline (per user authorization)

1. Edits land in spec order: tailwind.config.js → input.css → base.html → login.html → rustio.css. No reorder.
2. Run `make css` after each source edit; inspect the regenerated `admin.css` size delta — if Tailwind emits an unexpected diff, stop and investigate before proceeding to the next file.
3. After all edits land and `make css` is clean, run the acceptance criteria (cargo / clippy / sandbox tests / PG tests / `make css-check` / negative+positive grep verifications / browser smoke matrix).
4. **No autonomous commit.** Send `git diff --stat` + per-file diff + acceptance results table + browser smoke output to the user. Wait for explicit authorization.
5. Phase 11/b (template `text-rust` sweep + stat-card icons + 32px avatars) is deferred. Do not touch templates beyond the comment scrubs. Do not add new components.
