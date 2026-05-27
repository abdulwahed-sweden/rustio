//! Admin page assembler.
//!
//! Every admin page is rendered by `minijinja` against the templates
//! bundled under `rustio-core/assets/templates/`. The functions here
//! build the typed context dicts the templates consume — no HTML is
//! concatenated in Rust. Bootstrap 5 CSS/JS and `admin.css`/`app.js`
//! ship from `rustio-core/assets/static/` and are served under
//! `/admin/static/…` by the core (see `admin::templating`).

use std::collections::HashMap;

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
    links: Vec<PageLinkView>,
}

#[derive(serde::Serialize)]
struct ListPermissionsView {
    view: bool,
    create: bool,
    edit: bool,
    delete: bool,
}

/// 0.10+ list-page renderer. Searchable / filter / sort / paginate
/// query runs through `fetch_users_table_state`; the page renders via
/// `minijinja`. Create / edit / delete actions are RBAC-gated by the
/// caller.
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
                        // Status-shaped column: wrap in a pill badge that
                        // admin.css colours via `[data-status="<value>"]`.
                        // SQLite boolean columns return "0"/"1"; we
                        // normalise to "Active"/"Inactive" so the label
                        // reads cleanly AND admin.css can colour the
                        // pill via `[data-status="active|inactive"]`.
                        // String statuses (todo / in_progress / done /
                        // pending …) pass through with the raw value
                        // lowercased for the data-status attribute.
                        // Empty values render as empty cells, not pills.
                        if raw.is_empty() {
                            return String::new();
                        }
                        let (data_value, label) = normalize_status_pill(&raw);
                        format!(
                            r#"<span class="badge-status" data-status="{value}">{label}</span>"#,
                            value = html_escape(&data_value),
                            label = html_escape(&label),
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

/// Normalise a raw cell value for status-pill rendering.
///
/// Returns `(data_status_value, display_label)`:
/// - `data_status_value` is what goes into the `data-status` attribute;
///   admin.css uses it to pick the pill colour
///   (`active`/`pending`/`inactive`/`info`).
/// - `display_label` is the human-readable text shown inside the pill.
///
/// SQLite booleans round-trip as `"0"` / `"1"` strings through the
/// persistence layer; we map those to the more readable `Active` /
/// `Inactive` here. String statuses (`todo` / `in_progress` / `done`)
/// pass through with the original casing for the label and lowercased
/// for `data-status` so CSS matchers fire on `[data-status="done"]`.
fn normalize_status_pill(raw: &str) -> (String, String) {
    let lc = raw.trim().to_lowercase();
    match lc.as_str() {
        "1" | "true" | "yes" | "on" => ("active".to_string(), "Active".to_string()),
        "0" | "false" | "no" | "off" => ("inactive".to_string(), "Inactive".to_string()),
        _ => (lc, raw.to_string()),
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
    fn normalize_status_pill_passes_through_string_statuses() {
        // String statuses keep their original casing in the label but
        // lowercase in the data-status attribute (so admin.css matchers
        // like `[data-status="done"]` fire reliably).
        let (data, label) = normalize_status_pill("In_Progress");
        assert_eq!(data, "in_progress");
        assert_eq!(label, "In_Progress");

        let (data, label) = normalize_status_pill("DONE");
        assert_eq!(data, "done");
        assert_eq!(label, "DONE");

        let (data, label) = normalize_status_pill("todo");
        assert_eq!(data, "todo");
        assert_eq!(label, "todo");

        // Unknown value: passes through. The base `[data-status]` rule
        // still applies the pill shape; only the colour is missing.
        let (data, label) = normalize_status_pill("custom");
        assert_eq!(data, "custom");
        assert_eq!(label, "custom");
    }
}
