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
) -> String {
    let dashboard_entries = collect_dashboard_entries(db, registry).await;
    let sidebar = sidebar_merged(&dashboard_entries, legacy_entries, Some(model.slug()));

    let ui_fields = model.fields();
    let schema_model = schema_model_from_ui(model.model_name(), &ui_fields);
    let spec = resolve_view(base, model.model_name(), &schema_model);

    let fields: Vec<EditorFieldView> = spec
        .fields
        .iter()
        .map(|fs| {
            // Prefer the model's UI label; fall back to humanising the source.
            let label = ui_fields
                .iter()
                .find(|f| f.name == fs.source)
                .map(|f| humanize_field_label(f.label))
                .unwrap_or_else(|| humanize_field_label(&fs.source));
            EditorFieldView {
                source: fs.source.clone(),
                label,
                role: field_role_key(fs.role).to_string(),
            }
        })
        .collect();

    let slug = model.slug();
    let model_view = EditorModelView {
        display_name: format!("{}s", model.model_name()),
        singular_name: model.model_name().to_string(),
        list_url: format!("/admin/{slug}"),
    };
    let design = design_view();
    let user = user_view(identity);

    let env = crate::admin::templating::env();
    match env.get_template("admin/view_editor.html").and_then(|tmpl| {
        tmpl.render(minijinja::context! {
            design => design,
            current_user => user,
            sidebar_entries => sidebar,
            model => model_view,
            fields => fields,
            roles => role_options(),
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

/// Build a candidate ViewSpec from `spec` by applying the submitted roles
/// (Phase 9a). The handler drives off `spec.fields` (the authority), so
/// **order, merge, filterable, filters, version, and model are preserved**
/// exactly — only roles change.
///
/// For each field, `role[<source>]` is read: a present value MUST parse to
/// one of the six roles, otherwise this returns `Err(message)` so the
/// handler re-renders the editor with the error and **writes nothing**
/// (a bad submission never silently falls back to the existing role). An
/// omitted key keeps the field's current role.
///
/// 9b/9c/9d plug in here: 9b will sort `new_fields` by a submitted order,
/// 9c will read `filterable[<source>]` + a `filters[]` set, 9d will read a
/// merge grouping — all feeding the same candidate → `validate` → write.
pub(crate) fn build_role_edited_spec(
    spec: &crate::viewspec::ViewSpec,
    form: &crate::http::FormData,
) -> Result<crate::viewspec::ViewSpec, String> {
    let mut new_fields = Vec::with_capacity(spec.fields.len());
    for f in &spec.fields {
        let role = match form.get(&format!("role[{}]", f.source)) {
            Some(value) => parse_role_strict(value).ok_or_else(|| {
                format!(
                    "Unknown role \u{201c}{value}\u{201d} for field \u{201c}{}\u{201d}.",
                    f.source
                )
            })?,
            None => f.role,
        };
        new_fields.push(crate::viewspec::FieldSpec {
            source: f.source.clone(),
            role,
            merge: f.merge.clone(),
            filterable: f.filterable,
        });
    }
    let mut candidate = spec.clone();
    candidate.fields = new_fields;
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
fn view_columns(
    spec: &crate::viewspec::ViewSpec,
    layout: crate::viewspec::ViewLayout,
    fields: &[AdminUiField],
) -> Vec<ColumnView> {
    // Selection is independent of row data — probe with a single empty row
    // and read which sources the requested layout surfaces, in order. The
    // Phase-3 renderer owns the per-layout cell set (Table = all visible,
    // List drops Meta, Compact = Title + Badge, …); the admin adds none.
    let probe: Vec<crate::viewspec::render::Row> = vec![std::collections::BTreeMap::new()];
    let view = crate::viewspec::render::RenderedView::render_with_layout(spec, layout, &probe);
    let selected: Vec<&str> = view
        .rows
        .first()
        .map(|r| {
            r.cells
                .iter()
                .filter_map(|c| c.sources.first().map(String::as_str))
                .collect()
        })
        .unwrap_or_default();

    selected
        .iter()
        .filter_map(|name| {
            fields.iter().find(|f| f.name == *name).map(|f| ColumnView {
                name: f.name.to_string(),
                label: humanize_field_label(f.label),
                sortable: f.sortable,
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

    let (rows_raw, total, current_page, total_pages, validated_sort, validated_dir) =
        fetch_users_table_state(db, model, query, filters, page, sort, dir).await;

    let fields = model.fields();
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
    let schema_model = schema_model_from_ui(model.model_name(), &fields);
    let saved = load_saved_view(base, model.model_name());
    let active_layout = resolve_effective_layout(layout, saved.as_ref());
    let spec = saved.unwrap_or_else(|| crate::viewspec::ViewSpec::from_schema_model(&schema_model));
    let columns: Vec<ColumnView> = view_columns(&spec, active_layout, &fields);

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
    let rows: Vec<RowView> = rows_raw
        .iter()
        .map(|row| {
            let id = row.get(pk).cloned().unwrap_or_default();
            let cells = columns
                .iter()
                .enumerate()
                .map(|(col_idx, col)| {
                    let raw = row.get(&col.name).cloned().unwrap_or_default();
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
            layout => layout_key(active_layout),
            layout_options => layout_options,
            return_to => return_to,
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
    let user_v = user_view(identity);
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
    let user_v = user_view(identity);
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
        let cols = view_columns(&spec, crate::viewspec::ViewLayout::Table, &fields);
        let names: Vec<&str> = cols.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, vec!["name", "email", "status"]);
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
    fn build_role_edited_spec_changes_roles_preserves_everything_else() {
        let spec = custom_gadget_spec();
        // Change name → subtitle, notes → hidden; leave status omitted.
        let form = FormData::parse("role[name]=subtitle&role[notes]=hidden");
        let edited = build_role_edited_spec(&spec, &form).unwrap();

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
    fn build_role_edited_spec_rejects_unknown_role() {
        let spec = custom_gadget_spec();
        let form = FormData::parse("role[name]=banana");
        let err = build_role_edited_spec(&spec, &form).unwrap_err();
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
        let edited = build_role_edited_spec(&derived, &form).unwrap();
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
