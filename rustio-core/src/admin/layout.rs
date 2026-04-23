//! Admin-new page assembler.
//!
//! Stitches the `ui` component renderers into a full HTML document,
//! injects the approved design CSS (`assets/admin-new/theme.css` +
//! `components.css`), bundles the minimal JS that powers the search
//! keyboard shortcuts, and exposes [`admin_index`] — the handler body
//! that `/admin-new` serves.
//!
//! All CSS and JS is baked in at compile time via `include_str!` and
//! inline `const` strings. No filesystem reads, no `/static` links,
//! no external stylesheet dependencies.

use std::collections::HashMap;

use crate::admin::admin_form_bridge::{
    resolve_filter_type, AdminDataType, AdminUiField, AdminUiModel, FilterType,
};
use crate::admin::auto_form::{AutoField, FieldOverride, FormBuilder, FormModel};
use crate::admin::form::{render_form, FieldConfig, FieldType, FormConfig};
use crate::admin::persistence::{self, PersistableModel};
use crate::admin::ui::html_escape;
use crate::admin::ui::{
    render_page_header, render_sidebar, render_table_shell, render_toolbar, render_topbar,
    BadgeVariant, Breadcrumb, FilterChip, PageAction, PageHeaderConfig, SearchConfig,
    SearchProminence, SidebarGroup, SidebarItem, TableCell, TableColumn, TableRow,
    TableShellConfig, TopbarConfig,
};
use crate::orm::Db;

// ---------------------------------------------------------------
// Bundled CSS + JS
// ---------------------------------------------------------------

const THEME_CSS: &str = include_str!("../../assets/admin-new/theme.css");
const COMPONENTS_CSS: &str = include_str!("../../assets/admin-new/components.css");

/// Thin glue layer on top of the approved components CSS:
///
/// - `.sr-only` for accessible-but-invisible labels (the approved
///   design uses placeholders as the visible label, but assistive tech
///   still needs a real `<label>`).
/// - `.search-primary` size bump for `SearchProminence::Primary`.
/// - `.search-hint` row below the toolbar that surfaces the keyboard
///   shortcut in plain text (text-first rule).
/// - Wrapper classes around the search form so the approved flex
///   toolbar keeps its behaviour without a layout rewrite.
const LAYOUT_EXTRAS_CSS: &str = r#"
.sr-only {
  position: absolute;
  width: 1px;
  height: 1px;
  padding: 0;
  margin: -1px;
  overflow: hidden;
  clip: rect(0, 0, 0, 0);
  white-space: nowrap;
  border: 0;
}

.toolbar .search-form {
  flex: 1;
  display: flex;
  min-width: 280px;
}
.toolbar .search-form .search {
  flex: 1;
  min-width: 0;
}

.search-primary-wrap {
  margin-bottom: 12px;
}
.search-primary-wrap .search-form {
  display: block;
}
.search-primary-wrap .search.search-primary {
  min-width: 0;
  width: 100%;
}
.search-primary-wrap .search.search-primary input {
  font-size: 15px;
  padding: 11px 96px 11px 40px;
}
.search-primary-wrap .search.search-primary .search-icon {
  left: 14px;
}
.toolbar.toolbar-filters-only {
  border-radius: 8px 8px 0 0;
}

.search-hint {
  font-family: var(--mono);
  font-size: 12px;
  color: var(--ink-subtle);
  padding: 6px 4px 10px;
}
.search-hint .kbd-inline {
  font-family: var(--mono);
  font-size: 11px;
  padding: 1px 6px;
  border: 1px solid var(--border-strong);
  border-bottom-width: 2px;
  border-radius: 3px;
  background: var(--bg);
  color: var(--ink-muted);
}

/* Give the Inter reference a sensible default when the webfont isn't
   loaded — the approved design expects Inter, but the engine must not
   depend on a network stylesheet. System stack below renders very
   close on macOS/iOS/Win11. */
html, body {
  font-family: 'Inter', -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
}

/* Apply the approved 1400px max-width centering on large screens. */
.main {
  margin: 0 auto;
}
"#;

/// Global keyboard shortcuts — the only JS that ships with the
/// foundation. Silent no-op when no search input is present.
///
/// - `/` focuses the visible search (unless the user is already typing)
/// - `Ctrl+K` / `Cmd+K` also focuses it
/// - `Escape` blurs the focused search
///
/// Deliberately does NOT open a palette, a modal, or any other
/// discovery affordance — the approved rule is: text-first, search-first.
const KEYBOARD_JS: &str = r#"
(function () {
  function search() {
    return document.querySelector('[data-role="search-input"]');
  }
  function isTyping(el) {
    if (!el || !el.tagName) return false;
    var t = el.tagName;
    return t === 'INPUT' || t === 'TEXTAREA' || t === 'SELECT' || el.isContentEditable === true;
  }
  document.addEventListener('keydown', function (e) {
    var el = search();
    if ((e.key === 'k' || e.key === 'K') && (e.ctrlKey || e.metaKey)) {
      if (el) { e.preventDefault(); el.focus(); if (el.select) el.select(); }
      return;
    }
    if (e.key === '/' && !isTyping(document.activeElement)) {
      if (el) { e.preventDefault(); el.focus(); }
      return;
    }
    if (e.key === 'Escape') {
      if (el && document.activeElement === el) { el.blur(); }
    }
  });
})();
"#;

/// Form-scoped keyboard helpers. Active only when the keydown
/// originates inside a `[data-admin-form]` subtree, so the search
/// shortcuts above and unrelated UI are never affected.
///
/// - Ctrl/Cmd + Enter explicitly submits — required because plain
///   Enter inside a `<textarea>` inserts a newline rather than
///   submitting. Plain Enter inside text inputs already submits
///   natively via the form's `type="submit"` Save button, so no
///   handler is needed for that path.
/// - Escape blurs the focused control inside the form. Doesn't close
///   the drawer — there is no JS-driven drawer toggle yet.
const FORM_KEYBOARD_JS: &str = r#"
(function () {
  document.addEventListener('keydown', function (e) {
    var form = (e.target && typeof e.target.closest === 'function')
      ? e.target.closest('[data-admin-form]')
      : null;
    if (!form) return;
    if (e.key === 'Escape') {
      if (e.target && typeof e.target.blur === 'function') e.target.blur();
      return;
    }
    if (e.key === 'Enter' && (e.ctrlKey || e.metaKey)) {
      e.preventDefault();
      if (typeof form.requestSubmit === 'function') {
        form.requestSubmit();
      } else {
        form.submit();
      }
    }
  });
})();
"#;

/// Switch toggle handler. Listens for clicks on `.switch`, flips the
/// inner `<input type="checkbox">`, and syncs the visual `.on` class
/// in the same step (the approved CSS keys the track-knob position
/// off `.switch.on`, not off `:checked`, so the class swap is what
/// makes the visual move).
///
/// `e.preventDefault()` suppresses the browser's native label-input
/// activation — without it, the label-wrapped checkbox would receive
/// a synthesized click immediately after our handler runs and toggle
/// itself a second time, cancelling the JS toggle. The `INPUT` early
/// return covers direct clicks on the (hidden) checkbox, e.g. from
/// keyboard activation, so those go through the native path
/// untouched.
const SWITCH_JS: &str = r#"
(function () {
  document.addEventListener('click', function (e) {
    if (e.target && e.target.tagName === 'INPUT') return;
    var sw = (e.target && typeof e.target.closest === 'function')
      ? e.target.closest('.switch')
      : null;
    if (!sw) return;
    var checkbox = sw.querySelector('input[type="checkbox"]');
    if (!checkbox) return;
    e.preventDefault();
    checkbox.checked = !checkbox.checked;
    sw.classList.toggle('on', checkbox.checked);
  });
})();
"#;

// ---------------------------------------------------------------
// Shell assembler
// ---------------------------------------------------------------

/// Assemble a full admin-new page. `topbar` / `sidebar` / `content`
/// are already-rendered HTML fragments; they are embedded verbatim
/// into the approved layout shell.
pub fn render_layout(topbar: String, sidebar: String, content: String) -> String {
    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>RustIO Admin</title>
<style>
{theme}
{components}
{extras}
</style>
</head>
<body>
{topbar}
<div class="layout">
{sidebar}
<main class="main">
{content}
</main>
</div>
<script>{keyboard}{form_keyboard}{switch}</script>
</body>
</html>"#,
        theme = THEME_CSS,
        components = COMPONENTS_CSS,
        extras = LAYOUT_EXTRAS_CSS,
        topbar = topbar,
        sidebar = sidebar,
        content = content,
        keyboard = KEYBOARD_JS,
        form_keyboard = FORM_KEYBOARD_JS,
        switch = SWITCH_JS,
    )
}

// ---------------------------------------------------------------
// Public entry point — handler body for /admin-new
// ---------------------------------------------------------------

/// Render the foundation page for `/admin-new` — sync entry point.
///
/// `prefill = None` falls back to the demo form; `prefill = Some(map)`
/// builds the form from `UserAdmin` and pre-populates each field's
/// `value` from the map.
///
/// **Without a `Db` handle the table renders empty** ("No records
/// found" + "auth.User · 0 records"). For a real, data-backed page
/// use [`admin_index_get`] / [`admin_index_post`] — both are async
/// and pull the user list + count from the demo table.
pub fn admin_index(prefill: Option<&HashMap<String, String>>, editing_id: Option<&str>) -> String {
    let drawer = build_drawer_for_get(prefill, editing_id);
    admin_index_with_drawer(
        drawer,
        Vec::new(),
        0,
        None,
        1,
        1,
        &HashMap::new(),
        None,
        None,
    )
}

/// GET orchestrator: ensures the demo table exists, loads the row
/// identified by `editing_id` (when present) so the form is
/// pre-filled, and pulls either the full list or a search result
/// (when `query` is `Some`) so the table chrome reflects what the
/// user typed. Falls back gracefully on any DB error — the page
/// always renders.
pub async fn admin_index_get(
    db: &Db,
    editing_id: Option<&str>,
    query: Option<&str>,
    page: i64,
    filters: &HashMap<String, String>,
    sort: Option<&str>,
    dir: Option<&str>,
) -> String {
    // Ensure the table exists before *any* DB read below — both the
    // single-row lookup and the list path need it. Idempotent.
    let _ = persistence::ensure_demo_table(db).await;

    let prefill = match editing_id {
        Some(id) if !id.is_empty() => {
            match persistence::get_record_by_id(db, UserAdmin::table_name(), id).await {
                Ok(map) if !map.is_empty() => Some(map),
                _ => None,
            }
        }
        _ => None,
    };
    // If lookup didn't find a row, drop the editing id too so the
    // hidden field is empty and a subsequent submit becomes an
    // INSERT rather than a no-op UPDATE against a phantom id.
    let effective_id = if prefill.is_some() { editing_id } else { None };

    let drawer = build_drawer_for_get(prefill.as_ref(), effective_id);
    let (rows, total, current_page, total_pages, validated_sort, validated_dir) =
        fetch_users_table_state(db, query, filters, page, sort, dir).await;
    admin_index_with_drawer(
        drawer,
        rows,
        total,
        query,
        current_page,
        total_pages,
        filters,
        validated_sort.as_deref(),
        validated_dir.as_deref(),
    )
}

/// Build the form-side drawer for the GET path. Shared between the
/// sync [`admin_index`] entry point and [`admin_index_get`] so both
/// paths produce an identical drawer for the same `(prefill, id)`
/// inputs.
fn build_drawer_for_get(
    prefill: Option<&HashMap<String, String>>,
    editing_id: Option<&str>,
) -> String {
    match prefill {
        Some(values) if !values.is_empty() => {
            let mut form = build_user_admin_form();
            for field in form.fields.iter_mut() {
                if let Some(v) = values.get(&field.name) {
                    field.value = Some(v.clone());
                }
            }
            let drawer = render_form(&form);
            inject_hidden_id(&drawer, editing_id)
        }
        _ => demo_admin_form(None),
    }
}

/// Pull the users-table window + count from the demo table. When a
/// non-empty `query` is supplied, dispatches to
/// [`persistence::search_records`] / [`persistence::count_search_records`]
/// so the same (rows, total) shape works for both list and search
/// modes. Failures degrade silently to `(empty Vec, 0)` so the page
/// chrome can still render — a transient DB error must not 500 a
/// page that is mostly chrome.
/// Returns `(rows, total, current_page, total_pages)`. Filters are
/// classified by metadata (`resolve_filter_type`) into equality vs
/// `LIKE` clauses, then handed to [`persistence::filter_records`] /
/// [`persistence::count_filtered_records`] alongside the search
/// query. Total is fetched first so the page can be clamped to a
/// valid range before the windowed query runs — `?page=999` against
/// a 30-row table snaps to the last real page instead of returning
/// nothing. `total_pages` is always at least `1` so the chrome can
/// render `(Page 1 of 1)` even on an empty DB.
async fn fetch_users_table_state(
    db: &Db,
    query: Option<&str>,
    filters: &HashMap<String, String>,
    page: i64,
    sort: Option<&str>,
    dir: Option<&str>,
) -> (
    Vec<HashMap<String, String>>,
    i64,
    i64,
    i64,
    Option<String>,
    Option<String>,
) {
    const PAGE_SIZE: i64 = 20;
    let table = UserAdmin::table_name();
    let (eq_filters, like_filters) = classify_user_admin_filters(filters);
    let (validated_sort, validated_dir) = validate_sort_state(sort, dir);

    let total = persistence::count_filtered_records(db, table, &eq_filters, &like_filters, query)
        .await
        .unwrap_or(0);

    let total_pages = if total > 0 {
        // `i64::div_ceil` is still unstable as of MSRV 1.75; the
        // unsigned counterpart is stable, so cast through `u64`.
        ((total as u64).div_ceil(PAGE_SIZE as u64) as i64).max(1)
    } else {
        1
    };
    let current_page = page.clamp(1, total_pages);
    let offset = (current_page - 1) * PAGE_SIZE;

    let rows = persistence::filter_records(
        db,
        table,
        &eq_filters,
        &like_filters,
        query,
        validated_sort.as_deref(),
        validated_dir.as_deref(),
        PAGE_SIZE,
        offset,
    )
    .await
    .unwrap_or_default();

    (
        rows,
        total,
        current_page,
        total_pages,
        validated_sort,
        validated_dir,
    )
}

/// Validate the URL's `sort` + `dir` against `UserAdmin` metadata.
/// `sort` must name a field that's both declared and `sortable`;
/// `dir` is normalised to `"asc"` or `"desc"` (any other value
/// becomes `"asc"`). An invalid sort drops both to `None` so
/// persistence falls back to `ORDER BY "id" DESC`. All validation
/// happens here so persistence stays a simple SQL emitter that
/// trusts its inputs.
fn validate_sort_state(sort: Option<&str>, dir: Option<&str>) -> (Option<String>, Option<String>) {
    let valid_sort = sort.filter(|s| {
        UserAdmin::fields()
            .iter()
            .any(|f| f.name == *s && f.sortable)
    });
    match valid_sort {
        Some(s) => {
            let d = if matches!(dir, Some("desc")) {
                "desc"
            } else {
                "asc"
            };
            (Some(s.to_string()), Some(d.to_string()))
        }
        None => (None, None),
    }
}

/// Single source of truth for back-link URLs — pagination links and
/// sortable header links both go through this so the parameter set
/// stays consistent. `None`/empty values are skipped; filter keys
/// are sorted so the same state always produces the same URL string
/// (browser cache + back-button friendly).
fn build_query_url(
    page: Option<i64>,
    query: Option<&str>,
    filters: &HashMap<String, String>,
    sort: Option<&str>,
    dir: Option<&str>,
) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(p) = page {
        parts.push(format!("page={p}"));
    }
    if let Some(q) = query {
        if !q.is_empty() {
            parts.push(format!("q={}", url_encode_value(q)));
        }
    }
    let mut filter_keys: Vec<&String> = filters.keys().collect();
    filter_keys.sort();
    for k in filter_keys {
        if let Some(v) = filters.get(k) {
            if !v.is_empty() {
                parts.push(format!("{}={}", url_encode_value(k), url_encode_value(v),));
            }
        }
    }
    if let Some(s) = sort {
        parts.push(format!("sort={}", url_encode_value(s)));
    }
    if let Some(d) = dir {
        parts.push(format!("dir={}", url_encode_value(d)));
    }
    if parts.is_empty() {
        String::new()
    } else {
        format!("?{}", parts.join("&"))
    }
}

/// Walk `UserAdmin::fields()` and split the raw URL filter map into
/// two buckets keyed off [`resolve_filter_type`]: equality filters
/// (Boolean, Select) and `LIKE` filters (Exact text). Any URL key
/// that doesn't correspond to a declared `AdminUiField` is silently
/// dropped — this is the security boundary that stops an attacker
/// from injecting `?random_column=x` to query columns that admin
/// metadata never exposed as filterable.
fn classify_user_admin_filters(
    raw: &HashMap<String, String>,
) -> (HashMap<String, String>, HashMap<String, String>) {
    let fields = UserAdmin::fields();
    let mut eq = HashMap::new();
    let mut like = HashMap::new();
    for (k, v) in raw {
        let Some(field) = fields.iter().find(|f| f.name == k.as_str()) else {
            continue;
        };
        // Don't filter on a column the admin model didn't mark
        // filterable — same idea as the unknown-key drop above,
        // just for declared-but-unfilterable columns.
        if !field.filterable && !field.advanced_filter {
            continue;
        }
        match resolve_filter_type(field) {
            FilterType::Boolean | FilterType::Select => {
                eq.insert(k.clone(), v.clone());
            }
            FilterType::Exact => {
                like.insert(k.clone(), v.clone());
            }
        }
    }
    (eq, like)
}

/// POST path for `/admin-new`. Builds the form, binds the submitted
/// values, runs validation, and — if nothing failed — calls into
/// [`crate::admin::persistence`] to either INSERT (no `editing_id`)
/// or UPDATE (existing `editing_id`). The returned HTML is the same
/// foundation page with the drawer reflecting:
///
/// - bound form values,
/// - validation errors (if any),
/// - "Saved successfully" banner on a clean save,
/// - "Failed to save record" banner if validation passed but the DB
///   write returned an error.
///
/// The hidden `id` input is injected into the rendered drawer so
/// subsequent POSTs from the same page hit the UPDATE path
/// automatically (after a CREATE, the new row id is round-tripped).
#[allow(clippy::too_many_arguments)]
pub async fn admin_index_post(
    db: &Db,
    params: &HashMap<String, String>,
    editing_id: Option<&str>,
    query: Option<&str>,
    page: i64,
    filters: &HashMap<String, String>,
    sort: Option<&str>,
    dir: Option<&str>,
) -> String {
    // First POST against a fresh DB needs the demo table; the helper
    // is `CREATE TABLE IF NOT EXISTS` so this is cheap to call every
    // time (O(1) once the table exists).
    let _ = persistence::ensure_demo_table(db).await;

    let mut form = build_user_admin_form();
    crate::admin::form::bind_form(&mut form, params);
    crate::admin::form::validate_form(&mut form);

    let any_errors = form.fields.iter().any(|f| f.error.is_some());

    let mut effective_id = editing_id.map(String::from);
    let mut save_failed = false;

    if !any_errors {
        if let Some(id) = effective_id.as_deref() {
            // UPDATE: known row id → write the form's column map
            // back through the helper.
            let data = UserAdmin::to_update_map(&form);
            if let Err(err) =
                persistence::update_record(db, UserAdmin::table_name(), id, &data).await
            {
                eprintln!("admin-new update error: {err}");
                save_failed = true;
            }
        } else {
            // INSERT: no row id yet; round-trip the new id back into
            // the rendered hidden input so a subsequent submit on
            // the same page becomes an UPDATE rather than a
            // duplicate INSERT.
            let data = UserAdmin::to_insert_map(&form);
            match persistence::insert_record(db, UserAdmin::table_name(), &data).await {
                Ok(new_id) => effective_id = Some(new_id.to_string()),
                Err(err) => {
                    eprintln!("admin-new insert error: {err}");
                    save_failed = true;
                }
            }
        }
    }

    let drawer = render_form(&form);
    let drawer = inject_hidden_id(&drawer, effective_id.as_deref());
    let drawer = patch_save_banner(&drawer, save_failed);

    let (rows, total, current_page, total_pages, validated_sort, validated_dir) =
        fetch_users_table_state(db, query, filters, page, sort, dir).await;
    admin_index_with_drawer(
        drawer,
        rows,
        total,
        query,
        current_page,
        total_pages,
        filters,
        validated_sort.as_deref(),
        validated_dir.as_deref(),
    )
}

/// Build the page shell — topbar / sidebar / page header / table /
/// foundation note — and embed `drawer` as the right-side form. Both
/// the GET path ([`admin_index`]) and the POST path
/// ([`admin_index_post`]) share this so the surrounding chrome is
/// identical regardless of how the drawer was produced. `rows` and
/// `total` come from the persistence layer (or `(empty, 0)` for the
/// no-DB sync path).
#[allow(clippy::too_many_arguments)]
fn admin_index_with_drawer(
    drawer: String,
    rows: Vec<HashMap<String, String>>,
    total: i64,
    query: Option<&str>,
    current_page: i64,
    total_pages: i64,
    filters: &HashMap<String, String>,
    sort: Option<&str>,
    dir: Option<&str>,
) -> String {
    let topbar = render_topbar(&TopbarConfig {
        brand: "RustIO".into(),
        brand_mark: "R".into(),
        env_label: "admin".into(),
        user_initials: "AM".into(),
        user_email: "admin@rustio.dev".into(),
    });

    let sidebar = render_sidebar(&sample_sidebar());

    let breadcrumbs = vec![
        Breadcrumb {
            label: "Home".into(),
            href: Some("/admin-new".into()),
        },
        Breadcrumb {
            label: "Auth".into(),
            href: Some("/admin-new".into()),
        },
        Breadcrumb {
            label: "Users".into(),
            href: None,
        },
    ];

    let trimmed_query = query.map(str::trim).filter(|s| !s.is_empty());
    let subtitle = match trimmed_query {
        Some(q) => {
            format!("Search: '{q}' · {total} results (Page {current_page} of {total_pages})")
        }
        None => format!("auth.User · {total} records (Page {current_page} of {total_pages})"),
    };

    let page_header = render_page_header(&PageHeaderConfig {
        breadcrumbs,
        title: "Users".into(),
        subtitle: Some(subtitle),
        actions: vec![
            PageAction {
                label: "Export CSV".into(),
                href: None,
                primary: false,
            },
            PageAction {
                label: "+ Add user".into(),
                href: None,
                primary: true,
            },
        ],
    });

    let search_cfg = SearchConfig {
        enabled: true,
        prominence: SearchProminence::Standard,
        label: "Search users".into(),
        placeholder: "Search users by username, email, or full name".into(),
        keyboard_enabled: true,
        // Preserve the typed query so the input keeps its value after
        // a search submit — UX stays stable, input never clears
        // mid-search.
        value: trimmed_query.unwrap_or("").to_string(),
        action: "/admin-new".into(),
        filters: vec![FilterChip {
            label: "All".into(),
            count: Some(total.to_string()),
            active: true,
        }],
    };

    // Render the canonical toolbar (search input + filter chips
    // exactly as `ui::render_toolbar` produces it), then weave the
    // metadata-driven filter inputs *into* the search form so they
    // submit alongside `q` on a single GET. Finally append the
    // "+ Add filter" stub + the (currently hidden) advanced-filter
    // block so the affordance is visible without any JS.
    let toolbar_base = render_toolbar(&search_cfg);
    let toolbar_with_filters = inject_filter_inputs_into_toolbar(&toolbar_base, filters);
    let toolbar = format!(
        "{toolbar_with_filters}{}",
        render_advanced_filter_section(filters),
    );

    let table = render_users_table(
        &rows,
        total,
        trimmed_query,
        current_page,
        total_pages,
        filters,
        sort,
        dir,
    );
    let foundation_note = r#"<p style="margin: 20px 0 0; font-family: var(--mono); font-size: 12px; color: var(--ink-subtle);">Live data from <code>admin_new_demo_users</code>. Submit the drawer to insert / update.</p>"#;

    let content = format!("{page_header}{toolbar}{table}{foundation_note}{drawer}");

    render_layout(topbar, sidebar, content)
}

/// Translate the DB rows into a [`TableShellConfig`] and render via
/// the existing [`render_table_shell`]. Empty result set is
/// post-processed into the spec's `<tr><td colspan="100%">No records
/// found</td></tr>` row (or the search-specific "No results found
/// for …" variant when `query` is supplied) so we don't need a new
/// variant on [`TableCell`] (which lives in `ui.rs`, off-limits).
#[allow(clippy::too_many_arguments)]
fn render_users_table(
    rows: &[HashMap<String, String>],
    total: i64,
    query: Option<&str>,
    current_page: i64,
    total_pages: i64,
    filters: &HashMap<String, String>,
    sort: Option<&str>,
    dir: Option<&str>,
) -> String {
    // Each (label, sort_key) pair drives both the rendered `<th>`
    // and the post-process pass that linkifies the header. The sort
    // arrow is set on whichever column matches the current sort.
    let header_specs: &[(&str, &str)] = &[
        ("Username", "username"),
        ("Email", "email"),
        ("Status", "is_active"),
        ("Doctor", "doctor_id"),
        ("Salary", "salary_amount"),
    ];
    let columns = {
        let mut cols = vec![TableColumn::checkbox()];
        for (label, key) in header_specs {
            let mut col = TableColumn::text(*label);
            if sort == Some(*key) {
                let arrow = if dir == Some("desc") { "↓" } else { "↑" };
                col = col.sorted(arrow);
            }
            cols.push(col);
        }
        cols
    };

    let table_rows: Vec<TableRow> = rows
        .iter()
        .map(|r| {
            let username = r.get("username").map(String::as_str).unwrap_or("");
            let email = r.get("email").map(String::as_str).unwrap_or("");
            let is_active_str = r.get("is_active").map(String::as_str).unwrap_or("false");
            let active = matches!(is_active_str, "true" | "1" | "yes");
            let (variant, label) = if active {
                (BadgeVariant::Success, "ACTIVE")
            } else {
                (BadgeVariant::Muted, "INACTIVE")
            };
            let doctor = r.get("doctor_id").map(String::as_str).unwrap_or("");
            let salary = r.get("salary_amount").map(String::as_str).unwrap_or("");
            TableRow {
                selected: false,
                cells: vec![
                    TableCell::Checkbox { checked: false },
                    TableCell::Primary(username.to_string()),
                    TableCell::Mono(email.to_string()),
                    TableCell::Badge {
                        variant,
                        text: label.to_string(),
                    },
                    TableCell::Mono(doctor.to_string()),
                    TableCell::Mono(salary.to_string()),
                ],
            }
        })
        .collect();

    // We render `render_table_shell` with `pagination: None` and
    // append our own pagination block — the shell's built-in
    // pagination emits `<button>` elements without hrefs, which
    // doesn't navigate. The custom block uses `<a href>` so each
    // page link carries the current search query through.
    let cfg = TableShellConfig {
        columns,
        rows: table_rows,
        pagination: None,
    };

    let mut html = render_table_shell(&cfg);

    // Wrap each sortable header label in a real `<a href>` so the
    // column toggles sort state on click. We can't change
    // `render_table_shell` (lives in `ui.rs`, off-limits), so we
    // rebuild the exact substring it emits and swap it for a linked
    // version. For each column the toggle direction is computed
    // against the current state — fresh click → `asc`, click on the
    // already-asc column → `desc`, click on the already-desc column
    // → `asc`. New sorts always reset to page 1.
    let trimmed_for_link = query.map(str::trim).filter(|s| !s.is_empty());
    for (label, key) in header_specs {
        let escaped_label = html_escape(label);
        let is_current = sort == Some(*key);
        let next_dir = if is_current && dir == Some("asc") {
            "desc"
        } else {
            "asc"
        };
        let arrow_suffix = if is_current {
            if dir == Some("desc") {
                " ↓"
            } else {
                " ↑"
            }
        } else {
            ""
        };
        let href_url = build_query_url(
            None, // sort change resets to page 1
            trimmed_for_link,
            filters,
            Some(key),
            Some(next_dir),
        );
        let escaped_href = html_escape(&href_url);

        let original = if is_current {
            let arrow_char = if dir == Some("desc") { "↓" } else { "↑" };
            format!(
                r#"<th class="sorted">{escaped_label} <span class="sort-arrow">{arrow_char}</span></th>"#,
            )
        } else {
            format!(r#"<th>{escaped_label}</th>"#)
        };
        let replacement = if is_current {
            format!(
                r#"<th class="sorted"><a href="{escaped_href}">{escaped_label}{arrow_suffix}</a></th>"#,
            )
        } else {
            format!(r#"<th><a href="{escaped_href}">{escaped_label}</a></th>"#)
        };
        html = html.replacen(&original, &replacement, 1);
    }

    if rows.is_empty() {
        // `render_table_shell` emits `<tbody></tbody>` for an empty
        // row set — replace it with the spec's empty-state row. When
        // a search query was active, the message echoes the query
        // (HTML-escaped) so the user sees what didn't match.
        let replacement = match query {
            Some(q) => format!(
                r#"<tbody><tr><td colspan="100%">No results found for "<strong>{}</strong>"</td></tr></tbody>"#,
                html_escape(q),
            ),
            None => {
                r#"<tbody><tr><td colspan="100%">No records found</td></tr></tbody>"#.to_string()
            }
        };
        html = html.replace("<tbody></tbody>", &replacement);
    }

    // Inject the link-based pagination block just before the closing
    // `</div>` of `.table-wrap`. `render_table_shell` (with no
    // built-in pagination) emits exactly one `</div>` at the very
    // end, so a single `replacen` is safe.
    let pagination_html = render_users_pagination(
        query,
        current_page,
        total_pages,
        total,
        20,
        rows.len(),
        filters,
        sort,
        dir,
    );
    html = html.replacen("</div>", &format!("{pagination_html}</div>"), 1);
    html
}

/// Render the `.pagination` block with real navigation links. The
/// existing `render_pagination` in `ui.rs` emits `<button>` elements
/// only — they're not clickable for navigation. This builds our own
/// so each numbered page is an `<a href="?page=N&q=…">`, the current
/// page is a non-link button (`.active` + `disabled`), and Prev /
/// Next at the boundaries are `<button disabled>` (so the existing
/// `.page-btn:disabled` CSS does the visual work). Class names
/// (`.pagination`, `.pagination-controls`, `.page-btn`,
/// `.page-btn.active`) are reused unchanged.
///
/// The query string is preserved verbatim through every link so a
/// `search + page` combination round-trips cleanly.
#[allow(clippy::too_many_arguments)]
fn render_users_pagination(
    query: Option<&str>,
    current_page: i64,
    total_pages: i64,
    total: i64,
    page_size: i64,
    window_size: usize,
    filters: &HashMap<String, String>,
    sort: Option<&str>,
    dir: Option<&str>,
) -> String {
    use std::fmt::Write as _;

    let total_pages = total_pages.max(1);
    let current_page = current_page.clamp(1, total_pages);
    let (showing_from, showing_to) = if window_size == 0 {
        (0, 0)
    } else {
        let from = (current_page - 1) * page_size + 1;
        let to = from + window_size as i64 - 1;
        (from, to)
    };

    let trimmed_query = query.map(str::trim).filter(|s| !s.is_empty());
    let make_href = |page: i64| -> String {
        // Reuses the shared link builder so pagination + sort
        // headers can never drift on the parameter set.
        html_escape(&build_query_url(
            Some(page),
            trimmed_query,
            filters,
            sort,
            dir,
        ))
    };

    let mut html = String::from(r#"<div class="pagination">"#);
    let _ = write!(
        html,
        r#"<div>Showing <span>{showing_from}</span>–<span>{showing_to}</span> of <span>{total}</span></div>"#,
    );
    html.push_str(r#"<div class="pagination-controls">"#);

    // Prev — clickable when not on the first page.
    if current_page > 1 {
        let _ = write!(
            html,
            r#"<a class="page-btn" href="{}">‹ Prev</a>"#,
            make_href(current_page - 1),
        );
    } else {
        html.push_str(r#"<button type="button" class="page-btn" disabled>‹ Prev</button>"#);
    }

    // Numbered pages — `<a>` for navigable, `<button disabled>` for
    // the current page (visually highlighted via `.active`,
    // unclickable).
    for p in 1..=total_pages {
        if p == current_page {
            let _ = write!(
                html,
                r#"<button type="button" class="page-btn active" disabled aria-current="page">{p}</button>"#,
            );
        } else {
            let _ = write!(
                html,
                r#"<a class="page-btn" href="{}">{p}</a>"#,
                make_href(p),
            );
        }
    }

    // Next — clickable when not on the last page.
    if current_page < total_pages {
        let _ = write!(
            html,
            r#"<a class="page-btn" href="{}">Next ›</a>"#,
            make_href(current_page + 1),
        );
    } else {
        html.push_str(r#"<button type="button" class="page-btn" disabled>Next ›</button>"#);
    }

    html.push_str("</div></div>");
    html
}

/// Inject the metadata-driven filter inputs (one per `filterable`
/// AdminUiField) into the toolbar's search form, just before its
/// closing `</form>`. This piggy-backs on the existing search form
/// — no new form, no new toolbar — so filters submit alongside `q`
/// on a single GET via the existing search-submit button or Enter.
///
/// Each input's value is re-populated from the current `filters`
/// map so the toolbar reflects the active filter state on every
/// re-render. Boolean → tri-state `<select>`, FK / enum → typed
/// `<select>` from `options`, free text → `<input type="text">`.
fn inject_filter_inputs_into_toolbar(
    toolbar_html: &str,
    filters: &HashMap<String, String>,
) -> String {
    let inputs = build_filter_inputs(false, filters);
    if inputs.is_empty() {
        return toolbar_html.to_string();
    }
    // Single search form per toolbar — `replacen(1)` is safe.
    toolbar_html.replacen("</form>", &format!("{inputs}</form>"), 1)
}

/// Render the "+ Add filter" affordance + the (HTML-`hidden`)
/// advanced-filter block. Only renders when at least one
/// `advanced_filter == true` field exists. The block lives in the
/// DOM so the structure is observable, but stays invisible until JS
/// or a future server-side toggle reveals it.
fn render_advanced_filter_section(filters: &HashMap<String, String>) -> String {
    let advanced_inputs = build_filter_inputs(true, filters);
    if advanced_inputs.is_empty() {
        return String::new();
    }
    format!(
        r#"<div class="toolbar toolbar-filters-only" style="border-radius:0;border-top:none;border-bottom:none;">
  <button type="button" class="btn">+ Add filter</button>
</div>
<div class="advanced-filters" hidden>{advanced_inputs}</div>"#
    )
}

/// Walk `UserAdmin::fields()` and emit the input HTML for each
/// field that matches `advanced` (`false` = default toolbar
/// filters, `true` = advanced filters). Output respects the spec's
/// HTML patterns: tri-state `<select>` for Boolean, typed
/// `<select>` for Select / FK, plain `<input type="text">` for
/// Exact. Each control's `value` is taken from `filters` so the
/// toolbar stays consistent with the URL after every submit.
fn build_filter_inputs(advanced: bool, filters: &HashMap<String, String>) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    for field in UserAdmin::fields() {
        let want = if advanced {
            field.advanced_filter
        } else {
            field.filterable
        };
        if !want {
            continue;
        }
        let current = filters.get(field.name).map(String::as_str).unwrap_or("");
        match resolve_filter_type(&field) {
            FilterType::Boolean => {
                let _ = write!(
                    out,
                    r#"<select name="{name}" aria-label="{label}"><option value="">All {label}</option><option value="true"{sel_t}>{label}: Yes</option><option value="false"{sel_f}>{label}: No</option></select>"#,
                    name = html_escape(field.name),
                    label = html_escape(field.label),
                    sel_t = if current == "true" { " selected" } else { "" },
                    sel_f = if current == "false" { " selected" } else { "" },
                );
            }
            FilterType::Select => {
                let _ = write!(
                    out,
                    r#"<select name="{name}" aria-label="{label}"><option value="">All {label}</option>"#,
                    name = html_escape(field.name),
                    label = html_escape(field.label),
                );
                for (val, label) in &field.options {
                    let sel = if val == current { " selected" } else { "" };
                    let _ = write!(
                        out,
                        r#"<option value="{}"{}>{}</option>"#,
                        html_escape(val),
                        sel,
                        html_escape(label),
                    );
                }
                out.push_str("</select>");
            }
            FilterType::Exact => {
                let _ = write!(
                    out,
                    r#"<input type="text" name="{name}" value="{value}" placeholder="{label}" aria-label="{label}">"#,
                    name = html_escape(field.name),
                    value = html_escape(current),
                    label = html_escape(field.label),
                );
            }
        }
    }
    out
}

/// Percent-encode a query-parameter value per RFC 3986. Only the
/// unreserved set (`ALPHA / DIGIT / -._~`) is left alone; everything
/// else is `%HH`. Avoids pulling in the `urlencoding` crate just
/// for the search-preserving links.
fn url_encode_value(s: &str) -> String {
    use std::fmt::Write as _;
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(b as char);
            }
            _ => {
                let _ = write!(out, "%{b:02X}");
            }
        }
    }
    out
}

// ---------------------------------------------------------------
// Drawer post-processing helpers (form.rs is off-limits, so the
// hidden id input + banner-text swap happen here as substring edits)
// ---------------------------------------------------------------

/// Inject `<input type="hidden" name="id" value="...">` immediately
/// after the rendered `<form data-admin-form ...>` opening tag. An
/// empty `id` value is still emitted so the POST handler can
/// distinguish "no editing id" (insert) from "editing id present"
/// (update) via the same single body field — and so a successful
/// CREATE round-trips its new row id back into the form for a
/// subsequent UPDATE.
fn inject_hidden_id(drawer: &str, id: Option<&str>) -> String {
    let value = id.unwrap_or("");
    drawer.replacen(
        r#"<form data-admin-form action="" method="post">"#,
        &format!(
            r#"<form data-admin-form action="" method="post"><input type="hidden" name="id" value="{}">"#,
            html_escape(value),
        ),
        1,
    )
}

/// Two-step swap on the rendered drawer:
///
/// 1. Always replace `Saved successfully (simulation)` →
///    `Saved successfully` (the persistence layer makes this a real
///    save now, not a simulation).
/// 2. If the DB write failed despite valid input, swap the success
///    banner for `<div class="form-error-summary">Failed to save
///    record</div>` so the user sees an explicit failure rather than
///    a silent success.
fn patch_save_banner(drawer: &str, save_failed: bool) -> String {
    let after_text = drawer.replace(
        r#"<div class="form-success" role="status">Saved successfully (simulation)</div>"#,
        r#"<div class="form-success" role="status">Saved successfully</div>"#,
    );
    if save_failed {
        after_text.replace(
            r#"<div class="form-success" role="status">Saved successfully</div>"#,
            r#"<div class="form-error-summary" role="alert">Failed to save record</div>"#,
        )
    } else {
        after_text
    }
}

// ---------------------------------------------------------------
// Bridge form construction (shared by GET demo + POST handler)
// ---------------------------------------------------------------

/// Build the `UserAdmin` form with the same `override_field` chain
/// the demo uses. Extracted so the persist path can reuse the exact
/// same shape without re-stating the help-text override.
fn build_user_admin_form() -> FormConfig {
    FormBuilder::from_admin_ui_model::<UserAdmin>()
        .override_field(
            "doctor_id",
            FieldOverride {
                field_type: None,
                label: None,
                help: Some("Assigned doctor — shown by name, never by id.".into()),
            },
        )
        .build()
}

// ---------------------------------------------------------------
// AdminModel-bridge demo (rendered on /admin-new today)
// ---------------------------------------------------------------

/// Tag struct used purely as a type parameter for
/// `FormBuilder::from_admin_ui_model::<UserAdmin>()`. The bridge's
/// `AdminUiModel` carries the `Ui` suffix so it can't collide with
/// the framework's existing `crate::admin::AdminModel` trait.
struct UserAdmin;

impl PersistableModel for UserAdmin {
    fn table_name() -> &'static str {
        "admin_new_demo_users"
    }

    fn primary_key() -> &'static str {
        "id"
    }

    /// INSERT and UPDATE use the same column projection: every form
    /// field except the primary key. Booleans are stored as text
    /// (`"true"` / `"false"`) — matches the values produced by
    /// [`crate::admin::form::bind_form`]'s Boolean branch.
    fn to_insert_map(form: &FormConfig) -> HashMap<String, String> {
        user_admin_column_map(form)
    }

    fn to_update_map(form: &FormConfig) -> HashMap<String, String> {
        user_admin_column_map(form)
    }
}

fn user_admin_column_map(form: &FormConfig) -> HashMap<String, String> {
    let mut m = HashMap::new();
    for f in &form.fields {
        // Never write the primary key from form fields. The `id` is
        // either auto-generated on INSERT or comes from the URL /
        // hidden input on UPDATE.
        if f.name == "id" {
            continue;
        }
        m.insert(f.name.clone(), f.value.clone().unwrap_or_default());
    }
    m
}

impl AdminUiModel for UserAdmin {
    fn model_name() -> &'static str {
        "User"
    }

    fn fields() -> Vec<AdminUiField> {
        vec![
            AdminUiField {
                name: "username",
                label: "Username",
                data_type: AdminDataType::String,
                required: true,
                readonly: false,
                is_relation: false,
                options: vec![],
                filterable: true,
                advanced_filter: false,
                sortable: true,
            },
            AdminUiField {
                name: "email",
                label: "Email",
                data_type: AdminDataType::Email,
                required: true,
                readonly: false,
                is_relation: false,
                options: vec![],
                filterable: false,
                advanced_filter: true,
                sortable: true,
            },
            AdminUiField {
                name: "is_active",
                label: "Active",
                data_type: AdminDataType::Boolean,
                required: false,
                readonly: false,
                is_relation: false,
                options: vec![],
                filterable: true,
                advanced_filter: false,
                sortable: true,
            },
            AdminUiField {
                name: "doctor_id",
                label: "Doctor",
                data_type: AdminDataType::Integer,
                required: true,
                readonly: false,
                is_relation: true,
                options: vec![
                    ("1".into(), "Dr. Erik".into()),
                    ("2".into(), "Dr. Sara".into()),
                ],
                filterable: true,
                advanced_filter: false,
                sortable: true,
            },
            AdminUiField {
                name: "salary_amount",
                label: "Salary",
                data_type: AdminDataType::Float,
                required: false,
                readonly: false,
                is_relation: false,
                options: vec![],
                filterable: false,
                advanced_filter: true,
                sortable: true,
            },
        ]
    }
}

/// Render the bridge-driven form for [`UserAdmin`].
///
/// Two paths share one body, depending on whether the page was
/// reached via GET (`submitted = None`) or POST (`submitted = Some`):
///
/// - **GET** seeds an invalid demo state so the page is self-explanatory
///   on first load: `username` is left empty (required error),
///   `email` is `"not-an-email"`, `salary_amount` is non-numeric.
/// - **POST** binds values via [`crate::admin::form::bind_form`], then
///   validates. Boolean fields submit as missing-when-unchecked, so
///   `bind_form` derives them from key presence in `params`.
///
/// In both paths the same [`render_form`] runs, so the user sees a
/// drawer with their values intact, error states (or success banner)
/// on top, and `class="invalid"` on any field that failed.
pub fn demo_admin_form(submitted: Option<&HashMap<String, String>>) -> String {
    let mut form = FormBuilder::from_admin_ui_model::<UserAdmin>()
        .override_field(
            "doctor_id",
            FieldOverride {
                field_type: None,
                label: None,
                help: Some("Assigned doctor — shown by name, never by id.".into()),
            },
        )
        .build();

    match submitted {
        None => {
            // GET: seed invalid demo values so the page is self-
            // explanatory on first load (matches the previous
            // step's behaviour exactly).
            for field in form.fields.iter_mut() {
                match field.name.as_str() {
                    // `username` left as None → triggers "required".
                    "email" => field.value = Some("not-an-email".into()),
                    "salary_amount" => field.value = Some("twelve thousand".into()),
                    _ => {}
                }
            }
        }
        Some(params) => {
            // POST: bind real submitted values; flips the form's
            // `submitted` flag, which the renderer uses to show the
            // success banner when validation passes.
            crate::admin::form::bind_form(&mut form, params);
        }
    }

    crate::admin::form::validate_form(&mut form);
    render_form(&form)
}

// ---------------------------------------------------------------
// Auto-form demo model (still exported, no longer rendered)
// ---------------------------------------------------------------

/// Tag struct used purely as a type parameter for
/// [`FormBuilder::from_model::<User>()`]. The model declares its
/// form shape via [`FormModel`] instead of being hand-wired.
struct User;

impl FormModel for User {
    fn form_title() -> &'static str {
        "Edit user"
    }

    fn form_fields() -> Vec<AutoField> {
        vec![
            AutoField {
                name: "username",
                label: "Username",
                field_type: None,
                required: true,
                is_foreign_key: false,
                options: vec![],
            },
            AutoField {
                name: "email",
                label: "Email",
                field_type: None,
                required: true,
                is_foreign_key: false,
                options: vec![],
            },
            AutoField {
                name: "is_active",
                label: "Active — user can log in",
                field_type: None,
                required: false,
                is_foreign_key: false,
                options: vec![],
            },
            AutoField {
                name: "doctor_id",
                label: "Doctor",
                field_type: None,
                required: true,
                is_foreign_key: true,
                options: vec![
                    ("1".into(), "Dr. Erik".into()),
                    ("2".into(), "Dr. Sara".into()),
                ],
            },
            AutoField {
                name: "salary_amount",
                label: "Salary",
                field_type: None,
                required: false,
                is_foreign_key: false,
                options: vec![],
            },
        ]
    }
}

/// Render the auto-generated form for [`User`]. Demonstrates:
/// - inference (`username` → Text, `email` → Email, `is_active` →
///   Boolean switch, `salary_amount` → Number, `doctor_id` → FK
///   dropdown);
/// - the FK "no raw IDs" rule (options carry human labels);
/// - the hybrid override path via [`FormBuilder::override_field`]
///   (attaching `help` text post-generation).
pub fn demo_auto_form() -> String {
    let form = FormBuilder::from_model::<User>()
        .override_field(
            "email",
            FieldOverride {
                field_type: Some(FieldType::Email),
                label: None,
                help: Some("Work email only.".into()),
            },
        )
        .override_field(
            "doctor_id",
            FieldOverride {
                field_type: None,
                label: None,
                help: Some("Linked clinician — shown by name, never by id.".into()),
            },
        )
        .build();
    render_form(&form)
}

/// Manually-configured demo form retained from the previous step.
/// Not rendered on `/admin-new` anymore, but still exported so
/// callers can render it directly. Kept intact to satisfy the
/// "don't break current demo form" rule.
pub fn demo_form() -> String {
    render_form(&FormConfig {
        title: "Edit user".into(),
        subtitle: "auth.User · id=1".into(),
        submitted: false,
        fields: vec![
            FieldConfig {
                name: "username".into(),
                label: "Username".into(),
                field_type: FieldType::Text,
                required: true,
                readonly: false,
                placeholder: Some("lowercase, max 32 chars".into()),
                help: Some("Lowercase, alphanumeric, underscores.".into()),
                value: Some("amansour".into()),
                options: vec![],
                error: None,
            },
            FieldConfig {
                name: "email".into(),
                label: "Email".into(),
                field_type: FieldType::Email,
                required: true,
                readonly: false,
                placeholder: None,
                help: None,
                value: Some("admin@rustio.dev".into()),
                options: vec![],
                error: None,
            },
            FieldConfig {
                name: "doctor_id".into(),
                label: "Doctor".into(),
                field_type: FieldType::ForeignKey,
                required: true,
                readonly: false,
                placeholder: None,
                help: Some("Linked clinician — shown by name, never by id.".into()),
                value: Some("1".into()),
                options: vec![
                    ("1".into(), "Dr. Erik".into()),
                    ("2".into(), "Dr. Sara".into()),
                ],
                error: None,
            },
            FieldConfig {
                name: "role".into(),
                label: "Role".into(),
                field_type: FieldType::Select,
                required: true,
                readonly: false,
                placeholder: None,
                help: None,
                value: Some("editor".into()),
                options: vec![
                    ("viewer".into(), "Viewer".into()),
                    ("editor".into(), "Editor".into()),
                    ("admin".into(), "Admin".into()),
                ],
                error: None,
            },
            FieldConfig {
                name: "is_active".into(),
                label: "Active — user can log in".into(),
                field_type: FieldType::Boolean,
                required: false,
                readonly: false,
                placeholder: None,
                help: None,
                value: Some("true".into()),
                options: vec![],
                error: None,
            },
            FieldConfig {
                name: "salary_amount".into(),
                label: "Salary".into(),
                field_type: FieldType::Number,
                required: false,
                readonly: false,
                placeholder: Some("e.g. 65000".into()),
                help: Some("Annual gross, in the org's base currency.".into()),
                value: Some("72000".into()),
                options: vec![],
                error: None,
            },
            FieldConfig {
                name: "starts_at".into(),
                label: "Starts at".into(),
                field_type: FieldType::DateTime,
                required: false,
                readonly: false,
                placeholder: None,
                help: None,
                value: Some("2026-05-01T09:00".into()),
                options: vec![],
                error: None,
            },
            FieldConfig {
                name: "notes".into(),
                label: "Notes".into(),
                field_type: FieldType::TextArea,
                required: false,
                readonly: false,
                placeholder: Some("Internal notes — not visible to user.".into()),
                help: None,
                value: None,
                options: vec![],
                error: None,
            },
        ],
    })
}

// ---------------------------------------------------------------
// Sample data (foundation step — replaced by DB in a later step)
// ---------------------------------------------------------------

fn sample_sidebar() -> Vec<SidebarGroup> {
    vec![
        SidebarGroup {
            label: "Auth".into(),
            items: vec![
                SidebarItem {
                    label: "Users".into(),
                    count: Some("142".into()),
                    href: "/admin-new".into(),
                    active: true,
                },
                SidebarItem {
                    label: "Groups".into(),
                    count: Some("8".into()),
                    href: "#".into(),
                    active: false,
                },
                SidebarItem {
                    label: "Permissions".into(),
                    count: Some("64".into()),
                    href: "#".into(),
                    active: false,
                },
                SidebarItem {
                    label: "API Tokens".into(),
                    count: Some("23".into()),
                    href: "#".into(),
                    active: false,
                },
            ],
        },
        SidebarGroup {
            label: "Content".into(),
            items: vec![
                SidebarItem {
                    label: "Articles".into(),
                    count: Some("318".into()),
                    href: "#".into(),
                    active: false,
                },
                SidebarItem {
                    label: "Categories".into(),
                    count: Some("12".into()),
                    href: "#".into(),
                    active: false,
                },
                SidebarItem {
                    label: "Media".into(),
                    count: Some("1.2k".into()),
                    href: "#".into(),
                    active: false,
                },
            ],
        },
        SidebarGroup {
            label: "System".into(),
            items: vec![
                SidebarItem {
                    label: "Migrations".into(),
                    count: Some("47".into()),
                    href: "#".into(),
                    active: false,
                },
                SidebarItem {
                    label: "Audit Log".into(),
                    count: Some("—".into()),
                    href: "#".into(),
                    active: false,
                },
                SidebarItem {
                    label: "Settings".into(),
                    count: None,
                    href: "#".into(),
                    active: false,
                },
            ],
        },
    ]
}

// `sample_users_table()` (the previous static demo data source) was
// removed when the table was wired to `admin_new_demo_users`. The
// real table builder lives at `render_users_table` near
// `admin_index_with_drawer`.
