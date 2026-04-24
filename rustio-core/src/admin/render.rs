//! Template context builders. Every piece of data the admin templates
//! need comes from here, as a `serde::Serialize` struct. No HTML lives
//! in Rust code.

use serde::Serialize;

use super::types::{AdminEntry, EditRow, ListRow};
use crate::auth::Identity;

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
pub(crate) struct FlashCtx {
    pub kind: &'static str,
    pub message: String,
}

// ---- Page contexts --------------------------------------------------------

#[derive(Serialize)]
pub(crate) struct DashboardCtx {
    pub page_title: &'static str,
    pub identity: IdentityCtx,
    pub entries: Vec<DashboardEntry>,
    pub flash: Option<FlashCtx>,
    pub csrf_token: String,
}

#[derive(Serialize)]
pub(crate) struct DashboardEntry {
    pub admin_name: &'static str,
    pub display_name: &'static str,
    pub field_count: usize,
}

pub(crate) fn dashboard_ctx(
    identity: &Identity,
    entries: &[AdminEntry],
    csrf_token: String,
) -> DashboardCtx {
    let dash_entries = entries
        .iter()
        .map(|e| DashboardEntry {
            admin_name: e.admin_name,
            display_name: e.display_name,
            field_count: e.fields.len(),
        })
        .collect();
    DashboardCtx {
        page_title: "Dashboard",
        identity: identity.into(),
        entries: dash_entries,
        flash: None,
        csrf_token,
    }
}

#[derive(Serialize)]
pub(crate) struct ListCtx {
    pub page_title: String,
    pub identity: IdentityCtx,
    pub entries: Vec<SidebarEntry>,
    pub admin_name: &'static str,
    pub display_name: &'static str,
    pub singular_name: &'static str,
    pub columns: Vec<String>,
    pub rows: Vec<ListRowCtx>,
    pub flash: Option<FlashCtx>,
    pub csrf_token: String,
}

#[derive(Serialize)]
pub(crate) struct ListRowCtx {
    pub id: i64,
    pub cells: Vec<String>,
}

pub(crate) fn list_ctx(
    identity: &Identity,
    all_entries: &[AdminEntry],
    entry: &AdminEntry,
    rows: Vec<ListRow>,
    csrf_token: String,
) -> ListCtx {
    ListCtx {
        page_title: entry.display_name.to_string(),
        identity: identity.into(),
        entries: all_entries.iter().map(SidebarEntry::from).collect(),
        admin_name: entry.admin_name,
        display_name: entry.display_name,
        singular_name: entry.singular_name,
        columns: entry.fields.iter().map(|f| f.label.to_string()).collect(),
        rows: rows
            .into_iter()
            .map(|r| ListRowCtx { id: r.id, cells: r.cells })
            .collect(),
        flash: None,
        csrf_token,
    }
}

#[derive(Serialize)]
pub(crate) struct FormCtx {
    pub page_title: String,
    pub identity: IdentityCtx,
    pub entries: Vec<SidebarEntry>,
    pub admin_name: &'static str,
    pub singular_name: &'static str,
    pub mode: &'static str, // "new" or "edit"
    pub fields: Vec<FormField>,
    pub errors: Vec<String>,
    pub flash: Option<FlashCtx>,
    pub csrf_token: String,
}

#[derive(Serialize)]
pub(crate) struct FormField {
    pub name: &'static str,
    pub label: &'static str,
    pub widget: &'static str,
    pub value: String,
    pub hint: Option<String>,
}

pub(crate) fn form_ctx(
    identity: &Identity,
    all_entries: &[AdminEntry],
    entry: &AdminEntry,
    mode: &'static str,
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
                .and_then(|row| row.values.iter().find(|(col, _)| col == f.name).map(|(_, v)| v.clone()))
                .unwrap_or_default();
            let hint = f.relation.as_ref().map(|r| {
                format!(
                    "Foreign key to {}{}",
                    r.target_model,
                    match r.display_field {
                        Some(d) => format!(" (display: {d})"),
                        None => String::new(),
                    }
                )
            });
            FormField {
                name: f.name,
                label: f.label,
                widget: f.field_type.widget(),
                value,
                hint,
            }
        })
        .collect();

    FormCtx {
        page_title: match mode {
            "new" => format!("Add {}", entry.singular_name),
            _ => format!("Edit {}", entry.singular_name),
        },
        identity: identity.into(),
        entries: all_entries.iter().map(SidebarEntry::from).collect(),
        admin_name: entry.admin_name,
        singular_name: entry.singular_name,
        mode,
        fields,
        errors,
        flash: None,
        csrf_token,
    }
}

#[derive(Serialize)]
pub(crate) struct ConfirmDeleteCtx {
    pub page_title: String,
    pub identity: IdentityCtx,
    pub entries: Vec<SidebarEntry>,
    pub admin_name: &'static str,
    pub singular_name: &'static str,
    pub object_label: String,
    pub flash: Option<FlashCtx>,
    pub csrf_token: String,
}

pub(crate) fn confirm_delete_ctx(
    identity: &Identity,
    all_entries: &[AdminEntry],
    entry: &AdminEntry,
    object_label: String,
    csrf_token: String,
) -> ConfirmDeleteCtx {
    ConfirmDeleteCtx {
        page_title: format!("Delete {}", entry.singular_name),
        identity: identity.into(),
        entries: all_entries.iter().map(SidebarEntry::from).collect(),
        admin_name: entry.admin_name,
        singular_name: entry.singular_name,
        object_label,
        flash: None,
        csrf_token,
    }
}

#[derive(Serialize)]
pub(crate) struct LoginCtx {
    pub error: Option<String>,
    pub csrf_token: String,
}
