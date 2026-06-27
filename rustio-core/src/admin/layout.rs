//! Admin page assembler.
//!
//! Every admin page is rendered by `minijinja` against the templates
//! bundled under `rustio-core/assets/templates/`. The functions here
//! build the typed context dicts the templates consume — no HTML is
//! concatenated in Rust. Bootstrap 5 CSS/JS and `admin.css`/`app.js`
//! ship from `rustio-core/assets/static/` and are served under
//! `/admin/static/…` by the core (see `admin::templating`).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::admin::admin_form_bridge::{
    resolve_filter_type, AdminDataType, AdminUiField, AdminUiModel, FilterType,
};
use crate::admin::persistence;
use crate::admin::ui::html_escape;
use crate::orm::Db;

// ---------------------------------------------------------------
// Dashboard (admin index — GET /admin)
// ---------------------------------------------------------------

/// One card + one sidebar entry per registered model.
struct DashboardEntry {
    slug: &'static str,
    model_name: &'static str,
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
            count,
        });
    }
    out
}

/// Walk legacy `AdminEntry`s, skipping framework-internal (`core`)
/// entries and any slugs already covered by the new-engine registry,
/// and return one `DashboardEntry` per remaining model. Mirrors the
/// shape of [`collect_dashboard_entries`] so both lists are
/// interchangeable downstream.
///
/// This is what makes `Admin::new().model::<T>()`-registered models
/// appear on the `/admin` dashboard, not just in the sidebar. Without
/// this walk, the cards listed only what the new `AdminUiModel`
/// registry knew about — projects scaffolded via the standard
/// `rustio new app` path were invisible on the overview.
async fn collect_legacy_dashboard_entries(
    db: &Db,
    legacy_entries: &[crate::admin::AdminEntry],
    already_listed: &std::collections::HashSet<&str>,
) -> Vec<DashboardEntry> {
    use sqlx::Row;
    let mut out = Vec::new();
    for entry in legacy_entries {
        if entry.core || already_listed.contains(entry.admin_name) {
            continue;
        }
        let count: i64 = {
            let sql = format!(
                "SELECT COUNT(*) AS c FROM \"{}\"",
                entry.table.replace('"', "\"\""),
            );
            match sqlx::query(&sql).fetch_one(db.pool()).await {
                Ok(row) => row.try_get::<i64, _>("c").unwrap_or(0),
                Err(_) => 0,
            }
        };
        out.push(DashboardEntry {
            slug: entry.admin_name,
            // `singular_name` is "Task" / "Project"; the card template
            // pluralizes via `format!("{}s", …)` for the label.
            model_name: entry.singular_name,
            count,
        });
    }
    // Sort by slug for deterministic card order — matches what
    // `collect_dashboard_entries` does for the new-engine half.
    out.sort_by_key(|e| e.slug);
    out
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
    view_filters: &[String],
    exact_match: &[String],
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
    let (eq_filters, like_filters) = classify_filters(model, filters, view_filters, exact_match);
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
    // i18n DW-1 — the view's declared filter set (`ViewSpec.filters`). A field
    // is filterable on the live list if the view declares it OR the admin model
    // marked it `filterable`/`advanced_filter` (back-compat with typed models).
    view_filters: &[String],
    // DW-1 — fields whose filter renders as a dropdown (a discrete choice), so
    // they match exactly (`=`) instead of substring (`LIKE`).
    exact_match: &[String],
) -> (HashMap<String, String>, HashMap<String, String>) {
    let fields = model.fields();
    let mut eq = HashMap::new();
    let mut like = HashMap::new();
    for (k, v) in raw {
        // Empty value = "All" / no filter — never apply `column = ''`.
        if v.is_empty() {
            continue;
        }
        let Some(field) = fields.iter().find(|f| f.name == k.as_str()) else {
            continue;
        };
        // Don't filter on a column the view didn't expose AND the admin model
        // didn't mark filterable.
        if !field.filterable && !field.advanced_filter && !view_filters.iter().any(|s| s == k) {
            continue;
        }
        let force_eq = exact_match.iter().any(|s| s == k);
        match resolve_filter_type(field) {
            FilterType::Boolean | FilterType::Select => {
                eq.insert(k.clone(), v.clone());
            }
            FilterType::Exact if force_eq => {
                eq.insert(k.clone(), v.clone());
            }
            FilterType::Exact => {
                like.insert(k.clone(), v.clone());
            }
        }
    }
    (eq, like)
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

// ---------------------------------------------------------------
// Template-based renderers (0.10.0+)
//
// Every admin page is rendered by `minijinja`. The sidebar is built
// from the registered `AdminUiModel` registry (plus any legacy
// `AdminEntry` models, merged in by `sidebar_merged`) — no
// placeholder groups.
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
    /// i18n L4b — the language-switcher options for this user, carried on
    /// `current_user` so the topbar + sidebar includes render them without
    /// every page threading a new context key. Endonyms (display) with ISO
    /// codes as values (stored); the user's saved preference is `selected`.
    language_options: Vec<LangOption>,
    /// Shell i18n — the active UI language for THIS user (preference → `"en"`),
    /// read by the templates' `t()` function. Carried on `current_user` so
    /// every page can translate its chrome without threading a new key.
    active_language: String,
    /// Text direction for the active language (`"rtl"` for Arabic/Persian/…,
    /// else `"ltr"`). Set as `dir` on `<html>` so the layout mirrors.
    text_dir: String,
}

/// One option in the language switcher (i18n L4b). `value` is the ISO code
/// (or `""` for "Default" — clears the preference); `label` is the endonym.
#[derive(serde::Serialize)]
struct LangOption {
    value: String,
    label: String,
    selected: bool,
}

#[derive(serde::Serialize)]
struct SidebarEntryView {
    label: String,
    href: String,
    active: bool,
    visible: bool,
    /// Row count for the underlying table. -1 means "no count
    /// available" (used for legacy `AdminEntry`s); the template
    /// hides the badge in that case. Otherwise the count renders
    /// in `.rio-sidebar__count` to the right of the label.
    count: i64,
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

/// Build the `current_user` view, including the i18n L4b language-switcher
/// options (the user's saved preference marked `selected`; a leading "Default"
/// option clears it). Async + db because the preference is per-user state.
async fn user_view(db: &Db, identity: Option<&crate::auth::Identity>) -> Option<UserView> {
    let id = identity?;
    // Saved preference (None when unset → "Default" is selected).
    let pref = crate::auth::user::preferred_language(db, id.user_id)
        .await
        .ok()
        .flatten();
    let mut language_options = vec![LangOption {
        value: String::new(),
        label: "Default".to_string(),
        selected: pref.is_none(),
    }];
    // Offered languages = built-in registry first (stable order + endonyms),
    // then any extra language localised via `rustio.locale.json`. So editing
    // the locale file to add a language makes it selectable here too.
    let mut codes: Vec<String> = languages().iter().map(|(c, _)| c.to_string()).collect();
    for code in crate::admin::uilang::catalog_languages() {
        if !codes.contains(&code) {
            codes.push(code);
        }
    }
    for code in &codes {
        language_options.push(LangOption {
            value: code.clone(),
            label: crate::admin::uilang::endonym(code),
            selected: pref.as_deref() == Some(code.as_str()),
        });
    }
    // Active shell language: the user's preference, else English. (Per-view
    // header labels resolve their own active language including the view's
    // default; the shell has no per-view default, so it's preference → "en".)
    let active_language = pref.clone().unwrap_or_else(|| "en".to_string());
    let text_dir = if crate::admin::uilang::is_rtl(&active_language) {
        "rtl".to_string()
    } else {
        "ltr".to_string()
    };
    Some(UserView {
        email: id.email.clone(),
        display_name: id.email.clone(),
        language_options,
        active_language,
        text_dir,
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
            count: e.count,
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
            count: -1,
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
            r#"<select class="rio-form__input" id="{id}" name="{name}"{readonly}{required}>{options}</select>"#,
        );
    }
    if field.is_relation {
        // FK without options (target table missing, query failed,
        // or 0 rows) — fall back to a plain number input so the
        // form still submits. This matches the 0.9 relation-layer
        // rule: "never guess, never hide".
        return format!(
            r#"<input type="number" step="1" class="rio-form__input" id="{id}" name="{name}" value="{val}"{readonly}{required} placeholder="id">"#,
        );
    }

    match field.data_type {
        AdminDataType::Text => format!(
            r#"<textarea class="rio-form__input rio-form__input--textarea" id="{id}" name="{name}"{readonly}{required} rows="4">{val}</textarea>"#,
        ),
        AdminDataType::Email => format!(
            r#"<input type="email" class="rio-form__input" id="{id}" name="{name}" value="{val}"{readonly}{required} autocomplete="off">"#,
        ),
        AdminDataType::Integer => format!(
            r#"<input type="number" step="1" class="rio-form__input" id="{id}" name="{name}" value="{val}"{readonly}{required}>"#,
        ),
        AdminDataType::Float => format!(
            r#"<input type="number" step="any" class="rio-form__input" id="{id}" name="{name}" value="{val}"{readonly}{required}>"#,
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
                r#"<input type="hidden" name="{name}" value="0"><input type="checkbox" class="rio-form__check" id="{id}" name="{name}" value="1"{checked}{readonly}>"#,
            )
        }
        AdminDataType::DateTime => format!(
            r#"<input type="datetime-local" class="rio-form__input" id="{id}" name="{name}" value="{val}"{readonly}{required}>"#,
        ),
        AdminDataType::String => format!(
            r#"<input type="text" class="rio-form__input" id="{id}" name="{name}" value="{val}"{readonly}{required}>"#,
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
                label: humanize_field_label(f.label),
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
    let user = user_view(db, identity).await;

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

// ---------------------------------------------------------------------------
// Composition editor — field-role editing (Phase 9a)
// ---------------------------------------------------------------------------

#[derive(serde::Serialize)]
struct RoleOptionView {
    key: String,
    label: String,
}

#[derive(serde::Serialize)]
struct EditorFieldView {
    source: String,
    label: String,
    /// The field's current role key (matched against `roles[].key` in the
    /// template to pre-select the `<select>`).
    role: String,
    /// Whether the field is currently a list filter (pre-checks the box).
    filterable: bool,
    /// `true` when the current role is Hidden — the template disables the
    /// filter checkbox (the server also enforces Hidden ⇒ not filterable).
    is_hidden: bool,
    /// Phase 9d — the anchor this field is currently merged into (its
    /// `<select>`'s selected value), or `""` for a standalone field.
    merge_into: String,
    /// i18n L3 — the explicit display label for the editing language
    /// (`label_for(source, editing_lang)`), or `""` when none is set so the
    /// input shows its placeholder (the humanised fallback, [`Self::label`]).
    label_value: String,
}

/// A field that can be a merge anchor (a standalone, non-Hidden field).
#[derive(serde::Serialize)]
struct MergeTargetView {
    source: String,
    label: String,
}

/// One value's row in the value-label editor (i18n value labels). `value` is
/// the canonical (lowercased) stored token shown read-only; `current` is its
/// explicit label for the editing language (or `""` → placeholder shows the
/// default display).
#[derive(serde::Serialize)]
struct ValueLabelRow {
    value: String,
    current: String,
    placeholder: String,
}

/// A field that exposes value labels in the editor: its discovered/authored
/// values, plus `keys` (the comma-joined canonical values) which the editor
/// submits as a hidden field so the builder knows which `(source, value)`
/// pairs were offered (FormData can't enumerate keys).
#[derive(serde::Serialize)]
struct ValueLabelField {
    source: String,
    label: String,
    keys: String,
    values: Vec<ValueLabelRow>,
}

/// Distinct non-empty stored values for a column, capped at `limit`. The
/// identifiers come from the schema/model (never user input), matching the
/// existing query builders' interpolation. Handles TEXT and INTEGER columns.
async fn distinct_values(db: &Db, table: &str, column: &str, limit: i64) -> Vec<String> {
    use sqlx::Row;
    let sql = format!(
        "SELECT DISTINCT \"{column}\" AS v FROM \"{table}\" \
         WHERE \"{column}\" IS NOT NULL AND \"{column}\" != '' LIMIT {limit}"
    );
    let Ok(rows) = sqlx::query(&sql).fetch_all(db.pool()).await else {
        return Vec::new();
    };
    rows.iter()
        .filter_map(|r| {
            r.try_get::<String, _>("v")
                .ok()
                .or_else(|| r.try_get::<i64, _>("v").ok().map(|n| n.to_string()))
        })
        .collect()
}

#[derive(serde::Serialize)]
struct EditorModelView {
    display_name: String,
    singular_name: String,
    list_url: String,
}

/// Render the composition editor page (`admin/view_editor.html`): a
/// field/role table where each field's role can be changed, plus a Save
/// POST form. Reads the current roles from the resolved ViewSpec (saved or
/// derived). `error` re-renders the page with a banner after a rejected
/// save (the editor is shown with the model's *current* roles — nothing
/// changed). Data-only context; no HTML is built in Rust.
#[allow(clippy::too_many_arguments)]
pub async fn view_editor_render(
    base: &Path,
    db: &Db,
    registry: &crate::admin::admin_form_bridge::AdminRegistry,
    legacy_entries: &[crate::admin::AdminEntry],
    model: &dyn AdminUiModel,
    identity: Option<&crate::auth::Identity>,
    csrf_token: Option<&str>,
    return_to: &str,
    error: Option<&str>,
    // i18n L3 — the language whose labels are being edited (the `?lang=` in
    // effect). Empty ⇒ use the view's stored default_language. This is purely
    // the EDITING language; it changes via a GET reload, never on save, so a
    // language switch can't clobber another language's label.
    editing_lang: &str,
) -> String {
    let dashboard_entries = collect_dashboard_entries(db, registry).await;
    let sidebar = sidebar_merged(&dashboard_entries, legacy_entries, Some(model.slug()));

    let ui_fields = model.fields();
    let schema_model = schema_model_from_ui(model.model_name(), &ui_fields);
    let spec = resolve_view(base, model.model_name(), &schema_model);
    // Derived defaults supply a role for any merged-in member (its standalone
    // role isn't stored while merged — unmerging restores the derived role).
    let derived = crate::viewspec::ViewSpec::from_schema_model(&schema_model);

    let label_of = |source: &str| -> String {
        ui_fields
            .iter()
            .find(|f| f.name == source)
            .map(|f| humanize_field_label(f.label))
            .unwrap_or_else(|| humanize_field_label(source))
    };
    let derived_role = |source: &str| -> crate::viewspec::FieldRole {
        derived
            .fields
            .iter()
            .find(|f| f.source == source)
            .map(|f| f.role)
            .unwrap_or(crate::viewspec::FieldRole::Meta)
    };

    // The editing language: the `?lang=` in effect, or the view's stored
    // default when none was requested. Labels are pre-filled for THIS language
    // only; switching it is a GET reload (no save), so no cross-language clobber.
    let editing_lang = if editing_lang.trim().is_empty() {
        spec.default_language.clone()
    } else {
        editing_lang.trim().to_string()
    };
    // The label for the editing language EXACTLY — a strict lookup with NO
    // fallback to default_language (unlike `label_for`/`label`, which are for
    // rendering). If the editing language has no label the input is empty, so
    // switching languages can never show — and therefore never save — another
    // language's text. "" lets the placeholder show the humanised hint.
    let label_value = |source: &str| -> String {
        spec.labels
            .get(source)
            .and_then(|by_lang| by_lang.get(&editing_lang))
            .cloned()
            .unwrap_or_default()
    };

    // Reconstruct the FULL field universe as editor rows: each standalone
    // field, with its merged-in members listed right after (so they can be
    // un-merged). Members show their derived role + their anchor.
    let mut fields: Vec<EditorFieldView> = Vec::new();
    for fs in &spec.fields {
        fields.push(EditorFieldView {
            source: fs.source.clone(),
            label: label_of(&fs.source),
            role: field_role_key(fs.role).to_string(),
            filterable: fs.filterable,
            is_hidden: fs.role == crate::viewspec::FieldRole::Hidden,
            merge_into: String::new(),
            label_value: label_value(&fs.source),
        });
        if let Some(m) = &fs.merge {
            for member in m.iter().filter(|s| *s != &fs.source) {
                let role = derived_role(member);
                fields.push(EditorFieldView {
                    source: member.clone(),
                    label: label_of(member),
                    role: field_role_key(role).to_string(),
                    filterable: false,
                    is_hidden: role == crate::viewspec::FieldRole::Hidden,
                    merge_into: fs.source.clone(),
                    label_value: label_value(member),
                });
            }
        }
    }

    // Editing-language options: a small fixed set, unioned with the editing
    // language and the view's stored default so neither is ever lost. ISO
    // codes for L3 (endonym display names are the L4 registry).
    let language_options: Vec<String> = {
        let mut set: std::collections::BTreeSet<String> =
            ["en", "sv"].iter().map(|s| s.to_string()).collect();
        set.insert(editing_lang.clone());
        set.insert(spec.default_language.clone());
        set.into_iter().collect()
    };

    // Merge targets = standalone, non-Hidden fields (potential anchors).
    let merge_targets: Vec<MergeTargetView> = spec
        .fields
        .iter()
        .filter(|f| f.role != crate::viewspec::FieldRole::Hidden)
        .map(|f| MergeTargetView {
            source: f.source.clone(),
            label: label_of(&f.source),
        })
        .collect();

    // i18n value labels — for each field, the values to label = DISTINCT
    // stored values ∪ any value already labelled (so hand-authored labels for
    // any field stay editable). Discovered for status-shaped fields, and for
    // non-status STRING fields that are low-cardinality (enum-like). Pre-filled
    // with the STRICT explicit label for the editing language (no default
    // fallback, mirroring the field-label fix), so a switch can't leak/clobber.
    const VALUE_CAP: i64 = 50; // display cap for status fields
    const ENUM_CAP: i64 = 12; // a non-status String field with <= this many distinct values is enum-like
    let table = model.table_name();
    // A non-status field is an enum candidate when it's a `String` column (FK
    // ids are integers → excluded by type) whose role renders a translatable
    // plain cell — so the Title (rendered as the primary cell, not the plain
    // branch) and Hidden (never rendered) fields are excluded.
    let field_ty = |source: &str| -> Option<&str> {
        schema_model
            .fields
            .iter()
            .find(|f| f.name == source)
            .map(|f| f.ty.as_str())
    };
    let mut value_label_fields: Vec<ValueLabelField> = Vec::new();
    for fs in &spec.fields {
        let mut values: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        if is_status_field_name(&fs.source) {
            let raws = distinct_values(db, table, &fs.source, VALUE_CAP + 1).await;
            if raws.len() as i64 > VALUE_CAP {
                eprintln!(
                    "admin value-labels: `{}` has >{} distinct values; capping the editor list",
                    fs.source, VALUE_CAP
                );
            }
            for raw in raws.into_iter().take(VALUE_CAP as usize) {
                let (data_value, _) = normalize_status_pill(&raw);
                if !data_value.is_empty() {
                    values.insert(data_value);
                }
            }
        } else if !matches!(
            fs.role,
            crate::viewspec::FieldRole::Title | crate::viewspec::FieldRole::Hidden
        ) && field_ty(&fs.source) == Some("String")
        {
            // Non-status enum candidate: offer ONLY if low-cardinality. The
            // LIMIT ENUM_CAP+1 query early-stops; >ENUM_CAP distinct values
            // means free-text / identity, not an enum → skip. Keys are the
            // lowercased stored value, matching the plain-cell render lookup.
            let raws = distinct_values(db, table, &fs.source, ENUM_CAP + 1).await;
            if (raws.len() as i64) <= ENUM_CAP {
                for raw in raws {
                    let key = raw.trim().to_lowercase();
                    if !key.is_empty() {
                        values.insert(key);
                    }
                }
            }
        }
        if let Some(by_value) = spec.value_labels.get(&fs.source) {
            values.extend(by_value.keys().cloned());
        }
        if values.is_empty() {
            continue;
        }
        let is_status = is_status_field_name(&fs.source);
        let rows: Vec<ValueLabelRow> = values
            .iter()
            .map(|v| {
                let current = spec
                    .value_labels
                    .get(&fs.source)
                    .and_then(|bv| bv.get(v))
                    .and_then(|bl| bl.get(&editing_lang))
                    .cloned()
                    .unwrap_or_default();
                let placeholder = if is_status {
                    normalize_status_pill(v).1
                } else {
                    v.clone()
                };
                ValueLabelRow {
                    value: v.clone(),
                    current,
                    placeholder,
                }
            })
            .collect();
        value_label_fields.push(ValueLabelField {
            source: fs.source.clone(),
            label: label_of(&fs.source),
            keys: values.iter().cloned().collect::<Vec<_>>().join(","),
            values: rows,
        });
    }

    let slug = model.slug();
    let model_view = EditorModelView {
        display_name: format!("{}s", model.model_name()),
        singular_name: model.model_name().to_string(),
        list_url: format!("/admin/{slug}"),
    };
    let design = design_view();
    let user = user_view(db, identity).await;

    let env = crate::admin::templating::env();
    match env.get_template("admin/view_editor.html").and_then(|tmpl| {
        tmpl.render(minijinja::context! {
            design => design,
            current_user => user,
            sidebar_entries => sidebar,
            model => model_view,
            fields => fields,
            roles => role_options(),
            merge_targets => merge_targets,
            editing_lang => editing_lang,
            view_default_language => spec.default_language,
            language_options => language_options,
            value_label_fields => value_label_fields,
            save_action => format!("/admin/{slug}/view"),
            return_to => return_to,
            error => error,
            page_title => format!("Edit view · {}s", model.model_name()),
            csrf_token => csrf_token.unwrap_or(""),
            rustio_version => env!("CARGO_PKG_VERSION"),
        })
    }) {
        Ok(html) => html,
        Err(err) => {
            eprintln!("admin view-editor template render failed: {err}");
            format!(
                "<!doctype html><html><head><meta charset=\"utf-8\"><title>Edit view</title></head><body style=\"font-family:system-ui\"><h1>Edit view — {mn}</h1><p>The view editor failed to render. Check the server log.</p><p><a href=\"/admin/{slug}\">Back to list</a></p></body></html>",
                mn = html_escape(model.model_name()),
                slug = html_escape(slug),
            )
        }
    }
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

/// 0.10+ dashboard renderer. Collects the registry-driven entry list,
/// builds a typed context, and lets `minijinja` render
/// `admin/dashboard.html`.
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
    // Cards come from two sources, in priority order:
    //   1. the new `AdminUiModel` registry (one entry per registered slug)
    //   2. the legacy `Admin::new().model::<T>()` path, filtered to
    //      non-`core` entries not already covered by source 1
    // Same dedup rule as `sidebar_merged` keeps a model registered
    // through both paths from appearing twice. Before this dual-source
    // build, the dashboard cards only reflected source 1 — every
    // `rustio new app`-scaffolded model was invisible at /admin.
    let new_entries = collect_dashboard_entries(db, registry).await;
    let known: std::collections::HashSet<&str> = new_entries.iter().map(|e| e.slug).collect();
    let legacy_dash = collect_legacy_dashboard_entries(db, legacy_entries, &known).await;
    let all_entries: Vec<&DashboardEntry> = new_entries.iter().chain(legacy_dash.iter()).collect();

    let sidebar = sidebar_merged(&new_entries, legacy_entries, None);
    let cards: Vec<DashboardCardView> = all_entries
        .iter()
        .map(|e| DashboardCardView {
            label: format!("{}s", e.model_name),
            value: e.count,
        })
        .collect();
    let design = design_view();
    let user = user_view(db, identity).await;

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
            // Fallback path also gets the combined list so a template
            // failure doesn't silently regress the bug we just fixed.
            let combined: Vec<DashboardEntry> =
                new_entries.into_iter().chain(legacy_dash).collect();
            dashboard_fallback(&combined)
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
    /// Phase 9d — when this column is a merged cell, the full list of merge
    /// sources (anchor first). Empty for a normal single-source column. The
    /// row renderer joins these sources' values with " · ".
    #[serde(skip)]
    merge: Vec<String>,
}

/// DW-1 — one filter control in the list toolbar, built from a `ViewSpec.filters`
/// entry. `kind` is `"boolean" | "select" | "exact"`; `current` is the value in
/// effect (from the query). For boolean/select, `options` carry the choices.
#[derive(serde::Serialize)]
struct FilterControlView {
    source: String,
    label: String,
    kind: String,
    options: Vec<FilterOptionView>,
    current: String,
}

/// One option in a select/boolean filter. `value` is the English stored token
/// submitted to the query; `label` is its display (translated via value labels
/// where available). DW-1.
#[derive(serde::Serialize)]
struct FilterOptionView {
    value: String,
    label: String,
    selected: bool,
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
    per_page: i64,
    total: i64,
    from: i64,
    to: i64,
    links: Vec<PageLinkView>,
}

#[derive(serde::Serialize)]
struct ListPermissionsView {
    view: bool,
    create: bool,
    edit: bool,
    delete: bool,
}

// ---------------------------------------------------------------------------
// ViewSpec-driven column selection (Phase 6)
//
// The list page chooses its columns through the model's ViewSpec
// (`crate::viewspec`), resolved + rendered by the deterministic Phase-3
// renderer. These helpers turn the live `AdminUiField` metadata into a
// ViewSpec, resolve a saved-or-derived view, and map the renderer's
// Table-layout cell selection back to `ColumnView`s. They are called
// directly by `list_render` — the live admin path.
// ---------------------------------------------------------------------------

/// `CamelCase` model name → `snake_case` file stem (`Booking` →
/// `booking`). Matches the `rustio view` CLI so the admin and CLI resolve
/// the same `<model>.view.json`.
fn view_snake_case(name: &str) -> String {
    let mut out = String::with_capacity(name.len() + 4);
    for (i, ch) in name.chars().enumerate() {
        if ch.is_ascii_uppercase() {
            if i != 0 {
                out.push('_');
            }
            out.push(ch.to_ascii_lowercase());
        } else {
            out.push(ch);
        }
    }
    out
}

/// Map a UI data type to the schema type vocabulary `from_schema_model`
/// classifies on. Float collapses to a numeric type (the schema vocabulary
/// has no float); Email/Text are plain strings.
fn ui_data_type_to_schema(dt: AdminDataType) -> &'static str {
    match dt {
        AdminDataType::Integer => "i64",
        AdminDataType::Float => "i64",
        AdminDataType::Boolean => "bool",
        AdminDataType::DateTime => "DateTime",
        AdminDataType::String | AdminDataType::Text | AdminDataType::Email => "String",
    }
}

/// Build a `schema::SchemaModel` from the live `AdminUiField` list so a
/// default ViewSpec can be derived. Only `name` + `ty` matter to
/// `from_schema_model`; the other schema fields are placeholders.
pub(crate) fn schema_model_from_ui(
    model_name: &str,
    fields: &[AdminUiField],
) -> crate::schema::SchemaModel {
    use crate::schema::{SchemaField, SchemaModel};
    let schema_fields = fields
        .iter()
        .map(|f| SchemaField {
            name: f.name.to_string(),
            ty: ui_data_type_to_schema(f.data_type).to_string(),
            nullable: !f.required,
            editable: !f.readonly,
            relation: None,
        })
        .collect();
    SchemaModel {
        name: model_name.to_string(),
        table: String::new(),
        admin_name: String::new(),
        display_name: String::new(),
        singular_name: model_name.to_string(),
        fields: schema_fields,
        relations: Vec::new(),
        core: false,
    }
}

/// The `<model_snake>.view.json` path under `base`. **The single place the
/// filename is computed**, shared by the reader and the writer so they can
/// never diverge (constraint 3). `base` is the directory anchor — the cwd
/// (`Path::new(".")`) in production, a temp dir in tests.
fn view_file_path(base: &Path, model_name: &str) -> PathBuf {
    base.join(format!("{}.view.json", view_snake_case(model_name)))
}

/// Load + parse the saved ViewSpec for a model, or `None` when the file is
/// absent **or** invalid (a missing/broken file is never an error). Shared
/// by the list-page resolver, the layout-precedence reader, and the
/// writer's load-existing step.
fn load_saved_view(base: &Path, model_name: &str) -> Option<crate::viewspec::ViewSpec> {
    let raw = std::fs::read_to_string(view_file_path(base, model_name)).ok()?;
    crate::viewspec::ViewSpec::parse(&raw).ok()
}

/// Parse a `?layout=` (or form) value into a [`ViewLayout`](crate::viewspec::ViewLayout),
/// returning `None` for anything that isn't one of the four exact keys.
/// Used both for the request param and for validating the POSTed layout.
pub(crate) fn parse_layout_strict(raw: Option<&str>) -> Option<crate::viewspec::ViewLayout> {
    use crate::viewspec::ViewLayout;
    match raw {
        Some("table") => Some(ViewLayout::Table),
        Some("list") => Some(ViewLayout::List),
        Some("cards") => Some(ViewLayout::Cards),
        Some("compact") => Some(ViewLayout::Compact),
        _ => None,
    }
}

/// Parse a field-role value (the `<select>` option / `role[...]` form
/// value) into a [`FieldRole`](crate::viewspec::FieldRole). Returns `None`
/// for anything that isn't one of the six exact serialized keys. Mirrors
/// `FieldRole`'s `#[serde(rename_all = "snake_case")]` naming.
pub(crate) fn parse_role_strict(s: &str) -> Option<crate::viewspec::FieldRole> {
    use crate::viewspec::FieldRole;
    match s {
        "title" => Some(FieldRole::Title),
        "subtitle" => Some(FieldRole::Subtitle),
        "badge" => Some(FieldRole::Badge),
        "timestamp" => Some(FieldRole::Timestamp),
        "meta" => Some(FieldRole::Meta),
        "hidden" => Some(FieldRole::Hidden),
        _ => None,
    }
}

/// Stable serialized key for a field role (for the current-selection in the
/// editor). In-crate exhaustive — a new variant must be mapped here.
fn field_role_key(role: crate::viewspec::FieldRole) -> &'static str {
    use crate::viewspec::FieldRole;
    match role {
        FieldRole::Title => "title",
        FieldRole::Subtitle => "subtitle",
        FieldRole::Badge => "badge",
        FieldRole::Timestamp => "timestamp",
        FieldRole::Meta => "meta",
        FieldRole::Hidden => "hidden",
    }
}

/// The six role options for the editor's `<select>`, in display order.
fn role_options() -> Vec<RoleOptionView> {
    [
        ("title", "Title"),
        ("subtitle", "Subtitle"),
        ("badge", "Badge"),
        ("timestamp", "Timestamp"),
        ("meta", "Meta"),
        ("hidden", "Hidden"),
    ]
    .iter()
    .map(|(key, label)| RoleOptionView {
        key: key.to_string(),
        label: label.to_string(),
    })
    .collect()
}

/// Build a candidate ViewSpec from `spec` by applying the submitted field
/// **roles** (9a) and **order** (9b). The handler drives off `spec.fields`
/// (the authority), so the field **set** is preserved exactly — every field
/// appears once, **never dropped, duplicated, or invented** — and each
/// field's `merge`/`filterable`, plus the spec's `filters`/`version`/
/// `model`/`layout`, are preserved. Only roles and sequence change.
///
/// - **Roles:** `role[<source>]`. A present value MUST parse to one of the
///   six roles, else this returns `Err(message)` so the handler re-renders
///   the editor with the error and **writes nothing** (a bad submission
///   never silently falls back to the existing role). Omitted → keep
///   current role.
/// - **Order:** `order[<source>]=<index>`. Fields are **stable-sorted by
///   `(index, original_position)`**. The original-position tiebreaker makes
///   this a **total, deterministic permutation even under duplicate,
///   missing, or tampered indices** — and untouched/original indices (the
///   no-JS / no-change path) sort to **identity**. A missing/garbage index
///   falls back to the field's original position.
///
/// - **Filters (9c):** gated on a `filters_submitted` sentinel the editor
///   always sends. When present, each field's `filterable` is DERIVED from
///   its `filterable[<source>]` checkbox **and** `role != Hidden` (a Hidden
///   field is never filterable — the server wins even if the box was
///   checked), and the spec's `filters` list is rebuilt as the
///   filterable sources in display order. When the sentinel is **absent**
///   (older / role-or-order-only submits), `filterable` and `filters` are
///   preserved unchanged. Because `filters` ⊆ filterable sources by
///   construction, [`ViewSpec::validate`](crate::viewspec::ViewSpec::validate)'s
///   filter rule passes — and still runs as the load-bearing guard.
///
/// 9d (merge) is gated on a separate `merge_submitted` sentinel: when
/// present, [`build_edited_spec_with_merge`] takes over (it reconstructs the
/// full field universe so merges can be expanded/collapsed); when absent,
/// the role/order/filter path below runs unchanged.
///
/// i18n L3 (display labels) is applied last, as a post-step over the settled
/// candidate ([`apply_label_edits`]) — keyed by the candidate's real field
/// sources — so it composes with all of the above in a single Save.
pub(crate) fn build_edited_spec(
    spec: &crate::viewspec::ViewSpec,
    form: &crate::http::FormData,
) -> Result<crate::viewspec::ViewSpec, String> {
    let mut candidate = if form.get("merge_submitted").is_some() {
        build_edited_spec_with_merge(spec, form)?
    } else {
        build_edited_spec_no_merge(spec, form)?
    };
    apply_label_edits(&mut candidate, form);
    apply_value_label_edits(&mut candidate, form);
    Ok(candidate)
}

/// i18n L3 — apply display-label edits to a settled candidate, keyed by its
/// **real field sources** (the authority; the source itself is never editable,
/// and a label for an unknown source is ignored — [`ViewSpec::validate`]'s
/// `UnknownLabelSource` backstops anyway).
///
/// - **Editing language:** `editing_lang` (the `?lang=` the editor was rendered
///   with, carried as a hidden field); falls back to the view's stored default.
///   Switching it is a GET reload, never a save, so a switch can't clobber
///   another language's label — what the inputs show is exactly what saves.
/// - **Stored default:** changed only by the explicit `set_as_default` control
///   (kept separate from the editing-language switch, by design).
/// - **Labels:** gated on the `labels_submitted` sentinel. For each real
///   source, a non-empty `label[<source>]` is written to
///   `labels[source][editing_lang]`; an **empty/absent** input **removes** that
///   entry (never stores `""` — so clearing falls back to the humaniser and a
///   label-less spec stays byte-identical to pre-i18n). Labels for *other*
///   languages are preserved.
/// - **Prune:** labels whose source is no longer a field (e.g. a member merged
///   away in the same Save) are dropped, so `labels ⊆ fields` holds and
///   `validate` passes. Unmerging restores the humaniser fallback (consistent
///   with the 9d "merged-in member loses its role" behaviour).
fn apply_label_edits(candidate: &mut crate::viewspec::ViewSpec, form: &crate::http::FormData) {
    let editing_lang = form
        .get("editing_lang")
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .unwrap_or_else(|| candidate.default_language.clone());

    // Separate, explicit control: make the editing language the view's stored
    // default. Never inferred from the editing-language switch.
    if form.get("set_as_default").is_some() && !editing_lang.is_empty() {
        candidate.default_language = editing_lang.clone();
    }

    if form.get("labels_submitted").is_some() {
        let sources: Vec<String> = candidate.fields.iter().map(|f| f.source.clone()).collect();
        for src in &sources {
            match form
                .get(&format!("label[{src}]"))
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                Some(value) => {
                    candidate
                        .labels
                        .entry(src.clone())
                        .or_default()
                        .insert(editing_lang.clone(), value.to_string());
                }
                None => {
                    if let Some(by_lang) = candidate.labels.get_mut(src) {
                        by_lang.remove(&editing_lang);
                        if by_lang.is_empty() {
                            candidate.labels.remove(src);
                        }
                    }
                }
            }
        }
    }

    // Prune any label whose source is no longer a field (merge-away, etc.).
    let live: std::collections::BTreeSet<&str> =
        candidate.fields.iter().map(|f| f.source.as_str()).collect();
    candidate
        .labels
        .retain(|src, _| live.contains(src.as_str()));
}

/// i18n value labels — apply per-value display-label edits to the settled
/// candidate (the value-level analog of [`apply_label_edits`]). Gated on the
/// `value_labels_submitted` sentinel. For each real field source the editor
/// submits a hidden `value_keys[<source>]` (the canonical values it offered,
/// comma-joined, since FormData can't enumerate keys); for each value, a
/// non-empty `value_label[<source>][<value>]` is written to
/// `value_labels[source][value][editing_lang]`, and an **empty/absent** input
/// **removes** that entry (never stores `""`; empty value-/source-maps are
/// pruned). The value KEY is never editable — it's the English stored token.
/// Labels whose source is no longer a field are pruned so `validate` passes.
fn apply_value_label_edits(
    candidate: &mut crate::viewspec::ViewSpec,
    form: &crate::http::FormData,
) {
    if form.get("value_labels_submitted").is_some() {
        let editing_lang = form
            .get("editing_lang")
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from)
            .unwrap_or_else(|| candidate.default_language.clone());

        let sources: Vec<String> = candidate.fields.iter().map(|f| f.source.clone()).collect();
        for src in &sources {
            let Some(keys) = form.get(&format!("value_keys[{src}]")) else {
                continue;
            };
            for val in keys.split(',').map(str::trim).filter(|s| !s.is_empty()) {
                match form
                    .get(&format!("value_label[{src}][{val}]"))
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                {
                    Some(label) => {
                        candidate
                            .value_labels
                            .entry(src.clone())
                            .or_default()
                            .entry(val.to_string())
                            .or_default()
                            .insert(editing_lang.clone(), label.to_string());
                    }
                    None => {
                        if let Some(by_value) = candidate.value_labels.get_mut(src) {
                            if let Some(by_lang) = by_value.get_mut(val) {
                                by_lang.remove(&editing_lang);
                                if by_lang.is_empty() {
                                    by_value.remove(val);
                                }
                            }
                            if by_value.is_empty() {
                                candidate.value_labels.remove(src);
                            }
                        }
                    }
                }
            }
        }
    }

    // Prune value labels for sources that are no longer fields (merge-away).
    let live: std::collections::BTreeSet<&str> =
        candidate.fields.iter().map(|f| f.source.as_str()).collect();
    candidate
        .value_labels
        .retain(|src, _| live.contains(src.as_str()));
}

/// Roles (9a) + order (9b) + filters (9c), with merges **preserved** as-is.
/// This is the path for non-merge-editing submits; merges are only touched
/// when the `merge_submitted` sentinel is present (see [`build_edited_spec`]).
fn build_edited_spec_no_merge(
    spec: &crate::viewspec::ViewSpec,
    form: &crate::http::FormData,
) -> Result<crate::viewspec::ViewSpec, String> {
    use crate::viewspec::FieldRole;
    // The real editor always sends this; its presence means "filters were
    // edited — derive them", its absence means "leave filters untouched".
    let filters_submitted = form.get("filters_submitted").is_some();

    // (original_position, order_index, field) — one entry per existing field.
    let mut entries: Vec<(usize, i64, crate::viewspec::FieldSpec)> =
        Vec::with_capacity(spec.fields.len());
    for (pos, f) in spec.fields.iter().enumerate() {
        let role = match form.get(&format!("role[{}]", f.source)) {
            Some(value) => parse_role_strict(value).ok_or_else(|| {
                format!(
                    "Unknown role \u{201c}{value}\u{201d} for field \u{201c}{}\u{201d}.",
                    f.source
                )
            })?,
            None => f.role,
        };
        // Submitted order index; missing/garbage → keep original position.
        let order = form
            .get(&format!("order[{}]", f.source))
            .and_then(|s| s.trim().parse::<i64>().ok())
            .unwrap_or(pos as i64);
        // Role is decided above, so the Hidden ⇒ not-filterable rule is
        // applied against the *new* role within the same submit.
        let filterable = if filters_submitted {
            form.get(&format!("filterable[{}]", f.source)).is_some() && role != FieldRole::Hidden
        } else {
            f.filterable
        };
        entries.push((
            pos,
            order,
            crate::viewspec::FieldSpec {
                source: f.source.clone(),
                role,
                merge: f.merge.clone(),
                filterable,
            },
        ));
    }
    // Stable, deterministic permutation: by submitted index, then by
    // original position to break ties (so duplicate/missing indices can
    // never drop a field or reorder nondeterministically).
    entries.sort_by(|a, b| a.1.cmp(&b.1).then(a.0.cmp(&b.0)));

    let mut candidate = spec.clone();
    candidate.fields = entries.into_iter().map(|(_, _, fs)| fs).collect();
    // Derive `filters` from the (now display-ordered) filterable fields, so
    // `filters` and `filterable` can never disagree. Only when filters were
    // submitted; otherwise the cloned spec's filters are kept as-is.
    if filters_submitted {
        candidate.filters = candidate
            .fields
            .iter()
            .filter(|f| f.filterable)
            .map(|f| f.source.clone())
            .collect();
    }
    Ok(candidate)
}

/// Roles + order + filters + **merge** (9d). Reconstructs the full field
/// universe — standalone fields plus members currently collapsed inside
/// anchors' `merge` vecs — so the editor can expand (unmerge) any field.
///
/// Merge model (collapsed, renderer-correct): a merged cell is one anchor
/// `FieldSpec` whose `merge` vec lists `[anchor, members…]`; the non-anchor
/// members are **removed** from `fields` (the renderer would otherwise emit
/// them twice). Server-side enforcements:
/// - the merge target must be a standalone field (its own `merge[...]`
///   empty) — kills chains/self-loops;
/// - one source → one target (a single `<select>`) ⇒ **no overlap**;
/// - **Hidden** sources are dropped from any merge (the Hidden value never
///   enters the join), and a Hidden anchor forms no group;
/// - a group below 2 members after drops is dropped entirely (all
///   standalone). [`ViewSpec::validate`](crate::viewspec::ViewSpec::validate)
///   backstops `MergeTooShort`.
///
/// Round-trip note: a member's role/order/filterable aren't stored while
/// merged, so **unmerging restores it at its derived-default role** (the
/// editor pre-fills that; here a member reconstructed without a submitted
/// role falls back to `Meta`).
fn build_edited_spec_with_merge(
    spec: &crate::viewspec::ViewSpec,
    form: &crate::http::FormData,
) -> Result<crate::viewspec::ViewSpec, String> {
    use crate::viewspec::{FieldRole, FieldSpec};
    use std::collections::{BTreeMap, BTreeSet, HashMap};

    let filters_submitted = form.get("filters_submitted").is_some();

    // --- Field universe: standalone sources, each anchor's members after it.
    struct Uni {
        source: String,
        role_fallback: FieldRole,
        filterable_fallback: bool,
    }
    let mut uni: Vec<Uni> = Vec::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    for f in &spec.fields {
        uni.push(Uni {
            source: f.source.clone(),
            role_fallback: f.role,
            filterable_fallback: f.filterable,
        });
        seen.insert(f.source.clone());
        if let Some(m) = &f.merge {
            for member in m.iter().filter(|s| *s != &f.source) {
                if seen.insert(member.clone()) {
                    uni.push(Uni {
                        source: member.clone(),
                        role_fallback: FieldRole::Meta,
                        filterable_fallback: false,
                    });
                }
            }
        }
    }

    // --- Per-source: role, order, filterable, merge target.
    struct Built {
        source: String,
        role: FieldRole,
        order: i64,
        filterable: bool,
        merge_into: Option<String>,
    }
    let mut built: Vec<Built> = Vec::with_capacity(uni.len());
    for (pos, u) in uni.iter().enumerate() {
        let role = match form.get(&format!("role[{}]", u.source)) {
            Some(value) => parse_role_strict(value).ok_or_else(|| {
                format!(
                    "Unknown role \u{201c}{value}\u{201d} for field \u{201c}{}\u{201d}.",
                    u.source
                )
            })?,
            None => u.role_fallback,
        };
        let order = form
            .get(&format!("order[{}]", u.source))
            .and_then(|s| s.trim().parse::<i64>().ok())
            .unwrap_or(pos as i64);
        let filterable = if filters_submitted {
            form.get(&format!("filterable[{}]", u.source)).is_some() && role != FieldRole::Hidden
        } else {
            u.filterable_fallback
        };
        let merge_into = form
            .get(&format!("merge[{}]", u.source))
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from);
        built.push(Built {
            source: u.source.clone(),
            role,
            order,
            filterable,
            merge_into,
        });
    }

    let by_source: HashMap<&str, &Built> = built.iter().map(|b| (b.source.as_str(), b)).collect();
    let order_of = |s: &str| -> i64 { by_source.get(s).map(|b| b.order).unwrap_or(i64::MAX) };

    // --- Resolve groups: anchor -> members. A member is valid only when its
    // target is a standalone, non-Hidden field and the member itself isn't
    // Hidden. Single-select ⇒ a source is in at most one group (no overlap).
    let mut members_of: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for b in &built {
        if b.role == FieldRole::Hidden {
            continue; // Hidden never participates in a merge.
        }
        let Some(anchor) = &b.merge_into else {
            continue;
        };
        if anchor == &b.source {
            continue;
        }
        let anchor_ok = by_source
            .get(anchor.as_str())
            .map(|a| a.merge_into.is_none() && a.role != FieldRole::Hidden)
            .unwrap_or(false);
        if anchor_ok {
            members_of
                .entry(anchor.clone())
                .or_default()
                .push(b.source.clone());
        }
    }
    // Drop groups that fell below 2 (anchor + members) → all standalone.
    members_of.retain(|_, members| 1 + members.len() >= 2);
    let mut is_member: BTreeSet<String> = BTreeSet::new();
    for members in members_of.values() {
        for m in members {
            is_member.insert(m.clone());
        }
    }

    // --- Final fields: non-members → FieldSpec; anchors carry the merge vec.
    let mut final_entries: Vec<(i64, usize, FieldSpec)> = Vec::new();
    for (pos, b) in built.iter().enumerate() {
        if is_member.contains(&b.source) {
            continue; // members live only inside the anchor's merge vec
        }
        let merge = members_of.get(&b.source).map(|members| {
            let mut sorted = members.clone();
            sorted.sort_by_key(|m| (order_of(m), m.clone()));
            let mut v = vec![b.source.clone()];
            v.extend(sorted);
            v
        });
        final_entries.push((
            b.order,
            pos,
            FieldSpec {
                source: b.source.clone(),
                role: b.role,
                merge,
                filterable: b.filterable,
            },
        ));
    }
    final_entries.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));

    let mut candidate = spec.clone();
    candidate.fields = final_entries.into_iter().map(|(_, _, fs)| fs).collect();
    // Re-derive filters from the surviving filterable fields (a field merged
    // away can't be a filter) so filters ⊆ filterable holds → validate passes.
    candidate.filters = candidate
        .fields
        .iter()
        .filter(|f| f.filterable)
        .map(|f| f.source.clone())
        .collect();
    Ok(candidate)
}

/// List-page layout precedence (Phase 8):
///
/// 1. a present-**and-valid** `?layout=` wins (ephemeral override, Phase 7);
/// 2. else the saved ViewSpec's `layout` (NEW — the persisted default);
/// 3. else `Table` (the Phase-6 fallback for models with no saved view).
///
/// This **deliberately changes** Phase 6's "always Table": a saved view's
/// layout now wins over Table when no `?layout=` is present.
fn resolve_effective_layout(
    param: Option<&str>,
    saved: Option<&crate::viewspec::ViewSpec>,
) -> crate::viewspec::ViewLayout {
    parse_layout_strict(param)
        .or_else(|| saved.map(|v| v.layout))
        .unwrap_or(crate::viewspec::ViewLayout::Table)
}

/// Persist a model's default list layout into its `<model>.view.json`,
/// reusing [`ViewSpec::write_to`](crate::viewspec::ViewSpec::write_to)
/// (atomic temp-file + rename — constraint 2). If a saved view exists it is
/// loaded and **only** its `layout` is changed (every other field — fields,
/// roles, filters, version, model — preserved); otherwise the Phase-2
/// default is derived, its layout set, and the file created (constraint 4).
pub(crate) fn save_layout_default(
    base: &Path,
    model_name: &str,
    schema_model: &crate::schema::SchemaModel,
    layout: crate::viewspec::ViewLayout,
) -> Result<(), crate::Error> {
    let mut spec = load_saved_view(base, model_name)
        .unwrap_or_else(|| crate::viewspec::ViewSpec::from_schema_model(schema_model));
    spec.layout = layout;
    save_view_spec(base, model_name, &spec)
}

/// Resolve the model's current ViewSpec: the saved `<model>.view.json` if
/// present + valid, otherwise the Phase-2 derived default. The single read
/// path the editor (and the layout saver) edit from.
pub(crate) fn resolve_view(
    base: &Path,
    model_name: &str,
    schema_model: &crate::schema::SchemaModel,
) -> crate::viewspec::ViewSpec {
    load_saved_view(base, model_name)
        .unwrap_or_else(|| crate::viewspec::ViewSpec::from_schema_model(schema_model))
}

/// Generalized writer (Phase 9a): persist a **full** ViewSpec to
/// `<model>.view.json`. Reuses [`ViewSpec::write_to`](crate::viewspec::ViewSpec::write_to),
/// which **validates first** (so an invalid spec is never written) and
/// then writes atomically (temp-file + rename). No new file-writing or
/// validation logic — the layout saver and the composition editor both go
/// through here, so there is one write path.
pub(crate) fn save_view_spec(
    base: &Path,
    model_name: &str,
    spec: &crate::viewspec::ViewSpec,
) -> Result<(), crate::Error> {
    spec.write_to(&view_file_path(base, model_name))
}

/// Strictly validate a `_return` path before redirecting to it after a
/// state change. Only a relative path targeting **this model's** list is
/// honoured — `/admin/<slug>` optionally followed by `?query`. Anything
/// else (absolute URLs, scheme-relative `//host`, backslashes, `..`
/// traversal, or a different path) falls back to `/admin/<slug>`. No
/// open-redirect surface.
pub(crate) fn sanitize_return(slug: &str, raw: &str) -> String {
    let base = format!("/admin/{slug}");
    let ok = raw.starts_with(&base)
        && matches!(raw.as_bytes().get(base.len()), None | Some(b'?'))
        && !raw.contains("..")
        && !raw.contains('\\')
        && !raw.contains("//");
    if ok {
        raw.to_string()
    } else {
        base
    }
}

/// Select the list table's columns through the ViewSpec. Renders `spec`
/// under `layout` via the Phase-3 renderer to get the ordered,
/// **non-Hidden** source names, and maps each back to its `AdminUiField`
/// (preserving `label` / `sortable`). Hidden-role fields (`id`, `*_hash`,
/// …) never appear — the end-to-end Hidden guarantee.
///
/// A merged ViewSpec cell maps to its anchor (`sources[0]`) column; a
/// source naming a field that isn't on the model (e.g. a stale saved view)
/// is skipped rather than crashing the page.
/// i18n L4 — the admin-layer language registry: ISO 639-1 code → endonym
/// display name. Open/extensible (add a tuple to support a language). Codes
/// are what's stored everywhere; endonyms are shown by the switcher (L4b).
pub(crate) fn languages() -> &'static [(&'static str, &'static str)] {
    &[("en", "English"), ("sv", "Svenska")]
}

/// Whether `code` is a known language. Used to validate the set-language
/// action; an empty code is handled separately (it clears the preference).
/// Accepts the built-in registry plus any language localised via
/// `rustio.locale.json`, so a project-added language is also settable.
pub(crate) fn is_known_language(code: &str) -> bool {
    languages().iter().any(|(c, _)| *c == code)
        || crate::admin::uilang::catalog_languages()
            .iter()
            .any(|c| c == code)
}

/// i18n L4 — resolve the ACTIVE render language for a request:
/// **user preference → view/project `default_language` → `"en"`**. The admin
/// reads user state here and passes the resulting string into the *pure* label
/// resolver ([`ViewSpec::label_for`]); core never reads the user. Setting a
/// language never mutates a ViewSpec — this is read-only resolution.
pub(crate) async fn resolve_active_language(
    db: &Db,
    identity: Option<&crate::auth::Identity>,
    spec: &crate::viewspec::ViewSpec,
) -> String {
    if let Some(id) = identity {
        if let Ok(Some(pref)) = crate::auth::user::preferred_language(db, id.user_id).await {
            return pref;
        }
    }
    if !spec.default_language.is_empty() {
        return spec.default_language.clone();
    }
    "en".to_string()
}

fn view_columns(
    spec: &crate::viewspec::ViewSpec,
    layout: crate::viewspec::ViewLayout,
    fields: &[AdminUiField],
    // i18n L4 — the ACTIVE render language (admin-resolved: user pref →
    // default_language → "en"). L2/L3 callers pass `&spec.default_language`,
    // preserving their behaviour exactly.
    active_lang: &str,
) -> Vec<ColumnView> {
    // Selection is independent of row data — probe with a single empty row
    // and read which sources the requested layout surfaces, in order. The
    // Phase-3 renderer owns the per-layout cell set (Table = all visible,
    // List drops Meta, Compact = Title + Badge, …); the admin adds none.
    let probe: Vec<crate::viewspec::render::Row> = vec![std::collections::BTreeMap::new()];
    let view = crate::viewspec::render::RenderedView::render_with_layout(spec, layout, &probe);
    let cells: &[crate::viewspec::render::RenderedCell] =
        view.rows.first().map(|r| r.cells.as_slice()).unwrap_or(&[]);

    cells
        .iter()
        .filter_map(|c| {
            // Anchor source = first; a merged cell carries every source.
            let anchor = c.sources.first()?;
            let f = fields.iter().find(|f| f.name == anchor)?;
            let merged = c.sources.len() > 1;
            Some(ColumnView {
                name: f.name.to_string(),
                // i18n L2/L4 — header TEXT resolves through the view's display
                // labels for the ACTIVE language; an unlabelled field keeps the
                // admin's `_id`-stripping humaniser (byte-identical to pre-i18n).
                // Iron rule intact: `name`/sorting/links/data all still key off
                // the English `anchor` source — only the header string is
                // translated. Merged columns use the anchor source.
                label: spec
                    .label_for(anchor, active_lang)
                    .unwrap_or_else(|| humanize_field_label(f.label)),
                // A merged column has no single sortable source.
                sortable: f.sortable && !merged,
                merge: if merged {
                    c.sources.clone()
                } else {
                    Vec::new()
                },
            })
        })
        .collect()
}

/// One entry in the list-page layout switcher. `href` preserves the
/// current search / sort / filter state so switching layout never drops
/// the user's filters.
#[derive(serde::Serialize)]
struct LayoutOptionView {
    key: String,
    label: String,
    href: String,
    active: bool,
}

/// Stable lowercase key for a layout (URL param + template branch value).
fn layout_key(layout: crate::viewspec::ViewLayout) -> &'static str {
    use crate::viewspec::ViewLayout;
    // `ViewLayout` is `#[non_exhaustive]`, but only for downstream crates;
    // inside rustio-core the four variants are exhaustive — a new variant
    // must be mapped here, so no wildcard arm.
    match layout {
        ViewLayout::Table => "table",
        ViewLayout::List => "list",
        ViewLayout::Cards => "cards",
        ViewLayout::Compact => "compact",
    }
}

/// Build a list-page URL for `layout_key` that preserves the current
/// search (`q`), sort, dir, and column filters. Filter keys are emitted in
/// sorted order so the href is deterministic. `page` is intentionally
/// dropped (a layout switch resets to page 1).
fn list_layout_href(
    slug: &str,
    layout_key: &str,
    query: Option<&str>,
    sort: Option<&str>,
    dir: Option<&str>,
    filters: &HashMap<String, String>,
) -> String {
    let mut parts = vec![format!("layout={layout_key}")];
    if let Some(q) = query.filter(|s| !s.is_empty()) {
        parts.push(format!("q={}", urlencode(q)));
    }
    if let Some(s) = sort.filter(|s| !s.is_empty()) {
        parts.push(format!("sort={}", urlencode(s)));
    }
    if let Some(d) = dir.filter(|s| !s.is_empty()) {
        parts.push(format!("dir={}", urlencode(d)));
    }
    let mut keys: Vec<&String> = filters.keys().collect();
    keys.sort();
    for k in keys {
        parts.push(format!("{}={}", urlencode(k), urlencode(&filters[k])));
    }
    format!("/admin/{slug}?{}", parts.join("&"))
}

/// Server-built `_return` value for the "Set as default" form: the current
/// list URL **without** a `layout` param (so the freshly-saved default
/// applies after the redirect), preserving search / sort / filters. Filter
/// keys sorted for determinism. Validated by [`sanitize_return`] on POST.
fn list_return_href(
    slug: &str,
    query: Option<&str>,
    sort: Option<&str>,
    dir: Option<&str>,
    filters: &HashMap<String, String>,
) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(q) = query.filter(|s| !s.is_empty()) {
        parts.push(format!("q={}", urlencode(q)));
    }
    if let Some(s) = sort.filter(|s| !s.is_empty()) {
        parts.push(format!("sort={}", urlencode(s)));
    }
    if let Some(d) = dir.filter(|s| !s.is_empty()) {
        parts.push(format!("dir={}", urlencode(d)));
    }
    let mut keys: Vec<&String> = filters.keys().collect();
    keys.sort();
    for k in keys {
        parts.push(format!("{}={}", urlencode(k), urlencode(&filters[k])));
    }
    if parts.is_empty() {
        format!("/admin/{slug}")
    } else {
        format!("/admin/{slug}?{}", parts.join("&"))
    }
}

/// 0.10+ list-page renderer. Searchable / filter / sort / paginate
/// query runs through `fetch_users_table_state`; the page renders via
/// `minijinja`. Create / edit / delete actions are RBAC-gated by the
/// caller.
#[allow(clippy::too_many_arguments)]
pub async fn list_render(
    base: &Path,
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
    layout: Option<&str>,
    identity: Option<&crate::auth::Identity>,
    csrf_token: Option<&str>,
) -> String {
    if let Some(sql) = model.ensure_table_sql() {
        let _ = persistence::ensure_table(db, sql).await;
    }

    let dashboard_entries = collect_dashboard_entries(db, registry).await;
    let sidebar = sidebar_merged(&dashboard_entries, legacy_entries, Some(model.slug()));

    let fields = model.fields();
    // Resolve the view up front: its `filters` set gates which query params
    // actually filter (DW-1), and the spec also drives columns + active lang.
    let schema_model = schema_model_from_ui(model.model_name(), &fields);
    let saved = load_saved_view(base, model.model_name());
    let spec = saved
        .clone()
        .unwrap_or_else(|| crate::viewspec::ViewSpec::from_schema_model(&schema_model));

    let active_layout = resolve_effective_layout(layout, saved.as_ref());
    // i18n L4 — the active render language for THIS user (pref → default → en).
    let active_lang = resolve_active_language(db, identity, &spec).await;

    // DW-1 — filter controls for each field the view declares as a filter,
    // built BEFORE the fetch so the dropdown-vs-text decision drives `=` vs
    // `LIKE`. Boolean → tri-state; FK → related-row dropdown (resolved labels);
    // low-cardinality column → value dropdown (i18n value labels, value =
    // English token); high-cardinality → free-text box.
    const FILTER_ENUM_CAP: usize = 20;
    use crate::admin::admin_form_bridge::FilterType;
    let table = model.table_name();
    let mut filter_controls: Vec<FilterControlView> = Vec::new();
    for source in &spec.filters {
        let Some(field) = fields.iter().find(|f| f.name == *source) else {
            continue;
        };
        let current = filters.get(source).cloned().unwrap_or_default();
        let header = spec
            .label_for(source, &active_lang)
            .unwrap_or_else(|| humanize_field_label(field.label));

        // FK relation filter — dropdown of distinct related ids, displayed as
        // their resolved labels (value = the id, matched exactly).
        if field.is_relation {
            let resolved = legacy_source
                .and_then(|src| {
                    src.fields
                        .iter()
                        .find(|f| f.name == *source)
                        .and_then(|f| f.relation)
                })
                .and_then(|rel| {
                    legacy_entries
                        .iter()
                        .find(|e| e.singular_name == rel.model)
                        .map(|t| (t, rel.display_field))
                });
            let Some((target_entry, display_field)) = resolved else {
                continue; // can't resolve the FK target → no control
            };
            let ids = distinct_values(db, table, source, 51).await;
            if ids.is_empty() {
                continue;
            }
            let id_to_label = fk_lookup_batch(db, target_entry, display_field, &ids).await;
            let mut options = vec![FilterOptionView {
                value: String::new(),
                label: "All".to_string(),
                selected: current.is_empty(),
            }];
            for id in ids {
                let label = id_to_label
                    .get(&id)
                    .cloned()
                    .unwrap_or_else(|| format!("#{id}"));
                let selected = current == id;
                options.push(FilterOptionView {
                    value: id,
                    label,
                    selected,
                });
            }
            filter_controls.push(FilterControlView {
                source: source.clone(),
                label: header,
                kind: "select".to_string(),
                options,
                current,
            });
            continue;
        }

        // Helper: an "All" option followed by one option per distinct value,
        // with the English token as the value and a value-label display.
        let select_options = |raws: Vec<String>| -> Vec<FilterOptionView> {
            let mut opts = vec![FilterOptionView {
                value: String::new(),
                label: "All".to_string(),
                selected: current.is_empty(),
            }];
            for raw in raws {
                let key = raw.trim().to_lowercase();
                let label = spec
                    .value_label_for(source, &key, &active_lang)
                    .unwrap_or_else(|| raw.clone());
                let selected = current == raw;
                opts.push(FilterOptionView {
                    value: raw,
                    label,
                    selected,
                });
            }
            opts
        };

        let (kind, options) = match resolve_filter_type(field) {
            FilterType::Boolean => (
                "boolean",
                vec![
                    FilterOptionView {
                        value: String::new(),
                        label: "All".to_string(),
                        selected: current.is_empty(),
                    },
                    FilterOptionView {
                        value: "1".to_string(),
                        label: "Yes".to_string(),
                        selected: current == "1",
                    },
                    FilterOptionView {
                        value: "0".to_string(),
                        label: "No".to_string(),
                        selected: current == "0",
                    },
                ],
            ),
            FilterType::Select => (
                "select",
                select_options(distinct_values(db, table, source, 51).await),
            ),
            FilterType::Exact => {
                let raws = distinct_values(db, table, source, FILTER_ENUM_CAP as i64 + 1).await;
                if !raws.is_empty() && raws.len() <= FILTER_ENUM_CAP {
                    ("select", select_options(raws))
                } else {
                    ("exact", Vec::new())
                }
            }
        };
        filter_controls.push(FilterControlView {
            source: source.clone(),
            label: header,
            kind: kind.to_string(),
            options,
            current,
        });
    }
    // Dropdown filters (boolean/select) match exactly; text filters substring.
    let exact_match: Vec<String> = filter_controls
        .iter()
        .filter(|c| c.kind != "exact")
        .map(|c| c.source.clone())
        .collect();

    let (rows_raw, total, current_page, total_pages, validated_sort, validated_dir) =
        fetch_users_table_state(
            db,
            model,
            query,
            filters,
            &spec.filters,
            &exact_match,
            page,
            sort,
            dir,
        )
        .await;

    // Phase 6 — column selection runs through the model's ViewSpec via the
    // deterministic Phase-3 renderer (`crate::viewspec::render`), replacing
    // the raw `visible_in_table` dump. The ViewSpec decides WHICH columns
    // appear, their ORDER, and which are Hidden; the cell rendering below
    // (FK linked labels, status pills, primary cell) is unchanged, so those
    // live features are preserved.
    //
    // Deliberate behaviour changes vs. the pre-ViewSpec list:
    //   * The primary-key / `id` column is Hidden by default (raw ids
    //     aren't shown), as are secret-shaped fields (`*_hash`, `password`,
    //     `token`) and opaque PII — the end-to-end Hidden guarantee.
    //   * The old row-expansion of overflow columns is gone (no expand
    //     panel); Meta-role fields are just normal columns.
    //   * A saved `<model_snake>.view.json` in `base` overrides the derived
    //     default; a missing/invalid file silently derives it.
    //   * NOTE: the live list path applies NO PII masking today — it only
    //     OMITS Hidden fields. Adding masking to shown cells here is a
    //     deliberate follow-up, intentionally out of this phase's scope.
    //
    // Phase 7 — the `?layout=` switcher selects which of the four layouts
    // the renderer arranges. Phase 8 — layout precedence is now:
    //   1. present-and-valid `?layout=` (ephemeral),
    //   2. else the saved ViewSpec's `layout` (persisted default),
    //   3. else Table.
    // This CHANGES Phase 6's "always Table": a saved layout now wins over
    // Table when no `?layout=` is present. The renderer still picks the
    // cell set per layout; the template arranges those same cells.
    let columns: Vec<ColumnView> = view_columns(&spec, active_layout, &fields, &active_lang);

    // One batch `SELECT … WHERE id IN (…)` per FK column visible on
    // this page of rows. Cells for matching FK values are rewritten
    // into `<a href="/admin/<target>/<id>">display</a>`. Unresolved
    // ids (stale, deleted, target wiped) render as `#<id>` — never
    // the raw integer with no context.
    let fk_lookups = build_fk_lookups(db, legacy_source, &columns, &rows_raw, legacy_entries).await;

    let pk = model.primary_key();
    let slug = model.slug();
    // §4.8 — the first non-id column is the "primary" cell (bold name).
    let primary_col = columns
        .iter()
        .find(|c| c.name.as_str() != pk)
        .map(|c| c.name.clone());
    // PII masking — columns whose field the intelligence layer classifies as
    // sensitive (email / phone / personal id) are masked in their shown cells.
    // (Hidden fields are already omitted; this covers SHOWN sensitive ones.)
    // Context-gated, as the intelligence layer intends ("sensitive up, never
    // down — no surprises"): mask shown sensitive cells (email / phone /
    // personal id) ONLY when the project declares a `rustio.context.json`.
    // Without a context the list renders exactly as before — masking is an
    // explicit, opt-in posture, not a silent default.
    let sensitive_cols: std::collections::HashSet<&str> =
        match crate::admin::intelligence::context_global() {
            Some(ctx) => legacy_source
                .map(|src| {
                    src.fields
                        .iter()
                        .filter(|f| {
                            crate::admin::intelligence::classify_field(f, Some(ctx)).is_sensitive()
                        })
                        .map(|f| f.name)
                        .collect()
                })
                .unwrap_or_default(),
            None => std::collections::HashSet::new(),
        };
    let rows: Vec<RowView> = rows_raw
        .iter()
        .map(|row| {
            let id = row.get(pk).cloned().unwrap_or_default();
            let cells = columns
                .iter()
                .enumerate()
                .map(|(col_idx, col)| {
                    // §9d — merged cell: join each merge source's value with
                    // " · " (matching the Phase-3 renderer). Checked first so
                    // a merged anchor isn't treated as FK/status/primary. PII —
                    // each merge source is masked individually if it's
                    // sensitive, so a merge can never leak a value its own
                    // column would have masked.
                    if !col.merge.is_empty() {
                        return col
                            .merge
                            .iter()
                            .map(|s| {
                                let v = row.get(s).cloned().unwrap_or_default();
                                if !v.is_empty() && sensitive_cols.contains(s.as_str()) {
                                    crate::admin::intelligence::mask_pii(&v)
                                } else {
                                    v
                                }
                            })
                            .filter(|v| !v.is_empty())
                            .map(|v| html_escape(&v))
                            .collect::<Vec<_>>()
                            .join(" · ");
                    }
                    let raw = row.get(&col.name).cloned().unwrap_or_default();
                    // PII — mask a shown sensitive cell (keeps a short prefix,
                    // rest as •). Overrides FK/status/primary formatting.
                    if sensitive_cols.contains(col.name.as_str()) {
                        return if raw.is_empty() {
                            String::new()
                        } else {
                            html_escape(&crate::admin::intelligence::mask_pii(&raw))
                        };
                    }
                    if col.name.as_str() == pk {
                        // §3.4 — ID column: rust mono `#<id>`.
                        if raw.is_empty() {
                            return String::new();
                        }
                        return format!(
                            r#"<span class="rio-cell-id">#{}</span>"#,
                            html_escape(&raw)
                        );
                    }
                    if let Some(fk) = fk_lookups.iter().find(|f| f.column_index == col_idx) {
                        // FK column: render as a clickable link to the
                        // target row (or `#<id>` if the target is gone).
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
                    } else if is_status_field_name(&col.name) {
                        // §3.5 — status-shaped column: vivid colour pill.
                        // Booleans ("0"/"1") normalise to Active/Inactive;
                        // string statuses keep their label. Empty → empty.
                        if raw.is_empty() {
                            return String::new();
                        }
                        let (data_value, label) = normalize_status_pill(&raw);
                        // i18n value labels — translate the displayed text for
                        // the active language, keyed by the lowercased English
                        // value. The pill COLOUR still keys off `data_value`
                        // (English): only the shown text changes (iron rule).
                        let label = spec
                            .value_label_for(&col.name, &data_value, &active_lang)
                            .unwrap_or(label);
                        format!(
                            r#"<span class="{cls}">{label}</span>"#,
                            cls = status_pill_color(&data_value),
                            label = html_escape(&label),
                        )
                    } else if primary_col.as_deref() == Some(col.name.as_str()) {
                        // §4.8 — primary-name cell (bold).
                        format!(
                            r#"<span class="rio-cell-primary">{}</span>"#,
                            html_escape(&raw)
                        )
                    } else {
                        // i18n value labels — an enum-like value's label for the
                        // active language, keyed by the lowercased stored value;
                        // falls back to the raw English value.
                        let key = raw.trim().to_lowercase();
                        match spec.value_label_for(&col.name, &key, &active_lang) {
                            Some(translated) => html_escape(&translated),
                            None => html_escape(&raw),
                        }
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
        total,
        &validated_sort,
        &validated_dir,
        layout_key(active_layout),
    );

    // Layout switcher — one link per layout, preserving the current
    // search / sort / filter state so switching never drops filters.
    use crate::viewspec::ViewLayout;
    let layout_options: Vec<LayoutOptionView> = [
        (ViewLayout::Table, "Table"),
        (ViewLayout::List, "List"),
        (ViewLayout::Cards, "Cards"),
        (ViewLayout::Compact, "Compact"),
    ]
    .iter()
    .map(|(lay, label)| LayoutOptionView {
        key: layout_key(*lay).to_string(),
        label: label.to_string(),
        href: list_layout_href(slug, layout_key(*lay), query, sort, dir, filters),
        active: *lay == active_layout,
    })
    .collect();

    // Server-built return path for the "Set as default" POST (no layout
    // param, so the saved default applies after redirect).
    let return_to = list_return_href(slug, query, sort, dir, filters);

    // DW-1 — "Clear filters" target = the same view minus the filter params.
    let clear_filters_href = list_return_href(slug, query, sort, dir, &HashMap::new());
    let any_filter_active = filter_controls.iter().any(|c| !c.current.is_empty());

    let model_view = ModelView {
        display_name: format!("{}s", model.model_name()),
        singular_name: model.model_name().to_string(),
        new_url: format!("/admin/{slug}/new"),
    };

    // Per-model RBAC — resolve the signed-in user's role and derive the
    // permission matrix for THIS model's table (app tables vs framework
    // `rustio_` tables differ). SuperAdmin/Admin → full on app models, Editor
    // → no delete, Viewer → view-only; a signed-in user with an unrecognised
    // role degrades to view-only (safe), and no identity → nothing.
    let permissions = {
        let perms = if let Some(id) = identity {
            let role_str: Option<String> =
                sqlx::query_scalar("SELECT role FROM rustio_users WHERE id = ?")
                    .bind(id.user_id)
                    .fetch_optional(db.pool())
                    .await
                    .ok()
                    .flatten();
            role_str
                .and_then(|s| crate::admin::rbac::Role::from_role_string(&s))
                .map(|r| r.permissions_for(model.table_name()))
                .unwrap_or(crate::admin::rbac::PermissionSet::VIEW_ONLY)
        } else {
            // No identity (only reachable in tests; `admin_guard` blocks
            // unauthenticated requests in production) → view-only.
            crate::admin::rbac::PermissionSet::VIEW_ONLY
        };
        ListPermissionsView {
            view: perms.view,
            create: perms.create,
            edit: perms.edit,
            delete: perms.delete,
        }
    };

    let design = design_view();
    let user = user_view(db, identity).await;

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
            layout => layout_key(active_layout),
            layout_options => layout_options,
            return_to => return_to,
            page_title => format!("{}s", model.model_name()),
            query => query.unwrap_or(""),
            sort => validated_sort.clone().unwrap_or_default(),
            dir => validated_dir.clone().unwrap_or_default(),
            filter_controls => filter_controls,
            clear_filters_href => clear_filters_href,
            any_filter_active => any_filter_active,
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

#[allow(clippy::too_many_arguments)]
fn build_pagination_view(
    slug: &str,
    query: Option<&str>,
    current: i64,
    pages: i64,
    total: i64,
    sort: &Option<String>,
    dir: &Option<String>,
    layout_key: &str,
) -> PaginationView {
    // `fetch_users_table_state` uses PAGE_SIZE = 20; keep that here. If the
    // page-size constant ever moves, thread it through instead of copying.
    let per_page: i64 = 20;
    let from = if total == 0 {
        0
    } else {
        (current - 1) * per_page + 1
    };
    let to = (current * per_page).min(total).max(from);
    if pages <= 1 {
        return PaginationView {
            pages,
            current,
            per_page,
            total,
            from,
            to,
            links: Vec::new(),
        };
    }
    let q_param = query.unwrap_or("");
    let sort_param = sort.as_deref().unwrap_or("");
    let dir_param = dir.as_deref().unwrap_or("");
    let base_href = |p: i64| -> String {
        let mut parts = vec![format!("page={p}")];
        // Keep paging within the current layout.
        if layout_key != "table" {
            parts.push(format!("layout={layout_key}"));
        }
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
        per_page,
        total,
        from,
        to,
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

/// One permission row on the account page (Console theme): a named capability
/// and whether the signed-in user's role allows it.
#[derive(serde::Serialize)]
struct AccountPermView {
    label: String,
    detail: String,
    allowed: bool,
}

/// One role in the "Roles in this workspace" reference list.
#[derive(serde::Serialize)]
struct AccountRoleRefView {
    name: String,
    style: String, // "admin" | "developer" | "customer"
    initials: String,
    desc: String,
    is_you: bool,
}

/// The full account-page model — identity, role narrative, real permission
/// map, roles reference, and account/security details, all derived from the
/// signed-in user and the RBAC matrix.
#[derive(serde::Serialize)]
struct AccountView {
    name: String,
    email: String,
    initials: String,
    role_name: String,
    role_style: String,
    role_blurb: String,
    user_id: i64,
    member_since: String,
    language: String,
    sessions: i64,
    perms: Vec<AccountPermView>,
    roles: Vec<AccountRoleRefView>,
}

/// Map a stored role string to a display name, a Console badge/avatar style
/// (`admin`/`developer`/`customer` — the three styles the theme defines), and
/// a first-person blurb. Unknown roles degrade to view-only "Viewer".
fn account_role_display(role_str: &str) -> (&'static str, &'static str, &'static str) {
    use crate::admin::rbac::Role;
    match Role::from_role_string(role_str) {
        Some(Role::SuperAdmin) => (
            "Administrator",
            "admin",
            "You can manage every record, user, and list view in this workspace. Schema evolution and the CLI are developer tools, run outside the admin.",
        ),
        Some(Role::Admin) => (
            "Administrator",
            "admin",
            "You manage records and list views across the workspace; framework tables stay read-only.",
        ),
        Some(Role::Editor) => (
            "Editor",
            "developer",
            "You can create and edit records across every model, but not delete them.",
        ),
        Some(Role::Viewer) | None => (
            "Viewer",
            "customer",
            "You have read-only access to every record in this workspace.",
        ),
    }
}

/// First two alphanumeric characters of an email's local part, uppercased.
fn account_initials(email: &str) -> String {
    let local = email.split('@').next().unwrap_or(email);
    let s: String = local
        .chars()
        .filter(|c| c.is_alphanumeric())
        .take(2)
        .collect::<String>()
        .to_uppercase();
    if s.is_empty() {
        "?".to_string()
    } else {
        s
    }
}

/// Build the account-page view from the signed-in user + the RBAC matrix and
/// a couple of cheap lookups (created-at, language preference, live sessions).
async fn build_account_view(db: &Db, user: &crate::auth::User) -> AccountView {
    use crate::admin::rbac::Role;
    let (role_name, role_style, role_blurb) = account_role_display(&user.role);
    let role = Role::from_role_string(&user.role).unwrap_or(Role::Viewer);

    // Active language for this user (preference → English) — the role narrative,
    // permission rows, and role reference are translated through the same
    // `uilang` catalog as the shell, so the whole account page follows the
    // language switch.
    let pref = crate::auth::user::preferred_language(db, user.id)
        .await
        .ok()
        .flatten();
    let lang = pref.clone().unwrap_or_else(|| "en".to_string());
    let tr = |s: &str| crate::admin::uilang::translate(&lang, s);

    // App-model matrix vs framework-table matrix — "manage users" keys off the
    // latter (the users model is a `rustio_*` system table).
    let app = role.permissions_for("records");
    let sys = role.permissions_for("rustio_users");
    let perm = |label: &str, detail: &str, allowed: bool| AccountPermView {
        label: tr(label),
        detail: tr(detail),
        allowed,
    };
    let perms = vec![
        perm(
            "View records",
            "Read every model in the workspace",
            app.view,
        ),
        perm(
            "Create & edit",
            "Add and update records across all models",
            app.create && app.edit,
        ),
        perm(
            "Delete records",
            "Remove records, with confirmation",
            app.delete,
        ),
        perm(
            "Manage users & roles",
            "Create users and assign their roles",
            sys.create,
        ),
        perm(
            "Reshape list views",
            "Edit ViewSpec roles, filters, and labels",
            app.edit,
        ),
        perm(
            "Evolve schema",
            "Add or change fields — a developer / CLI tool",
            false,
        ),
        perm(
            "Run migrations & CLI",
            "Apply migrations and use the rustio CLI",
            false,
        ),
    ];

    let you_style = role_style;
    let mk = |name: &str, style: &str, initials: &str, desc: &str| AccountRoleRefView {
        name: tr(name),
        style: style.to_string(),
        initials: initials.to_string(),
        desc: tr(desc),
        is_you: style == you_style,
    };
    let roles = vec![
        mk(
            "Viewer",
            "customer",
            "VI",
            "Read-only access to every record in the workspace.",
        ),
        mk(
            "Editor",
            "developer",
            "ED",
            "Create and edit records across all models; cannot delete.",
        ),
        mk(
            "Administrator",
            "admin",
            "AD",
            "Manages all records, users, and list views.",
        ),
    ];

    // Cheap account facts.
    let member_since: String =
        sqlx::query_scalar::<_, String>("SELECT created_at FROM rustio_users WHERE id = ?")
            .bind(user.id)
            .fetch_optional(db.pool())
            .await
            .ok()
            .flatten()
            .map(|s| s.chars().take(10).collect()) // YYYY-MM-DD
            .unwrap_or_default();
    let sessions: i64 = crate::auth::session::count_active(db, user.id)
        .await
        .unwrap_or(0);
    let language = match &pref {
        Some(code) => crate::admin::uilang::endonym(code),
        None => tr("Project default"),
    };

    let name = {
        let local = user.email.split('@').next().unwrap_or(&user.email);
        let mut chars = local.chars();
        match chars.next() {
            Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
            None => user.email.clone(),
        }
    };

    AccountView {
        name,
        email: user.email.clone(),
        initials: account_initials(&user.email),
        role_name: tr(role_name),
        role_style: role_style.to_string(),
        role_blurb: tr(role_blurb),
        user_id: user.id,
        member_since,
        language,
        sessions,
        perms,
        roles,
    }
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

    // Rich account model for the Console account page (real role + RBAC).
    let account = match user {
        Some(u) => build_account_view(db, u).await,
        None => {
            build_account_view(
                db,
                &crate::auth::User {
                    id: 0,
                    email: "unknown".into(),
                    password_hash: String::new(),
                    is_active: false,
                    role: "viewer".into(),
                },
            )
            .await
        }
    };

    let design = design_view();
    let user_v = user_view(db, identity).await;

    let env = crate::admin::templating::env();
    match env.get_template("admin/profile.html").and_then(|tmpl| {
        tmpl.render(minijinja::context! {
            design => design,
            current_user => user_v,
            sidebar_entries => sidebar,
            profile => profile,
            account => account,
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
    let user_v = user_view(db, identity).await;

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

#[derive(serde::Serialize)]
pub struct SuggestionReviewView {
    pub model: String,
    pub field: String,
    pub industry: String,
    pub confidence_label: String,
    pub confidence_class: String,
    pub apply_url: String,
    pub can_apply: bool,
    pub step_descriptions: Vec<String>,
    pub schema_diff_html: String,
    pub explanation: String,
    pub risk_label: String,
    pub risk_class: String,
    pub adds_fields: u32,
    pub destructive: bool,
    pub validation_ok: bool,
    pub validation_message: Option<String>,
    pub warnings: Vec<String>,
    pub error: Option<String>,
}

#[derive(serde::Serialize)]
pub struct AppliedFileView {
    pub kind: String,
    pub path: String,
}

#[derive(serde::Serialize)]
pub struct SuggestionAppliedView {
    pub change_lines: Vec<String>,
    pub files: Vec<AppliedFileView>,
}

/// Render `admin/suggestion_review.html`. All AI-pipeline work
/// (planner, reviewer, diff, confidence) happens in the caller;
/// this function only lays out the values.
pub async fn suggestion_review_render(
    db: &Db,
    registry: &crate::admin::admin_form_bridge::AdminRegistry,
    legacy_entries: &[crate::admin::AdminEntry],
    identity: Option<&crate::auth::Identity>,
    csrf_token: Option<&str>,
    view: SuggestionReviewView,
) -> String {
    let dashboard_entries = collect_dashboard_entries(db, registry).await;
    let sidebar = sidebar_merged(&dashboard_entries, legacy_entries, None);
    let design = design_view();
    let user_v = user_view(db, identity).await;
    let env = crate::admin::templating::env();
    match env
        .get_template("admin/suggestion_review.html")
        .and_then(|tmpl| {
            tmpl.render(minijinja::context! {
                design => design,
                current_user => user_v,
                sidebar_entries => sidebar,
                page_title => format!("Review: add {} to {}", view.field, view.model),
                csrf_token => csrf_token.unwrap_or(""),
                rustio_version => env!("CARGO_PKG_VERSION"),
                view => view,
            })
        }) {
        Ok(html) => html,
        Err(err) => {
            eprintln!("admin suggestion_review template render failed: {err}");
            "<!doctype html><html><body><h1>Review suggestion</h1><p>Template failed.</p></body></html>".into()
        }
    }
}

/// Render `admin/suggestion_applied.html` — success page after a
/// suggestion is applied.
pub async fn suggestion_applied_render(
    db: &Db,
    registry: &crate::admin::admin_form_bridge::AdminRegistry,
    legacy_entries: &[crate::admin::AdminEntry],
    identity: Option<&crate::auth::Identity>,
    csrf_token: Option<&str>,
    applied: SuggestionAppliedView,
) -> String {
    let dashboard_entries = collect_dashboard_entries(db, registry).await;
    let sidebar = sidebar_merged(&dashboard_entries, legacy_entries, None);
    let design = design_view();
    let user_v = user_view(db, identity).await;
    let env = crate::admin::templating::env();
    match env
        .get_template("admin/suggestion_applied.html")
        .and_then(|tmpl| {
            tmpl.render(minijinja::context! {
                design => design,
                current_user => user_v,
                sidebar_entries => sidebar,
                page_title => "Changes applied",
                csrf_token => csrf_token.unwrap_or(""),
                rustio_version => env!("CARGO_PKG_VERSION"),
                applied => applied,
            })
        }) {
        Ok(html) => html,
        Err(err) => {
            eprintln!("admin suggestion_applied template render failed: {err}");
            "<!doctype html><html><body><h1>Changes applied</h1><p>Template failed.</p></body></html>".into()
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
    let user_v = user_view(db, identity).await;
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
    let user_v = user_view(db, identity).await;
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

/// Heuristic for "does this column look like a status?" — used by
/// the list-page cell renderer to opt into the `badge-status` pill
/// styling via a `data-status=<value>` attribute. Name-based only;
/// the type check is implicit (`bool`s typically carry `is_` /
/// `has_` prefixes already).
///
/// Examples that match: `status`, `state`, `task_status`,
/// `is_active`, `is_published`, `has_paid`, `published`, `active`.
/// Examples that don't: `title`, `description`, `priority`,
/// `created_at`, `project_id`.
fn is_status_field_name(name: &str) -> bool {
    let n = name.to_lowercase();
    n == "status"
        || n == "state"
        || n == "active"
        || n == "published"
        || n.ends_with("_status")
        || n.ends_with("_state")
        || n.starts_with("is_")
        || n.starts_with("has_")
}

/// Normalise a raw cell value for status rendering.
///
/// Returns `(data_status_value, display_label)`:
/// - `data_status_value` is the lowercased value placed in the
///   `data-status` attribute. The 0.10.x design system renders every
///   status uniformly in `--text-secondary` regardless of the value,
///   but the attribute is retained so a project can re-introduce
///   colour-coding via its own `templates/static/admin.css` override.
/// - `display_label` is the sentence-case text shown in the cell. The
///   visual spec mandates sentence case everywhere — never `TODO`,
///   never `In_Progress`. Underscores in the raw value are replaced
///   with spaces and only the first letter is upper-cased.
///
/// SQLite booleans round-trip as `"0"` / `"1"` strings through the
/// persistence layer; both are mapped to the readable `Active` /
/// `Inactive` labels.
/// Map a lowercased status value to a vivid pill class (v8 §3.5).
/// Three buckets: emerald (good / done), amber (pending / attention),
/// slate (inactive / closed). Unknown values fall back to slate.
fn status_pill_color(data_value: &str) -> &'static str {
    match data_value.trim() {
        "active" | "approved" | "published" | "live" | "completed" | "complete" | "done"
        | "finished" | "resolved" | "paid" => "rio-pill rio-pill-emerald",
        "referred" | "pending" | "todo" | "queued" | "open" | "new" | "scheduled" | "draft"
        | "sent" | "in progress" | "in review" | "review" | "overdue" | "on leave" => {
            "rio-pill rio-pill-amber"
        }
        _ => "rio-pill rio-pill-slate",
    }
}

fn normalize_status_pill(raw: &str) -> (String, String) {
    let lc = raw.trim().to_lowercase();
    match lc.as_str() {
        "1" | "true" | "yes" | "on" => ("active".to_string(), "Active".to_string()),
        "0" | "false" | "no" | "off" => ("inactive".to_string(), "Inactive".to_string()),
        _ => (lc.clone(), humanize_status_label(raw)),
    }
}

/// Turn a raw status value into a sentence-case display label:
/// `"in_progress"` → `"In progress"`, `"TODO"` → `"Todo"`,
/// `"done"` → `"Done"`. Underscores become spaces. Only the first
/// character is upper-cased; the rest stay lower-case (sentence case,
/// not Title Case).
fn humanize_status_label(raw: &str) -> String {
    let spaced = raw.trim().replace('_', " ").to_lowercase();
    let mut chars = spaced.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
    }
}

/// Convert a database column name into a sentence-cased display label.
///
/// Rules (in order):
///   1. Empty input returns empty.
///   2. Bare `"id"` becomes `"ID"` — the conventional case for an
///      identifier column.
///   3. A label with no underscores whose first char is already
///      uppercase is treated as user-set (e.g. `"Username"` from
///      `AdminUiField { label: "Username", … }`) and passed through
///      unchanged so explicit labels aren't lowercased.
///   4. A trailing `"_id"` is stripped (`"project_id"` → `"project"`)
///      so foreign-key columns show the model name rather than the
///      column name.
///   5. Underscores become spaces, the whole label is lowercased,
///      then the first character is uppercased: `"due_at"` →
///      `"Due at"`, `"first_name"` → `"First name"`.
///
/// Idempotent — `humanize_field_label("Title") == "Title"`.
fn humanize_field_label(raw: &str) -> String {
    if raw == "id" {
        return "ID".to_string();
    }
    if !raw.contains('_') && raw.chars().next().is_some_and(|c| c.is_uppercase()) {
        return raw.to_string();
    }
    let stripped = raw.strip_suffix("_id").unwrap_or(raw);
    let spaced = stripped.replace('_', " ").to_lowercase();
    let mut chars = spaced.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::admin::{AdminEntry, AdminField, FieldType};

    /// Helper: build a minimal `AdminEntry` for the dashboard walker
    /// to chew on. Only the fields the walker actually reads are
    /// populated; the rest fall back to empty slices / defaults.
    fn entry(
        admin: &'static str,
        singular: &'static str,
        table: &'static str,
        core: bool,
    ) -> AdminEntry {
        const NO_FIELDS: &[AdminField] = &[AdminField {
            name: "id",
            ty: FieldType::I64,
            editable: false,
            nullable: false,
            relation: None,
        }];
        AdminEntry {
            admin_name: admin,
            display_name: singular,
            singular_name: singular,
            table,
            fields: NO_FIELDS,
            core,
        }
    }

    #[tokio::test]
    async fn legacy_dashboard_walk_returns_one_entry_per_non_core_model() {
        let db = Db::memory().await.unwrap();
        sqlx::query("CREATE TABLE projects (id INTEGER PRIMARY KEY)")
            .execute(db.pool())
            .await
            .unwrap();
        sqlx::query("CREATE TABLE tasks (id INTEGER PRIMARY KEY)")
            .execute(db.pool())
            .await
            .unwrap();
        sqlx::query("INSERT INTO projects DEFAULT VALUES")
            .execute(db.pool())
            .await
            .unwrap();
        sqlx::query("INSERT INTO projects DEFAULT VALUES")
            .execute(db.pool())
            .await
            .unwrap();
        sqlx::query("INSERT INTO tasks DEFAULT VALUES")
            .execute(db.pool())
            .await
            .unwrap();

        let legacy = [
            entry("projects", "Project", "projects", false),
            entry("tasks", "Task", "tasks", false),
        ];
        let known = std::collections::HashSet::new();

        let got = collect_legacy_dashboard_entries(&db, &legacy, &known).await;

        assert_eq!(got.len(), 2);
        assert_eq!(got[0].slug, "projects");
        assert_eq!(got[0].count, 2);
        assert_eq!(got[1].slug, "tasks");
        assert_eq!(got[1].count, 1);
    }

    #[tokio::test]
    async fn legacy_dashboard_walk_skips_core_entries() {
        let db = Db::memory().await.unwrap();
        // No table needed — `core` filter should bail out before any SQL runs.
        let legacy = [
            entry("rustio_users", "User", "rustio_users", true),
            entry("projects", "Project", "projects", false),
        ];
        sqlx::query("CREATE TABLE projects (id INTEGER PRIMARY KEY)")
            .execute(db.pool())
            .await
            .unwrap();

        let known = std::collections::HashSet::new();
        let got = collect_legacy_dashboard_entries(&db, &legacy, &known).await;

        assert_eq!(got.len(), 1, "core entry should be skipped");
        assert_eq!(got[0].slug, "projects");
    }

    #[tokio::test]
    async fn legacy_dashboard_walk_dedupes_against_already_listed_slugs() {
        let db = Db::memory().await.unwrap();
        sqlx::query("CREATE TABLE projects (id INTEGER PRIMARY KEY)")
            .execute(db.pool())
            .await
            .unwrap();
        sqlx::query("CREATE TABLE tasks (id INTEGER PRIMARY KEY)")
            .execute(db.pool())
            .await
            .unwrap();

        // Imagine the new-engine registry already covers `projects`.
        let mut known = std::collections::HashSet::new();
        known.insert("projects");

        let legacy = [
            entry("projects", "Project", "projects", false),
            entry("tasks", "Task", "tasks", false),
        ];

        let got = collect_legacy_dashboard_entries(&db, &legacy, &known).await;

        assert_eq!(got.len(), 1, "already-listed slug should be skipped");
        assert_eq!(got[0].slug, "tasks");
    }

    #[tokio::test]
    async fn legacy_dashboard_walk_falls_back_to_zero_when_table_missing() {
        let db = Db::memory().await.unwrap();
        // No `CREATE TABLE` — the COUNT(*) will fail; the walker
        // should degrade to count=0 instead of erroring or panicking.
        let legacy = [entry("ghosts", "Ghost", "ghosts", false)];
        let known = std::collections::HashSet::new();

        let got = collect_legacy_dashboard_entries(&db, &legacy, &known).await;

        assert_eq!(got.len(), 1);
        assert_eq!(got[0].count, 0);
    }

    // --- Phase 6: ViewSpec-driven list columns (LIVE path) ---------------
    //
    // These tests drive the real list renderer. The first two call
    // `list_render` end-to-end (the exact function `admin_model_index_get`
    // invokes), proving the running admin behaviour. The remaining two
    // exercise `view_selected_columns` / `resolve_list_view_inner`, which
    // `list_render` calls directly — live code, not isolated helpers.

    fn registry_empty() -> crate::admin::admin_form_bridge::AdminRegistry {
        crate::admin::admin_form_bridge::AdminRegistry::new()
    }

    /// A fresh, unique temp directory so view-file reads/writes are isolated
    /// and deterministic (no cwd pollution, no cross-test interference).
    fn tmp_dir() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "rustio-viewspec-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

    #[tokio::test]
    async fn list_render_hides_id_and_secret_columns_live() {
        let db = Db::memory().await.unwrap();
        sqlx::query(
            "CREATE TABLE widgets (id INTEGER PRIMARY KEY, name TEXT, status TEXT, password_hash TEXT)",
        )
        .execute(db.pool())
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO widgets (name, status, password_hash) VALUES ('Alpha','active','topsecret-xyz')",
        )
        .execute(db.pool())
        .await
        .unwrap();

        const FIELDS: &[AdminField] = &[
            AdminField {
                name: "id",
                ty: FieldType::I64,
                editable: false,
                nullable: false,
                relation: None,
            },
            AdminField {
                name: "name",
                ty: FieldType::String,
                editable: true,
                nullable: false,
                relation: None,
            },
            AdminField {
                name: "status",
                ty: FieldType::String,
                editable: true,
                nullable: false,
                relation: None,
            },
            AdminField {
                name: "password_hash",
                ty: FieldType::String,
                editable: false,
                nullable: false,
                relation: None,
            },
        ];
        let entry = AdminEntry {
            admin_name: "widgets",
            display_name: "Widget",
            singular_name: "Widget",
            table: "widgets",
            fields: FIELDS,
            core: false,
        };
        let model = LegacyEntryModel::new(&entry);
        let registry = registry_empty();
        let legacy = [entry.clone()];
        let filters = HashMap::new();

        let html = list_render(
            std::path::Path::new("."),
            &db,
            &registry,
            &legacy,
            &model,
            Some(&entry),
            None,
            1,
            &filters,
            None,
            None,
            None, // layout → defaults to Table
            None,
            None,
        )
        .await;

        // Hidden guarantee, end to end: the secret value and its column
        // name never reach the HTML.
        assert!(
            !html.contains("topsecret-xyz"),
            "hidden field VALUE leaked into live list HTML"
        );
        assert!(
            !html.to_lowercase().contains("password_hash"),
            "hidden field column leaked into live list HTML"
        );
        // The id column header is gone too (`id` is Hidden by default).
        assert!(
            !html.contains(r#"<th scope="col">Id</th>"#)
                && !html.contains(r#"<th scope="col">ID</th>"#),
            "id column should be hidden by default"
        );
        // Shown columns are present, and the status pill survived.
        assert!(html.contains("Alpha"), "title value missing from list HTML");
        assert!(
            html.contains("rio-pill"),
            "status pill markup lost from live list HTML"
        );
    }

    #[tokio::test]
    async fn list_render_preserves_fk_label_live() {
        let db = Db::memory().await.unwrap();
        sqlx::query("CREATE TABLE customers (id INTEGER PRIMARY KEY, name TEXT)")
            .execute(db.pool())
            .await
            .unwrap();
        sqlx::query("INSERT INTO customers (id, name) VALUES (1, 'Acme Corp')")
            .execute(db.pool())
            .await
            .unwrap();
        sqlx::query("CREATE TABLE orders (id INTEGER PRIMARY KEY, code TEXT, customer_id INTEGER)")
            .execute(db.pool())
            .await
            .unwrap();
        sqlx::query("INSERT INTO orders (code, customer_id) VALUES ('OR-1', 1)")
            .execute(db.pool())
            .await
            .unwrap();

        const ORDER_FIELDS: &[AdminField] = &[
            AdminField {
                name: "id",
                ty: FieldType::I64,
                editable: false,
                nullable: false,
                relation: None,
            },
            AdminField {
                name: "code",
                ty: FieldType::String,
                editable: true,
                nullable: false,
                relation: None,
            },
            AdminField {
                name: "customer_id",
                ty: FieldType::I64,
                editable: true,
                nullable: false,
                relation: Some(crate::admin::AdminRelation {
                    kind: crate::schema::RelationKind::BelongsTo,
                    model: "Customer",
                    display_field: Some("name"),
                }),
            },
        ];
        const CUSTOMER_FIELDS: &[AdminField] = &[
            AdminField {
                name: "id",
                ty: FieldType::I64,
                editable: false,
                nullable: false,
                relation: None,
            },
            AdminField {
                name: "name",
                ty: FieldType::String,
                editable: true,
                nullable: false,
                relation: None,
            },
        ];
        let orders_entry = AdminEntry {
            admin_name: "orders",
            display_name: "Order",
            singular_name: "Order",
            table: "orders",
            fields: ORDER_FIELDS,
            core: false,
        };
        let customers_entry = AdminEntry {
            admin_name: "customers",
            display_name: "Customer",
            singular_name: "Customer",
            table: "customers",
            fields: CUSTOMER_FIELDS,
            core: false,
        };
        let model = LegacyEntryModel::new(&orders_entry);
        let registry = registry_empty();
        let legacy = [orders_entry.clone(), customers_entry];
        let filters = HashMap::new();

        let html = list_render(
            std::path::Path::new("."),
            &db,
            &registry,
            &legacy,
            &model,
            Some(&orders_entry),
            None,
            1,
            &filters,
            None,
            None,
            None, // layout → defaults to Table
            None,
            None,
        )
        .await;

        // Hero feature preserved: FK column renders the related label +
        // link, not a bare integer.
        assert!(html.contains("Acme Corp"), "FK label missing: {html}");
        assert!(
            html.contains(r#"href="/admin/customers/1""#),
            "FK link missing: {html}"
        );
    }

    fn ui_fields_for_selection() -> Vec<AdminUiField> {
        vec![
            AdminUiField::integer("id", "id"),
            AdminUiField::text("name", "name"),
            AdminUiField::text("email", "email"),
            AdminUiField::text("status", "status"),
            AdminUiField::text("password_hash", "password_hash"),
        ]
    }

    #[test]
    fn view_columns_omits_hidden_in_schema_order() {
        // No saved view → derived default. id + password_hash are Hidden;
        // the rest appear in declared order.
        let fields = ui_fields_for_selection();
        let model = schema_model_from_ui("Widget", &fields);
        let spec = crate::viewspec::ViewSpec::from_schema_model(&model);
        let cols = view_columns(
            &spec,
            crate::viewspec::ViewLayout::Table,
            &fields,
            &spec.default_language,
        );
        let names: Vec<&str> = cols.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, vec!["name", "email", "status"]);
    }

    // --- i18n L2: headers through display labels --------------------------

    fn header_of(cols: &[ColumnView], name: &str) -> String {
        cols.iter().find(|c| c.name == name).unwrap().label.clone()
    }

    /// Widget (name/email/status visible) + a label map helper.
    fn widget_spec() -> (Vec<AdminUiField>, crate::viewspec::ViewSpec) {
        let fields = ui_fields_for_selection();
        let model = schema_model_from_ui("Widget", &fields);
        let spec = crate::viewspec::ViewSpec::from_schema_model(&model);
        (fields, spec)
    }
    fn put_label(spec: &mut crate::viewspec::ViewSpec, source: &str, lang: &str, text: &str) {
        spec.labels
            .entry(source.to_string())
            .or_default()
            .insert(lang.to_string(), text.to_string());
    }

    #[test]
    fn headers_use_default_language_labels_sv() {
        let (fields, mut spec) = widget_spec();
        spec.default_language = "sv".to_string();
        put_label(&mut spec, "name", "sv", "Namn");
        put_label(&mut spec, "status", "sv", "Status");
        spec.validate().unwrap();
        let cols = view_columns(
            &spec,
            crate::viewspec::ViewLayout::Table,
            &fields,
            &spec.default_language,
        );
        assert_eq!(header_of(&cols, "name"), "Namn"); // sv label
        assert_eq!(header_of(&cols, "status"), "Status"); // sv label
        assert_eq!(header_of(&cols, "email"), "Email"); // unlabelled → humanised
    }

    #[test]
    fn label_less_headers_are_byte_identical_to_today() {
        // A label-less view (default "en") → headers are exactly the admin's
        // own humaniser output, unchanged from pre-i18n.
        let (fields, spec) = widget_spec();
        let cols = view_columns(
            &spec,
            crate::viewspec::ViewLayout::Table,
            &fields,
            &spec.default_language,
        );
        for c in &cols {
            let f = fields.iter().find(|f| f.name == c.name).unwrap();
            assert_eq!(
                c.label,
                humanize_field_label(f.label),
                "byte-identical fallback"
            );
        }
        assert_eq!(header_of(&cols, "name"), "Name");
        assert_eq!(header_of(&cols, "email"), "Email");
    }

    #[test]
    fn en_labels_override_humanised_header() {
        let (fields, mut spec) = widget_spec(); // default "en"
        put_label(&mut spec, "email", "en", "E-mail address");
        spec.validate().unwrap();
        let cols = view_columns(
            &spec,
            crate::viewspec::ViewLayout::Table,
            &fields,
            &spec.default_language,
        );
        assert_eq!(header_of(&cols, "email"), "E-mail address"); // en label wins
        assert_eq!(header_of(&cols, "name"), "Name"); // unlabelled → humanised
    }

    #[test]
    fn non_default_language_label_falls_back_to_humanised() {
        // A label only in "de" with default_language "sv": the sv render must
        // NOT use the de label — it falls back to the humanised source.
        let (fields, mut spec) = widget_spec();
        spec.default_language = "sv".to_string();
        put_label(&mut spec, "name", "de", "Name(DE)");
        spec.validate().unwrap();
        let cols = view_columns(
            &spec,
            crate::viewspec::ViewLayout::Table,
            &fields,
            &spec.default_language,
        );
        assert_eq!(header_of(&cols, "name"), "Name"); // humanised, de ignored
    }

    #[test]
    fn view_columns_headers_are_deterministic() {
        let (fields, mut spec) = widget_spec();
        spec.default_language = "sv".to_string();
        put_label(&mut spec, "name", "sv", "Namn");
        let a = view_columns(
            &spec,
            crate::viewspec::ViewLayout::Table,
            &fields,
            &spec.default_language,
        );
        let b = view_columns(
            &spec,
            crate::viewspec::ViewLayout::Table,
            &fields,
            &spec.default_language,
        );
        let la: Vec<&str> = a.iter().map(|c| c.label.as_str()).collect();
        let lb: Vec<&str> = b.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(la, lb);
    }

    #[tokio::test]
    async fn list_render_translates_headers_data_unchanged() {
        // Live path: an sv-default view with one sv label → the header is
        // Swedish, while the data cell and Hidden guarantee are untouched.
        let base = tmp_dir();
        let mut spec = crate::viewspec::ViewSpec::from_schema_model(&gadget_schema_model());
        spec.default_language = "sv".to_string();
        put_label(&mut spec, "status", "sv", "Status (sv)");
        spec.validate().unwrap();
        save_view_spec(&base, "Gadget", &spec).unwrap();

        let html = render_gadget_layout_in(&base, Some("table"), &HashMap::new()).await;
        assert!(
            html.contains("Status (sv)"),
            "Swedish header rendered:\n{html}"
        );
        assert!(
            html.contains("Alpha"),
            "data cell unchanged (English value)"
        );
        assert!(!html.contains("topsecret-xyz"), "Hidden guarantee intact");
        std::fs::remove_dir_all(&base).ok();
    }

    #[tokio::test]
    async fn merged_column_header_uses_anchor_label() {
        // Merge email into name, label the anchor (name) in the default
        // language → the single merged column shows the anchor's label.
        let base = tmp_dir();
        let derived = crate::viewspec::ViewSpec::from_schema_model(&gadget_schema_model());
        let mut spec = build_edited_spec(
            &derived,
            &FormData::parse("merge_submitted=1&merge[email]=name"),
        )
        .unwrap();
        spec.default_language = "sv".to_string();
        put_label(&mut spec, "name", "sv", "Namn");
        spec.validate().unwrap();
        save_view_spec(&base, "Gadget", &spec).unwrap();

        let html = render_gadget_layout_in(&base, Some("table"), &HashMap::new()).await;
        // Anchor's sv label heads the merged column …
        assert!(
            html.contains(">Namn</th>"),
            "merged column uses anchor label:\n{html}"
        );
        // … and the merged cell still joins both values.
        assert!(html.contains("Alpha · alpha@x.example"));
        std::fs::remove_dir_all(&base).ok();
    }

    // --- i18n L3: edit display labels in the composition editor -----------

    fn label_in(spec: &crate::viewspec::ViewSpec, source: &str, lang: &str) -> Option<String> {
        spec.labels.get(source).and_then(|m| m.get(lang)).cloned()
    }

    #[test]
    fn label_edit_persists_under_editing_language() {
        let derived = crate::viewspec::ViewSpec::from_schema_model(&gadget_schema_model());
        let form = FormData::parse("labels_submitted=1&editing_lang=sv&label[status]=Status (sv)");
        let edited = build_edited_spec(&derived, &form).unwrap();
        assert_eq!(
            label_in(&edited, "status", "sv").as_deref(),
            Some("Status (sv)")
        );
        edited.validate().unwrap();
    }

    #[test]
    fn clearing_label_removes_entry_not_empty_string() {
        // Start with an sv label, then submit an empty input for it.
        let mut spec = crate::viewspec::ViewSpec::from_schema_model(&gadget_schema_model());
        put_label(&mut spec, "status", "sv", "Status (sv)");
        let form = FormData::parse("labels_submitted=1&editing_lang=sv&label[status]=");
        let edited = build_edited_spec(&spec, &form).unwrap();
        assert_eq!(
            label_in(&edited, "status", "sv"),
            None,
            "entry removed, not blanked"
        );
        // Source map pruned → byte-identical to a label-less spec.
        assert!(edited.labels.is_empty());
        assert!(!edited.to_pretty_json().unwrap().contains("labels"));
    }

    #[test]
    fn editing_one_language_does_not_clobber_another() {
        // THE no-cross-language-clobber test: an en label exists; switch the
        // editing language to sv and save an sv label → both coexist, en
        // untouched (the sv save never reads or overwrites the en value).
        let mut spec = crate::viewspec::ViewSpec::from_schema_model(&gadget_schema_model());
        put_label(&mut spec, "status", "en", "Status (en)");
        let form = FormData::parse("labels_submitted=1&editing_lang=sv&label[status]=Status (sv)");
        let edited = build_edited_spec(&spec, &form).unwrap();
        assert_eq!(
            label_in(&edited, "status", "en").as_deref(),
            Some("Status (en)")
        );
        assert_eq!(
            label_in(&edited, "status", "sv").as_deref(),
            Some("Status (sv)")
        );
        edited.validate().unwrap();
    }

    #[test]
    fn label_edit_preserves_roles_order_filters_merge() {
        // A label-only save leaves roles/order/filters/merge intact.
        let spec = custom_gadget_spec(); // [name(merge name+email), status(filter), notes]
        let form = FormData::parse("labels_submitted=1&editing_lang=en&label[status]=State");
        let edited = build_edited_spec(&spec, &form).unwrap();
        assert_eq!(field_order(&edited), field_order(&spec)); // order preserved
        assert_eq!(edited.filters, spec.filters); // filters preserved
        let name = edited.fields.iter().find(|f| f.source == "name").unwrap();
        assert_eq!(name.merge, spec.fields[0].merge); // merge preserved
        assert_eq!(label_in(&edited, "status", "en").as_deref(), Some("State"));
    }

    #[test]
    fn label_preserved_when_only_role_changes() {
        let mut spec = custom_gadget_spec();
        put_label(&mut spec, "status", "en", "State");
        // A role-only submit (no labels_submitted) must preserve the label.
        let form = FormData::parse("role[name]=subtitle");
        let edited = build_edited_spec(&spec, &form).unwrap();
        assert_eq!(label_in(&edited, "status", "en").as_deref(), Some("State"));
    }

    #[test]
    fn label_for_unknown_source_is_ignored() {
        // The source is never editable: a label for a source that is not a
        // field is simply not written (apply_label_edits keys off real fields).
        let derived = crate::viewspec::ViewSpec::from_schema_model(&gadget_schema_model());
        let form = FormData::parse("labels_submitted=1&editing_lang=en&label[ghost_field]=Ghost");
        let edited = build_edited_spec(&derived, &form).unwrap();
        assert!(label_in(&edited, "ghost_field", "en").is_none());
        edited.validate().unwrap(); // no UnknownLabelSource — nothing was written
    }

    #[test]
    fn merge_away_prunes_orphaned_label() {
        // email has an sv label; merging it into name removes email as a field
        // → its label is pruned so labels ⊆ fields holds (validate passes).
        let mut spec = crate::viewspec::ViewSpec::from_schema_model(&gadget_schema_model());
        put_label(&mut spec, "email", "sv", "E-post");
        let form = FormData::parse("merge_submitted=1&merge[email]=name");
        let edited = build_edited_spec(&spec, &form).unwrap();
        assert!(edited.fields.iter().all(|f| f.source != "email"));
        assert!(
            label_in(&edited, "email", "sv").is_none(),
            "orphaned label pruned"
        );
        edited.validate().unwrap();
    }

    #[test]
    fn set_as_default_changes_stored_default_language() {
        let derived = crate::viewspec::ViewSpec::from_schema_model(&gadget_schema_model());
        assert_eq!(derived.default_language, "en");
        let form = FormData::parse(
            "labels_submitted=1&editing_lang=sv&set_as_default=1&label[status]=Status (sv)",
        );
        let edited = build_edited_spec(&derived, &form).unwrap();
        assert_eq!(edited.default_language, "sv"); // explicit control changed it
        assert_eq!(
            label_in(&edited, "status", "sv").as_deref(),
            Some("Status (sv)")
        );
    }

    #[test]
    fn label_switch_without_set_as_default_keeps_stored_default() {
        // Editing sv labels WITHOUT the checkbox leaves the stored default at
        // "en" (switching the editing language never changes the stored one).
        let derived = crate::viewspec::ViewSpec::from_schema_model(&gadget_schema_model());
        let form = FormData::parse("labels_submitted=1&editing_lang=sv&label[status]=Status (sv)");
        let edited = build_edited_spec(&derived, &form).unwrap();
        assert_eq!(edited.default_language, "en");
    }

    #[test]
    fn label_on_hidden_field_is_stored_harmlessly() {
        // A field can be set Hidden AND carry a label in the same save — the
        // label is stored (unused until unhidden); no validate conflict.
        let derived = crate::viewspec::ViewSpec::from_schema_model(&gadget_schema_model());
        let form = FormData::parse(
            "labels_submitted=1&editing_lang=en&role[notes]=hidden&label[notes]=Notes",
        );
        let edited = build_edited_spec(&derived, &form).unwrap();
        let notes = edited.fields.iter().find(|f| f.source == "notes").unwrap();
        assert_eq!(notes.role, FieldRole::Hidden);
        assert_eq!(label_in(&edited, "notes", "en").as_deref(), Some("Notes"));
        edited.validate().unwrap();
    }

    #[test]
    fn label_edit_composes_with_role_order_filter_merge() {
        // One Save: role + order + filter + merge + label, all persist.
        let derived = crate::viewspec::ViewSpec::from_schema_model(&gadget_schema_model());
        let form = FormData::parse(
            "merge_submitted=1&filters_submitted=1&labels_submitted=1&editing_lang=sv\
             &merge[email]=name&order[status]=0&order[name]=1&filterable[name]=1\
             &role[status]=badge&label[name]=Namn",
        );
        let edited = build_edited_spec(&derived, &form).unwrap();
        assert!(edited
            .fields
            .iter()
            .find(|f| f.source == "name")
            .unwrap()
            .merge
            .is_some());
        assert!(edited.fields.iter().all(|f| f.source != "email"));
        assert!(edited.filters.contains(&"name".to_string()));
        let order = field_order(&edited);
        let pos = |s: &str| order.iter().position(|x| x == s).unwrap();
        assert!(pos("status") < pos("name"));
        assert_eq!(label_in(&edited, "name", "sv").as_deref(), Some("Namn"));
        edited.validate().unwrap();
    }

    #[tokio::test]
    async fn edited_label_renders_as_header_through_l2() {
        // End-to-end: set default_language=sv + an sv label via the editor
        // path, save, and the L2 list render shows the Swedish header.
        let base = tmp_dir();
        let derived = crate::viewspec::ViewSpec::from_schema_model(&gadget_schema_model());
        let form = FormData::parse(
            "labels_submitted=1&editing_lang=sv&set_as_default=1&label[status]=Status (sv)",
        );
        let edited = build_edited_spec(&derived, &form).unwrap();
        save_view_spec(&base, "Gadget", &edited).unwrap();
        let html = render_gadget_layout_in(&base, Some("table"), &HashMap::new()).await;
        assert!(
            html.contains("Status (sv)"),
            "editor label renders as header:\n{html}"
        );
        assert!(html.contains("Alpha"), "data unchanged");
        std::fs::remove_dir_all(&base).ok();
    }

    #[tokio::test]
    async fn editor_prefills_inputs_for_editing_language_only() {
        // Render the editor with editing_lang=sv when the field has BOTH en
        // and sv labels → the input shows the sv value, never the en one.
        let base = tmp_dir();
        let mut spec = crate::viewspec::ViewSpec::from_schema_model(&gadget_schema_model());
        put_label(&mut spec, "status", "en", "Status (en)");
        put_label(&mut spec, "status", "sv", "Status (sv)");
        save_view_spec(&base, "Gadget", &spec).unwrap();

        let html = render_editor_in(&base, "sv").await;
        assert!(
            html.contains(r#"value="Status (sv)""#),
            "sv input prefilled:\n{html}"
        );
        assert!(
            !html.contains(r#"value="Status (en)""#),
            "en value must NOT appear in the sv editing view"
        );
        // The editing-language hidden field + the labels sentinel are present.
        assert!(html.contains(r#"name="editing_lang" value="sv""#));
        assert!(html.contains(r#"name="labels_submitted""#));
        std::fs::remove_dir_all(&base).ok();
    }

    #[tokio::test]
    async fn editor_prefill_is_strict_no_default_language_fallback() {
        // Regression: a field labelled ONLY in the view's default language
        // (sv), opened for editing in a DIFFERENT language (en), must show an
        // EMPTY input — never the sv label (which `label_for`'s default-lang
        // fallback would leak, causing a save to clobber sv as en).
        let base = tmp_dir();
        let mut spec = crate::viewspec::ViewSpec::from_schema_model(&gadget_schema_model());
        spec.default_language = "sv".to_string();
        put_label(&mut spec, "status", "sv", "Status (sv)");
        save_view_spec(&base, "Gadget", &spec).unwrap();

        let html = render_editor_in(&base, "en").await;
        assert!(
            !html.contains(r#"value="Status (sv)""#),
            "the sv label must NOT prefill the en editing view:\n{html}"
        );
        // Opening in sv DOES show it.
        let html_sv = render_editor_in(&base, "sv").await;
        assert!(html_sv.contains(r#"value="Status (sv)""#));
        std::fs::remove_dir_all(&base).ok();
    }

    // --- i18n L4a: per-user language preference + resolution -------------

    #[test]
    fn language_registry_has_endonyms_and_validates_codes() {
        assert_eq!(languages(), &[("en", "English"), ("sv", "Svenska")]);
        assert!(is_known_language("en") && is_known_language("sv"));
        assert!(!is_known_language("xx") && !is_known_language(""));
    }

    #[tokio::test]
    async fn resolve_active_language_precedence() {
        let (db, identity) = db_with_user("pref@example.com").await;
        let mut spec = crate::viewspec::ViewSpec::from_schema_model(&gadget_schema_model());
        spec.default_language = "de".to_string(); // view default (not en)

        // No preference yet → view default_language.
        assert_eq!(
            resolve_active_language(&db, Some(&identity), &spec).await,
            "de"
        );
        // Preference set → it wins over default_language.
        crate::auth::user::set_preferred_language(&db, identity.user_id, "sv")
            .await
            .unwrap();
        assert_eq!(
            resolve_active_language(&db, Some(&identity), &spec).await,
            "sv"
        );
        // Cleared ("") → back to default_language.
        crate::auth::user::set_preferred_language(&db, identity.user_id, "")
            .await
            .unwrap();
        assert_eq!(
            resolve_active_language(&db, Some(&identity), &spec).await,
            "de"
        );
        // No identity → default_language.
        assert_eq!(resolve_active_language(&db, None, &spec).await, "de");
        // No identity + empty default_language → ultimate fallback "en".
        spec.default_language = String::new();
        assert_eq!(resolve_active_language(&db, None, &spec).await, "en");
    }

    #[tokio::test]
    async fn user_preference_overrides_default_language_in_headers() {
        // The saved view's default_language is "en", but the user prefers sv,
        // and an sv label exists → the header renders Swedish for THIS user.
        let base = tmp_dir();
        let (db, identity) = db_with_user("sv-user@example.com").await;
        crate::auth::user::set_preferred_language(&db, identity.user_id, "sv")
            .await
            .unwrap();

        let mut spec = crate::viewspec::ViewSpec::from_schema_model(&gadget_schema_model());
        assert_eq!(spec.default_language, "en"); // view default is English
        put_label(&mut spec, "status", "sv", "Status (sv)");
        save_view_spec(&base, "Gadget", &spec).unwrap();

        let html = render_gadget_list_as(&base, &db, Some(&identity)).await;
        assert!(
            html.contains("Status (sv)"),
            "user's sv preference must override default_language=en:\n{html}"
        );
        assert!(html.contains("Alpha"), "data unchanged (iron rule)");
        std::fs::remove_dir_all(&base).ok();
    }

    #[tokio::test]
    async fn preference_is_per_user_not_global() {
        // Two users on the same db + same view: only the one who set sv sees
        // Swedish; the other still sees the English default.
        let base = tmp_dir();
        let (db, sv_user) = db_with_user("a@example.com").await;
        let other = crate::auth::user::create(&db, "b@example.com", "pw-12345678", "admin")
            .await
            .unwrap();
        let en_user = crate::auth::Identity {
            user_id: other.id,
            email: other.email.clone(),
            is_admin: true,
        };
        crate::auth::user::set_preferred_language(&db, sv_user.user_id, "sv")
            .await
            .unwrap();

        let mut spec = crate::viewspec::ViewSpec::from_schema_model(&gadget_schema_model());
        put_label(&mut spec, "status", "sv", "Status (sv)");
        save_view_spec(&base, "Gadget", &spec).unwrap();

        let html_sv = render_gadget_list_as(&base, &db, Some(&sv_user)).await;
        let html_en = render_gadget_list_as(&base, &db, Some(&en_user)).await;
        assert!(html_sv.contains("Status (sv)"), "sv user sees Swedish");
        assert!(
            !html_en.contains("Status (sv)"),
            "the OTHER user is unaffected (per-user, not global)"
        );
        std::fs::remove_dir_all(&base).ok();
    }

    #[tokio::test]
    async fn no_preference_renders_byte_identical_to_l3() {
        // Backward compat: a user with no preference renders exactly as before
        // L4 (active language == the view's default_language).
        let base = tmp_dir();
        let (db, identity) = db_with_user("nopref@example.com").await;
        let spec = crate::viewspec::ViewSpec::from_schema_model(&gadget_schema_model());
        save_view_spec(&base, "Gadget", &spec).unwrap();

        let with_identity = render_gadget_list_as(&base, &db, Some(&identity)).await;
        let anon = render_gadget_list_as(&base, &db, None).await;
        // Same headers either way (no pref → default_language "en" → humaniser).
        assert!(with_identity.contains(">Status</th>"));
        assert!(anon.contains(">Status</th>"));
        std::fs::remove_dir_all(&base).ok();
    }

    #[tokio::test]
    async fn set_preferred_language_round_trips_and_clears() {
        let (db, identity) = db_with_user("rt@example.com").await;
        assert_eq!(
            crate::auth::user::preferred_language(&db, identity.user_id)
                .await
                .unwrap(),
            None
        );
        crate::auth::user::set_preferred_language(&db, identity.user_id, "sv")
            .await
            .unwrap();
        assert_eq!(
            crate::auth::user::preferred_language(&db, identity.user_id)
                .await
                .unwrap()
                .as_deref(),
            Some("sv")
        );
        // Empty clears → None ("no preference").
        crate::auth::user::set_preferred_language(&db, identity.user_id, "")
            .await
            .unwrap();
        assert_eq!(
            crate::auth::user::preferred_language(&db, identity.user_id)
                .await
                .unwrap(),
            None
        );
    }

    // --- i18n L4b: switcher options on current_user ----------------------

    #[tokio::test]
    async fn user_view_builds_switcher_options() {
        let (db, identity) = db_with_user("switcher@example.com").await;

        // No preference → "Default" selected; the endonyms are present and
        // unselected; values are ISO codes (stored), labels are endonyms.
        let uv = user_view(&db, Some(&identity)).await.unwrap();
        let default = uv
            .language_options
            .iter()
            .find(|o| o.value.is_empty())
            .unwrap();
        assert_eq!(default.label, "Default");
        assert!(default.selected, "no pref → Default selected");
        let en = uv
            .language_options
            .iter()
            .find(|o| o.value == "en")
            .unwrap();
        let sv = uv
            .language_options
            .iter()
            .find(|o| o.value == "sv")
            .unwrap();
        assert_eq!(
            (en.label.as_str(), sv.label.as_str()),
            ("English", "Svenska")
        );
        assert!(!en.selected && !sv.selected);

        // Preference set → that endonym is selected, Default no longer is.
        crate::auth::user::set_preferred_language(&db, identity.user_id, "sv")
            .await
            .unwrap();
        let uv2 = user_view(&db, Some(&identity)).await.unwrap();
        assert!(
            uv2.language_options
                .iter()
                .find(|o| o.value == "sv")
                .unwrap()
                .selected
        );
        assert!(
            !uv2.language_options
                .iter()
                .find(|o| o.value.is_empty())
                .unwrap()
                .selected
        );

        // No identity → no user view (login page renders no switcher).
        assert!(user_view(&db, None).await.is_none());
    }

    // --- i18n enum/value display labels (admin render) -------------------

    fn put_value_label(
        spec: &mut crate::viewspec::ViewSpec,
        source: &str,
        value: &str,
        lang: &str,
        text: &str,
    ) {
        spec.value_labels
            .entry(source.to_string())
            .or_default()
            .entry(value.to_string())
            .or_default()
            .insert(lang.to_string(), text.to_string());
    }

    #[tokio::test]
    async fn status_value_label_translates_text_keeps_color() {
        // gadget status = 'active'. Translate the pill TEXT for sv; the pill
        // COLOUR must still derive from the English value "active".
        let base = tmp_dir();
        let mut spec = crate::viewspec::ViewSpec::from_schema_model(&gadget_schema_model());
        spec.default_language = "sv".to_string();
        put_value_label(&mut spec, "status", "active", "sv", "Aktiv");
        spec.validate().unwrap();
        save_view_spec(&base, "Gadget", &spec).unwrap();

        let html = render_gadget_layout_in(&base, Some("table"), &HashMap::new()).await;
        assert!(html.contains("Aktiv"), "status text translated:\n{html}");
        assert!(!html.contains(">Active</span>"), "English label replaced");
        // Iron rule: the colour class still keys off the English "active".
        assert!(
            html.contains(status_pill_color("active")),
            "pill colour keyed off the English value, not the translation"
        );
        std::fs::remove_dir_all(&base).ok();
    }

    #[tokio::test]
    async fn plain_value_label_translates_cell_text() {
        // gadget notes = 'MY-META-NOTE' (a plain, non-status cell). The lookup
        // key is the lowercased stored value.
        let base = tmp_dir();
        let mut spec = crate::viewspec::ViewSpec::from_schema_model(&gadget_schema_model());
        spec.default_language = "sv".to_string();
        put_value_label(&mut spec, "notes", "my-meta-note", "sv", "Översatt");
        spec.validate().unwrap();
        save_view_spec(&base, "Gadget", &spec).unwrap();

        let html = render_gadget_layout_in(&base, Some("table"), &HashMap::new()).await;
        assert!(html.contains("Översatt"), "plain cell translated:\n{html}");
        assert!(
            !html.contains("MY-META-NOTE"),
            "English value text replaced"
        );
        std::fs::remove_dir_all(&base).ok();
    }

    #[tokio::test]
    async fn unlabelled_value_renders_as_before() {
        // No value_labels → status pill + plain cell render exactly as today.
        let base = tmp_dir();
        let spec = crate::viewspec::ViewSpec::from_schema_model(&gadget_schema_model());
        save_view_spec(&base, "Gadget", &spec).unwrap();
        let html = render_gadget_layout_in(&base, Some("table"), &HashMap::new()).await;
        assert!(
            html.contains(">Active</span>"),
            "status English label unchanged"
        );
        assert!(html.contains("MY-META-NOTE"), "plain value unchanged");
        std::fs::remove_dir_all(&base).ok();
    }

    #[tokio::test]
    async fn user_preference_drives_value_label_language() {
        // default_language is "en" (no en value label), but the user prefers
        // sv and an sv value label exists → the pill shows Swedish for them.
        let base = tmp_dir();
        let (db, identity) = db_with_user("ev@example.com").await;
        crate::auth::user::set_preferred_language(&db, identity.user_id, "sv")
            .await
            .unwrap();
        let mut spec = crate::viewspec::ViewSpec::from_schema_model(&gadget_schema_model());
        assert_eq!(spec.default_language, "en");
        put_value_label(&mut spec, "status", "active", "sv", "Aktiv");
        save_view_spec(&base, "Gadget", &spec).unwrap();

        let html = render_gadget_list_as(&base, &db, Some(&identity)).await;
        assert!(
            html.contains("Aktiv"),
            "user's sv preference drives the value label:\n{html}"
        );
        std::fs::remove_dir_all(&base).ok();
    }

    // --- i18n value labels: editor editing -------------------------------

    fn vlabel(
        spec: &crate::viewspec::ViewSpec,
        src: &str,
        val: &str,
        lang: &str,
    ) -> Option<String> {
        spec.value_labels
            .get(src)
            .and_then(|bv| bv.get(val))
            .and_then(|bl| bl.get(lang))
            .cloned()
    }

    #[test]
    fn value_label_edit_persists_and_clears() {
        let spec = crate::viewspec::ViewSpec::from_schema_model(&gadget_schema_model());
        let form = FormData::parse(
            "value_labels_submitted=1&editing_lang=sv\
             &value_keys[status]=active&value_label[status][active]=Aktiv",
        );
        let edited = build_edited_spec(&spec, &form).unwrap();
        assert_eq!(
            vlabel(&edited, "status", "active", "sv").as_deref(),
            Some("Aktiv")
        );
        edited.validate().unwrap();

        // Clearing the input removes the entry (not "") → pruned to empty.
        let form2 = FormData::parse(
            "value_labels_submitted=1&editing_lang=sv\
             &value_keys[status]=active&value_label[status][active]=",
        );
        let edited2 = build_edited_spec(&edited, &form2).unwrap();
        assert!(
            edited2.value_labels.is_empty(),
            "cleared value label pruned"
        );
        assert!(!edited2.to_pretty_json().unwrap().contains("value_labels"));
    }

    #[test]
    fn value_label_edit_no_cross_language_clobber() {
        let mut spec = crate::viewspec::ViewSpec::from_schema_model(&gadget_schema_model());
        put_value_label(&mut spec, "status", "active", "en", "Active!");
        let form = FormData::parse(
            "value_labels_submitted=1&editing_lang=sv\
             &value_keys[status]=active&value_label[status][active]=Aktiv",
        );
        let edited = build_edited_spec(&spec, &form).unwrap();
        assert_eq!(
            vlabel(&edited, "status", "active", "en").as_deref(),
            Some("Active!")
        );
        assert_eq!(
            vlabel(&edited, "status", "active", "sv").as_deref(),
            Some("Aktiv")
        );
    }

    #[test]
    fn value_label_preserved_on_non_value_save() {
        // A submit without the value_labels sentinel leaves value labels intact.
        let mut spec = crate::viewspec::ViewSpec::from_schema_model(&gadget_schema_model());
        put_value_label(&mut spec, "status", "active", "en", "Active!");
        let edited = build_edited_spec(&spec, &FormData::parse("role[name]=subtitle")).unwrap();
        assert_eq!(
            vlabel(&edited, "status", "active", "en").as_deref(),
            Some("Active!")
        );
    }

    #[test]
    fn value_label_pruned_when_field_merged_away() {
        // status has a value label; merging it into name removes status as a
        // field → its value labels are pruned (labels ⊆ fields → validate ok).
        let mut spec = crate::viewspec::ViewSpec::from_schema_model(&gadget_schema_model());
        put_value_label(&mut spec, "status", "active", "sv", "Aktiv");
        let edited = build_edited_spec(
            &spec,
            &FormData::parse("merge_submitted=1&merge[status]=name"),
        )
        .unwrap();
        assert!(edited.fields.iter().all(|f| f.source != "status"));
        assert!(
            !edited.value_labels.contains_key("status"),
            "orphaned value labels pruned"
        );
        edited.validate().unwrap();
    }

    #[test]
    fn value_label_composes_with_field_label_in_one_save() {
        let spec = crate::viewspec::ViewSpec::from_schema_model(&gadget_schema_model());
        let form = FormData::parse(
            "labels_submitted=1&value_labels_submitted=1&editing_lang=sv\
             &label[status]=Status (sv)\
             &value_keys[status]=active&value_label[status][active]=Aktiv",
        );
        let edited = build_edited_spec(&spec, &form).unwrap();
        // field label …
        assert_eq!(
            edited
                .labels
                .get("status")
                .and_then(|m| m.get("sv"))
                .map(String::as_str),
            Some("Status (sv)")
        );
        // … and value label, together.
        assert_eq!(
            vlabel(&edited, "status", "active", "sv").as_deref(),
            Some("Aktiv")
        );
        edited.validate().unwrap();
    }

    #[tokio::test]
    async fn editor_discovers_status_values_and_inputs() {
        // gadget_db stores status='active' → the editor auto-lists it with an
        // input + the hidden value_keys + the sentinel + a normalized placeholder.
        let base = tmp_dir();
        let html = render_editor_in(&base, "sv").await;
        assert!(
            html.contains(r#"name="value_label[status][active]""#),
            "value input for the discovered status value:\n{html}"
        );
        assert!(
            html.contains(r#"name="value_keys[status]""#),
            "hidden value_keys list"
        );
        assert!(
            html.contains(r#"name="value_labels_submitted""#),
            "sentinel"
        );
        assert!(
            html.contains(r#"placeholder="Active""#),
            "normalized default placeholder"
        );
    }

    #[tokio::test]
    async fn editor_value_label_prefill_is_strict_for_editing_language() {
        // status=active labelled in BOTH en and sv. Editing in sv shows the sv
        // value only (never the en one); editing in en shows the en value.
        let base = tmp_dir();
        let mut spec = crate::viewspec::ViewSpec::from_schema_model(&gadget_schema_model());
        put_value_label(&mut spec, "status", "active", "en", "Active(EN)");
        put_value_label(&mut spec, "status", "active", "sv", "Aktiv(SV)");
        save_view_spec(&base, "Gadget", &spec).unwrap();

        let html_sv = render_editor_in(&base, "sv").await;
        assert!(
            html_sv.contains(r#"value="Aktiv(SV)""#),
            "sv value prefilled:\n{html_sv}"
        );
        assert!(
            !html_sv.contains(r#"value="Active(EN)""#),
            "en value must NOT appear in the sv editing view"
        );
        let html_en = render_editor_in(&base, "en").await;
        assert!(html_en.contains(r#"value="Active(EN)""#));
        std::fs::remove_dir_all(&base).ok();
    }

    #[tokio::test]
    async fn editor_discovers_low_cardinality_non_status_string_field() {
        // gadget columns: name(Title), email(Subtitle), status(Badge,status),
        // notes(Meta), password_hash(Hidden). Seed so notes is HIGH-cardinality
        // (>12 distinct → not an enum) and email is LOW-cardinality (2 distinct
        // → enum-like, discovered).
        let base = tmp_dir();
        let db = Db::memory().await.unwrap();
        db.execute(
            "CREATE TABLE gadgets (id INTEGER PRIMARY KEY, name TEXT, email TEXT, status TEXT, notes TEXT, password_hash TEXT)",
        )
        .await
        .unwrap();
        for i in 0..15 {
            let email = if i % 2 == 0 { "a@x" } else { "b@x" }; // 2 distinct
            db.execute(&format!(
                "INSERT INTO gadgets (name,email,status,notes,password_hash) \
                 VALUES ('n{i}','{email}','active','note{i}','h{i}')"
            ))
            .await
            .unwrap();
        }

        let html = render_editor_with_db(&base, &db, "en").await;
        // email — String, Subtitle, 2 distinct → enum-like → discovered.
        assert!(
            html.contains(r#"name="value_label[email]["#),
            "low-cardinality non-status string field discovered:\n{html}"
        );
        // notes — String, 15 distinct (> ENUM_CAP) → free-text → NOT offered.
        assert!(
            !html.contains(r#"name="value_label[notes]["#),
            "high-cardinality string field must not be offered"
        );
        // name — Title (identity, renders via the primary branch) → excluded.
        assert!(
            !html.contains(r#"name="value_label[name]["#),
            "Title field excluded"
        );
        // password_hash — Hidden (never renders) → excluded.
        assert!(
            !html.contains(r#"name="value_label[password_hash]["#),
            "Hidden field excluded"
        );
        // id — integer (FK ids are ints too) → excluded by type.
        assert!(
            !html.contains(r#"name="value_label[id]["#),
            "non-String field excluded by type"
        );
        // status — status-shaped → still auto-discovered.
        assert!(html.contains(r#"name="value_label[status][active]""#));
        std::fs::remove_dir_all(&base).ok();
    }

    #[tokio::test]
    async fn non_status_enum_label_persists_and_renders() {
        // End-to-end: a discovered non-status value label is editable AND
        // renders through the plain-cell branch.
        let base = tmp_dir();
        let mut spec = crate::viewspec::ViewSpec::from_schema_model(&gadget_schema_model());
        spec.default_language = "sv".to_string();
        // notes value 'my-meta-note' (gadget_db single row) → translate.
        put_value_label(&mut spec, "notes", "my-meta-note", "sv", "Anteckning");
        spec.validate().unwrap();
        save_view_spec(&base, "Gadget", &spec).unwrap();

        // Editor offers it (gadget_db has notes='MY-META-NOTE', 1 distinct).
        let editor = render_editor_in(&base, "sv").await;
        assert!(editor.contains(r#"name="value_label[notes][my-meta-note]""#));
        assert!(editor.contains(r#"value="Anteckning""#), "prefilled in sv");

        // And it renders in the list (plain cell).
        let html = render_gadget_layout_in(&base, Some("table"), &HashMap::new()).await;
        assert!(
            html.contains("Anteckning"),
            "non-status value translated in list:\n{html}"
        );
        assert!(
            !html.contains("MY-META-NOTE"),
            "English value text replaced"
        );
        std::fs::remove_dir_all(&base).ok();
    }

    // --- DW-1: ViewSpec.filters → live list filter bar -------------------

    #[test]
    fn classify_filters_honors_view_filters_and_skips_empty() {
        let entry = gadget_entry();
        let model = LegacyEntryModel::new(&entry);
        let mut raw = HashMap::new();
        raw.insert("notes".to_string(), "x".to_string());
        raw.insert("name".to_string(), String::new()); // empty → never applied

        // The view declares `notes` a filter → it's accepted even though the
        // gadget model didn't mark it filterable.
        let (eq, like) = classify_filters(&model, &raw, &["notes".to_string()], &[]);
        assert!(
            eq.contains_key("notes") || like.contains_key("notes"),
            "a ViewSpec.filters field is applied"
        );
        assert!(
            !eq.contains_key("name") && !like.contains_key("name"),
            "empty filter value ('All') is never applied"
        );

        // Without the view declaring it (and not macro-filterable) → dropped.
        let (eq2, like2) = classify_filters(&model, &raw, &[], &[]);
        assert!(
            !eq2.contains_key("notes") && !like2.contains_key("notes"),
            "a field neither view-declared nor macro-filterable is dropped"
        );

        // exact_match forces `=` (eq) instead of `LIKE` for a dropdown field.
        let (eq3, like3) =
            classify_filters(&model, &raw, &["notes".to_string()], &["notes".to_string()]);
        assert!(
            eq3.contains_key("notes") && !like3.contains_key("notes"),
            "a dropdown (exact_match) filter matches exactly, not by substring"
        );
    }

    #[test]
    fn account_role_display_and_initials() {
        assert_eq!(account_role_display("admin").0, "Administrator"); // legacy admin → SuperAdmin
        assert_eq!(account_role_display("admin").1, "admin");
        assert_eq!(account_role_display("editor").0, "Editor");
        assert_eq!(account_role_display("editor").1, "developer");
        assert_eq!(account_role_display("viewer").0, "Viewer");
        assert_eq!(account_role_display("nonsense").0, "Viewer"); // unknown → safe view-only
        assert_eq!(account_initials("admin@bookflow.local"), "AD");
        assert_eq!(account_initials("x@y"), "X");
    }

    #[tokio::test]
    async fn account_view_reflects_real_role_permissions() {
        let db = Db::memory().await.unwrap();
        crate::auth::ensure_core_tables(&db).await.unwrap();
        let admin = crate::auth::user::create(&db, "a@x.com", "pw-12345678", "admin")
            .await
            .unwrap();
        let av = build_account_view(&db, &admin).await;
        assert_eq!(av.role_name, "Administrator");
        let allowed: Vec<&str> = av
            .perms
            .iter()
            .filter(|p| p.allowed)
            .map(|p| p.label.as_str())
            .collect();
        assert!(allowed.contains(&"Delete records") && allowed.contains(&"Manage users & roles"));
        assert!(av
            .perms
            .iter()
            .any(|p| p.label == "Evolve schema" && !p.allowed));
        assert!(av
            .roles
            .iter()
            .any(|r| r.name == "Administrator" && r.is_you));

        // A viewer: read-only, no create/edit/delete.
        let viewer = crate::auth::user::create(&db, "v@x.com", "pw-12345678", "user")
            .await
            .unwrap();
        db.execute(&format!(
            "UPDATE rustio_users SET role = 'viewer' WHERE id = {}",
            viewer.id
        ))
        .await
        .unwrap();
        let reloaded = crate::auth::user::find_by_id(&db, viewer.id)
            .await
            .unwrap()
            .unwrap();
        let vv = build_account_view(&db, &reloaded).await;
        assert_eq!(vv.role_name, "Viewer");
        assert!(vv
            .perms
            .iter()
            .any(|p| p.label == "View records" && p.allowed));
        assert!(vv
            .perms
            .iter()
            .any(|p| p.label == "Create & edit" && !p.allowed));
        assert!(vv
            .perms
            .iter()
            .any(|p| p.label == "Delete records" && !p.allowed));
        assert!(vv.roles.iter().any(|r| r.name == "Viewer" && r.is_you));
    }

    #[tokio::test]
    async fn rbac_gates_list_actions_by_role() {
        // gadgets is an app table → Admin gets full CRUD, Viewer view-only.
        let base = tmp_dir();
        let db = Db::memory().await.unwrap();
        crate::auth::ensure_core_tables(&db).await.unwrap();
        seed_gadgets(&db).await;
        let admin = crate::auth::user::create(&db, "adm@x.com", "pw-12345678", "admin")
            .await
            .unwrap();
        let viewer = crate::auth::user::create(&db, "view@x.com", "pw-12345678", "user")
            .await
            .unwrap();
        // Promote the second user to the explicit Viewer role (the create API
        // only mints admin/user; richer roles are assigned directly).
        db.execute(&format!(
            "UPDATE rustio_users SET role = 'viewer' WHERE id = {}",
            viewer.id
        ))
        .await
        .unwrap();
        let id = |u: &crate::auth::User| crate::auth::Identity {
            user_id: u.id,
            email: u.email.clone(),
            is_admin: true,
        };

        let admin_html = render_gadget_list_as(&base, &db, Some(&id(&admin))).await;
        let viewer_html = render_gadget_list_as(&base, &db, Some(&id(&viewer))).await;
        // Admin → create allowed; Viewer → not (the "+ Add" control is gated
        // on `permissions.create`).
        assert!(
            admin_html.contains("+ Add"),
            "admin can create:\n{admin_html}"
        );
        assert!(!viewer_html.contains("+ Add"), "viewer cannot create");
        // Both can view the rows.
        assert!(admin_html.contains("Alpha") && viewer_html.contains("Alpha"));
        std::fs::remove_dir_all(&base).ok();
    }

    #[tokio::test]
    async fn list_renders_filter_control_for_view_filter() {
        // gadget's derived spec has filters=["status"]; status is low-card
        // (1 distinct: 'active') → a Select dropdown control renders.
        let base = tmp_dir();
        let html = render_gadget_layout_in(&base, Some("table"), &HashMap::new()).await;
        assert!(
            html.contains(r#"name="status" data-filter"#),
            "a filter control for the view's status filter renders:\n{html}"
        );
        assert!(
            html.contains(r#"<option value="active""#),
            "the distinct value is an option"
        );
        // No control for a non-filter field.
        assert!(!html.contains(r#"name="email" data-filter"#));
    }

    #[tokio::test]
    async fn view_filter_applies_and_excludes_on_live_list() {
        // The filter now actually filters rows on the live list path.
        let base = tmp_dir();
        let mut matching = HashMap::new();
        matching.insert("status".to_string(), "active".to_string());
        let hit = render_gadget_layout_in(&base, Some("table"), &matching).await;
        assert!(hit.contains("Alpha"), "matching filter keeps the row");

        let mut nonmatching = HashMap::new();
        nonmatching.insert("status".to_string(), "archived".to_string());
        let miss = render_gadget_layout_in(&base, Some("table"), &nonmatching).await;
        assert!(
            !miss.contains("Alpha"),
            "a non-matching filter excludes the row:\n{miss}"
        );
        std::fs::remove_dir_all(&base).ok();
    }

    #[tokio::test]
    async fn filter_option_uses_value_label_display() {
        // The dropdown shows the translated value label but submits the English
        // token (iron rule), composing with the value-label work.
        let base = tmp_dir();
        let mut spec = crate::viewspec::ViewSpec::from_schema_model(&gadget_schema_model());
        spec.default_language = "sv".to_string();
        put_value_label(&mut spec, "status", "active", "sv", "Aktiv");
        save_view_spec(&base, "Gadget", &spec).unwrap();

        let html = render_gadget_layout_in(&base, Some("table"), &HashMap::new()).await;
        // Option value = English token; label = translation.
        assert!(
            html.contains(r#"<option value="active">Aktiv</option>"#),
            "{html}"
        );
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn load_saved_view_reads_valid_skips_missing_and_invalid() {
        let base = tmp_dir();
        // Missing file → None.
        assert!(load_saved_view(&base, "Widget").is_none());
        // Valid file → Some, parsed.
        std::fs::write(
            view_file_path(&base, "Widget"),
            r#"{"version":1,"model":"Widget","layout":"cards",
               "fields":[{"source":"email","role":"title"}],"filters":[]}"#,
        )
        .unwrap();
        let spec = load_saved_view(&base, "Widget").expect("valid saved view loads");
        assert_eq!(spec.layout, crate::viewspec::ViewLayout::Cards);
        assert_eq!(spec.fields.len(), 1);
        // Invalid JSON → None (never an error).
        std::fs::write(view_file_path(&base, "Widget"), "{ not json").unwrap();
        assert!(load_saved_view(&base, "Widget").is_none());
        std::fs::remove_dir_all(&base).ok();
    }

    // --- Phase 7: ?layout= switcher (LIVE path) --------------------------
    //
    // Every test drives `list_render` end-to-end with a real layout param,
    // proving the running admin behaviour per layout.

    const GADGET_FIELDS: &[AdminField] = &[
        AdminField {
            name: "id",
            ty: FieldType::I64,
            editable: false,
            nullable: false,
            relation: None,
        },
        AdminField {
            name: "name",
            ty: FieldType::String,
            editable: true,
            nullable: false,
            relation: None,
        },
        AdminField {
            name: "email",
            ty: FieldType::String,
            editable: true,
            nullable: false,
            relation: None,
        },
        AdminField {
            name: "status",
            ty: FieldType::String,
            editable: true,
            nullable: false,
            relation: None,
        },
        AdminField {
            name: "notes",
            ty: FieldType::String,
            editable: true,
            nullable: false,
            relation: None,
        },
        AdminField {
            name: "password_hash",
            ty: FieldType::String,
            editable: false,
            nullable: false,
            relation: None,
        },
    ];

    fn gadget_entry() -> AdminEntry {
        AdminEntry {
            admin_name: "gadgets",
            display_name: "Gadget",
            singular_name: "Gadget",
            table: "gadgets",
            fields: GADGET_FIELDS,
            core: false,
        }
    }

    async fn gadget_db() -> Db {
        let db = Db::memory().await.unwrap();
        sqlx::query(
            "CREATE TABLE gadgets (id INTEGER PRIMARY KEY, name TEXT, email TEXT, status TEXT, notes TEXT, password_hash TEXT)",
        )
        .execute(db.pool())
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO gadgets (name,email,status,notes,password_hash) VALUES ('Alpha','alpha@x.example','active','MY-META-NOTE','topsecret-xyz')",
        )
        .execute(db.pool())
        .await
        .unwrap();
        db
    }

    /// Render the gadget list through the live `list_render` for a given
    /// `?layout=` value and filter set.
    async fn render_gadget_layout(
        layout: Option<&str>,
        filters: &HashMap<String, String>,
    ) -> String {
        // Fresh empty base → no saved view → derived default (Table-first),
        // isolating these from any saved-layout state.
        render_gadget_layout_in(&tmp_dir(), layout, filters).await
    }

    async fn render_gadget_layout_in(
        base: &std::path::Path,
        layout: Option<&str>,
        filters: &HashMap<String, String>,
    ) -> String {
        let db = gadget_db().await;
        let entry = gadget_entry();
        let model = LegacyEntryModel::new(&entry);
        let registry = registry_empty();
        let legacy = [entry.clone()];
        list_render(
            base,
            &db,
            &registry,
            &legacy,
            &model,
            Some(&entry),
            None,
            1,
            filters,
            None,
            None,
            layout,
            None,
            None,
        )
        .await
    }

    /// Render the composition editor for the Gadget model in a given editing
    /// language (i18n L3). Mirrors `render_gadget_layout_in`.
    async fn render_editor_in(base: &std::path::Path, editing_lang: &str) -> String {
        let db = gadget_db().await;
        render_editor_with_db(base, &db, editing_lang).await
    }

    /// Like `render_editor_in` but against a caller-supplied db (so a test can
    /// seed many rows to exercise enum-cardinality discovery).
    async fn render_editor_with_db(base: &std::path::Path, db: &Db, editing_lang: &str) -> String {
        let entry = gadget_entry();
        let model = LegacyEntryModel::new(&entry);
        let registry = registry_empty();
        let legacy = [entry.clone()];
        view_editor_render(
            base,
            db,
            &registry,
            &legacy,
            &model,
            None,
            None,
            "/admin/gadgets",
            None,
            editing_lang,
        )
        .await
    }

    /// Render the Gadget list against a CALLER-SUPPLIED db (so a user row +
    /// preference set on it is visible) for a given identity (i18n L4a).
    async fn render_gadget_list_as(
        base: &std::path::Path,
        db: &Db,
        identity: Option<&crate::auth::Identity>,
    ) -> String {
        let entry = gadget_entry();
        let model = LegacyEntryModel::new(&entry);
        let registry = registry_empty();
        let legacy = [entry.clone()];
        list_render(
            base,
            db,
            &registry,
            &legacy,
            &model,
            Some(&entry),
            None,
            1,
            &HashMap::new(),
            None,
            None,
            Some("table"),
            identity,
            None,
        )
        .await
    }

    /// Insert the gadgets fixture into an arbitrary db (gadget_db makes a fresh
    /// in-memory one; here we reuse a db that also has the core user tables).
    async fn seed_gadgets(db: &Db) {
        db.execute(
            "CREATE TABLE gadgets (id INTEGER PRIMARY KEY, name TEXT, email TEXT, status TEXT, notes TEXT, password_hash TEXT)",
        )
        .await
        .unwrap();
        db.execute(
            "INSERT INTO gadgets (name,email,status,notes,password_hash) VALUES ('Alpha','alpha@x.example','active','MY-META-NOTE','topsecret-xyz')",
        )
        .await
        .unwrap();
    }

    /// A db with core auth tables + the gadgets fixture + one user, returning
    /// the user's identity.
    async fn db_with_user(email: &str) -> (Db, crate::auth::Identity) {
        let db = Db::memory().await.unwrap();
        crate::auth::ensure_core_tables(&db).await.unwrap();
        seed_gadgets(&db).await;
        let user = crate::auth::user::create(&db, email, "pw-12345678", "admin")
            .await
            .unwrap();
        let identity = crate::auth::Identity {
            user_id: user.id,
            email: user.email.clone(),
            is_admin: true,
        };
        (db, identity)
    }

    #[tokio::test]
    async fn compact_layout_shows_only_title_and_badge_live() {
        // Compact = Title (name) + Badge (status). Subtitle (email) and
        // Meta (notes) must NOT appear.
        let html = render_gadget_layout(Some("compact"), &HashMap::new()).await;
        assert!(html.contains("Alpha"), "title missing in compact: {html}");
        assert!(html.contains("rio-pill"), "badge pill missing in compact");
        assert!(
            !html.contains("alpha@x.example"),
            "subtitle (email) should not appear in compact"
        );
        assert!(
            !html.contains("MY-META-NOTE"),
            "meta (notes) should not appear in compact"
        );
        assert!(html.contains("rio-compact-row"), "compact markup missing");
    }

    #[tokio::test]
    async fn list_layout_drops_meta_live() {
        // List = Title + Subtitle + Badge + Timestamp; Meta (notes) dropped.
        let html = render_gadget_layout(Some("list"), &HashMap::new()).await;
        assert!(html.contains("Alpha"), "title missing in list");
        assert!(html.contains("alpha@x.example"), "subtitle missing in list");
        assert!(html.contains("rio-pill"), "badge missing in list");
        assert!(
            !html.contains("MY-META-NOTE"),
            "meta (notes) should be dropped in list: {html}"
        );
        assert!(html.contains("rio-list-item"), "list markup missing");
    }

    #[tokio::test]
    async fn cards_layout_includes_meta_and_card_markup_live() {
        // Cards = every visible role (same set as Table), arranged as cards.
        let html = render_gadget_layout(Some("cards"), &HashMap::new()).await;
        for needle in ["Alpha", "alpha@x.example", "MY-META-NOTE"] {
            assert!(html.contains(needle), "cards missing {needle}");
        }
        assert!(html.contains("rio-card-item"), "card markup missing");
        assert!(html.contains("rio-pill"), "badge pill missing in cards");
    }

    #[tokio::test]
    async fn hidden_secret_absent_in_all_four_layouts_live() {
        for layout in [
            None,
            Some("table"),
            Some("list"),
            Some("cards"),
            Some("compact"),
        ] {
            let html = render_gadget_layout(layout, &HashMap::new()).await;
            assert!(
                !html.contains("topsecret-xyz"),
                "hidden secret leaked in layout {layout:?}"
            );
            assert!(
                !html.to_lowercase().contains("password_hash"),
                "hidden column leaked in layout {layout:?}"
            );
        }
    }

    #[tokio::test]
    async fn table_layout_unchanged_no_regression_live() {
        // The switcher must ADD layouts without altering the Table path:
        // no-param and ?layout=table render byte-identical HTML, and an
        // unknown value falls back to that same Table output.
        let none = render_gadget_layout(None, &HashMap::new()).await;
        let table = render_gadget_layout(Some("table"), &HashMap::new()).await;
        let banana = render_gadget_layout(Some("banana"), &HashMap::new()).await;
        assert_eq!(none, table, "no-param and ?layout=table must be identical");
        assert_eq!(banana, table, "unknown layout must fall back to Table");

        // And the Table content is the Phase-6 shape: a table, id hidden,
        // pill present, Meta column present (Table shows Meta).
        assert!(
            table.contains("<table class=\"rio-table\">"),
            "table markup missing"
        );
        assert!(
            !table.to_lowercase().contains(">id</th>") && !table.contains("rio-cell-id"),
            "id column should be hidden in Table"
        );
        assert!(table.contains("rio-pill"), "status pill missing in Table");
        assert!(
            table.contains("MY-META-NOTE"),
            "Meta column should show in Table"
        );
    }

    #[tokio::test]
    async fn layout_toggle_links_preserve_filter_live() {
        // Switching layout must not drop an active filter: the switcher's
        // hrefs carry the current filter param.
        let mut filters = HashMap::new();
        filters.insert("status".to_string(), "active".to_string());
        let html = render_gadget_layout(Some("table"), &filters).await;
        // A switch-to-cards link that keeps the status filter.
        assert!(
            html.contains("layout=cards") && html.contains("status=active"),
            "layout toggle dropped the active filter: {html}"
        );
        assert!(
            html.contains("rio-layout-switch"),
            "layout switcher missing"
        );
    }

    #[tokio::test]
    async fn fk_label_preserved_in_cards_layout_live() {
        // FK linked labels (a Meta FK column) must still render in a
        // non-Table layout that includes the cell.
        let db = Db::memory().await.unwrap();
        sqlx::query("CREATE TABLE customers (id INTEGER PRIMARY KEY, name TEXT)")
            .execute(db.pool())
            .await
            .unwrap();
        sqlx::query("INSERT INTO customers (id, name) VALUES (1, 'Acme Corp')")
            .execute(db.pool())
            .await
            .unwrap();
        sqlx::query("CREATE TABLE orders (id INTEGER PRIMARY KEY, code TEXT, customer_id INTEGER)")
            .execute(db.pool())
            .await
            .unwrap();
        sqlx::query("INSERT INTO orders (code, customer_id) VALUES ('OR-1', 1)")
            .execute(db.pool())
            .await
            .unwrap();

        const ORDER_FIELDS: &[AdminField] = &[
            AdminField {
                name: "id",
                ty: FieldType::I64,
                editable: false,
                nullable: false,
                relation: None,
            },
            AdminField {
                name: "code",
                ty: FieldType::String,
                editable: true,
                nullable: false,
                relation: None,
            },
            AdminField {
                name: "customer_id",
                ty: FieldType::I64,
                editable: true,
                nullable: false,
                relation: Some(crate::admin::AdminRelation {
                    kind: crate::schema::RelationKind::BelongsTo,
                    model: "Customer",
                    display_field: Some("name"),
                }),
            },
        ];
        const CUSTOMER_FIELDS: &[AdminField] = &[
            AdminField {
                name: "id",
                ty: FieldType::I64,
                editable: false,
                nullable: false,
                relation: None,
            },
            AdminField {
                name: "name",
                ty: FieldType::String,
                editable: true,
                nullable: false,
                relation: None,
            },
        ];
        let orders_entry = AdminEntry {
            admin_name: "orders",
            display_name: "Order",
            singular_name: "Order",
            table: "orders",
            fields: ORDER_FIELDS,
            core: false,
        };
        let customers_entry = AdminEntry {
            admin_name: "customers",
            display_name: "Customer",
            singular_name: "Customer",
            table: "customers",
            fields: CUSTOMER_FIELDS,
            core: false,
        };
        let model = LegacyEntryModel::new(&orders_entry);
        let registry = registry_empty();
        let legacy = [orders_entry.clone(), customers_entry];
        let filters = HashMap::new();

        let html = list_render(
            std::path::Path::new("."),
            &db,
            &registry,
            &legacy,
            &model,
            Some(&orders_entry),
            None,
            1,
            &filters,
            None,
            None,
            Some("cards"),
            None,
            None,
        )
        .await;

        assert!(
            html.contains("Acme Corp"),
            "FK label missing in cards: {html}"
        );
        assert!(
            html.contains(r#"href="/admin/customers/1""#),
            "FK link missing in cards"
        );
        assert!(html.contains("rio-card-item"), "cards markup missing");
    }

    // --- Phase 8: persist per-model layout default -----------------------
    //
    // The write logic (`save_layout_default`), the handler's gates
    // (`require_csrf`, `admin_guard`), the `_return` sanitizer, and the
    // precedence resolver are tested directly — each is the real code the
    // POST handler / `list_render` invoke. `Request` has no public test
    // constructor, so the full HTTP handler glue is proven by the live
    // bookflow proof (see the phase notes), not a synthetic Request.

    fn gadget_schema_model() -> crate::schema::SchemaModel {
        let entry = gadget_entry();
        let model = LegacyEntryModel::new(&entry);
        schema_model_from_ui("Gadget", &model.fields())
    }

    #[test]
    fn save_layout_default_creates_file_when_none_exists() {
        use crate::viewspec::ViewLayout;
        let base = tmp_dir();
        assert!(
            load_saved_view(&base, "Gadget").is_none(),
            "precondition: no file"
        );

        save_layout_default(&base, "Gadget", &gadget_schema_model(), ViewLayout::Cards).unwrap();

        // File now exists with the chosen layout, derived for the rest.
        let spec = load_saved_view(&base, "Gadget").expect("file created");
        assert_eq!(spec.layout, ViewLayout::Cards);
        assert_eq!(spec.model, "Gadget");
        assert!(!spec.fields.is_empty(), "derived fields present");
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn save_layout_default_changes_only_layout_on_existing_view() {
        use crate::viewspec::ViewLayout;
        let base = tmp_dir();
        // A custom saved view with bespoke fields/roles/filters + List layout.
        let custom = r#"{
  "version": 1,
  "model": "Gadget",
  "layout": "list",
  "fields": [
    { "source": "name", "role": "title" },
    { "source": "status", "role": "badge", "filterable": true }
  ],
  "filters": ["status"]
}
"#;
        std::fs::write(view_file_path(&base, "Gadget"), custom).unwrap();
        let before = load_saved_view(&base, "Gadget").unwrap();

        save_layout_default(&base, "Gadget", &gadget_schema_model(), ViewLayout::Cards).unwrap();

        let after = load_saved_view(&base, "Gadget").unwrap();
        // ONLY layout changed; everything else byte-identical.
        assert_eq!(after.layout, ViewLayout::Cards);
        assert_eq!(after.version, before.version);
        assert_eq!(after.model, before.model);
        assert_eq!(after.filters, before.filters);
        assert_eq!(after.fields, before.fields);
        std::fs::remove_dir_all(&base).ok();
    }

    #[tokio::test]
    async fn saved_layout_is_used_by_list_render_without_param() {
        // Save cards as the default, then a no-`?layout=` render uses it;
        // an explicit `?layout=table` still overrides (ephemeral wins).
        use crate::viewspec::ViewLayout;
        let base = tmp_dir();
        save_layout_default(&base, "Gadget", &gadget_schema_model(), ViewLayout::Cards).unwrap();

        let no_param = render_gadget_layout_in(&base, None, &HashMap::new()).await;
        assert!(
            no_param.contains("rio-card-item"),
            "saved cards default should drive the no-param render: {no_param}"
        );

        let override_table = render_gadget_layout_in(&base, Some("table"), &HashMap::new()).await;
        assert!(
            override_table.contains("<table class=\"rio-table\">"),
            "?layout=table must override the saved default"
        );
        // Hidden guarantee still holds with a saved default in effect.
        assert!(!no_param.contains("topsecret-xyz"));
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn resolve_effective_layout_precedence() {
        use crate::viewspec::{ViewLayout, ViewSpec};
        let saved_cards = {
            let mut s = ViewSpec::from_schema_model(&gadget_schema_model());
            s.layout = ViewLayout::Cards;
            s
        };
        // 1. valid param wins over saved.
        assert_eq!(
            resolve_effective_layout(Some("list"), Some(&saved_cards)),
            ViewLayout::List
        );
        // 2. no param → saved layout.
        assert_eq!(
            resolve_effective_layout(None, Some(&saved_cards)),
            ViewLayout::Cards
        );
        // invalid param → falls through to saved.
        assert_eq!(
            resolve_effective_layout(Some("banana"), Some(&saved_cards)),
            ViewLayout::Cards
        );
        // 3. no param, no saved → Table.
        assert_eq!(resolve_effective_layout(None, None), ViewLayout::Table);
        // invalid param, no saved → Table.
        assert_eq!(
            resolve_effective_layout(Some("banana"), None),
            ViewLayout::Table
        );
    }

    #[test]
    fn sanitize_return_rejects_malicious_targets() {
        let slug = "bookings";
        // Honored: this list, with or without a query.
        assert_eq!(sanitize_return(slug, "/admin/bookings"), "/admin/bookings");
        assert_eq!(
            sanitize_return(slug, "/admin/bookings?q=a&status=active"),
            "/admin/bookings?q=a&status=active"
        );
        // Rejected → fall back to /admin/<slug>.
        for evil in [
            "//evil.com",
            "https://evil.com",
            "http://evil.com/admin/bookings",
            "/admin/../etc",
            "/admin/bookings/../../secrets",
            "/admin/other",
            "/admin/bookings\\@evil",
            "javascript:alert(1)",
        ] {
            assert_eq!(
                sanitize_return(slug, evil),
                "/admin/bookings",
                "must reject `{evil}`"
            );
        }
    }

    #[test]
    fn require_csrf_rejects_missing_or_wrong_token() {
        use crate::context::Context;
        use crate::http::FormData;
        let mut ctx = Context::new();
        ctx.insert(crate::auth::CsrfToken("good-token".to_string()));

        // Valid token → Ok.
        let ok_form = FormData::parse("_csrf=good-token&layout=cards");
        assert!(crate::admin::require_csrf(&ctx, &ok_form).is_ok());
        // Missing token → Forbidden.
        let no_tok = FormData::parse("layout=cards");
        assert!(matches!(
            crate::admin::require_csrf(&ctx, &no_tok),
            Err(crate::Error::Forbidden)
        ));
        // Wrong token → Forbidden.
        let wrong = FormData::parse("_csrf=bad&layout=cards");
        assert!(matches!(
            crate::admin::require_csrf(&ctx, &wrong),
            Err(crate::Error::Forbidden)
        ));
    }

    #[test]
    fn admin_guard_rejects_non_admin_and_unauthenticated() {
        use crate::context::Context;
        // No identity → Unauthorized (login page response).
        let empty = Context::new();
        assert!(crate::admin::admin_guard(&empty).is_err());

        // Signed-in but NOT admin → Forbidden (the edit/write gate).
        let mut non_admin = Context::new();
        non_admin.insert(crate::auth::Identity {
            user_id: 2,
            email: "viewer@example.com".to_string(),
            is_admin: false,
        });
        assert!(
            crate::admin::admin_guard(&non_admin).is_err(),
            "a non-admin (no edit) must be rejected by the write gate"
        );

        // Admin → Ok.
        let mut admin = Context::new();
        admin.insert(crate::auth::Identity {
            user_id: 1,
            email: "admin@example.com".to_string(),
            is_admin: true,
        });
        assert!(crate::admin::admin_guard(&admin).is_ok());
    }

    #[test]
    fn parse_layout_strict_rejects_unknown() {
        use crate::viewspec::ViewLayout;
        assert_eq!(parse_layout_strict(Some("cards")), Some(ViewLayout::Cards));
        assert_eq!(parse_layout_strict(Some("table")), Some(ViewLayout::Table));
        assert_eq!(parse_layout_strict(Some("banana")), None);
        assert_eq!(parse_layout_strict(None), None);
    }

    // --- Phase 9a: composition editor — field-role editing ---------------

    use crate::http::FormData;
    use crate::viewspec::{FieldRole, FieldSpec, ViewLayout, ViewSpec};

    /// A saved view with a custom role set, a merge, a filterable field, a
    /// filter, and a non-derived order — to prove role-only edits preserve
    /// everything else.
    fn custom_gadget_spec() -> ViewSpec {
        ViewSpec {
            version: 1,
            model: "Gadget".to_string(),
            layout: ViewLayout::Cards,
            fields: vec![
                FieldSpec {
                    source: "name".to_string(),
                    role: FieldRole::Title,
                    merge: Some(vec!["name".to_string(), "email".to_string()]),
                    filterable: false,
                },
                FieldSpec {
                    source: "status".to_string(),
                    role: FieldRole::Badge,
                    merge: None,
                    filterable: true,
                },
                FieldSpec {
                    source: "notes".to_string(),
                    role: FieldRole::Meta,
                    merge: None,
                    filterable: false,
                },
            ],
            filters: vec!["status".to_string()],
            default_language: "en".to_string(),
            labels: std::collections::BTreeMap::new(),
            value_labels: std::collections::BTreeMap::new(),
        }
    }

    #[test]
    fn parse_role_strict_round_trips_all_six() {
        for (key, role) in [
            ("title", FieldRole::Title),
            ("subtitle", FieldRole::Subtitle),
            ("badge", FieldRole::Badge),
            ("timestamp", FieldRole::Timestamp),
            ("meta", FieldRole::Meta),
            ("hidden", FieldRole::Hidden),
        ] {
            assert_eq!(parse_role_strict(key), Some(role));
            assert_eq!(field_role_key(role), key);
        }
        assert_eq!(parse_role_strict("banana"), None);
        assert_eq!(parse_role_strict(""), None);
    }

    #[test]
    fn build_edited_spec_changes_roles_preserves_everything_else() {
        let spec = custom_gadget_spec();
        // Change name → subtitle, notes → hidden; leave status omitted.
        let form = FormData::parse("role[name]=subtitle&role[notes]=hidden");
        let edited = build_edited_spec(&spec, &form).unwrap();

        // Roles updated.
        assert_eq!(edited.fields[0].role, FieldRole::Subtitle); // name
        assert_eq!(edited.fields[2].role, FieldRole::Hidden); // notes
                                                              // Omitted field keeps its existing role.
        assert_eq!(edited.fields[1].role, FieldRole::Badge); // status

        // Everything else byte-identical: order, sources, merge, filterable,
        // filters, layout, version, model.
        let sources: Vec<&str> = edited.fields.iter().map(|f| f.source.as_str()).collect();
        assert_eq!(sources, vec!["name", "status", "notes"]);
        assert_eq!(edited.fields[0].merge, spec.fields[0].merge);
        assert!(edited.fields[1].filterable);
        assert_eq!(edited.filters, spec.filters);
        assert_eq!(edited.layout, spec.layout);
        assert_eq!(edited.version, spec.version);
        assert_eq!(edited.model, spec.model);
    }

    #[test]
    fn build_edited_spec_rejects_unknown_role() {
        let spec = custom_gadget_spec();
        let form = FormData::parse("role[name]=banana");
        let err = build_edited_spec(&spec, &form).unwrap_err();
        assert!(
            err.contains("banana"),
            "error should name the bad value: {err}"
        );
        assert!(err.contains("name"), "error should name the field: {err}");
    }

    #[test]
    fn save_view_spec_rejects_invalid_and_writes_nothing() {
        let base = tmp_dir();
        // An invalid spec (no fields) must not be written.
        let invalid = ViewSpec {
            version: 1,
            model: "Gadget".to_string(),
            layout: ViewLayout::Table,
            fields: vec![],
            filters: vec![],
            default_language: "en".to_string(),
            labels: std::collections::BTreeMap::new(),
            value_labels: std::collections::BTreeMap::new(),
        };
        assert!(save_view_spec(&base, "Gadget", &invalid).is_err());
        assert!(
            load_saved_view(&base, "Gadget").is_none(),
            "no file may be written for an invalid spec"
        );
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn save_view_spec_preserves_an_existing_file_on_invalid_write() {
        let base = tmp_dir();
        // Seed a valid saved view.
        save_view_spec(&base, "Gadget", &custom_gadget_spec()).unwrap();
        let before = load_saved_view(&base, "Gadget").unwrap();
        // An attempted invalid write must leave the existing file unchanged.
        let invalid = ViewSpec {
            version: 1,
            model: "Gadget".to_string(),
            layout: ViewLayout::Table,
            fields: vec![],
            filters: vec![],
            default_language: "en".to_string(),
            labels: std::collections::BTreeMap::new(),
            value_labels: std::collections::BTreeMap::new(),
        };
        assert!(save_view_spec(&base, "Gadget", &invalid).is_err());
        assert_eq!(load_saved_view(&base, "Gadget").unwrap(), before);
        std::fs::remove_dir_all(&base).ok();
    }

    #[tokio::test]
    async fn role_edit_takes_effect_in_list_render() {
        // Save an edit (notes → Hidden, service-like field unchanged) and
        // confirm a no-param list_render reflects it: the Hidden field's
        // value is gone; a Badge field renders as a pill.
        let base = tmp_dir();
        let model = gadget_schema_model();
        // Start from the derived default, set notes → Hidden via the editor
        // path, keep status as Badge.
        let derived = ViewSpec::from_schema_model(&model);
        let form = FormData::parse("role[notes]=hidden&role[status]=badge");
        let edited = build_edited_spec(&derived, &form).unwrap();
        save_view_spec(&base, "Gadget", &edited).unwrap();

        // Render the gadget list (Table) from this saved view.
        let html = render_gadget_layout_in(&base, Some("table"), &HashMap::new()).await;
        // notes is Hidden → its demo value must be gone.
        assert!(
            !html.contains("MY-META-NOTE"),
            "a field set to Hidden must not render: {html}"
        );
        // status is Badge → renders as a pill.
        assert!(
            html.contains("rio-pill"),
            "a Badge field should render as a pill"
        );
        // Hidden guarantee still holds for the always-hidden secret.
        assert!(!html.contains("topsecret-xyz"));
        std::fs::remove_dir_all(&base).ok();
    }

    // --- Phase 9b: field reordering --------------------------------------

    /// Sources of a spec's fields, in order.
    fn field_order(spec: &ViewSpec) -> Vec<String> {
        spec.fields.iter().map(|f| f.source.clone()).collect()
    }

    #[test]
    fn reorder_sequences_fields_by_submitted_index() {
        let spec = custom_gadget_spec(); // [name, status, notes]
                                         // Reverse: notes, status, name.
        let form = FormData::parse("order[name]=2&order[status]=1&order[notes]=0");
        let edited = build_edited_spec(&spec, &form).unwrap();
        assert_eq!(field_order(&edited), vec!["notes", "status", "name"]);
    }

    #[test]
    fn reorder_preserves_field_set_even_with_garbage_indices() {
        use std::collections::BTreeSet;
        let spec = custom_gadget_spec();
        let before: BTreeSet<String> = field_order(&spec).into_iter().collect();
        // Duplicate + missing + out-of-range indices (tampered).
        let form = FormData::parse("order[name]=5&order[status]=5&order[notes]=banana");
        let edited = build_edited_spec(&spec, &form).unwrap();
        let after: BTreeSet<String> = field_order(&edited).into_iter().collect();
        assert_eq!(
            before, after,
            "the field SET must be identical — no drop/dup/invent"
        );
        assert_eq!(edited.fields.len(), spec.fields.len(), "no duplicates");
    }

    #[test]
    fn reorder_with_original_indices_is_identity() {
        // The no-JS / no-change path: submit the original indices → order
        // unchanged (stable-sort identity), never a crash or corruption.
        let spec = custom_gadget_spec(); // [name(0), status(1), notes(2)]
        let form = FormData::parse("order[name]=0&order[status]=1&order[notes]=2");
        let edited = build_edited_spec(&spec, &form).unwrap();
        assert_eq!(field_order(&edited), field_order(&spec));
    }

    #[test]
    fn reorder_with_no_order_keys_is_identity() {
        // No order[…] submitted at all (e.g. a role-only client) → fields
        // keep their existing sequence.
        let spec = custom_gadget_spec();
        let form = FormData::parse("role[name]=subtitle");
        let edited = build_edited_spec(&spec, &form).unwrap();
        assert_eq!(field_order(&edited), field_order(&spec));
        assert_eq!(edited.fields[0].role, FieldRole::Subtitle); // role still applied
    }

    #[test]
    fn reorder_composes_with_role_change_in_one_save() {
        let spec = custom_gadget_spec(); // [name, status, notes]
                                         // One submit: move notes to top AND change status → hidden.
        let form =
            FormData::parse("order[notes]=0&order[name]=1&order[status]=2&role[status]=hidden");
        let edited = build_edited_spec(&spec, &form).unwrap();
        assert_eq!(field_order(&edited), vec!["notes", "name", "status"]);
        let status = edited.fields.iter().find(|f| f.source == "status").unwrap();
        assert_eq!(status.role, FieldRole::Hidden);
    }

    #[test]
    fn reorder_preserves_merge_filterable_and_spec_metadata() {
        let spec = custom_gadget_spec();
        let form = FormData::parse("order[name]=2&order[status]=0&order[notes]=1");
        let edited = build_edited_spec(&spec, &form).unwrap();
        // name kept its merge; status kept filterable; spec-level fields kept.
        let name = edited.fields.iter().find(|f| f.source == "name").unwrap();
        let status = edited.fields.iter().find(|f| f.source == "status").unwrap();
        assert_eq!(name.merge, spec.fields[0].merge);
        assert!(status.filterable);
        assert_eq!(edited.filters, spec.filters);
        assert_eq!(edited.layout, spec.layout);
        assert_eq!(edited.version, spec.version);
        assert_eq!(edited.model, spec.model);
    }

    #[tokio::test]
    async fn reorder_takes_effect_in_list_render() {
        // Move `status` (a Badge) to the front and confirm the rendered
        // Table columns lead with it; the Hidden guarantee still holds.
        let base = tmp_dir();
        let model = gadget_schema_model();
        let derived = ViewSpec::from_schema_model(&model);
        // Derived order: id, name, email, status, notes, password_hash.
        // Put status first (everything else after, original-relative order).
        let form = FormData::parse(
            "order[status]=0&order[id]=1&order[name]=2&order[email]=3&order[notes]=4&order[password_hash]=5",
        );
        let edited = build_edited_spec(&derived, &form).unwrap();
        save_view_spec(&base, "Gadget", &edited).unwrap();

        let html = render_gadget_layout_in(&base, Some("table"), &HashMap::new()).await;
        // The first data column header should be Status (id/password_hash are
        // Hidden, so the first *visible* column is status).
        let first_th = html
            .split("<th scope=\"col\">")
            .nth(1)
            .and_then(|s| s.split("</th>").next())
            .unwrap_or("");
        assert_eq!(
            first_th, "Status",
            "reordered column should lead the table: {html}"
        );
        assert!(
            !html.contains("topsecret-xyz"),
            "Hidden guarantee after reorder"
        );
        std::fs::remove_dir_all(&base).ok();
    }

    // --- Phase 9c: filter toggles ----------------------------------------

    /// Assert a spec is internally consistent: every `filters` entry names a
    /// `filterable == true` field (i.e. `validate()` would pass the rule).
    fn assert_filters_consistent(spec: &ViewSpec) {
        let filterable: std::collections::BTreeSet<&str> = spec
            .fields
            .iter()
            .filter(|f| f.filterable)
            .map(|f| f.source.as_str())
            .collect();
        for name in &spec.filters {
            assert!(
                filterable.contains(name.as_str()),
                "filters entry `{name}` is not a filterable field"
            );
        }
        assert!(spec.validate().is_ok(), "candidate must validate");
    }

    #[test]
    fn filter_toggle_on_persists_and_off_removes() {
        // Start from custom_gadget_spec: status is the only filter.
        let spec = custom_gadget_spec();
        assert_eq!(spec.filters, vec!["status".to_string()]);

        // Turn status OFF, turn name ON (sentinel + the name checkbox only).
        let form = FormData::parse("filters_submitted=1&filterable[name]=1");
        let edited = build_edited_spec(&spec, &form).unwrap();
        assert_eq!(edited.filters, vec!["name".to_string()]);
        let name = edited.fields.iter().find(|f| f.source == "name").unwrap();
        let status = edited.fields.iter().find(|f| f.source == "status").unwrap();
        assert!(name.filterable, "name is now filterable");
        assert!(!status.filterable, "status filter turned off");
        assert_filters_consistent(&edited);
    }

    #[test]
    fn filters_derived_in_display_order() {
        // custom_gadget_spec fields: name, status, notes. Mark name + notes
        // filterable → filters in display order [name, notes].
        let spec = custom_gadget_spec();
        let form = FormData::parse("filters_submitted=1&filterable[notes]=1&filterable[name]=1");
        let edited = build_edited_spec(&spec, &form).unwrap();
        assert_eq!(
            edited.filters,
            vec!["name".to_string(), "notes".to_string()]
        );
    }

    #[test]
    fn no_sentinel_preserves_filters_and_filterable() {
        // A role/order-only submit (no filters_submitted) leaves filters and
        // filterable untouched — the 9a/9b behaviour.
        let spec = custom_gadget_spec();
        let form = FormData::parse("role[name]=subtitle");
        let edited = build_edited_spec(&spec, &form).unwrap();
        assert_eq!(edited.filters, spec.filters);
        let status = edited.fields.iter().find(|f| f.source == "status").unwrap();
        assert!(
            status.filterable,
            "filterable preserved without the sentinel"
        );
    }

    #[test]
    fn hidden_field_cannot_be_filter_server_enforced() {
        // One submit sets `status` → Hidden AND checks its filter box. The
        // server must drop it: not in filters, filterable = false.
        let spec = custom_gadget_spec();
        let form = FormData::parse(
            "filters_submitted=1&role[status]=hidden&filterable[status]=1&filterable[name]=1",
        );
        let edited = build_edited_spec(&spec, &form).unwrap();
        let status = edited.fields.iter().find(|f| f.source == "status").unwrap();
        assert_eq!(status.role, FieldRole::Hidden);
        assert!(!status.filterable, "a Hidden field must not be filterable");
        assert!(!edited.filters.contains(&"status".to_string()));
        assert!(edited.filters.contains(&"name".to_string()));
        assert_filters_consistent(&edited);
    }

    #[test]
    fn save_view_spec_rejects_inconsistent_filters_no_write() {
        // The guard now has teeth: a hand-built spec with a filter on a
        // non-filterable field is rejected and nothing is written.
        let base = tmp_dir();
        let bad = ViewSpec {
            version: 1,
            model: "Gadget".to_string(),
            layout: ViewLayout::Table,
            fields: vec![FieldSpec {
                source: "name".to_string(),
                role: FieldRole::Title,
                merge: None,
                filterable: false, // NOT filterable …
            }],
            filters: vec!["name".to_string()], // … but listed as a filter
            default_language: "en".to_string(),
            labels: std::collections::BTreeMap::new(),
            value_labels: std::collections::BTreeMap::new(),
        };
        assert!(
            bad.validate().is_err(),
            "validate must catch the inconsistency"
        );
        assert!(save_view_spec(&base, "Gadget", &bad).is_err());
        assert!(
            load_saved_view(&base, "Gadget").is_none(),
            "no file may be written for an inconsistent spec"
        );
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn filter_composes_with_role_and_order_in_one_save() {
        let spec = custom_gadget_spec(); // [name, status, notes], filter=status
                                         // One submit: reorder (notes first), change name → badge, filters = {notes}.
        let form = FormData::parse(
            "filters_submitted=1&order[notes]=0&order[name]=1&order[status]=2\
             &role[name]=badge&filterable[notes]=1",
        );
        let edited = build_edited_spec(&spec, &form).unwrap();
        assert_eq!(field_order(&edited), vec!["notes", "name", "status"]);
        let name = edited.fields.iter().find(|f| f.source == "name").unwrap();
        assert_eq!(name.role, FieldRole::Badge);
        assert_eq!(edited.filters, vec!["notes".to_string()]);
        assert_filters_consistent(&edited);
    }

    #[test]
    fn filter_edit_preserves_merge_and_spec_metadata() {
        let spec = custom_gadget_spec();
        let form = FormData::parse("filters_submitted=1&filterable[name]=1");
        let edited = build_edited_spec(&spec, &form).unwrap();
        // name keeps its merge; layout/version/model preserved.
        let name = edited.fields.iter().find(|f| f.source == "name").unwrap();
        assert_eq!(name.merge, spec.fields[0].merge);
        assert_eq!(edited.layout, spec.layout);
        assert_eq!(edited.version, spec.version);
        assert_eq!(edited.model, spec.model);
    }

    #[tokio::test]
    async fn filter_save_round_trips_and_keeps_hidden_guarantee() {
        // A full editor-style save (sentinel) persists to disk; reloading
        // shows the new filters; the always-hidden secret never leaks.
        let base = tmp_dir();
        let model = gadget_schema_model();
        let derived = ViewSpec::from_schema_model(&model);
        // Make `email` a filter (it's a Subtitle, non-hidden).
        let form = FormData::parse("filters_submitted=1&filterable[email]=1");
        let edited = build_edited_spec(&derived, &form).unwrap();
        save_view_spec(&base, "Gadget", &edited).unwrap();

        let reloaded = load_saved_view(&base, "Gadget").unwrap();
        assert_eq!(reloaded.filters, vec!["email".to_string()]);
        assert_filters_consistent(&reloaded);

        // Render still hides the secret field.
        let html = render_gadget_layout_in(&base, Some("table"), &HashMap::new()).await;
        assert!(!html.contains("topsecret-xyz"));
        std::fs::remove_dir_all(&base).ok();
    }

    // --- Phase 9d: merge ----------------------------------------------------

    fn merge_of<'a>(spec: &'a ViewSpec, source: &str) -> Option<&'a Vec<String>> {
        spec.fields
            .iter()
            .find(|f| f.source == source)
            .and_then(|f| f.merge.as_ref())
    }

    #[test]
    fn merge_groups_anchor_and_removes_member() {
        // gadget derived: name (Title) anchor, email (Subtitle) member.
        let derived = ViewSpec::from_schema_model(&gadget_schema_model());
        let form = FormData::parse("merge_submitted=1&merge[email]=name");
        let edited = build_edited_spec(&derived, &form).unwrap();

        assert_eq!(
            merge_of(&edited, "name"),
            Some(&vec!["name".to_string(), "email".to_string()])
        );
        // The member exists ONLY inside the anchor's merge vec, never as its
        // own FieldSpec (else the renderer would emit it twice).
        assert!(
            edited.fields.iter().all(|f| f.source != "email"),
            "merged member must be removed from fields"
        );
        assert!(edited.validate().is_ok());
    }

    #[tokio::test]
    async fn merge_renders_one_joined_cell_in_list() {
        let base = tmp_dir();
        let derived = ViewSpec::from_schema_model(&gadget_schema_model());
        let edited = build_edited_spec(
            &derived,
            &FormData::parse("merge_submitted=1&merge[email]=name"),
        )
        .unwrap();
        save_view_spec(&base, "Gadget", &edited).unwrap();

        let html = render_gadget_layout_in(&base, Some("table"), &HashMap::new()).await;
        // gadget_db row: name='Alpha', email='alpha@x.example'.
        assert!(
            html.contains("<td>Alpha · alpha@x.example</td>"),
            "one joined merged cell expected:\n{html}"
        );
        // Email is not its own column anymore.
        assert!(
            !html.contains(">Email</th>"),
            "email must not be a separate column"
        );
        assert!(!html.contains("topsecret-xyz"));
        std::fs::remove_dir_all(&base).ok();
    }

    #[tokio::test]
    async fn hidden_field_dropped_from_merge_value_absent() {
        // Set status → Hidden AND try to merge it into name (with email so the
        // group stays ≥ 2). status must be dropped from the merge and its
        // value must never enter the joined cell.
        let base = tmp_dir();
        let derived = ViewSpec::from_schema_model(&gadget_schema_model());
        let form = FormData::parse(
            "merge_submitted=1&role[status]=hidden&merge[status]=name&merge[email]=name",
        );
        let edited = build_edited_spec(&derived, &form).unwrap();

        let m = merge_of(&edited, "name").unwrap();
        assert!(
            !m.contains(&"status".to_string()),
            "a Hidden field must be dropped from the merge: {m:?}"
        );
        assert!(m.contains(&"email".to_string()));

        save_view_spec(&base, "Gadget", &edited).unwrap();
        let html = render_gadget_layout_in(&base, Some("table"), &HashMap::new()).await;
        // The joined cell is exactly name · email — the status value 'active'
        // is NOT appended (and status, now Hidden, renders no pill at all).
        assert!(html.contains("<td>Alpha · alpha@x.example</td>"));
        assert!(
            !html.contains("rio-pill"),
            "hidden status value must not render: {html}"
        );
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn one_member_merge_rejected_by_validate_no_write() {
        // A hand-built spec with a 1-entry merge must be rejected (teeth).
        let base = tmp_dir();
        let bad = ViewSpec {
            version: 1,
            model: "Gadget".to_string(),
            layout: ViewLayout::Table,
            fields: vec![FieldSpec {
                source: "name".to_string(),
                role: FieldRole::Title,
                merge: Some(vec!["name".to_string()]), // only 1 entry
                filterable: false,
            }],
            filters: vec![],
            default_language: "en".to_string(),
            labels: std::collections::BTreeMap::new(),
            value_labels: std::collections::BTreeMap::new(),
        };
        assert!(matches!(
            bad.validate(),
            Err(crate::viewspec::ViewSpecError::MergeTooShort { .. })
        ));
        assert!(save_view_spec(&base, "Gadget", &bad).is_err());
        assert!(
            load_saved_view(&base, "Gadget").is_none(),
            "no write for invalid merge"
        );
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn merge_chain_or_mutual_forms_no_group() {
        // Mutual: name→email and email→name. Neither is a standalone target
        // (both have a merge target), so NO group forms — both standalone.
        let derived = ViewSpec::from_schema_model(&gadget_schema_model());
        let form = FormData::parse("merge_submitted=1&merge[name]=email&merge[email]=name");
        let edited = build_edited_spec(&derived, &form).unwrap();
        assert!(merge_of(&edited, "name").is_none());
        assert!(merge_of(&edited, "email").is_none());
        assert!(edited.fields.iter().any(|f| f.source == "name"));
        assert!(edited.fields.iter().any(|f| f.source == "email"));
    }

    #[test]
    fn unmerge_restores_member_as_standalone_with_role() {
        // Merge, then submit merge[email]="" (+ its derived role) to unmerge.
        let derived = ViewSpec::from_schema_model(&gadget_schema_model());
        let merged = build_edited_spec(
            &derived,
            &FormData::parse("merge_submitted=1&merge[email]=name"),
        )
        .unwrap();
        assert!(merge_of(&merged, "name").is_some());
        assert!(!merged.fields.iter().any(|f| f.source == "email"));

        // The editor pre-fills role[email] with the derived role (subtitle).
        let unmerged = build_edited_spec(
            &merged,
            &FormData::parse("merge_submitted=1&role[email]=subtitle&merge[email]="),
        )
        .unwrap();
        assert!(
            merge_of(&unmerged, "name").is_none(),
            "name no longer merged"
        );
        let email = unmerged
            .fields
            .iter()
            .find(|f| f.source == "email")
            .expect("email restored as a standalone field");
        assert_eq!(email.role, FieldRole::Subtitle);
    }

    #[test]
    fn merge_composes_with_role_order_filter_in_one_save() {
        let derived = ViewSpec::from_schema_model(&gadget_schema_model());
        let form = FormData::parse(
            "merge_submitted=1&filters_submitted=1&merge[email]=name\
             &order[status]=0&order[name]=1&filterable[name]=1",
        );
        let edited = build_edited_spec(&derived, &form).unwrap();
        // merge applied …
        assert!(merge_of(&edited, "name").is_some());
        assert!(edited.fields.iter().all(|f| f.source != "email"));
        // … order applied (status before name) …
        let order = field_order(&edited);
        let pos = |s: &str| order.iter().position(|x| x == s).unwrap();
        assert!(pos("status") < pos("name"));
        // … filter applied (name is filterable + in filters).
        assert!(edited.filters.contains(&"name".to_string()));
        assert!(edited.validate().is_ok());
    }

    #[test]
    fn merge_preserves_layout_version_model() {
        let derived = ViewSpec::from_schema_model(&gadget_schema_model());
        let edited = build_edited_spec(
            &derived,
            &FormData::parse("merge_submitted=1&merge[email]=name"),
        )
        .unwrap();
        assert_eq!(edited.layout, derived.layout);
        assert_eq!(edited.version, derived.version);
        assert_eq!(edited.model, derived.model);
    }

    #[test]
    fn status_field_name_matches_known_patterns() {
        // Bare names
        assert!(is_status_field_name("status"));
        assert!(is_status_field_name("state"));
        assert!(is_status_field_name("active"));
        assert!(is_status_field_name("published"));
        // Case-insensitive
        assert!(is_status_field_name("Status"));
        assert!(is_status_field_name("STATE"));
        // Suffix patterns
        assert!(is_status_field_name("task_status"));
        assert!(is_status_field_name("order_state"));
        // Prefix patterns (booleans typically)
        assert!(is_status_field_name("is_active"));
        assert!(is_status_field_name("is_published"));
        assert!(is_status_field_name("has_paid"));
    }

    #[test]
    fn status_field_name_rejects_non_status_columns() {
        // Title-like text columns
        assert!(!is_status_field_name("title"));
        assert!(!is_status_field_name("description"));
        assert!(!is_status_field_name("name"));
        // Numerics
        assert!(!is_status_field_name("priority"));
        assert!(!is_status_field_name("count"));
        // Timestamps
        assert!(!is_status_field_name("created_at"));
        assert!(!is_status_field_name("due_at"));
        // FK columns
        assert!(!is_status_field_name("project_id"));
        assert!(!is_status_field_name("user_id"));
        // Edge case: substring "status" inside a word should NOT match
        assert!(!is_status_field_name("statustown"));
        assert!(!is_status_field_name("estatus_id"));
    }

    #[test]
    fn normalize_status_pill_maps_boolean_encodings() {
        // Truthy → active + "Active" label
        for raw in ["1", "true", "TRUE", " True ", "yes", "on"] {
            let (data, label) = normalize_status_pill(raw);
            assert_eq!(
                data, "active",
                "truthy raw {raw:?} should map to data=active"
            );
            assert_eq!(label, "Active", "truthy raw {raw:?} should label as Active");
        }
        // Falsy → inactive + "Inactive" label
        for raw in ["0", "false", "FALSE", "no", "off"] {
            let (data, label) = normalize_status_pill(raw);
            assert_eq!(
                data, "inactive",
                "falsy raw {raw:?} should map to data=inactive"
            );
            assert_eq!(
                label, "Inactive",
                "falsy raw {raw:?} should label as Inactive"
            );
        }
    }

    #[test]
    fn normalize_status_pill_humanizes_string_statuses() {
        // String statuses get sentence-case labels — never SCREAMING,
        // never Title_Case. `data-status` stays lowercased for CSS
        // matchers in projects that re-introduce colour coding.
        let (data, label) = normalize_status_pill("In_Progress");
        assert_eq!(data, "in_progress");
        assert_eq!(label, "In progress");

        let (data, label) = normalize_status_pill("DONE");
        assert_eq!(data, "done");
        assert_eq!(label, "Done");

        let (data, label) = normalize_status_pill("todo");
        assert_eq!(data, "todo");
        assert_eq!(label, "Todo");

        let (data, label) = normalize_status_pill("review");
        assert_eq!(data, "review");
        assert_eq!(label, "Review");

        // Unknown value: still humanised the same way.
        let (data, label) = normalize_status_pill("custom_state");
        assert_eq!(data, "custom_state");
        assert_eq!(label, "Custom state");
    }

    #[test]
    fn humanize_status_label_handles_edges() {
        assert_eq!(humanize_status_label(""), "");
        assert_eq!(humanize_status_label("a"), "A");
        assert_eq!(humanize_status_label(" trim "), "Trim");
        assert_eq!(
            humanize_status_label("multi_word_status"),
            "Multi word status"
        );
    }

    #[test]
    fn humanize_field_label_cases() {
        assert_eq!(humanize_field_label(""), "");
        assert_eq!(humanize_field_label("id"), "ID");
        assert_eq!(humanize_field_label("title"), "Title");
        assert_eq!(humanize_field_label("project_id"), "Project");
        assert_eq!(humanize_field_label("user_id"), "User");
        assert_eq!(humanize_field_label("due_at"), "Due at");
        assert_eq!(humanize_field_label("created_at"), "Created at");
        assert_eq!(humanize_field_label("first_name"), "First name");
        // Already-cased labels (e.g. user-set via AdminUiField.label =
        // "Username") pass through unchanged.
        assert_eq!(humanize_field_label("Username"), "Username");
        assert_eq!(humanize_field_label("User ID"), "User ID");
        // Idempotent on a previous humanize output.
        assert_eq!(
            humanize_field_label(&humanize_field_label("due_at")),
            "Due at"
        );
    }
}
