/** @type {import('tailwindcss').Config} */
module.exports = {
  // Tailwind scans these paths for class names. Anything not referenced
  // gets purged in the production build (`npm run css`).
  content: [
    "rustio-core/assets/templates/**/*.html",
    "rustio-core/assets/css/input.css",
  ],
  theme: {
    extend: {
      colors: {
        // Brand spec — see docs/brand.md
        rust: {
          DEFAULT: "#aa4422",
          hover:   "#88331b",
          glow:    "#d46644",
        },
        paper: "#f9f8f6",
        metal: {
          DEFAULT: "#2c303a",   // Body text
          dark:    "#181a1f",   // Top bar / code blocks
          surface: "#22252b",   // Cards on dark
        },
        teal: "#338899",
      },
      fontFamily: {
        sans: ["Inter", "system-ui", "sans-serif"],
      },
      fontSize: {
        // 16px body per Phase 7a/2 spec.
        base: ["16px", { lineHeight: "1.55" }],
      },
      boxShadow: {
        mark:        "0 1px 3px rgba(170,68,34,0.4)",
        btn:         "0 1px 2px rgba(170,68,34,0.3)",
        "btn-hover": "0 4px 10px rgba(170,68,34,0.25)",
        "card-hover":"0 8px 20px rgba(0,0,0,0.04)",
      },
      borderRadius: {
        DEFAULT: "6px",
        md:  "8px",
        lg:  "10px",
        xl:  "12px",
      },
    },
  },
  plugins: [],
};
