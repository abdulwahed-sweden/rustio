//! Admin page assembler (v3, 2026-04-23 full-UI-reset).
//!
//! Renders every page the admin engine serves. The shell is one
//! grid: sidebar on the left (real models only), sticky topbar on
//! the right, main content centered inside a 1280px-max column.
//!
//! All CSS + JS is inlined at compile time via `include_str!`. No
//! external stylesheets, no filesystem reads, no `/static` links.

use std::collections::HashMap;

use crate::admin::admin_form_bridge::{
    resolve_filter_type, AdminDataType, AdminUiField, AdminUiModel, FilterType,
};
use crate::admin::auto_form::{AutoField, FieldOverride, FormBuilder, FormModel};
use crate::admin::form::{render_form, FieldConfig, FieldType, FormConfig};
use crate::admin::persistence;
use crate::admin::ui::{
    html_escape, render_page_header, render_sidebar, render_topbar, PageAction, PageHeaderConfig,
    SidebarGroup, SidebarItem, TopbarConfig,
};
use crate::orm::Db;

// ---------------------------------------------------------------
// Bundled CSS + JS
// ---------------------------------------------------------------

const THEME_CSS: &str = include_str!("../../assets/admin-new/theme.css");
const COMPONENTS_CSS: &str = include_str!("../../assets/admin-new/components.css");
const ADMIN_JS: &str = include_str!("../../assets/admin-new/admin.js");

// ---------------------------------------------------------------
// Shell assembler
// ---------------------------------------------------------------

/// Assemble a full admin page. `topbar` / `sidebar` / `content`
/// are already-rendered HTML fragments; they are embedded into the
/// `<div class="app">` shell. `drawer_html` renders outside the
/// main column so its fixed-position styles layer above everything.
pub fn render_layout(
    topbar: String,
    sidebar: String,
    content: String,
    drawer_html: String,
) -> String {
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
</style>
</head>
<body>
<div class="app">
{sidebar}
<div class="main-col">
{topbar}
<main class="content">
{content}
</main>
</div>
</div>
{drawer_html}
<script>{admin_js}</script>
</body>
</html>"#,
        theme = THEME_CSS,
        components = COMPONENTS_CSS,
        topbar = topbar,
        sidebar = sidebar,
        content = content,
        drawer_html = drawer_html,
        admin_js = ADMIN_JS,
    )
}

// ---------------------------------------------------------------
// Dashboard (admin index — GET /admin)
// ---------------------------------------------------------------

/// One card + one sidebar entry per registered model.
struct DashboardEntry {
    slug: &'static str,
    model_name: &'static str,
    table_name: &'static str,
    count: i64,
}

/// Walk the registry, count rows per table, return a sorted list.
/// Extracted so both the dashboard and the per-model pages share one
/// source of truth for the sidebar contents.
async fn collect_dashboard_entries(
    db: &Db,
    registry: &crate::admin::admin_form_bridge::AdminRegistry,
) -> Vec<DashboardEntry> {
    use sqlx::Row;
    let mut slugs: Vec<&'static str> = registry.slugs().copied().collect();
    slugs.sort();
    let mut out = Vec::with_capacity(slugs.len());
    for slug in slugs {
        let Some(model) = registry.get(slug) else {
            continue;
        };
        if let Some(sql) = model.ensure_table_sql() {
            let _ = persistence::ensure_table(db, sql).await;
        }
        let table = model.table_name();
        let count: i64 = {
            let sql = format!(
                "SELECT COUNT(*) AS c FROM \"{}\"",
                table.replace('"', "\"\"")
            );
            match sqlx::query(&sql).fetch_one(db.pool()).await {
                Ok(row) => row.try_get::<i64, _>("c").unwrap_or(0),
                Err(_) => 0,
            }
        };
        out.push(DashboardEntry {
            slug: model.slug(),
            model_name: model.model_name(),
            table_name: table,
            count,
        });
    }
    out
}

/// Build the single "Models" sidebar group from the registry.
/// `active_slug = Some("users")` marks that row as active. Same
/// shape on the dashboard (no item active) and on per-model pages
/// (the current model active).
fn build_admin_sidebar(entries: &[DashboardEntry], active_slug: Option<&str>) -> String {
    let items: Vec<SidebarItem> = entries
        .iter()
        .map(|e| SidebarItem {
            // Pluralize by appending "s" — fine for users / orders;
            // future models with irregular plurals will need a
            // `plural_name` accessor on AdminUiModel.
            label: format!("{}s", e.model_name),
            count: Some(e.count.to_string()),
            href: format!("/admin/{}", e.slug),
            active: active_slug == Some(e.slug),
        })
        .collect();
    render_sidebar(&[SidebarGroup {
        label: Some("Models".to_string()),
        items,
    }])
}

/// Render the admin dashboard: one stat card per registered model.
/// Uses the same shell as per-model pages — sidebar shows every
/// registered model with its live row count.
pub async fn admin_dashboard_get(
    db: &Db,
    registry: &crate::admin::admin_form_bridge::AdminRegistry,
    csrf_token: Option<&str>,
) -> String {
    use std::fmt::Write as _;
    let entries = collect_dashboard_entries(db, registry).await;

    let mut cards = String::new();
    for e in &entries {
        let model_href = format!("/admin/{}", e.slug);
        let _ = write!(
            cards,
            r#"<a class="stat-card" href="{href}"><span class="stat-card-label">{label}</span><span class="stat-card-value">{count}</span><span class="stat-card-meta">{table}</span></a>"#,
            href = html_escape(&model_href),
            label = html_escape(&format!("{}s", e.model_name)),
            count = e.count,
            table = html_escape(e.table_name),
        );
    }

    let topbar = render_topbar(&TopbarConfig {
        title: "Dashboard".to_string(),
        user_initials: "AM".to_string(),
        user_email: "admin@rustio.dev".to_string(),
        csrf_token: csrf_token.map(String::from),
    });
    let sidebar = build_admin_sidebar(&entries, None);

    let header = render_page_header(&PageHeaderConfig {
        eyebrow: Some("Overview".to_string()),
        title: "Dashboard".to_string(),
        subtitle: Some(format!(
            "{} model{} registered",
            entries.len(),
            if entries.len() == 1 { "" } else { "s" }
        )),
        actions: Vec::new(),
        breadcrumbs: Vec::new(),
    });
    let content = format!(r#"{header}<div class="stat-grid">{cards}</div>"#);
    render_layout(topbar, sidebar, content, String::new())
}

/// Public entry point kept for backwards compat: expose the
/// registry-driven sidebar so the per-model page path can call it.
pub async fn render_admin_sidebar_for(
    db: &Db,
    registry: &crate::admin::admin_form_bridge::AdminRegistry,
    active_slug: Option<&str>,
) -> String {
    let entries = collect_dashboard_entries(db, registry).await;
    build_admin_sidebar(&entries, active_slug)
}

// ---------------------------------------------------------------
// Public entry point — handler body for /admin-new
// ---------------------------------------------------------------

/// GET orchestrator (model-driven): looks up the registered model
/// by slug, ensures its table exists, loads the row identified by
/// `editing_id` (if any) for prefill, fetches the table window +
/// count, and renders the page. Every piece of behaviour comes from
/// the model's metadata — no hardcoded column names, table names,
/// or model logic.
#[allow(clippy::too_many_arguments)]
pub async fn admin_index_get(
    db: &Db,
    registry: &crate::admin::admin_form_bridge::AdminRegistry,
    model: &dyn AdminUiModel,
    editing_id: Option<&str>,
    query: Option<&str>,
    page: i64,
    filters: &HashMap<String, String>,
    sort: Option<&str>,
    dir: Option<&str>,
    advanced: bool,
) -> String {
    if let Some(sql) = model.ensure_table_sql() {
        let _ = persistence::ensure_table(db, sql).await;
    }

    // Drawer mode:
    //   editing_id = None         → drawer not rendered (list-only view)
    //   editing_id = Some("new")  → drawer open, create mode (empty form)
    //   editing_id = Some("<id>") → drawer open, edit mode (prefilled)
    // A real id that doesn't match any row falls back to create mode
    // so "+ Add" links don't dead-end on a racing delete.
    let is_new = matches!(editing_id, Some("new"));
    let prefill = if is_new {
        None
    } else {
        match editing_id {
            Some(id) if !id.is_empty() => {
                match persistence::get_record_by_id(db, model.table_name(), id).await {
                    Ok(map) if !map.is_empty() => Some(map),
                    _ => None,
                }
            }
            _ => None,
        }
    };
    let drawer = match editing_id {
        Some(id) if !id.is_empty() => {
            // id is present — render the drawer. prefill decides
            // create vs edit mode.
            let effective_id = if is_new { None } else { Some(id) };
            build_drawer_for_get(model, prefill.as_ref(), effective_id)
        }
        _ => String::new(),
    };

    let (rows, total, current_page, total_pages, validated_sort, validated_dir) =
        fetch_users_table_state(db, model, query, filters, page, sort, dir).await;
    let sidebar_html = render_admin_sidebar_for(db, registry, Some(model.slug())).await;
    admin_index_with_drawer(
        model,
        sidebar_html,
        drawer,
        rows,
        total,
        query,
        current_page,
        total_pages,
        filters,
        validated_sort.as_deref(),
        validated_dir.as_deref(),
        advanced,
        None,
    )
}

/// Build the drawer for the GET path. Identical drawer for the
/// same `(model, prefill, id)` whether the page came from a fresh
/// load or a redirect.
fn build_drawer_for_get(
    model: &dyn AdminUiModel,
    prefill: Option<&HashMap<String, String>>,
    editing_id: Option<&str>,
) -> String {
    let mut form = build_admin_form(model);
    if let Some(values) = prefill.filter(|v| !v.is_empty()) {
        for field in form.fields.iter_mut() {
            if let Some(v) = values.get(&field.name) {
                field.value = Some(v.clone());
            }
        }
    }
    form.hidden_fields
        .push(("id".to_string(), editing_id.unwrap_or("").to_string()));
    render_form(&form)
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
#[allow(clippy::too_many_arguments)]
async fn fetch_users_table_state(
    db: &Db,
    model: &dyn AdminUiModel,
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
    let table = model.table_name();
    let searchable: Vec<&str> = model.searchable_fields();
    let (eq_filters, like_filters) = classify_filters(model, filters);
    let (validated_sort, validated_dir) = validate_sort_state(model, sort, dir);

    let total = persistence::count_filtered_records(
        db,
        table,
        &eq_filters,
        &like_filters,
        query,
        &searchable,
    )
    .await
    .unwrap_or(0);

    let total_pages = if total > 0 {
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
        &searchable,
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

/// Bulk-action POST path. Validates `ids` as numeric (anything that
/// can't `parse::<i64>` is dropped — the client should never send
/// non-numeric values, but this keeps a malicious / malformed body
/// from reaching the SQL layer). Dispatches `action` to the right
/// persistence helper, captures success / failure into a banner,
/// and re-renders the page with `page=1` per the spec's reset rule.
/// `q` / `filters` / `sort` / `dir` are preserved verbatim.
#[allow(clippy::too_many_arguments)]
pub async fn admin_index_bulk(
    db: &Db,
    registry: &crate::admin::admin_form_bridge::AdminRegistry,
    model: &dyn AdminUiModel,
    action: &str,
    ids: &[String],
    query: Option<&str>,
    filters: &HashMap<String, String>,
    sort: Option<&str>,
    dir: Option<&str>,
    advanced: bool,
) -> String {
    if let Some(sql) = model.ensure_table_sql() {
        let _ = persistence::ensure_table(db, sql).await;
    }

    let valid_ids: Vec<String> = ids
        .iter()
        .filter(|s| s.parse::<i64>().is_ok())
        .cloned()
        .collect();

    let mut banner = String::new();
    if !valid_ids.is_empty() {
        let table = model.table_name();
        // Activate / Deactivate target the model's primary status
        // field — `None` means the model didn't declare one, so
        // those actions silently no-op (Delete still works).
        let status_field = model.primary_status_field();
        // Each branch awaits inline so the futures don't have to
        // unify into a single opaque type.
        let result: Option<Result<(), crate::error::Error>> = match action {
            "activate" => match status_field {
                Some(f) => Some(persistence::bulk_update(db, table, &valid_ids, f, "true").await),
                None => None,
            },
            "deactivate" => match status_field {
                Some(f) => Some(persistence::bulk_update(db, table, &valid_ids, f, "false").await),
                None => None,
            },
            "delete" => Some(persistence::bulk_delete(db, table, &valid_ids).await),
            _ => None,
        };
        match result {
            Some(Ok(())) => {
                banner = String::from(
                    r#"<div class="form-success" role="status">Bulk action completed</div>"#,
                );
            }
            Some(Err(err)) => {
                eprintln!("admin-new bulk error: {err}");
                banner = String::from(
                    r#"<div class="form-error-summary" role="alert">Bulk action failed</div>"#,
                );
            }
            None => {}
        }
    }

    // Bulk action results land back on the list with drawer closed.
    let drawer = String::new();
    let (rows, total, current_page, total_pages, validated_sort, validated_dir) =
        fetch_users_table_state(db, model, query, filters, 1, sort, dir).await;
    let banner_opt = if banner.is_empty() {
        None
    } else {
        Some(banner.as_str())
    };
    let sidebar_html = render_admin_sidebar_for(db, registry, Some(model.slug())).await;
    admin_index_with_drawer(
        model,
        sidebar_html,
        drawer,
        rows,
        total,
        query,
        current_page,
        total_pages,
        filters,
        validated_sort.as_deref(),
        validated_dir.as_deref(),
        advanced,
        banner_opt,
    )
}

/// Validate the URL's `sort` + `dir` against `UserAdmin` metadata.
/// `sort` must name a field that's both declared and `sortable`;
/// `dir` is normalised to `"asc"` or `"desc"` (any other value
/// becomes `"asc"`). An invalid sort drops both to `None` so
/// persistence falls back to `ORDER BY "id" DESC`. All validation
/// happens here so persistence stays a simple SQL emitter that
/// trusts its inputs.
fn validate_sort_state(
    model: &dyn AdminUiModel,
    sort: Option<&str>,
    dir: Option<&str>,
) -> (Option<String>, Option<String>) {
    let valid_sort = sort.filter(|s| model.fields().iter().any(|f| f.name == *s && f.sortable));
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
    advanced: bool,
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
    if advanced {
        parts.push("advanced=1".to_string());
    }
    if parts.is_empty() {
        String::new()
    } else {
        format!("?{}", parts.join("&"))
    }
}

/// Walk `model.fields()` and split the raw URL filter map into two
/// buckets keyed off [`resolve_filter_type`]: equality filters
/// (Boolean, Select) and `LIKE` filters (Exact text). Any URL key
/// that doesn't correspond to a declared `AdminUiField` is silently
/// dropped — this is the security boundary that stops an attacker
/// from injecting `?random_column=x` to query columns that admin
/// metadata never exposed as filterable.
fn classify_filters(
    model: &dyn AdminUiModel,
    raw: &HashMap<String, String>,
) -> (HashMap<String, String>, HashMap<String, String>) {
    let fields = model.fields();
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
    registry: &crate::admin::admin_form_bridge::AdminRegistry,
    model: &dyn AdminUiModel,
    params: &HashMap<String, String>,
    editing_id: Option<&str>,
    query: Option<&str>,
    page: i64,
    filters: &HashMap<String, String>,
    sort: Option<&str>,
    dir: Option<&str>,
    advanced: bool,
) -> String {
    if let Some(sql) = model.ensure_table_sql() {
        let _ = persistence::ensure_table(db, sql).await;
    }

    let mut form = build_admin_form(model);
    crate::admin::form::bind_form(&mut form, params);
    crate::admin::form::validate_form(&mut form);

    let any_errors = form.fields.iter().any(|f| f.error.is_some());

    let mut effective_id = editing_id.map(String::from);
    let mut save_failed = false;

    if !any_errors {
        let table = model.table_name();
        let pk = model.primary_key();
        if let Some(id) = effective_id.as_deref() {
            let data = persistence::form_to_column_map(&form, pk);
            if let Err(err) = persistence::update_record(db, table, id, &data).await {
                eprintln!("admin-new update error: {err}");
                save_failed = true;
            }
        } else {
            let data = persistence::form_to_column_map(&form, pk);
            match persistence::insert_record(db, table, &data).await {
                Ok(new_id) => effective_id = Some(new_id.to_string()),
                Err(err) => {
                    eprintln!("admin-new insert error: {err}");
                    save_failed = true;
                }
            }
        }
    }

    form.hidden_fields
        .push(("id".to_string(), effective_id.clone().unwrap_or_default()));
    form.save_failed = save_failed;
    // After a successful save, close the drawer so the user sees the
    // list with the new / updated row. After a validation error or
    // save failure, leave the drawer open with the user's values and
    // the error banner so they can fix and retry.
    let drawer = if any_errors || save_failed {
        render_form(&form)
    } else {
        String::new()
    };

    let (rows, total, current_page, total_pages, validated_sort, validated_dir) =
        fetch_users_table_state(db, model, query, filters, page, sort, dir).await;
    let sidebar_html = render_admin_sidebar_for(db, registry, Some(model.slug())).await;
    admin_index_with_drawer(
        model,
        sidebar_html,
        drawer,
        rows,
        total,
        query,
        current_page,
        total_pages,
        filters,
        validated_sort.as_deref(),
        validated_dir.as_deref(),
        advanced,
        None,
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
    model: &dyn AdminUiModel,
    sidebar_html: String,
    drawer: String,
    rows: Vec<HashMap<String, String>>,
    total: i64,
    query: Option<&str>,
    current_page: i64,
    total_pages: i64,
    filters: &HashMap<String, String>,
    sort: Option<&str>,
    dir: Option<&str>,
    advanced: bool,
    top_banner: Option<&str>,
) -> String {
    let model_name = model.model_name();
    let model_slug = model.slug();
    let title_label = format!("{model_name}s");

    let topbar = render_topbar(&TopbarConfig {
        title: title_label.clone(),
        user_initials: "AM".into(),
        user_email: "admin@rustio.dev".into(),
        // Per-model pages render the topbar without a CSRF input
        // for now — its Sign-out link falls back to the GET
        // confirmation page. Threading CSRF here is a follow-up:
        // the model handlers would need to pass req.ctx() through.
        csrf_token: None,
    });

    let trimmed_query = query.map(str::trim).filter(|s| !s.is_empty());
    let subtitle = match trimmed_query {
        Some(q) => format!(
            "Searching “{q}” · {total} match{plural} · page {current_page} of {total_pages}",
            plural = if total == 1 { "" } else { "es" },
        ),
        None => format!(
            "{total} record{plural} · page {current_page} of {total_pages}",
            plural = if total == 1 { "" } else { "s" },
        ),
    };

    let add_href = format!("/admin/{model_slug}?id=new");
    let page_header = render_page_header(&PageHeaderConfig {
        eyebrow: Some("Models".to_string()),
        title: title_label,
        subtitle: Some(subtitle),
        actions: vec![PageAction {
            label: format!("New {model_name}"),
            href: Some(add_href),
            primary: true,
        }],
        breadcrumbs: Vec::new(),
    });

    let toolbar_html =
        render_users_toolbar(model, trimmed_query, filters, sort, dir, total, advanced);

    let table_html = render_users_table(
        model,
        &rows,
        total,
        trimmed_query,
        current_page,
        total_pages,
        filters,
        sort,
        dir,
        advanced,
    );

    let advanced_panel = if advanced {
        let inputs = build_filter_inputs(model, true, filters);
        if inputs.is_empty() {
            String::new()
        } else {
            format!(r#"<div class="advanced-panel">{inputs}</div>"#)
        }
    } else {
        String::new()
    };

    let surface = format!(
        r#"<section class="surface">{toolbar_html}{advanced_panel}<div class="table-wrap">{table_html}</div></section>"#,
    );

    let bulk_bar = render_bulk_bar();
    let hidden_state = render_bulk_hidden_state(trimmed_query, filters, sort, dir);
    let bulk_form = format!(
        r#"<form method="post" action="" data-bulk-form>{hidden_state}{surface}{bulk_bar}</form>"#,
    );

    let banner_html = top_banner.unwrap_or("");
    let content = format!("{banner_html}{page_header}{bulk_form}");

    render_layout(topbar, sidebar_html, content, drawer)
}

/// Render the entire users table — header (with sortable links and
/// a real disabled "select-all" checkbox), rows (with real per-row
/// `<input type="checkbox" name="ids">` bulk-action checkboxes),
/// empty-state row when `rows` is empty, and the link-based
/// pagination block. **No string post-processing** — every piece
/// of HTML comes from a typed render function below.
#[allow(clippy::too_many_arguments)]
fn render_users_table(
    model: &dyn AdminUiModel,
    rows: &[HashMap<String, String>],
    total: i64,
    query: Option<&str>,
    current_page: i64,
    total_pages: i64,
    filters: &HashMap<String, String>,
    sort: Option<&str>,
    dir: Option<&str>,
    advanced: bool,
) -> String {
    let trimmed_query = query.map(str::trim).filter(|s| !s.is_empty());
    let header_html = render_users_table_header(model, trimmed_query, filters, sort, dir, advanced);
    let body_html = render_users_table_rows(model, rows, trimmed_query);
    let pagination_html = render_users_pagination(
        trimmed_query,
        current_page,
        total_pages,
        total,
        20,
        rows.len(),
        filters,
        sort,
        dir,
        advanced,
    );
    format!(r#"<table class="data-table">{header_html}{body_html}</table>{pagination_html}"#,)
}

/// Emit the `<thead>` block: disabled select-all checkbox, one
/// `<th>` per `visible_in_table` field (sortable ones wrapped in a
/// clickable `<a href>`), then a final `<th>Actions</th>` for the
/// per-row Edit / Delete cell.
#[allow(clippy::too_many_arguments)]
fn render_users_table_header(
    model: &dyn AdminUiModel,
    query: Option<&str>,
    filters: &HashMap<String, String>,
    sort: Option<&str>,
    dir: Option<&str>,
    advanced: bool,
) -> String {
    use std::fmt::Write as _;
    let mut html = String::from(r#"<thead><tr>"#);
    html.push_str(
        r#"<th class="col-checkbox"><input type="checkbox" id="check-all" class="checkbox" aria-label="Select all rows"></th>"#,
    );
    for field in model.fields() {
        if !field.visible_in_table {
            continue;
        }
        let escaped_label = html_escape(field.label);
        let is_current = sort == Some(field.name);
        let next_dir = if is_current && dir == Some("asc") {
            "desc"
        } else {
            "asc"
        };
        let arrow = if is_current {
            if dir == Some("desc") {
                r#"<span class="sort-arrow">↓</span>"#
            } else {
                r#"<span class="sort-arrow">↑</span>"#
            }
        } else {
            ""
        };
        if !field.sortable {
            let _ = write!(html, r#"<th>{escaped_label}</th>"#);
            continue;
        }
        let href = html_escape(&build_query_url(
            None,
            query,
            filters,
            Some(field.name),
            Some(next_dir),
            advanced,
        ));
        let cls = if is_current { r#" class="sorted""# } else { "" };
        let _ = write!(
            html,
            r#"<th{cls}><a href="{href}">{escaped_label}{arrow}</a></th>"#,
        );
    }
    html.push_str(r#"<th class="col-actions">Actions</th>"#);
    html.push_str("</tr></thead>");
    html
}

/// Emit the `<tbody>` block — one `<tr>` per row with a per-row
/// bulk-action checkbox, the visible cells, then a final Actions
/// cell carrying Edit (link) + Delete (inline form).
///
/// The Delete `<form>` here is HTML5-parser-collapsed inside the
/// outer bulk form — its hidden inputs (`bulk_action=delete` +
/// `ids=<row_id>`) and submit button become siblings of the bulk
/// form's other inputs. Clicking the row's Delete button submits
/// the bulk form with that single id, which the existing
/// `bulk_action=delete` handler already understands. No new
/// persistence function needed.
fn render_users_table_rows(
    model: &dyn AdminUiModel,
    rows: &[HashMap<String, String>],
    query: Option<&str>,
) -> String {
    use std::fmt::Write as _;
    if rows.is_empty() {
        let inner = match query {
            Some(q) => format!(
                r#"<tr><td colspan="100%"><div class="empty-state"><p class="empty-state-title">No matches</p><p>Nothing matches “{}”. Try a different search.</p></div></td></tr>"#,
                html_escape(q),
            ),
            None => r#"<tr><td colspan="100%"><div class="empty-state"><p class="empty-state-title">No records yet</p><p>Use the New button to add the first row.</p></div></td></tr>"#.to_string(),
        };
        return format!("<tbody>{inner}</tbody>");
    }
    let model_slug = html_escape(model.slug());
    let visible_fields: Vec<AdminUiField> = model
        .fields()
        .into_iter()
        .filter(|f| f.visible_in_table)
        .collect();
    let mut html = String::from("<tbody>");
    for r in rows {
        let id = r.get("id").map(String::as_str).unwrap_or("");
        let escaped_id = html_escape(id);
        let edit_href = format!("/admin/{model_slug}?id={escaped_id}");
        let _ = write!(
            html,
            r#"<tr data-edit-href="{edit_href}"><td class="col-checkbox"><input type="checkbox" class="checkbox" name="ids" value="{escaped_id}" aria-label="Select row {escaped_id}"></td>"#,
        );
        for (i, field) in visible_fields.iter().enumerate() {
            let value = r.get(field.name).map(String::as_str).unwrap_or("");
            let cell = match field.data_type {
                AdminDataType::Boolean => {
                    let on = matches!(value, "true" | "1" | "yes" | "on");
                    let (badge_class, badge_label) = if on {
                        ("badge badge-success", "Active")
                    } else {
                        ("badge badge-muted", "Inactive")
                    };
                    format!(r#"<td><span class="{badge_class}">{badge_label}</span></td>"#,)
                }
                _ if i == 0 => format!(r#"<td class="cell-primary">{}</td>"#, html_escape(value),),
                _ => format!(r#"<td class="cell-mono">{}</td>"#, html_escape(value)),
            };
            html.push_str(&cell);
        }
        let _ = write!(
            html,
            r#"<td class="col-actions"><span class="row-actions"><a class="btn btn-ghost btn-sm" href="{edit_href}">Edit</a><form method="post" action=""><input type="hidden" name="bulk_action" value="delete"><input type="hidden" name="ids" value="{escaped_id}"><button type="submit" class="btn btn-danger btn-sm">Delete</button></form></span></td></tr>"#,
        );
    }
    html.push_str("</tbody>");
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
    advanced: bool,
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
            advanced,
        ))
    };

    // Pagination row: range counter on the left, navigation on the
    // right. The page-size select was removed — it had no backend
    // (`?per_page=` doesn't exist), and showing a disabled control
    // reads as broken. Bring it back when paging size is wired.
    let _ = page_size; // kept in signature for future per-page support
    let mut html = String::from(r#"<div class="pagination">"#);
    let _ = write!(
        html,
        r#"<div class="pagination-meta">Showing <strong>{showing_from}</strong>–<strong>{showing_to}</strong> of <strong>{total}</strong></div><div class="pagination-controls">"#,
    );

    // Prev
    if current_page > 1 {
        let _ = write!(
            html,
            r#"<a class="pagination-btn" href="{}" aria-label="Previous page">‹</a>"#,
            make_href(current_page - 1),
        );
    } else {
        html.push_str(r#"<button type="button" class="pagination-btn" disabled aria-label="Previous page">‹</button>"#);
    }

    // Numbered pages — current is non-clickable button with aria-current.
    for p in 1..=total_pages {
        if p == current_page {
            let _ = write!(
                html,
                r#"<button type="button" class="pagination-btn" disabled aria-current="page">{p}</button>"#,
            );
        } else {
            let _ = write!(
                html,
                r#"<a class="pagination-btn" href="{}">{p}</a>"#,
                make_href(p),
            );
        }
    }

    // Next
    if current_page < total_pages {
        let _ = write!(
            html,
            r#"<a class="pagination-btn" href="{}" aria-label="Next page">›</a>"#,
            make_href(current_page + 1),
        );
    } else {
        html.push_str(r#"<button type="button" class="pagination-btn" disabled aria-label="Next page">›</button>"#);
    }

    html.push_str("</div></div>");
    html
}

/// Render the bulk-action bar. Default-hidden via `.bulk-bar` (CSS
/// `display: none`); admin.js adds `.visible` and updates the
/// `.count-pill` whenever the user toggles row checkboxes. With JS
/// disabled, the bar stays hidden — bulk actions are still reachable
/// via direct URL POSTs from any HTTP client.
fn render_bulk_bar() -> String {
    String::from(
        r#"<div class="bulk-bar" role="region" aria-label="Bulk actions">
  <div class="bulk-bar-summary"><span class="bulk-bar-count">0</span> selected</div>
  <div class="bulk-bar-actions">
    <button type="submit" name="bulk_action" value="activate" class="bulk-btn">Activate</button>
    <button type="submit" name="bulk_action" value="deactivate" class="bulk-btn">Deactivate</button>
    <button type="submit" name="bulk_action" value="delete" class="bulk-btn bulk-btn-danger">Delete</button>
  </div>
</div>"#,
    )
}

/// Render the hidden inputs that round-trip URL state through the
/// bulk-action POST. Mirrors what `build_query_url` would emit but
/// as form fields. `page` is deliberately omitted — bulk resets to
/// page 1.
fn render_bulk_hidden_state(
    query: Option<&str>,
    filters: &HashMap<String, String>,
    sort: Option<&str>,
    dir: Option<&str>,
) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    if let Some(q) = query {
        let _ = write!(
            out,
            r#"<input type="hidden" name="q" value="{}">"#,
            html_escape(q),
        );
    }
    let mut keys: Vec<&String> = filters.keys().collect();
    keys.sort();
    for k in keys {
        if let Some(v) = filters.get(k) {
            if !v.is_empty() {
                let _ = write!(
                    out,
                    r#"<input type="hidden" name="{}" value="{}">"#,
                    html_escape(k),
                    html_escape(v),
                );
            }
        }
    }
    if let Some(s) = sort {
        let _ = write!(
            out,
            r#"<input type="hidden" name="sort" value="{}">"#,
            html_escape(s),
        );
    }
    if let Some(d) = dir {
        let _ = write!(
            out,
            r#"<input type="hidden" name="dir" value="{}">"#,
            html_escape(d),
        );
    }
    out
}

/// Render the entire toolbar block — search form (with filter
/// inputs woven in as proper siblings of the search div) + the
/// "All" filter chip + keyboard hint + the "+ Add filter" stub +
/// the (HTML-`hidden`) advanced-filter block.
///
/// Class names match what `render_toolbar` in `ui.rs` would have
/// produced, so the existing `components.css` rules apply
/// unchanged. **No string post-processing** — every piece is
/// emitted directly, in the right order, by this one function.
fn render_users_toolbar(
    model: &dyn AdminUiModel,
    query: Option<&str>,
    filters: &HashMap<String, String>,
    sort: Option<&str>,
    dir: Option<&str>,
    total: i64,
    advanced: bool,
) -> String {
    use std::fmt::Write as _;

    let value = html_escape(query.unwrap_or(""));
    let placeholder = html_escape(&format!(
        "Search {}s by {}",
        model.model_name().to_lowercase(),
        model.searchable_fields().join(", "),
    ));
    let label_text = html_escape(&format!("Search {}s", model.model_name().to_lowercase()));
    let chip_count = html_escape(&total.to_string());
    let pill_filters = build_filter_pills(model, filters);
    let advanced_inputs = build_filter_inputs(model, true, filters);
    let action_url = html_escape(&format!("/admin/{}", model.slug()));
    // "All" chip = same URL with no q + no filters (sort/advanced preserved).
    let empty_filters: HashMap<String, String> = HashMap::new();
    let chip_all_href = html_escape(&build_query_url(
        None,
        None,
        &empty_filters,
        sort,
        dir,
        advanced,
    ));
    // Hidden inputs that round-trip sort/dir/advanced through the
    // toolbar's GET form so a search/filter submit doesn't clobber them.
    let mut hidden_state = String::new();
    if let Some(s) = sort {
        let _ = write!(
            hidden_state,
            r#"<input type="hidden" name="sort" value="{}">"#,
            html_escape(s)
        );
    }
    if let Some(d) = dir {
        let _ = write!(
            hidden_state,
            r#"<input type="hidden" name="dir" value="{}">"#,
            html_escape(d)
        );
    }
    if advanced {
        hidden_state.push_str(r#"<input type="hidden" name="advanced" value="1">"#);
    }

    let mut html = String::new();
    let _ = write!(
        html,
        r#"<form class="toolbar-form" role="search" method="get" action="{action_url}" aria-label="{label_text}">{hidden_state}<div class="toolbar">
  <div class="toolbar-search">
    <svg class="toolbar-search-icon" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
      <circle cx="11" cy="11" r="8"></circle>
      <line x1="21" y1="21" x2="16.65" y2="16.65"></line>
    </svg>
    <label class="sr-only" for="admin-search">{label_text}</label>
    <input id="admin-search" name="q" type="search" value="{value}" placeholder="{placeholder}" autocomplete="off">
    <kbd class="toolbar-search-kbd">/</kbd>
  </div>
  <a class="filter-pill active" href="{chip_all_href}">All <span class="filter-pill-count">{chip_count}</span></a>
  {pill_filters}"#,
    );

    if !advanced_inputs.is_empty() {
        let toggle_href = html_escape(&build_query_url(None, query, filters, sort, dir, !advanced));
        let toggle_label = if advanced {
            "Hide advanced"
        } else {
            "More filters"
        };
        let _ = write!(
            html,
            r#"<a class="filter-pill" href="{toggle_href}">{toggle_label}</a>"#,
        );
    }

    html.push_str("</div></form>");
    html
}

/// Build the toolbar pill-style filters. Each renders as a
/// `<label class="field-pill">` wrapping a transparent `<select>`
/// so changing it submits the parent toolbar form (admin.js wires
/// the change → form.submit()). Free-text filters are skipped here
/// — they only fit the Advanced panel.
fn build_filter_pills(model: &dyn AdminUiModel, filters: &HashMap<String, String>) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    for field in model.fields() {
        if !field.filterable {
            continue;
        }
        let current = filters.get(field.name).map(String::as_str).unwrap_or("");
        let has_value = !current.is_empty();
        let pill_cls = if has_value {
            "field-pill has-value"
        } else {
            "field-pill"
        };
        // Choose visible value text + select options based on filter type.
        let (display_value, options_html) = match resolve_filter_type(&field) {
            FilterType::Boolean => {
                let display = match current {
                    "true" => "Yes",
                    "false" => "No",
                    _ => "any",
                };
                let opts = format!(
                    r#"<option value="">any</option><option value="true"{sel_t}>Yes</option><option value="false"{sel_f}>No</option>"#,
                    sel_t = if current == "true" { " selected" } else { "" },
                    sel_f = if current == "false" { " selected" } else { "" },
                );
                (display.to_string(), opts)
            }
            FilterType::Select => {
                let mut display = String::from("any");
                let mut opts = String::from(r#"<option value="">any</option>"#);
                for (val, label) in &field.options {
                    let sel = if val == current { " selected" } else { "" };
                    if val == current {
                        display = label.clone();
                    }
                    let _ = write!(
                        opts,
                        r#"<option value="{}"{}>{}</option>"#,
                        html_escape(val),
                        sel,
                        html_escape(label)
                    );
                }
                (display, opts)
            }
            FilterType::Exact => {
                // Free-text filters don't fit the pill metaphor cleanly;
                // skip and let them surface in the Advanced panel.
                continue;
            }
        };
        let _ = write!(
            out,
            r#"<label class="{cls}"><span class="field-pill-key">{key}:</span><span class="field-pill-val">{val}</span><span class="field-pill-caret">▾</span><select name="{name}" aria-label="Filter by {label}">{options}</select></label>"#,
            cls = pill_cls,
            key = html_escape(field.label),
            val = html_escape(&display_value),
            name = html_escape(field.name),
            label = html_escape(field.label),
            options = options_html,
        );
    }
    out
}

/// Walk `UserAdmin::fields()` and emit the input HTML for each
/// field that matches `advanced` (`false` = default toolbar
/// filters, `true` = advanced filters). Output respects the spec's
/// HTML patterns: tri-state `<select>` for Boolean, typed
/// `<select>` for Select / FK, plain `<input type="text">` for
/// Exact. Each control's `value` is taken from `filters` so the
/// toolbar stays consistent with the URL after every submit.
fn build_filter_inputs(
    model: &dyn AdminUiModel,
    advanced: bool,
    filters: &HashMap<String, String>,
) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    for field in model.fields() {
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
// Bridge form construction (shared by GET demo + POST handler)
// ---------------------------------------------------------------

/// Build a generic admin form for any `AdminUiModel`. The
/// `doctor_id` help-text override only applies to the `users`
/// model — `override_field` no-ops on unknown field names so this
/// helper is safe for any model.
pub fn build_admin_form(model: &dyn AdminUiModel) -> FormConfig {
    FormBuilder::from_admin_ui_model(model)
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
// Built-in demo model: User
// ---------------------------------------------------------------

/// Demo `AdminUiModel` registered as `"users"`. Backs the
/// `/admin-new/users` route. The struct is unit; all metadata lives
/// in the trait impl.
pub struct UserAdmin;

/// Factory used by the registry to produce a fresh boxed model per
/// request. `UserAdmin` is zero-sized so the allocation is free.
pub fn new_user_admin() -> Box<dyn AdminUiModel> {
    Box::new(UserAdmin)
}

impl AdminUiModel for UserAdmin {
    fn slug(&self) -> &'static str {
        "users"
    }
    fn model_name(&self) -> &'static str {
        "User"
    }
    fn table_name(&self) -> &'static str {
        "admin_new_demo_users"
    }
    fn primary_key(&self) -> &'static str {
        "id"
    }
    fn searchable_fields(&self) -> Vec<&'static str> {
        vec!["username", "email", "doctor_id"]
    }
    fn primary_status_field(&self) -> Option<&'static str> {
        Some("is_active")
    }
    fn ensure_table_sql(&self) -> Option<&'static str> {
        Some(
            "CREATE TABLE IF NOT EXISTS admin_new_demo_users (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                username TEXT NOT NULL,
                email TEXT NOT NULL,
                is_active TEXT NOT NULL DEFAULT 'false',
                doctor_id TEXT,
                salary_amount TEXT
            )",
        )
    }

    fn fields(&self) -> Vec<AdminUiField> {
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
                visible_in_table: true,
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
                visible_in_table: true,
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
                visible_in_table: true,
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
                visible_in_table: true,
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
                visible_in_table: true,
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
    let mut form = build_admin_form(&UserAdmin);
    // Always emit the hidden id field (empty when there's no
    // editing target) so the POST body shape is identical between
    // CREATE and UPDATE submissions.
    form.hidden_fields.push(("id".to_string(), String::new()));

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
        save_failed: false,
        hidden_fields: Vec::new(),
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

// The sample "Auth / Content / System" sidebar was retired 2026-04-23
// — the sidebar now comes straight from the registered AdminUiModel
// registry via `render_admin_sidebar_for`. No placeholder items, no
// fake groupings. Only real models ship.
