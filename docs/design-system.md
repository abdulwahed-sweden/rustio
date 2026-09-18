# Design system

The admin's design is ported from **[rustio-lite](https://github.com/abdulwahed-sweden/rustio-lite)**,
where it was designed and tested against thirteen contract pages. Nothing
here was invented for rustio: every colour, size, weight, radius and shadow
below comes from that project's stylesheet.

One file ships: **`rustio-core/assets/static/admin.css`**. `build.rs` runs it
through Tailwind (as a minifier — the file uses no Tailwind directives),
writes the result to `$OUT_DIR/admin.css`, and `templating.rs` bundles that
into the binary. It is served at `/admin/static/admin.css`, and it is the
only stylesheet any admin page links.

Light only. `color-scheme: light`, no `prefers-color-scheme` block, no
`[data-theme]` attribute, no toggle — the same posture as rustio-lite.

## Tokens

Declared once on `:root`. Use the variable, never the literal.

| Group | Tokens |
|---|---|
| Surfaces | `--page #f1f1ee` · `--surface #ffffff` · `--surface-soft #f7f9fc` · `--surface-head #f1f4f8` |
| Primary | `--blue #1f5797` · `--blue-dark #174578` · `--blue-soft #dfeafb` · `--blue-line #b7cdea` |
| Focus ring | `--focus #2f7bd6` — deliberately **not** `--blue`, so a focused control doesn't read as a button. 3:1 or better on every surface it can land on. |
| Ink | `--ink #202733` · `--ink-soft #4c5868` |
| Lines | `--border #d5dce5` · `--border-strong #b9c5d3` |
| Status | `--green #19724b` / `--green-soft #e8f6ee` · `--amber #935b0a` / `--amber-soft #fff4df` · `--red #a23f3a` / `--red-soft #fff0ee` |
| Rail | `--rail 260px` · `--rail-gap 14px` · `--rail-bg #f4f6f9` · `--rail-line #d3dbe5` |
| Shape | `--radius 9px` · `--content 1120px` · `--shadow 0 2px 5px rgba(24,39,61,.07)` |

**Type.** Inter first, then the OS native UI stack — no CDN, no webfont
download at runtime. Body 1.0625rem / 1.5 at weight 450. Sizes in use:
.875 · .9 · .9375 · .96 · .98 · 1 · 1.02 · 1.05 · 1.15 · 1.8rem.

**Weight** carries hierarchy, not size: 450 body · 620–720 secondary ·
760–790 emphasis · 800–850 display and labels. Adding a weight outside
that set is a change to the design system, not a use of it.

**Focus.** `:focus-visible { outline: 3px solid var(--focus); outline-offset: 2px }`.
The offset keeps the ring on the surface behind a control, never on its fill.

## Layout

```text
.shell
  aside.sidebar          72px top bar — brand | nav.nav | .sidebar-foot
  .content               grid: var(--rail) | minmax(0,1fr)
    aside.module-sidebar 260px rail — .module-title, .module-nav, .module-link
    main.main            width min(100% - 56px, var(--content))
    footer.app-footer
```

Breakpoints at 1040px, 760px and 480px collapse the rail and then the top
bar. `prefers-reduced-motion` disables transitions.

## Components

| Use | Classes |
|---|---|
| Page furniture | `.page-head`, `.page-head-actions`, `.breadcrumb` + `-sep` + `-current`, `.lead`, `.eyebrow`, `.hint`, `.error-text` |
| Surfaces | `.card` + `.card-head` + `.card-body` + `.card-title`, `.card-grid`, `.danger-zone` |
| Tables | `.table-wrap` (always — it is the horizontal scroll), bare `table`/`th`/`td`, `.table-compact`, `.table-link`, `.cell-muted`, `.cell-id`, `.cell-num`, `.cell-fit`, `.actions` |
| Buttons | `.button` + `.button-primary` / `-secondary` / `-quiet` / `-danger` / `-block` / `-sm`, `.button-row` |
| Forms | `.form`, `.field`, `.label`, bare `input`/`select`/`textarea`, `.hint`, `.error-text`, `.form-actions`, `.checkbox`, `.required-mark`, `.input-inline` / `-wide` / `-narrow` |
| Status | `.badge` + `-active` / `-disabled` / `-user` / `-admin` / `-developer` / `-customer` / `-warn` |
| Messages | `.alert` + `-info` / `-success` / `-warning` / `-error`, `.notice-bar`, `.env-chip` |
| Identity | `.avatar` (+ `-lg`, `-admin`, `-developer`, `-user`, `-customer`), `.profile` + `-id` / `-meta` / `-name` / `-email` |
| Detail views | `.details`, `.detail-grid`, `.detail-main`, `.detail-side`, `.detail-single`, `.detail-row` + `-label` / `-value` |
| Lists & dashboard | `.stack-list`, `.stat-grid`, `.stat` + `-value` / `-label` / `-featured`, `.empty` + `.empty-icon` |
| Navigation | `.toolbar` + `-count`, `.filters`, `.filter`, `.layout-switch`, `.pagination` + `-pages`, `.lang-switcher`, `.lang-select` |
| Permissions | `.perm`, `.perm-icon` (with `.is-allowed` / `.is-denied`) |
| Auth pages | `.login-shell`, `.login-wrap`, `.login-card`, `.login-brand`, `.login-title`, `.login-foot` |
| Accessibility | `.sr-only`, `.skip-link` |
| Spacing helpers | `.stack-sm` / `.stack` / `.stack-lg`, `.flush`, `.flush-top`, `.row`, `.row-tight`, `.inline-actions`, `.inline-form`, `.tag-row`, `.nowrap`, `.center` |

Everything from `.page-head-actions` down to the spacing helpers is
**composed for rustio** — rustio-lite has no dashboard, pagination, filter
toolbar, layout switch, language switcher or permission matrix, so those are
assembled from the tokens above. They introduce no new colour and no new
weight; that constraint is what keeps the two systems one system.

## Rules

1. **No literal colours in templates or handlers.** If a value isn't a
   token, it doesn't belong in the markup.
2. **No inline `style=`.** The templates and handlers carry none; keep it
   that way. If a rule is missing, add it here with a name.
3. **Wide content scrolls in `.table-wrap`,** never by widening the page.
4. **A class the stylesheet doesn't define is a bug,** not a shortcut.
   Fetch a page and diff its classes against this file.
5. **Changing a token changes the system.** Update this page in the same
   commit.

## Where things are

- `rustio-core/assets/static/admin.css` — the stylesheet.
- `rustio-core/assets/templates/` — 14 templates; `base_admin.html` owns
  the shell, `includes/` holds the three override seams.
- `rustio-core/src/admin/layout.rs` — the context dicts, and the handful of
  class strings Rust emits (status badges, form controls, table cells).
- `rustio-core/src/admin.rs` — the legacy string-built shell behind
  `/admin/<model>/create`, `/…/history`, `/…/delete` and `/admin/logout`.
- `rustio-core/build.rs` — the Tailwind/minify step and its passthrough.
- `rustio-core/src/admin/design.rs` — `rustio.design.json`: project name and
  logo initial.
