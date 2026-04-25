//! Template context builders. Every piece of data the admin templates
//! need comes from here, as a `serde::Serialize` struct. No HTML lives
//! in Rust code.
//!
//! # Phase 6a: shared `BaseContext`
//!
//! Every page context embeds [`BaseContext`] via `#[serde(flatten)]`.
//! That gives every template uniform access to `identity`,
//! `csrf_token`, `site_title`, and `site_header` without per-page
//! plumbing. `identity` is `Option<…>` because the login page renders
//! before authentication.

use serde::Serialize;

use super::audit::AdminAction;
use super::types::{Admin, AdminEntry, EditRow, ListRow};
use crate::auth::Identity;

#[derive(Serialize)]
pub(crate) struct IdentityCtx {
    pub email: String,
    pub is_admin: bool,
}

impl From<&Identity> for IdentityCtx {
    fn from(i: &Identity) -> Self {
        Self {
            email: i.email.clone(),
            is_admin: i.is_admin(),
        }
    }
}

#[derive(Serialize)]
pub(crate) struct BaseContext {
    pub identity: Option<IdentityCtx>,
    pub csrf_token: String,
    pub site_title: String,
    pub site_header: String,
    pub index_title: String,
    pub footer_copyright: String,
}

impl BaseContext {
    /// Build the shared base context every page extends. Reads the
    /// active branding from `&Admin` so projects can override defaults
    /// via `Admin::site_branding(...)`. Future Phase 8/9 may pull more
    /// from `Admin` (locale, theme) without re-touching every handler.
    pub fn new(identity: Option<&Identity>, csrf_token: String, admin: &Admin) -> Self {
        let b = admin.branding();
        Self {
            identity: identity.map(IdentityCtx::from),
            csrf_token,
            site_title: b.site_title.clone(),
            site_header: b.site_header.clone(),
            index_title: b.index_title.clone(),
            footer_copyright: b.footer_copyright.clone(),
        }
    }
}

#[derive(Serialize)]
pub(crate) struct SidebarEntry {
    pub admin_name: &'static str,
    pub display_name: &'static str,
}

impl From<&AdminEntry> for SidebarEntry {
    fn from(e: &AdminEntry) -> Self {
        Self {
            admin_name: e.admin_name,
            display_name: e.display_name,
        }
    }
}

#[derive(Serialize)]
pub(crate) struct FlashCtx {
    pub kind: &'static str,
    pub message: String,
}

// ---- Page contexts --------------------------------------------------------

#[derive(Serialize)]
pub(crate) struct LoginCtx {
    #[serde(flatten)]
    pub base: BaseContext,
    pub error: Option<String>,
}

// ---------------------------------------------------------------------------
// Dashboard
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub(crate) struct DashboardCtx {
    #[serde(flatten)]
    pub base: BaseContext,
    pub apps: Vec<DashboardApp>,
    pub recent_actions: Vec<RecentActionCtx>,
    pub flash: Option<FlashCtx>,
}

#[derive(Serialize)]
pub(crate) struct DashboardApp {
    pub label: String,
    pub models: Vec<DashboardModel>,
}

#[derive(Serialize)]
pub(crate) struct DashboardModel {
    pub admin_name: &'static str,
    pub display_name: &'static str,
    pub field_count: usize,
}

#[derive(Serialize)]
pub(crate) struct RecentActionCtx {
    pub action_type: String,
    pub label: &'static str,
    pub pill_class: &'static str,
    pub model_name: String,
    pub object_id: i64,
    pub user_email: String,
    pub summary: String,
    pub when_relative: String,
}

/// Group every `AdminEntry` by `app_label` derived from `admin_name`.
///
/// Convention: if `admin_name` contains a `.`, the prefix is the app
/// label (e.g. `"tolkhuset.translators"` → label `"Tolkhuset"`); the
/// remaining path is the model slug. Otherwise the whole `admin_name`
/// becomes a single-app label, capitalised.
pub(crate) fn group_entries_by_app(entries: &[AdminEntry]) -> Vec<DashboardApp> {
    let mut apps: Vec<DashboardApp> = Vec::new();
    for entry in entries {
        // Core entries (currently just the synthetic User) have a
        // bespoke admin page reachable via the header's Users link.
        // Listing them here would offer "Add"/"Change" actions that
        // route through CoreUserOps, which is schema-only — hitting
        // either button 500s. Skip them entirely.
        if entry.core {
            continue;
        }
        let label = app_label_for(entry.admin_name);
        let app = match apps.iter_mut().find(|a| a.label == label) {
            Some(a) => a,
            None => {
                apps.push(DashboardApp {
                    label: label.clone(),
                    models: Vec::new(),
                });
                apps.last_mut().unwrap()
            }
        };
        app.models.push(DashboardModel {
            admin_name: entry.admin_name,
            display_name: entry.display_name,
            field_count: entry.fields.len(),
        });
    }
    apps
}

pub(crate) fn app_label_for(admin_name: &str) -> String {
    let prefix = admin_name.split('.').next().unwrap_or(admin_name);
    capitalise(prefix)
}

fn capitalise(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

pub(crate) fn dashboard_ctx(
    identity: &Identity,
    admin: &Admin,
    recent_actions: Vec<AdminAction>,
    csrf_token: String,
) -> DashboardCtx {
    let recent = recent_actions
        .into_iter()
        .map(|a| RecentActionCtx {
            action_type: a.action_type.clone(),
            label: action_label(&a.action_type),
            pill_class: action_pill_class(&a.action_type),
            model_name: a.model_name,
            object_id: a.object_id,
            user_email: a.user_email.unwrap_or_else(|| "—".to_string()),
            summary: a.summary,
            when_relative: relative_time(a.timestamp),
        })
        .collect();

    DashboardCtx {
        base: BaseContext::new(Some(identity), csrf_token, admin),
        apps: group_entries_by_app(admin.entries()),
        recent_actions: recent,
        flash: None,
    }
}

fn action_label(action_type: &str) -> &'static str {
    match action_type {
        "create" => "Created",
        "update" => "Changed",
        "delete" => "Deleted",
        _ => "Action",
    }
}

fn action_pill_class(action_type: &str) -> &'static str {
    match action_type {
        "create" => "rio-pill rio-pill-emerald",
        "update" => "rio-pill rio-pill-indigo",
        "delete" => "rio-pill rio-pill-rose",
        _ => "rio-pill",
    }
}

fn relative_time(ts: chrono::DateTime<chrono::Utc>) -> String {
    let now = chrono::Utc::now();
    let delta = now - ts;
    if delta.num_seconds() < 60 {
        "just now".to_string()
    } else if delta.num_minutes() < 60 {
        format!("{}m ago", delta.num_minutes())
    } else if delta.num_hours() < 24 {
        format!("{}h ago", delta.num_hours())
    } else if delta.num_days() < 30 {
        format!("{}d ago", delta.num_days())
    } else {
        ts.format("%Y-%m-%d").to_string()
    }
}

// ---------------------------------------------------------------------------
// Changelist
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub(crate) struct ListCtx {
    #[serde(flatten)]
    pub base: BaseContext,
    pub page_title: String,
    pub entries: Vec<SidebarEntry>,
    pub admin_name: &'static str,
    pub display_name: &'static str,
    pub singular_name: &'static str,
    pub columns: Vec<String>,
    pub rows: Vec<ListRowCtx>,
    pub search_query: String,
    pub filters: Vec<FilterGroupCtx>,
    pub page: usize,
    pub total_pages: usize,
    pub per_page: usize,
    pub total_rows: usize,
    /// Whether the bulk-action UI should render. Always `false` in
    /// Phase 6a — the `/admin/<model>/_action` POST endpoint isn't
    /// wired until a later phase. Templates hide the action bar
    /// when this is `false` so we don't ship UI that 404s on submit.
    pub bulk_actions_enabled: bool,
    pub flash: Option<FlashCtx>,
}

#[derive(Serialize)]
pub(crate) struct ListRowCtx {
    pub id: i64,
    pub cells: Vec<String>,
}

#[derive(Serialize)]
pub(crate) struct FilterGroupCtx {
    pub field: String,
    pub label: String,
    pub options: Vec<FilterOptionCtx>,
    pub current: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct FilterOptionCtx {
    pub value: String,
    pub label: String,
    pub selected: bool,
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn list_ctx(
    identity: &Identity,
    admin: &Admin,
    entry: &AdminEntry,
    rows: Vec<ListRow>,
    search_query: String,
    filters: Vec<FilterGroupCtx>,
    page: usize,
    per_page: usize,
    total_rows: usize,
    csrf_token: String,
) -> ListCtx {
    let total_pages = total_rows.div_ceil(per_page.max(1)).max(1);
    ListCtx {
        base: BaseContext::new(Some(identity), csrf_token, admin),
        page_title: entry.display_name.to_string(),
        entries: admin.entries().iter().map(SidebarEntry::from).collect(),
        admin_name: entry.admin_name,
        display_name: entry.display_name,
        singular_name: entry.singular_name,
        columns: entry.fields.iter().map(|f| f.label.to_string()).collect(),
        rows: rows
            .into_iter()
            .map(|r| ListRowCtx {
                id: r.id,
                cells: r.cells,
            })
            .collect(),
        search_query,
        filters,
        page,
        total_pages,
        per_page,
        total_rows,
        bulk_actions_enabled: false,
        flash: None,
    }
}

// ---------------------------------------------------------------------------
// Change form
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub(crate) struct FormCtx {
    #[serde(flatten)]
    pub base: BaseContext,
    pub page_title: String,
    pub entries: Vec<SidebarEntry>,
    pub admin_name: &'static str,
    pub display_name: &'static str,
    pub singular_name: &'static str,
    pub mode: &'static str, // "new" or "edit"
    pub object_id: Option<i64>,
    pub fields: Vec<FormField>,
    pub errors: Vec<String>,
    pub flash: Option<FlashCtx>,
}

#[derive(Serialize)]
pub(crate) struct FormField {
    pub name: &'static str,
    pub label: &'static str,
    pub widget: &'static str,
    pub input_type: &'static str,
    pub value: String,
    pub hint: Option<String>,
    pub placeholder: Option<String>,
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn form_ctx(
    identity: &Identity,
    admin: &Admin,
    entry: &AdminEntry,
    mode: &'static str,
    object_id: Option<i64>,
    existing: Option<&EditRow>,
    errors: Vec<String>,
    csrf_token: String,
) -> FormCtx {
    let fields = entry
        .fields
        .iter()
        .filter(|f| f.editable)
        .map(|f| {
            let value = existing
                .and_then(|row| {
                    row.values
                        .iter()
                        .find(|(col, _)| col == f.name)
                        .map(|(_, v)| v.clone())
                })
                .unwrap_or_default();
            // Phase 6a: pass None to the classifier (ContextConfig
            // integration deferred to Phase 7).
            let ui = super::intelligence::field_ui_metadata(f, None);
            FormField {
                name: f.name,
                label: f.label,
                widget: f.field_type.widget(),
                input_type: html_input_type_for(f.field_type),
                value,
                hint: ui.hint.map(|s| s.to_string()),
                placeholder: ui.placeholder.map(|s| s.to_string()),
            }
        })
        .collect();

    FormCtx {
        base: BaseContext::new(Some(identity), csrf_token, admin),
        page_title: match mode {
            "new" => format!("Add {}", entry.singular_name),
            _ => format!("Change {}", entry.singular_name),
        },
        entries: admin.entries().iter().map(SidebarEntry::from).collect(),
        admin_name: entry.admin_name,
        display_name: entry.display_name,
        singular_name: entry.singular_name,
        mode,
        object_id,
        fields,
        errors,
        flash: None,
    }
}

fn html_input_type_for(ft: super::types::FieldType) -> &'static str {
    use super::types::FieldType::*;
    match ft {
        Bool => "checkbox",
        I32 | I64 | OptionalI64 => "number",
        DateTime | OptionalDateTime => "datetime-local",
        String | OptionalString => "text",
    }
}

// ---------------------------------------------------------------------------
// Confirm-delete (template body deferred to Phase 6b; context refactored now)
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub(crate) struct ConfirmDeleteCtx {
    #[serde(flatten)]
    pub base: BaseContext,
    pub page_title: String,
    pub entries: Vec<SidebarEntry>,
    pub admin_name: &'static str,
    pub singular_name: &'static str,
    pub object_id: i64,
    pub object_label: String,
    /// Models that point at this one via a `BelongsTo` FK. Each entry
    /// lists what *could* cascade if the DB has `ON DELETE CASCADE`
    /// on the FK constraint. Phase 6b doesn't query row counts —
    /// listing the affected model names is the operator-facing
    /// "are you sure" signal.
    pub cascading: Vec<CascadeItem>,
    pub flash: Option<FlashCtx>,
}

#[derive(Serialize)]
pub(crate) struct CascadeItem {
    pub source_display_name: String,
    pub source_admin_name: String,
    pub source_field: String,
}

pub(crate) fn confirm_delete_ctx(
    identity: &Identity,
    admin: &Admin,
    entry: &AdminEntry,
    object_id: i64,
    object_label: String,
    cascading: Vec<CascadeItem>,
    csrf_token: String,
) -> ConfirmDeleteCtx {
    ConfirmDeleteCtx {
        base: BaseContext::new(Some(identity), csrf_token, admin),
        page_title: format!("Delete {}", entry.singular_name),
        entries: admin.entries().iter().map(SidebarEntry::from).collect(),
        admin_name: entry.admin_name,
        singular_name: entry.singular_name,
        object_id,
        object_label,
        cascading,
        flash: None,
    }
}

// ---------------------------------------------------------------------------
// History pages (Phase 6b/2 — first real audit consumer).
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub(crate) struct HistoryEntryCtx {
    pub timestamp_iso: String,
    pub when_relative: String,
    pub user_email: String,
    pub action_type: String,
    pub label: &'static str,
    pub pill_class: &'static str,
    pub model_name: String,
    pub model_admin_name: String,
    pub object_id: i64,
    pub summary: String,
    pub ip_address: String,
}

#[derive(Serialize)]
pub(crate) struct ObjectHistoryCtx {
    #[serde(flatten)]
    pub base: BaseContext,
    pub page_title: String,
    pub admin_name: String,
    pub display_name: String,
    pub singular_name: String,
    pub object_id: i64,
    pub object_label: String,
    pub entries: Vec<HistoryEntryCtx>,
    pub flash: Option<FlashCtx>,
}

#[derive(Serialize)]
pub(crate) struct LogEntriesCtx {
    #[serde(flatten)]
    pub base: BaseContext,
    pub page_title: &'static str,
    pub entries: Vec<HistoryEntryCtx>,
    pub flash: Option<FlashCtx>,
}

pub(crate) fn map_audit_actions(actions: Vec<super::audit::AdminAction>) -> Vec<HistoryEntryCtx> {
    actions
        .into_iter()
        .map(|a| HistoryEntryCtx {
            timestamp_iso: a.timestamp.to_rfc3339(),
            when_relative: relative_time(a.timestamp),
            user_email: a.user_email.unwrap_or_else(|| "—".to_string()),
            label: action_label(&a.action_type),
            pill_class: action_pill_class(&a.action_type),
            model_name: a.model_name.clone(),
            // Phase 6b assumption: model_name in the audit row IS the
            // admin_name slug. The `audit::record` callsite that lands
            // when actions are logged (Phase 7+) must pass the admin
            // slug here. The dashboard uses the same convention.
            model_admin_name: a.model_name,
            action_type: a.action_type,
            object_id: a.object_id,
            summary: a.summary,
            ip_address: a.ip_address.unwrap_or_default(),
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Password change (self-service — Phase 6b/5).
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub(crate) struct PasswordChangeCtx {
    #[serde(flatten)]
    pub base: BaseContext,
    pub page_title: &'static str,
    pub errors: Vec<String>,
    pub success: bool,
}

// ---------------------------------------------------------------------------
// Error page (orphan render — no live caller in NEW; Phase 9 may add a 5xx
// handler that renders this. Kept Django-shape for design consistency.)
// ---------------------------------------------------------------------------

#[derive(Serialize)]
#[allow(dead_code)]
pub(crate) struct ErrorCtx {
    #[serde(flatten)]
    pub base: BaseContext,
    pub status_code: u16,
    pub status_message: String,
    pub details: String,
}
