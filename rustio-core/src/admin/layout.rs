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

// ---------------------------------------------------------------
// 0.10.0 template-based renderers (stage 4d+)
//
// These replace the string-concat renderers above for pages ported to
// `minijinja`. The old functions remain until stage 5 removes them;
// both paths coexist while the port is in progress.
// ---------------------------------------------------------------

#[derive(serde::Serialize)]
struct DesignView<'a> {
    project_name: &'a str,
    logo_initial: &'a str,
    primary_color: &'a str,
    accent_color: &'a str,
}

#[derive(serde::Serialize)]
struct UserView {
    email: String,
    display_name: String,
}

#[derive(serde::Serialize)]
struct SidebarEntryView {
    label: String,
    href: String,
    active: bool,
    visible: bool,
}

#[derive(serde::Serialize)]
struct DashboardCardView {
    label: String,
    value: i64,
}

fn design_view() -> DesignView<'static> {
    let d = crate::admin::design::Design::global();
    // `Design::global()` returns `&'static Self`, so field borrows
    // satisfy the `'static` lifetime without any allocation.
    DesignView {
        project_name: d.project_name.as_str(),
        logo_initial: d.logo_initial.as_str(),
        primary_color: d.primary_color.as_str(),
        accent_color: d.accent_color.as_str(),
    }
}

fn user_view(identity: Option<&crate::auth::Identity>) -> Option<UserView> {
    identity.map(|id| UserView {
        email: id.email.clone(),
        display_name: id.email.clone(),
    })
}

fn sidebar_from_entries(
    entries: &[DashboardEntry],
    active_slug: Option<&str>,
) -> Vec<SidebarEntryView> {
    entries
        .iter()
        .map(|e| SidebarEntryView {
            label: format!("{}s", e.model_name),
            href: format!("/admin/{}", e.slug),
            active: active_slug == Some(e.slug),
            visible: true,
        })
        .collect()
}

/// Merge sidebar sources: the "new" registry (DashboardEntry list
/// from `admin_new_registry`) first, then any legacy `AdminEntry`
/// registered via `Admin::model::<T>()` that isn't already in the
/// new registry. Dedup is by slug (`entry.admin_name`), so a model
/// registered in both surfaces keeps its new-registry entry. Core
/// entries (framework-synthetic) are omitted.
fn sidebar_merged(
    dashboard_entries: &[DashboardEntry],
    legacy_entries: &[crate::admin::AdminEntry],
    active_slug: Option<&str>,
) -> Vec<SidebarEntryView> {
    let mut merged = sidebar_from_entries(dashboard_entries, active_slug);
    let known: std::collections::HashSet<&str> = dashboard_entries.iter().map(|e| e.slug).collect();
    for entry in legacy_entries {
        if entry.core || known.contains(entry.admin_name) {
            continue;
        }
        merged.push(SidebarEntryView {
            label: entry.display_name.to_string(),
            href: format!("/admin/{}", entry.admin_name),
            active: active_slug == Some(entry.admin_name),
            visible: true,
        });
    }
    merged
}

/// Adapter that implements [`AdminUiModel`] for a legacy
/// [`crate::admin::AdminEntry`] so the template-based `list_render`
/// can serve its rows without a separate rendering path. No form /
/// mutation behaviour — legacy create / edit / delete still flow
/// through `mount_model`'s literal routes; this adapter only needs
/// to describe the table shape well enough for the list view.
pub struct LegacyEntryModel {
    entry: crate::admin::AdminEntry,
}

impl LegacyEntryModel {
    /// Clone-construct from an `AdminEntry` ref. Cheap — every field
    /// inside `AdminEntry` is either a `&'static str`, `&'static
    /// [AdminField]`, or a `bool`, so `Clone` is effectively a shallow
    /// pointer copy.
    pub fn new(entry: &crate::admin::AdminEntry) -> Self {
        Self {
            entry: entry.clone(),
        }
    }

    /// The underlying `AdminEntry`. Exposed so the form / list
    /// enrichment helpers can read the original `AdminField.relation`
    /// info (which `AdminUiField` doesn't currently carry).
    pub fn source_entry(&self) -> &crate::admin::AdminEntry {
        &self.entry
    }
}

/// Fetch `(id, display)` pairs from the table pointed at by an
/// `AdminRelation`. Used to populate a FK field's `<select>`.
///
/// The chosen display column is, in priority order:
/// 1. `relation.display_field` if set,
/// 2. otherwise the first non-id `FieldType::String` column on the
///    target entry,
/// 3. otherwise the id itself ("#123").
///
/// Cap is 500 rows — matches `RELATION_FILTER_DROPDOWN_CAP` so a
/// project with a huge target table doesn't render a form that
/// takes minutes to parse. Larger tables should move to a typeahead
/// control, which is a separate stage.
async fn fk_options(
    db: &Db,
    relation: crate::admin::AdminRelation,
    legacy_entries: &[crate::admin::AdminEntry],
) -> Vec<(String, String)> {
    use sqlx::Row as _;

    let Some(target_entry) = legacy_entries
        .iter()
        .find(|e| e.singular_name == relation.model)
    else {
        return Vec::new();
    };
    let display_col = relation
        .display_field
        .or_else(|| {
            target_entry
                .fields
                .iter()
                .filter(|f| f.name != "id" && matches!(f.ty, crate::admin::FieldType::String))
                .map(|f| f.name)
                .next()
        })
        .unwrap_or("id");

    let sql = format!(
        r#"SELECT "id", "{display}" FROM "{table}" ORDER BY "{display}" LIMIT 500"#,
        display = display_col.replace('"', "\"\""),
        table = target_entry.table.replace('"', "\"\""),
    );
    let Ok(rows) = sqlx::query(&sql).fetch_all(db.pool()).await else {
        return Vec::new();
    };
    rows.into_iter()
        .filter_map(|row| {
            let id: Option<i64> = row.try_get(0).ok();
            // Display column may be integer (when we fell back to
            // "id") or string. Try string first, then stringify the
            // integer.
            let display: Option<String> = row
                .try_get::<Option<String>, _>(1)
                .ok()
                .flatten()
                .or_else(|| {
                    row.try_get::<Option<i64>, _>(1)
                        .ok()
                        .flatten()
                        .map(|n| n.to_string())
                });
            match (id, display) {
                (Some(i), Some(d)) => Some((i.to_string(), d)),
                (Some(i), None) => Some((i.to_string(), format!("#{i}"))),
                _ => None,
            }
        })
        .collect()
}

/// Resolve `(id → label)` for one FK column across every row
/// currently on the page. One SQL `SELECT id, <display> FROM <target>
/// WHERE id IN (?, ?, …)` — so a list with 20 rows and 3 FK columns
/// costs 3 queries, not 60.
async fn fk_lookup_batch(
    db: &Db,
    target_entry: &crate::admin::AdminEntry,
    display_field: Option<&'static str>,
    ids: &[String],
) -> std::collections::HashMap<String, String> {
    use sqlx::Row as _;

    let mut out = std::collections::HashMap::new();
    if ids.is_empty() {
        return out;
    }
    let display_col = display_field
        .or_else(|| {
            target_entry
                .fields
                .iter()
                .filter(|f| f.name != "id" && matches!(f.ty, crate::admin::FieldType::String))
                .map(|f| f.name)
                .next()
        })
        .unwrap_or("id");

    let placeholders = vec!["?"; ids.len()].join(",");
    let sql = format!(
        r#"SELECT "id", "{display}" FROM "{table}" WHERE "id" IN ({placeholders})"#,
        display = display_col.replace('"', "\"\""),
        table = target_entry.table.replace('"', "\"\""),
    );
    let mut q = sqlx::query(&sql);
    for id in ids {
        q = q.bind(id);
    }
    let Ok(rows) = q.fetch_all(db.pool()).await else {
        return out;
    };
    for row in rows {
        let Ok(id) = row.try_get::<i64, _>(0) else {
            continue;
        };
        let label: Option<String> =
            row.try_get::<Option<String>, _>(1)
                .ok()
                .flatten()
                .or_else(|| {
                    row.try_get::<Option<i64>, _>(1)
                        .ok()
                        .flatten()
                        .map(|n| n.to_string())
                });
        if let Some(l) = label {
            out.insert(id.to_string(), l);
        }
    }
    out
}

/// Per-column FK resolution data used to rewrite list cells.
struct FkColumnInfo {
    column_index: usize,
    target_admin_name: String,
    id_to_label: std::collections::HashMap<String, String>,
}

/// For each visible column on the list page that points at another
/// model, batch-resolve the FK values displayed in the current page
/// of rows. One SQL query per FK column; callers render
/// `<a href="/admin/<target>/<id>">label</a>` in each matching cell.
async fn build_fk_lookups(
    db: &Db,
    source_entry: Option<&crate::admin::AdminEntry>,
    columns: &[ColumnView],
    rows_raw: &[HashMap<String, String>],
    legacy_entries: &[crate::admin::AdminEntry],
) -> Vec<FkColumnInfo> {
    let mut out = Vec::new();
    let Some(source) = source_entry else {
        return out;
    };
    for (idx, col) in columns.iter().enumerate() {
        let Some(source_field) = source.fields.iter().find(|f| f.name == col.name) else {
            continue;
        };
        let Some(relation) = source_field.relation else {
            continue;
        };
        let Some(target_entry) = legacy_entries
            .iter()
            .find(|e| e.singular_name == relation.model)
        else {
            continue;
        };
        let ids: Vec<String> = {
            let mut seen = std::collections::HashSet::new();
            let mut v = Vec::new();
            for row in rows_raw {
                if let Some(id) = row.get(&col.name) {
                    if !id.is_empty() && seen.insert(id.clone()) {
                        v.push(id.clone());
                    }
                }
            }
            v
        };
        if ids.is_empty() {
            continue;
        }
        let id_to_label = fk_lookup_batch(db, target_entry, relation.display_field, &ids).await;
        out.push(FkColumnInfo {
            column_index: idx,
            target_admin_name: target_entry.admin_name.to_string(),
            id_to_label,
        });
    }
    out
}

/// Produce `model.fields()` with FK options populated when the
/// source is a legacy `AdminEntry`. For new-engine models the
/// registration code is responsible for its own options — we pass
/// those through unchanged.
pub async fn enrich_fields_for_form(
    db: &Db,
    model: &dyn AdminUiModel,
    legacy_source: Option<&crate::admin::AdminEntry>,
    legacy_entries: &[crate::admin::AdminEntry],
) -> Vec<AdminUiField> {
    let mut fields = model.fields();
    let Some(source) = legacy_source else {
        return fields;
    };
    for field in fields.iter_mut() {
        let Some(source_field) = source.fields.iter().find(|f| f.name == field.name) else {
            continue;
        };
        let Some(relation) = source_field.relation else {
            continue;
        };
        field.is_relation = true;
        field.options = fk_options(db, relation, legacy_entries).await;
    }
    fields
}

fn admin_field_to_ui_field(field: &crate::admin::AdminField) -> AdminUiField {
    use crate::admin::FieldType;
    // FieldType is `#[non_exhaustive]`. Exhaustive matching inside
    // this crate is required — the compiler will flag a new variant
    // here if one lands without updating this mapping.
    let data_type = match field.ty {
        FieldType::String => AdminDataType::String,
        FieldType::I32 | FieldType::I64 => AdminDataType::Integer,
        FieldType::Bool => AdminDataType::Boolean,
        FieldType::DateTime => AdminDataType::DateTime,
    };
    AdminUiField {
        name: field.name,
        // Legacy `AdminField` carries no display label; synthesise
        // the column name itself. Stage 5 may capitalise / prettify
        // this, but the bare name is readable and unambiguous.
        label: field.name,
        data_type,
        required: !field.nullable,
        readonly: !field.editable,
        is_relation: field.relation.is_some(),
        options: Vec::new(),
        filterable: false,
        advanced_filter: false,
        sortable: matches!(
            data_type,
            AdminDataType::Integer
                | AdminDataType::Float
                | AdminDataType::DateTime
                | AdminDataType::String
                | AdminDataType::Email
        ),
        visible_in_table: true,
    }
}

// ---------------------------------------------------------------
// 0.10 form rendering (stage 4f-a)
//
// GET /admin/:model/new and GET /admin/:model/:id/edit both flow
// through `form_render`. POST submission, validation, and mutation
// land in stage 4f-b.
// ---------------------------------------------------------------

#[derive(serde::Serialize)]
struct FormFieldView {
    id: String,
    name: String,
    label: String,
    required: bool,
    readonly: bool,
    control: String,
    help: Option<String>,
    error: Option<String>,
}

#[derive(serde::Serialize)]
struct FormView {
    title: String,
    action: String,
    cancel_url: String,
    submit_label: String,
    error: Option<String>,
    fields: Vec<FormFieldView>,
}

fn render_field_control(field: &AdminUiField, value: &str) -> String {
    let id = format!("field_{}", field.name);
    let name = field.name;
    let val = html_escape(value);
    let readonly = if field.readonly { " readonly" } else { "" };
    let required = if field.required && !field.readonly {
        " required"
    } else {
        ""
    };

    // FK fields render as a `<select>` regardless of the underlying
    // data_type (FKs stored in `i64` columns would otherwise fall
    // into the Integer branch and show a raw number input).
    if field.is_relation && !field.options.is_empty() {
        let mut options = String::from(r#"<option value="">— choose —</option>"#);
        for (ov, ol) in &field.options {
            let sel = if ov == value { " selected" } else { "" };
            options.push_str(&format!(
                r#"<option value="{v}"{sel}>{l}</option>"#,
                v = html_escape(ov),
                l = html_escape(ol),
            ));
        }
        return format!(
            r#"<select class="form-select" id="{id}" name="{name}"{readonly}{required}>{options}</select>"#,
        );
    }
    if field.is_relation {
        // FK without options (target table missing, query failed,
        // or 0 rows) — fall back to a plain number input so the
        // form still submits. This matches the 0.9 relation-layer
        // rule: "never guess, never hide".
        return format!(
            r#"<input type="number" step="1" class="form-control" id="{id}" name="{name}" value="{val}"{readonly}{required} placeholder="id">"#,
        );
    }

    match field.data_type {
        AdminDataType::Text => format!(
            r#"<textarea class="form-control" id="{id}" name="{name}"{readonly}{required} rows="4">{val}</textarea>"#,
        ),
        AdminDataType::Email => format!(
            r#"<input type="email" class="form-control" id="{id}" name="{name}" value="{val}"{readonly}{required} autocomplete="off">"#,
        ),
        AdminDataType::Integer => format!(
            r#"<input type="number" step="1" class="form-control" id="{id}" name="{name}" value="{val}"{readonly}{required}>"#,
        ),
        AdminDataType::Float => format!(
            r#"<input type="number" step="any" class="form-control" id="{id}" name="{name}" value="{val}"{readonly}{required}>"#,
        ),
        AdminDataType::Boolean => {
            let checked = if value == "1" || value.eq_ignore_ascii_case("true") {
                " checked"
            } else {
                ""
            };
            // Hidden input keeps the field in the POST body when the
            // box is unchecked, so "unchecked" means "false" rather
            // than "omitted".
            format!(
                r#"<input type="hidden" name="{name}" value="0"><div class="form-check"><input type="checkbox" class="form-check-input" id="{id}" name="{name}" value="1"{checked}{readonly}></div>"#,
            )
        }
        AdminDataType::DateTime => format!(
            r#"<input type="datetime-local" class="form-control" id="{id}" name="{name}" value="{val}"{readonly}{required}>"#,
        ),
        AdminDataType::String => format!(
            r#"<input type="text" class="form-control" id="{id}" name="{name}" value="{val}"{readonly}{required}>"#,
        ),
    }
}

/// Render the form page for `GET /admin/:model/new` (when
/// `editing_id = None`) or `GET /admin/:model/:id/edit` (when
/// `editing_id = Some(id)`).
///
/// `legacy_source` is `Some(&entry)` when the model came from the
/// legacy `AdminEntry` path — this unlocks FK options enrichment
/// (the legacy field type doesn't carry pre-populated
/// `AdminUiField.options`). For new-engine models this is `None`
/// and their own registration code is responsible for options.
#[allow(clippy::too_many_arguments)]
pub async fn form_render(
    db: &Db,
    registry: &crate::admin::admin_form_bridge::AdminRegistry,
    legacy_entries: &[crate::admin::AdminEntry],
    model: &dyn AdminUiModel,
    legacy_source: Option<&crate::admin::AdminEntry>,
    editing_id: Option<&str>,
    identity: Option<&crate::auth::Identity>,
    csrf_token: Option<&str>,
    form_error: Option<&str>,
) -> String {
    if let Some(sql) = model.ensure_table_sql() {
        let _ = persistence::ensure_table(db, sql).await;
    }

    let dashboard_entries = collect_dashboard_entries(db, registry).await;
    let sidebar = sidebar_merged(&dashboard_entries, legacy_entries, Some(model.slug()));

    let is_edit = editing_id.is_some();
    let prefill = if let Some(id) = editing_id {
        persistence::get_record_by_id(db, model.table_name(), id)
            .await
            .unwrap_or_default()
    } else {
        HashMap::new()
    };

    let pk = model.primary_key();
    let slug = model.slug();
    let enriched = enrich_fields_for_form(db, model, legacy_source, legacy_entries).await;
    let fields: Vec<FormFieldView> = enriched
        .into_iter()
        .filter(|f| {
            // Create form skips the PK (DB auto-assigns). Edit form
            // keeps it visible + readonly so the reader sees which
            // row they're editing.
            if !is_edit && f.name == pk {
                return false;
            }
            true
        })
        .map(|mut f| {
            if f.name == pk {
                f.readonly = true;
            }
            let raw_value = prefill.get(f.name).cloned().unwrap_or_default();
            let control = render_field_control(&f, &raw_value);
            FormFieldView {
                id: format!("field_{}", f.name),
                name: f.name.to_string(),
                label: f.label.to_string(),
                required: f.required && !f.readonly,
                readonly: f.readonly,
                control,
                help: None,
                error: None,
            }
        })
        .collect();

    let (title, action, submit_label) = match editing_id {
        Some(id) => (
            format!("Edit {}", model.model_name()),
            format!("/admin/{slug}/{id}/edit"),
            "Save changes".to_string(),
        ),
        None => (
            format!("New {}", model.model_name()),
            format!("/admin/{slug}/new"),
            format!("Create {}", model.model_name()),
        ),
    };

    let form = FormView {
        title,
        action,
        cancel_url: format!("/admin/{slug}"),
        submit_label,
        error: form_error.map(str::to_string),
        fields,
    };

    let design = design_view();
    let user = user_view(identity);

    let env = crate::admin::templating::env();
    match env.get_template("admin/form.html").and_then(|tmpl| {
        tmpl.render(minijinja::context! {
            design => design,
            current_user => user,
            sidebar_entries => sidebar,
            form => form,
            page_title => format!(
                "{} · {}s",
                if is_edit { "Edit" } else { "New" },
                model.model_name()
            ),
            csrf_token => csrf_token.unwrap_or(""),
            rustio_version => env!("CARGO_PKG_VERSION"),
        })
    }) {
        Ok(html) => html,
        Err(err) => {
            eprintln!("admin form template render failed: {err}");
            form_fallback(model, editing_id)
        }
    }
}

fn form_fallback(model: &dyn AdminUiModel, editing_id: Option<&str>) -> String {
    let kind = if editing_id.is_some() { "Edit" } else { "New" };
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>{kind} {mn}</title></head><body style=\"font-family:system-ui\"><h1>{kind} {mn}</h1><p>The form template failed to render. Check the server log.</p><p><a href=\"/admin/{slug}\">Back to list</a></p></body></html>",
        mn = html_escape(model.model_name()),
        slug = html_escape(model.slug()),
    )
}

impl AdminUiModel for LegacyEntryModel {
    fn slug(&self) -> &'static str {
        self.entry.admin_name
    }
    fn model_name(&self) -> &'static str {
        self.entry.singular_name
    }
    fn table_name(&self) -> &'static str {
        self.entry.table
    }
    fn primary_key(&self) -> &'static str {
        // Convention: the first non-editable field is the PK ("id").
        // Falls back to "id" if no match.
        self.entry
            .fields
            .iter()
            .find(|f| !f.editable && f.name == "id")
            .map(|f| f.name)
            .unwrap_or("id")
    }
    fn fields(&self) -> Vec<AdminUiField> {
        self.entry
            .fields
            .iter()
            .map(admin_field_to_ui_field)
            .collect()
    }
    fn searchable_fields(&self) -> Vec<&'static str> {
        self.entry
            .fields
            .iter()
            .filter(|f| matches!(f.ty, crate::admin::FieldType::String))
            .map(|f| f.name)
            .collect()
    }
    fn primary_status_field(&self) -> Option<&'static str> {
        None
    }
    fn ensure_table_sql(&self) -> Option<&'static str> {
        None
    }
}

/// 0.10+ dashboard renderer. Collects the same registry-driven entry
/// list as the legacy `admin_dashboard_get`, but builds a typed
/// context and lets `minijinja` render `admin/dashboard.html`.
///
/// `csrf_token` is rendered as a hidden input inside the header's
/// logout form (the only state-changing form on the dashboard). If
/// the template fails to render, falls back to a minimal inline
/// shell so the server never crashes on a bad override.
pub async fn dashboard_render(
    db: &Db,
    registry: &crate::admin::admin_form_bridge::AdminRegistry,
    legacy_entries: &[crate::admin::AdminEntry],
    identity: Option<&crate::auth::Identity>,
    csrf_token: Option<&str>,
) -> String {
    let entries = collect_dashboard_entries(db, registry).await;
    let sidebar = sidebar_merged(&entries, legacy_entries, None);
    let cards: Vec<DashboardCardView> = entries
        .iter()
        .map(|e| DashboardCardView {
            label: format!("{}s", e.model_name),
            value: e.count,
        })
        .collect();
    let design = design_view();
    let user = user_view(identity);

    let env = crate::admin::templating::env();
    match env.get_template("admin/dashboard.html").and_then(|tmpl| {
        tmpl.render(minijinja::context! {
            design => design,
            current_user => user,
            sidebar_entries => sidebar,
            dashboard_cards => cards,
            page_title => "Dashboard",
            csrf_token => csrf_token.unwrap_or(""),
            rustio_version => env!("CARGO_PKG_VERSION"),
        })
    }) {
        Ok(html) => html,
        Err(err) => {
            eprintln!("admin dashboard template render failed: {err}");
            dashboard_fallback(&entries)
        }
    }
}

#[derive(serde::Serialize)]
struct ModelView {
    display_name: String,
    singular_name: String,
    new_url: String,
}

#[derive(serde::Serialize)]
struct ColumnView {
    name: String,
    label: String,
    sortable: bool,
}

#[derive(serde::Serialize)]
struct RowView {
    id: String,
    cells: Vec<String>,
    edit_url: String,
    delete_url: String,
}

#[derive(serde::Serialize)]
struct PageLinkView {
    label: String,
    href: String,
    active: bool,
    disabled: bool,
}

#[derive(serde::Serialize)]
struct PaginationView {
    pages: i64,
    current: i64,
    links: Vec<PageLinkView>,
}

#[derive(serde::Serialize)]
struct ListPermissionsView {
    view: bool,
    create: bool,
    edit: bool,
    delete: bool,
}

/// 0.10+ list-page renderer. Mirrors the data flow of the legacy
/// `admin_index_get` — same searchable / filter / sort / paginate
/// query under `fetch_users_table_state` — but renders through
/// `minijinja`. Create / edit / delete actions are hidden at this
/// stage (permissions are pinned to view-only); stage 4f will add the
/// form routes and flip them on when RBAC allows.
#[allow(clippy::too_many_arguments)]
pub async fn list_render(
    db: &Db,
    registry: &crate::admin::admin_form_bridge::AdminRegistry,
    legacy_entries: &[crate::admin::AdminEntry],
    model: &dyn AdminUiModel,
    legacy_source: Option<&crate::admin::AdminEntry>,
    query: Option<&str>,
    page: i64,
    filters: &HashMap<String, String>,
    sort: Option<&str>,
    dir: Option<&str>,
    identity: Option<&crate::auth::Identity>,
    csrf_token: Option<&str>,
) -> String {
    if let Some(sql) = model.ensure_table_sql() {
        let _ = persistence::ensure_table(db, sql).await;
    }

    let dashboard_entries = collect_dashboard_entries(db, registry).await;
    let sidebar = sidebar_merged(&dashboard_entries, legacy_entries, Some(model.slug()));

    let (rows_raw, total, current_page, total_pages, validated_sort, validated_dir) =
        fetch_users_table_state(db, model, query, filters, page, sort, dir).await;

    let fields = model.fields();
    let columns: Vec<ColumnView> = fields
        .iter()
        .filter(|f| f.visible_in_table)
        .map(|f| ColumnView {
            name: f.name.to_string(),
            label: f.label.to_string(),
            sortable: f.sortable,
        })
        .collect();

    // One batch `SELECT … WHERE id IN (…)` per FK column visible on
    // this page of rows. Cells for matching FK values are rewritten
    // into `<a href="/admin/<target>/<id>">display</a>`. Unresolved
    // ids (stale, deleted, target wiped) render as `#<id>` — never
    // the raw integer with no context.
    let fk_lookups = build_fk_lookups(db, legacy_source, &columns, &rows_raw, legacy_entries).await;

    let pk = model.primary_key();
    let slug = model.slug();
    let rows: Vec<RowView> = rows_raw
        .iter()
        .map(|row| {
            let id = row.get(pk).cloned().unwrap_or_default();
            let cells = columns
                .iter()
                .enumerate()
                .map(|(col_idx, col)| {
                    let raw = row.get(&col.name).cloned().unwrap_or_default();
                    if let Some(fk) = fk_lookups.iter().find(|f| f.column_index == col_idx) {
                        if raw.is_empty() {
                            return String::new();
                        }
                        match fk.id_to_label.get(&raw) {
                            Some(label) => format!(
                                r#"<a href="/admin/{slug}/{id}">{label}</a>"#,
                                slug = html_escape(&fk.target_admin_name),
                                id = html_escape(&raw),
                                label = html_escape(label),
                            ),
                            None => format!("#{}", html_escape(&raw)),
                        }
                    } else {
                        html_escape(&raw)
                    }
                })
                .collect();
            RowView {
                id: id.clone(),
                cells,
                edit_url: format!("/admin/{slug}/{id}/edit"),
                delete_url: format!("/admin/{slug}/{id}/delete"),
            }
        })
        .collect();

    let pagination = build_pagination_view(
        slug,
        query,
        current_page,
        total_pages,
        &validated_sort,
        &validated_dir,
    );

    let model_view = ModelView {
        display_name: format!("{}s", model.model_name()),
        singular_name: model.model_name().to_string(),
        new_url: format!("/admin/{slug}/new"),
    };

    // Stage 4f-b: full CRUD wired. Gate each action on "user is
    // signed in" for now; per-model RBAC resolution lands in a
    // follow-up once the Role is surfaced in the request context.
    let signed_in = identity.is_some();
    let permissions = ListPermissionsView {
        view: true,
        create: signed_in,
        edit: signed_in,
        delete: signed_in,
    };

    let design = design_view();
    let user = user_view(identity);

    let env = crate::admin::templating::env();
    match env.get_template("admin/list.html").and_then(|tmpl| {
        tmpl.render(minijinja::context! {
            design => design,
            current_user => user,
            sidebar_entries => sidebar,
            model => model_view,
            columns => columns,
            rows => rows,
            total => total,
            pagination => pagination,
            permissions => permissions,
            page_title => format!("{}s", model.model_name()),
            query => query.unwrap_or(""),
            csrf_token => csrf_token.unwrap_or(""),
            rustio_version => env!("CARGO_PKG_VERSION"),
        })
    }) {
        Ok(html) => html,
        Err(err) => {
            eprintln!("admin list template render failed: {err}");
            list_fallback(model, &rows_raw, &columns)
        }
    }
}

fn build_pagination_view(
    slug: &str,
    query: Option<&str>,
    current: i64,
    pages: i64,
    sort: &Option<String>,
    dir: &Option<String>,
) -> PaginationView {
    if pages <= 1 {
        return PaginationView {
            pages,
            current,
            links: Vec::new(),
        };
    }
    let q_param = query.unwrap_or("");
    let sort_param = sort.as_deref().unwrap_or("");
    let dir_param = dir.as_deref().unwrap_or("");
    let base_href = |p: i64| -> String {
        let mut parts = vec![format!("page={p}")];
        if !q_param.is_empty() {
            parts.push(format!("q={}", urlencode(q_param)));
        }
        if !sort_param.is_empty() {
            parts.push(format!("sort={sort_param}"));
        }
        if !dir_param.is_empty() {
            parts.push(format!("dir={dir_param}"));
        }
        format!("/admin/{slug}?{}", parts.join("&"))
    };

    let mut links = Vec::with_capacity(pages as usize + 2);
    links.push(PageLinkView {
        label: "‹ Prev".into(),
        href: if current > 1 {
            base_href(current - 1)
        } else {
            "#".into()
        },
        active: false,
        disabled: current <= 1,
    });
    for p in 1..=pages {
        links.push(PageLinkView {
            label: p.to_string(),
            href: base_href(p),
            active: p == current,
            disabled: false,
        });
    }
    links.push(PageLinkView {
        label: "Next ›".into(),
        href: if current < pages {
            base_href(current + 1)
        } else {
            "#".into()
        },
        active: false,
        disabled: current >= pages,
    });

    PaginationView {
        pages,
        current,
        links,
    }
}

/// Minimal percent-encoding for pagination query params. Only covers
/// the subset of ASCII that needs escaping in a URL query value —
/// enough for search strings. Not a general-purpose encoder.
fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn list_fallback(
    model: &dyn AdminUiModel,
    rows: &[HashMap<String, String>],
    columns: &[ColumnView],
) -> String {
    let mut out = format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>{} - list</title></head><body style=\"font-family:system-ui\"><h1>{}s</h1><table border=\"1\" cellpadding=\"6\"><tr>",
        html_escape(model.model_name()),
        html_escape(model.model_name()),
    );
    for c in columns {
        out.push_str(&format!("<th>{}</th>", html_escape(&c.label)));
    }
    out.push_str("</tr>");
    for row in rows {
        out.push_str("<tr>");
        for c in columns {
            let v = row.get(&c.name).cloned().unwrap_or_default();
            out.push_str(&format!("<td>{}</td>", html_escape(&v)));
        }
        out.push_str("</tr>");
    }
    out.push_str("</table></body></html>");
    out
}

#[derive(serde::Serialize)]
struct ProfileView {
    email: String,
    user_id: i64,
    role: String,
    is_active: bool,
}

/// 0.10+ renderer for `GET /admin/profile`. Builds the merged
/// sidebar (same as dashboard / list) and renders
/// `admin/profile.html`.
pub async fn profile_render(
    db: &Db,
    registry: &crate::admin::admin_form_bridge::AdminRegistry,
    legacy_entries: &[crate::admin::AdminEntry],
    identity: Option<&crate::auth::Identity>,
    user: Option<&crate::auth::User>,
    csrf_token: Option<&str>,
) -> String {
    let dashboard_entries = collect_dashboard_entries(db, registry).await;
    let sidebar = sidebar_merged(&dashboard_entries, legacy_entries, None);

    let profile = match user {
        Some(u) => ProfileView {
            email: u.email.clone(),
            user_id: u.id,
            role: u.role.clone(),
            is_active: u.is_active,
        },
        None => ProfileView {
            email: "unknown".into(),
            user_id: 0,
            role: "?".into(),
            is_active: false,
        },
    };

    let design = design_view();
    let user_v = user_view(identity);

    let env = crate::admin::templating::env();
    match env.get_template("admin/profile.html").and_then(|tmpl| {
        tmpl.render(minijinja::context! {
            design => design,
            current_user => user_v,
            sidebar_entries => sidebar,
            profile => profile,
            page_title => "Your account",
            csrf_token => csrf_token.unwrap_or(""),
            rustio_version => env!("CARGO_PKG_VERSION"),
        })
    }) {
        Ok(html) => html,
        Err(err) => {
            eprintln!("admin profile template render failed: {err}");
            format!(
                "<!doctype html><html><head><meta charset=\"utf-8\"><title>Your account</title></head><body><h1>Your account</h1><p>Email: {}</p><p><a href=\"/admin\">Back</a></p></body></html>",
                html_escape(&profile.email),
            )
        }
    }
}

#[derive(serde::Serialize)]
struct ActionRowView {
    timestamp: String,
    user_email: Option<String>,
    action_type: String,
    model_name: String,
    object_id: i64,
    object_url: Option<String>,
    summary: String,
}

#[derive(serde::Serialize)]
struct OptionView {
    value: String,
    label: String,
    selected: bool,
}

/// Render `admin/actions.html` — the project-wide audit timeline.
#[allow(clippy::too_many_arguments)]
pub async fn actions_render(
    db: &Db,
    registry: &crate::admin::admin_form_bridge::AdminRegistry,
    legacy_entries: &[crate::admin::AdminEntry],
    identity: Option<&crate::auth::Identity>,
    csrf_token: Option<&str>,
    actions: &[crate::admin::audit::AdminAction],
    model_filter: Option<&str>,
    action_filter: Option<&str>,
) -> String {
    let dashboard_entries = collect_dashboard_entries(db, registry).await;
    let sidebar = sidebar_merged(&dashboard_entries, legacy_entries, None);
    let design = design_view();
    let user_v = user_view(identity);

    let model_options: Vec<OptionView> = legacy_entries
        .iter()
        .filter(|e| !e.core)
        .map(|e| OptionView {
            value: e.admin_name.to_string(),
            label: e.display_name.to_string(),
            selected: model_filter == Some(e.admin_name),
        })
        .collect();

    let action_options: Vec<OptionView> = [
        ("", "All actions"),
        ("create", "Created"),
        ("update", "Updated"),
        ("delete", "Deleted"),
    ]
    .into_iter()
    .map(|(v, l)| OptionView {
        value: v.to_string(),
        label: l.to_string(),
        selected: match v {
            "" => action_filter.is_none(),
            other => action_filter == Some(other),
        },
    })
    .collect();

    let action_rows: Vec<ActionRowView> = actions
        .iter()
        .map(|a| {
            let object_url = legacy_entries
                .iter()
                .find(|e| e.singular_name == a.model_name || e.display_name == a.model_name)
                .map(|e| format!("/admin/{}/{}/edit", e.admin_name, a.object_id));
            ActionRowView {
                timestamp: a.timestamp.format("%Y-%m-%d %H:%M UTC").to_string(),
                user_email: a.user_email.clone(),
                action_type: a.action_type.clone(),
                model_name: a.model_name.clone(),
                object_id: a.object_id,
                object_url,
                summary: a.summary.clone(),
            }
        })
        .collect();

    let count_label = if actions.len() == 1 {
        "1 action".to_string()
    } else {
        format!("{} actions", actions.len())
    };
    let filters_active = model_filter.is_some() || action_filter.is_some();

    let env = crate::admin::templating::env();
    match env.get_template("admin/actions.html").and_then(|tmpl| {
        tmpl.render(minijinja::context! {
            design => design,
            current_user => user_v,
            sidebar_entries => sidebar,
            page_title => "Recent actions",
            csrf_token => csrf_token.unwrap_or(""),
            rustio_version => env!("CARGO_PKG_VERSION"),
            actions => action_rows,
            model_options => model_options,
            action_options => action_options,
            filters_active => filters_active,
            count_label => count_label,
        })
    }) {
        Ok(html) => html,
        Err(err) => {
            eprintln!("admin actions template render failed: {err}");
            "<!doctype html><html><body><h1>Recent actions</h1><p>Template failed.</p></body></html>".into()
        }
    }
}

/// Render `admin/password_change.html`. `error` shows as an alert
/// banner on top when the previous submit failed.
pub async fn password_change_render(
    db: &Db,
    registry: &crate::admin::admin_form_bridge::AdminRegistry,
    legacy_entries: &[crate::admin::AdminEntry],
    identity: Option<&crate::auth::Identity>,
    csrf_token: Option<&str>,
    error: Option<&str>,
) -> String {
    let dashboard_entries = collect_dashboard_entries(db, registry).await;
    let sidebar = sidebar_merged(&dashboard_entries, legacy_entries, None);
    let design = design_view();
    let user_v = user_view(identity);
    let env = crate::admin::templating::env();
    match env
        .get_template("admin/password_change.html")
        .and_then(|tmpl| {
            tmpl.render(minijinja::context! {
                design => design,
                current_user => user_v,
                sidebar_entries => sidebar,
                page_title => "Change password",
                csrf_token => csrf_token.unwrap_or(""),
                error => error,
                rustio_version => env!("CARGO_PKG_VERSION"),
            })
        }) {
        Ok(html) => html,
        Err(err) => {
            eprintln!("admin password_change template render failed: {err}");
            "<!doctype html><html><body><h1>Change password</h1><p>Template failed.</p></body></html>".into()
        }
    }
}

/// Render `admin/password_change_done.html`.
pub async fn password_change_done_render(
    db: &Db,
    registry: &crate::admin::admin_form_bridge::AdminRegistry,
    legacy_entries: &[crate::admin::AdminEntry],
    identity: Option<&crate::auth::Identity>,
    csrf_token: Option<&str>,
) -> String {
    let dashboard_entries = collect_dashboard_entries(db, registry).await;
    let sidebar = sidebar_merged(&dashboard_entries, legacy_entries, None);
    let design = design_view();
    let user_v = user_view(identity);
    let env = crate::admin::templating::env();
    match env
        .get_template("admin/password_change_done.html")
        .and_then(|tmpl| {
            tmpl.render(minijinja::context! {
                design => design,
                current_user => user_v,
                sidebar_entries => sidebar,
                page_title => "Password changed",
                csrf_token => csrf_token.unwrap_or(""),
                rustio_version => env!("CARGO_PKG_VERSION"),
            })
        }) {
        Ok(html) => html,
        Err(err) => {
            eprintln!("admin password_change_done template render failed: {err}");
            "<!doctype html><html><body><h1>Password changed</h1><p><a href=\"/admin\">Back</a></p></body></html>".into()
        }
    }
}

fn dashboard_fallback(entries: &[DashboardEntry]) -> String {
    let mut out = String::from(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>Dashboard</title></head><body style=\"font-family:system-ui\"><h1>Dashboard</h1><ul>",
    );
    for e in entries {
        out.push_str(&format!(
            "<li><a href=\"/admin/{}\">{}</a> ({})</li>",
            html_escape(e.slug),
            html_escape(e.model_name),
            e.count
        ));
    }
    out.push_str("</ul></body></html>");
    out
}
