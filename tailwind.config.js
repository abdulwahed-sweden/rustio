/** @type {import('tailwindcss').Config} */
//
// Phase 2 — Design system mapping.
// Source of truth: docs/design-system.json. Do not hand-edit values
// here without updating the JSON; mirror both deliberately.
//
// Strategy:
//   - Semantic colors (surface, text, accent, …) are wired through CSS
//     custom properties so the same Tailwind class adapts to the
//     active theme (`<html class="dark">` or `<html class="theme-rust">`).
//   - Legacy brand colors (rust, paper, metal, teal) are kept as-is so
//     Phase 7a/2 templates that still reference them don't break before
//     the template-refactor sub-phase ships.
//   - Spacing, radius, shadow, fontFamily, fontSize all override
//     Tailwind defaults with the JSON ramp.
//
module.exports = {
  // Phase 2 — JSON `darkMode.strategy = "class"`. Switching themes is a
  // runtime toggle (add/remove `dark` on <html>/<body>); no media-query
  // automatic flip.
  darkMode: "class",

  // Tailwind scans these paths for class names. Anything not referenced
  // gets purged in the production build (`npm run css`).
  content: [
    "rustio-core/assets/templates/**/*.html",
    "rustio-core/assets/css/input.css",
  ],

  theme: {
    extend: {
      colors: {
        // ---------------------------------------------------------------
        // Semantic tokens (Phase 2 — design-system.json).
        // Backed by CSS variables defined in input.css under :root /
        // .dark / .theme-rust. The `rgb(... / <alpha-value>)` form lets
        // utilities like `bg-surface/50` keep working.
        // ---------------------------------------------------------------
        bg:             "rgb(var(--ds-color-bg) / <alpha-value>)",
        surface:        "rgb(var(--ds-color-surface) / <alpha-value>)",
        "surface-muted":"rgb(var(--ds-color-surface-muted) / <alpha-value>)",
        text:           "rgb(var(--ds-color-text) / <alpha-value>)",
        "text-muted":   "rgb(var(--ds-color-text-muted) / <alpha-value>)",
        border:         "rgb(var(--ds-color-border) / <alpha-value>)",
        primary:        "rgb(var(--ds-color-primary) / <alpha-value>)",
        accent:         "rgb(var(--ds-color-accent) / <alpha-value>)",
        success:        "rgb(var(--ds-color-success) / <alpha-value>)",
        warning:        "rgb(var(--ds-color-warning) / <alpha-value>)",
        danger:         "rgb(var(--ds-color-danger) / <alpha-value>)",
        // Dark-theme stacked layers (themes.dark.layer1–layer4). Empty
        // in light/rust themes — utilities still resolve, but pages
        // shouldn't depend on these outside of dark-only chrome.
        "layer-1":      "rgb(var(--ds-color-layer-1) / <alpha-value>)",
        "layer-2":      "rgb(var(--ds-color-layer-2) / <alpha-value>)",
        "layer-3":      "rgb(var(--ds-color-layer-3) / <alpha-value>)",
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

      // -------------------------------------------------------------------
      // Typography — typography.fontFamilies in the JSON. The font files
      // for Roboto / Space Grotesk / JetBrains Mono are NOT loaded yet;
      // a follow-up will self-host them under assets/fonts/. Until then
      // the browser falls back to the next stack entry (system sans /
      // monospace). Inter remains @font-face-loaded in input.css for
      // backward compatibility with anything still requesting it.
      // -------------------------------------------------------------------
      fontFamily: {
        sans:    ["Roboto", "sans-serif"],
        heading: ["Space Grotesk", "sans-serif"],
        mono:    ["JetBrains Mono", "monospace"],
      },

      // -------------------------------------------------------------------
      // typography.fontSizes — seven-step ramp from the JSON. The
      // utility names map directly: text-display, text-h1, …, text-caption.
      // Tailwind's default xs/sm/base/lg/2xl etc. remain available
      // alongside (no override). Existing templates that use arbitrary
      // values (`text-[14.5px]`) keep rendering exactly as before —
      // arbitrary values bypass this map entirely.
      // -------------------------------------------------------------------
      fontSize: {
        display: ["40px", { lineHeight: "1.2" }],
        h1:      ["32px", { lineHeight: "1.3" }],
        h2:      ["24px", { lineHeight: "1.35" }],
        h3:      ["20px", { lineHeight: "1.4" }],
        body:    ["16px", { lineHeight: "1.6" }],
        small:   ["14px", { lineHeight: "1.5" }],
        caption: ["12px", { lineHeight: "1.4" }],
      },

      // -------------------------------------------------------------------
      // spacing — six-step JSON scale. These add named keys on top of
      // Tailwind's numeric scale (p-1 / p-4 / p-6 still work); new
      // utilities like p-md, gap-lg, mt-2xl become available too.
      // -------------------------------------------------------------------
      spacing: {
        xs:    "4px",
        sm:    "8px",
        md:    "16px",
        lg:    "24px",
        xl:    "32px",
        "2xl": "48px",
      },

      // -------------------------------------------------------------------
      // borderRadius — JSON values REPLACE the previous Phase 7a/2
      // scale (which was DEFAULT 6 / md 8 / lg 10 / xl 12).
      // Visual shift on next render: rounded-md 8→10px, rounded-lg
      // 10→14px, rounded-xl 12→18px. This is intentional — the JSON
      // is the source of truth.
      // -------------------------------------------------------------------
      borderRadius: {
        sm: "6px",
        md: "10px",
        lg: "14px",
        xl: "18px",
      },

      // -------------------------------------------------------------------
      // boxShadow — JSON's three-step elevation scale. Existing
      // rust-flavoured shadows (mark, btn, btn-hover, card-hover) are
      // kept under their original names so legacy components (.btn-primary
      // pre-Phase 2) still resolve; the new component layer below uses
      // shadow-card / shadow-dropdown / shadow-modal.
      // -------------------------------------------------------------------
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
    },
  },
  plugins: [],
};
