# Phase 6a — Admin UI: Django layout, RustIO brand, system fonts

Built on top of Phase 5b (commit `bde08bc`).

## Commits shipped

```
HEAD     phase 6a: report + verification                                                ← final commit (this report)
75d0478  phase 6a/5: change form (Django fieldset, 4 save buttons, submit intent routing)
34d6da1  phase 6a/4: changelist (search, filter sidebar, pagination, action bar)
6c53960  phase 6a/3: dashboard (app list grouped by app_label, recent actions sidebar)
add9a6d  phase 6a/2: login page (Django card layout, RustIO brand)
99addf5  phase 6a/1: foundation (dual-path loader, base.html, admin.css, BaseContext, /static/admin.css route)
```

## Step 1 audit (recap)

NEW had a working but bespoke admin: `.admin-header`/`.admin-sidebar` CSS in `rustio.css`, left-nav layout, no app-label grouping, no recent-actions sidebar, two-button forms. Templates already loaded via a startup-time embedded+disk merge in `templates.rs`. Phase 6a replaces the visual surface (4 pages + base.html + new admin.css) and refactors the loader to per-request lookup. Six pages (`confirm_delete`, `error`, `users_list`, `user_edit`, `groups_list`, `group_edit`) stay on bespoke class names — see "Known visual regression" below.

## Template-system architecture (4 decisions)

### Decision 1: Embedded + filesystem override (per-request)

- **Where:** `rustio-core/src/templates.rs` (rewritten, 207 LOC).
- **Strategy:** `Environment::set_loader(closure)` resolves every `get_template(name)` call through the closure. The closure tries `<RUSTIO_TEMPLATE_DIR>/<name>` on disk first, then falls back to the embedded constant.
- **Restart-free dev edits:** every `render` call invokes `env.clear_templates()` before `get_template`, busting minijinja's cache so a disk edit is visible on the next request without a process restart.
- **Default disk root:** callers pass it explicitly. Both blog (`examples/blog/src/main.rs:79-83`) and CLI (`rustio-cli/src/main.rs:554-555`) read `RUSTIO_TEMPLATE_DIR` (default `templates`) and feed it to `Templates::new(Some(...))`.
- **Tests (5):** `loader_registers_all_embedded_templates`, `missing_template_errors_cleanly`, `disk_override_wins_over_embedded`, `embedded_fallback_when_disk_missing`, **`live_edit_visible_on_next_render_without_restart`** — the last one writes a file, renders, edits the file in place, renders again, asserts the second render reflects the edit.

### Decision 2: Hybrid composition

- **Base template:** `rustio-core/assets/templates/admin/base.html` — provides the page shell (`#header`, `.breadcrumbs`, `#content`, `#content-related` sidebar, footer). 54 LOC.
- **Includes** (created on first use only):
  - `admin/includes/_field_errors.html` (10 LOC) — first used by login (3a) and change form (3d). Renders `<ul class="errorlist">`.
  - `_module_header.html`, `_action_button.html`, `_pagination.html` — **deferred**: each repetition site (.module headers, action buttons, pagination) is currently inline in 1 template; the rule is "extract on second use." None of these reached two uses in Phase 6a.

### Decision 3: All pages extend `admin/base.html`

- Login (`bodyclass=login`) overrides `branding` + `breadcrumbs` to nothing → card-only layout.
- Dashboard (`bodyclass=dashboard`, `coltype=colMS`) uses the right-sidebar grid layout for Recent actions.
- Changelist (`bodyclass=change-list`) overrides `breadcrumbs` to add the model name; uses no sidebar block.
- Change form (`bodyclass=change-form`) overrides `breadcrumbs` with model + action label.
- Blocks exposed: `title`, `bodyclass`, `branding`, `breadcrumbs`, `coltype`, `content`, `sidebar`, `extra_head`, `extra_js`.

### Decision 4: Per-model override hook (skeleton only)

- **API:** `Templates::render_for_model(model: &str, name: &str, ctx: &S) -> Result<String>` (`templates.rs:79-99`).
- Tries `admin/<model>/<page>` first, falls back to `name`. The loader closure resolves either path the same way (disk → embedded).
- **No handler in Phase 6a calls this.** Marked `#[allow(dead_code)]`. Phase 7 wires it when tolkhuset proves a real per-model override need.

## Routes added

| Route | Source | Notes |
|---|---|---|
| `GET /static/admin.css` | `rustio-core/src/admin/routes.rs:131-141` | Embedded via `crate::server::embedded_admin_css()` (`server.rs:191-193`). `cache-control: public, max-age=3600`. Coexists with the existing `/static/rustio.css` route. |

## New templates

| Template | LOC | Replaces (old LOC) | Notes |
|---|---:|---|---|
| `admin/base.html` | 54 | bespoke shell (48) | Django classic layout. Block names match Django (`branding`, `breadcrumbs`, `coltype`, `content`, `sidebar`, `extra_js`). |
| `admin/login.html` | 35 | bespoke card (24) | Extends `admin/base.html`, overrides every shell block to render a card-only layout. |
| `admin/index.html` | 51 | `.cards`/`.card` grid (22) | App-label grouping + Recent actions sidebar. |
| `admin/list.html` | 130 | `.data-table` (36) | `.object-tools` toolbar, `.search`, `.actions`, `.results`, `.paginator`, right `#changelist-filter` sidebar. |
| `admin/form.html` | 57 | bespoke 2-button (43) | Single `fieldset.module.aligned`, **4 submit buttons** (`Delete` left, `Save and add another`, `Save and continue editing`, `Save` right). |
| `admin/includes/_field_errors.html` | 10 | new | `<ul class="errorlist">` block. |

**Embedded list updated** in `templates.rs:118-133`: `admin/includes/_field_errors.html` added; existing names retained (no rename of `list.html`/`form.html` to Django's `change_list.html`/`change_form.html` — kept the existing file names to avoid a sweep across `EMBEDDED_TEMPLATES` and handlers; the visual layout is fully Django-shape).

## New CSS

`rustio-core/assets/static/css/admin.css` — **664 LOC**.

Structure: `:root` palette → reset → layout shell (`#container`/`#header`/`.breadcrumbs`/`#content`/`footer`) → `.module` + dashboard rows + `.rio-pill-*` action pills → login `#login-form` card → changelist (`.object-tools`, `#toolbar .search`, `.actions`, `.results`, `.paginator`, `#changelist-filter`) → change form (`fieldset.module`, `.form-row`, `.errorlist`, `.submit-row`).

System font stack only:
```
-apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, Helvetica, Arial, sans-serif
```
No `@font-face`, no Google Fonts, no `<link>` to any web font.

## New context structs

`rustio-core/src/admin/render.rs` (415 LOC after refactor):

| Struct | Phase 6a addition |
|---|---|
| `BaseContext` | **new** — `identity: Option<IdentityCtx>`, `csrf_token: String`, `site_title: &'static str`, `site_header: &'static str`. Embedded into every page context via `#[serde(flatten)]`. |
| `LoginCtx` | refactored — now `{ base: BaseContext, error: Option<String> }`. |
| `DashboardCtx` | refactored — `{ base, page_title, apps: Vec<DashboardApp>, recent_actions: Vec<RecentActionCtx>, flash }`. |
| `DashboardApp` / `DashboardModel` | **new** — app-label grouping. |
| `RecentActionCtx` | **new** — derived from `audit::AdminAction` with `pill_class` + `when_relative` already computed. |
| `ListCtx` | refactored — added `search_query`, `filters: Vec<FilterGroupCtx>`, `page`, `total_pages`, `per_page`, `total_rows`. |
| `FilterGroupCtx` / `FilterOptionCtx` | **new** — derived from `intelligence::infer_filters`. |
| `FormCtx` | refactored — added `object_id: Option<i64>`, `display_name`. |
| `FormField` | refactored — added `input_type: &'static str`, `placeholder: Option<String>`. Hint now sourced from `intelligence::field_ui_metadata`. |
| `ConfirmDeleteCtx` | refactored — embeds `BaseContext`. Template body untouched (Phase 6b). |

## Handler changes

| Handler | Change |
|---|---|
| `show_login` / `do_login` | Build `LoginCtx { base: BaseContext::new(None, …), error }` — `identity` is `None` (login page renders pre-auth). |
| `dashboard` | Now also reads `audit::recent(&db, 10, None, None)` and **silently degrades** to `Vec::new()` if the audit table doesn't exist (`unwrap_or_default`). No Phase 6a wiring point calls `audit::ensure_table`; this is the deferred wiring deferred to a later phase. |
| `list_model` | Reads `?q=`, `?<field>=value`, `?p=N` from query string. In-memory search/filter/pagination — `entry.ops.list(&db)` still fetches everything (push-down to `AdminOps::list_paged` would mean touching `types.rs`, out of scope for 6a). Page size 100. |
| `show_new_form` / `show_edit_form` | Pass `object_id: None` / `Some(id)` to `form_ctx`. |
| `do_create` / `do_update` | Read `_save` / `_continue` / `_addanother` from the form. New helpers `submit_intent` + `redirect_after_save` pick the right redirect target: list / same edit form / fresh add form. |

## app_label derivation rule

Implemented in `render.rs::app_label_for(admin_name)`:

- If `admin_name` contains a `.`, split on the first `.` and capitalise the prefix. So `"tolkhuset.translators"` → `"Tolkhuset"`.
- Otherwise capitalise the whole thing. So `"posts"` → `"Posts"`.
- Models with the same prefix collapse into one app block in the dashboard, in registration order.

The blog example registers a single model (`Post`, `admin_name = "posts"`) so the dashboard shows one `Posts` app with one model row. A multi-app project (e.g. `tolkhuset.translators` + `tolkhuset.bookings` + `housing.applicants`) gets two app blocks: `Tolkhuset` (with two models) and `Housing` (with one).

## Approved-deviation notes

- **`ContextConfig` integration deferred to Phase 7.** Phase 6a passes `None` to `intelligence::infer_filters` and `field_ui_metadata`. Industry packs / per-app PII rules wire in when tolkhuset's Swedish data demands it.
- **Filter rendering scope.** `intelligence::infer_filters` returns 5 `FilterKind` variants (`BoolYesNo`, `DropdownText`, `DateRange`, `NumericExact`, `ExactMatch`, `RelationDropdown`). Phase 6a renders **only `BoolYesNo`** interactively in the sidebar. Other kinds need either input widgets or live-row plumbing — Phase 7+ work.
- **No file renames** (`list.html` not `change_list.html`, `form.html` not `change_form.html`). Same Django visual semantics; kept names to avoid a 10-line sweep across `EMBEDDED_TEMPLATES` + 6 handler call sites + tests. Future phase can rename if it adds value.

## Known visual regression

**6 pages render with browser-default styling until Phase 6b.** They extend `admin/base.html` (which gained the new Django shell + admin.css link) but their bespoke class names (`.cards`, `.card`, `.data-table`, `.btn-ghost`, `.actions-col`, `.model-form`) only have CSS in the legacy `/static/rustio.css` — which the new base.html does not link.

Affected pages:

- `admin/confirm_delete.html` (`/admin/<model>/<id>/delete`)
- `admin/error.html` (5xx error fallback)
- `admin/users_list.html` (`/admin/users`)
- `admin/user_edit.html` (`/admin/users/<id>/edit`)
- `admin/groups_list.html` (`/admin/groups`)
- `admin/group_edit.html` (`/admin/groups/<id>/edit`)

**Functionality preserved** — forms submit, data shows, links work. Per-flag-1 decision: no compat shim in admin.css (would create dead code Phase 6b has to clean up).

## Test count in the full suite

| | Tests passing (sandbox) | Ignored | Delta |
|---|---:|---:|---|
| Phase 5b baseline (`bde08bc`) | 286 | 21 | — |
| **Phase 6a** | **289** | **21** | **+3** loader tests |

The 3 new tests are `disk_override_wins_over_embedded`, `embedded_fallback_when_disk_missing`, `live_edit_visible_on_next_render_without_restart` in `src/templates.rs`.

```
$ cargo test --workspace 2>&1 | grep "^test result"
test result: ok. 0 passed; 0 failed; 0 ignored
test result: ok. 0 passed; 0 failed; 0 ignored
test result: ok. 289 passed; 0 failed; 21 ignored
test result: ok. 0 passed; 0 failed; 0 ignored
test result: ok. 0 passed; 0 failed; 0 ignored
test result: ok. 0 passed; 0 failed; 0 ignored
```

```
$ cargo clippy --workspace --all-targets -- -D warnings    → clean
$ cargo check --workspace --all-targets                    → clean
```

## Browser verification (your turn)

Sandbox testing is limited to the unit tests above (loader live-edit is mechanically proven). The visual checks need a real browser against a running blog instance.

```bash
make up                       # postgres + meilisearch
cargo run -p blog
# in another terminal: open the URLs below
```

| Page | URL | Expected |
|---|---|---|
| Login | http://localhost:8000/admin/login | Steel header (#1A202C). Centered card ~340px. Card header is the steel strip with **rust-colored** "RustIO administration" text. "Log in" button = rust bg, uppercase, 11px. View source → `<link rel="stylesheet" href="/static/admin.css">` present. View source → `<input type="hidden" name="_csrf" ...>` present. |
| Dashboard | http://localhost:8000/admin (after `admin@example.com / admin`) | Steel header + breadcrumbs. **Posts** app block (steel-muted header strip). Right-side **Recent actions** sidebar — likely showing "No recent activity yet." since `audit::ensure_table` isn't wired into blog's startup yet. |
| Changelist | http://localhost:8000/admin/posts/ | "Select post to change" h1, "Add post +" green link. `.results` table with first column linked. Right `.filter` sidebar (only Bool fields render filters in 6a). Pagination only if >100 rows. |
| Change form | http://localhost:8000/admin/posts/1/edit | Breadcrumbs include "Change". Single fieldset "General". 4 submit buttons in spec colors and order: **Delete (red, far left)** … **Save and add another (gray)**, **Save and continue editing (steel-muted)**, **Save (rust, default, far right)**. Add form `/admin/posts/new` shows the same fieldset minus the Delete button. |

For each page:
- View source: `<link rel="stylesheet" href="/static/admin.css">`
- View source: `<input type="hidden" name="_csrf" value="...">` in every form
- Network tab: `/static/admin.css` returns 200 with `content-type: text/css; charset=utf-8`
- No flash of unstyled content

## Override mechanism verification

```bash
# 1. Confirm the embedded login button reads "Log in".
curl -s http://localhost:8000/admin/login | grep -o 'Log in'

# 2. Override on disk.
mkdir -p ./templates/admin
cp rustio-core/assets/templates/admin/login.html ./templates/admin/login.html

# 3. Edit the override:
sed -i.bak 's/Log in/ENTER/' ./templates/admin/login.html

# 4. Hit the page WITHOUT restarting blog. Should show ENTER.
curl -s http://localhost:8000/admin/login | grep -o 'ENTER'

# 5. Remove the override + clean up.
rm -rf ./templates/admin/login.html ./templates/admin/login.html.bak

# 6. Hit again — back to "Log in" (no restart).
curl -s http://localhost:8000/admin/login | grep -o 'Log in'
```

The third and sixth steps prove the loader's two key properties: **disk wins** and **embedded falls back** — both without a process restart. This was the actual win of the loader refactor and is also covered by the unit test `live_edit_visible_on_next_render_without_restart` (programmatic, runs in `cargo test`).

## `RUSTIO_TEMPLATE_DIR` documentation

The blog example and the CLI both read this env var (default `templates`) and pass it to `Templates::new`. To use a custom override directory:

```bash
RUSTIO_TEMPLATE_DIR=./my-templates cargo run -p blog
```

Setting it to a non-existent path is safe — the loader silently falls back to embedded for every name.

To disable overrides entirely (embedded only), call `Templates::new(None)` directly. The two existing callers always pass `Some(...)`, so this codepath is currently exercised only by tests.

## Open questions for Phase 6b

1. **Where does `audit::ensure_table` get called from?** Phase 5b deferred this. Dashboard's Recent actions sidebar will stay empty until either the blog (or any caller) calls `ensure_table` at startup, or `register_admin_routes` calls it on first use. **Bias:** call from `Admin::seed_permissions` (already runs at boot, already touches the DB) — but that's a `types.rs` adjacent change.
2. **Filter widgets beyond `BoolYesNo`.** The other 4 `FilterKind` variants (DateRange, DropdownText, NumericExact, ExactMatch, RelationDropdown) need layout decisions before Phase 7 wires them.
3. **Push-down search/filter to AdminOps.** In-memory filtering on the changelist is fine for small models; >10k rows will lag. Need a `list_paged(db, opts) -> ListPage` method on the `AdminOps` trait — `types.rs` change.
4. **6 deferred templates' rewrite.** `confirm_delete`, `error`, `users_list`, `user_edit`, `groups_list`, `group_edit` — the targets the user named for "Phase 6b later: delete confirm, user mgmt, group mgmt, password change, history log."
5. **Per-model templates wiring.** `Templates::render_for_model` exists but no handler calls it. Phase 7's tolkhuset use case will exercise it; before then, decide whether the change-form is the natural first per-model override site (a typical use is "the User edit form needs an extra warning about role downgrade").

## Confirmation

- **No HTML in Rust code.** Every template lives in `assets/templates/`.
- **No JS framework.** One inline `<script>` in `list.html` (~25 LOC) for select-all + `/`-key search focus.
- **No web fonts.** System font stack everywhere.
- **Sharp corners.** No `border-radius` greater than 3px.
- **Flat.** No `box-shadow` (except focus rings via `outline`).
- **Single theme.** No dark mode.
- **CSRF preserved.** Every form contains `<input type="hidden" name="_csrf" value="{{ csrf_token }}">`.
- **Sandbox suite**: 289 passed, 21 ignored (13 from prior phases + 8 audit), 0 failed. Clippy `-D warnings` clean.
