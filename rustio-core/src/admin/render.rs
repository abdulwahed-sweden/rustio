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

use std::collections::HashMap;

use serde::Serialize;

use super::audit::AdminAction;
use super::types::{Admin, AdminEntry, EditRow, ListRow};
use crate::auth::Identity;

#[derive(Serialize)]
pub(crate) struct IdentityCtx {
    pub email: String,
    pub is_admin: bool,
    /// Phase 7a/2 — exposed so the sidebar can show the
    /// Developer-only section (`/admin/__schema__`, `/__logs__`,
    /// `/__sql_console__`) without making it look like dead nav for
    /// Administrator-rank users.
    pub is_developer: bool,
}

impl From<&Identity> for IdentityCtx {
    fn from(i: &Identity) -> Self {
        Self {
            email: i.email.clone(),
            is_admin: i.is_admin(),
            is_developer: i.is_active && i.role.includes(crate::auth::Role::Developer),
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
    /// Phase 7a/0.5/d — `true` when the active session belongs to a
    /// demo user (`is_demo` column on `rustio_users`). Templates use
    /// this to render the red banner above the page content.
    pub is_demo_session: bool,
    /// Optional human-readable label for the demo user
    /// ("Demo Staff", "Demo Administrator"). Rendered in parens after
    /// the banner's "DEMO USER" text when present.
    pub demo_label: Option<String>,
}

impl BaseContext {
    /// Build the shared base context every page extends. Reads the
    /// active branding from `&Admin` so projects can override defaults
    /// via `Admin::site_branding(...)`. Future Phase 8/9 may pull more
    /// from `Admin` (locale, theme) without re-touching every handler.
    pub fn new(identity: Option<&Identity>, csrf_token: String, admin: &Admin) -> Self {
        let b = admin.branding();
        let (is_demo_session, demo_label) = match identity {
            Some(i) => (i.is_demo, i.demo_label.clone()),
            None => (false, None),
        };
        Self {
            identity: identity.map(IdentityCtx::from),
            csrf_token,
            site_title: b.site_title.clone(),
            site_header: b.site_header.clone(),
            index_title: b.index_title.clone(),
            footer_copyright: b.footer_copyright.clone(),
            is_demo_session,
            demo_label,
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
    /// Phase 7a/2 — sidebar nav. Same shape every other page already
    /// uses (list/form/confirm-delete/builtin), so the base.html
    /// sidebar partial works uniformly across all pages.
    pub entries: Vec<SidebarEntry>,
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
        entries: admin.entries().iter().filter(|e| !e.core).map(SidebarEntry::from).collect(),
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
        "create" => "badge-success",
        "update" => "badge-neutral",
        "delete" => "badge-danger",
        _ => "badge-neutral",
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

/// Phase 5/a — describes one column of the changelist table. Replaces
/// the previous `columns: Vec<String>` shape on `ListCtx` so templates
/// can drive both the header label AND the row-cell key from a single
/// loop (`{% for field in fields %}<td>{{ row[field.name] }}</td>{% endfor %}`).
#[derive(Serialize)]
pub(crate) struct ListField {
    pub name: String,
    pub label: String,
}

#[derive(Serialize)]
pub(crate) struct ListCtx {
    #[serde(flatten)]
    pub base: BaseContext,
    pub page_title: String,
    pub entries: Vec<SidebarEntry>,
    pub admin_name: &'static str,
    pub display_name: &'static str,
    pub singular_name: &'static str,
    pub fields: Vec<ListField>,
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

/// Phase 5/a — `values` is flattened into the JSON object so template
/// code can do `row[field.name]` (minijinja resolves dict subscript on
/// the merged map). The explicit `id: i64` struct field stays out of
/// the flattened map (the loader skips inserting an "id" key) so
/// `row.id` continues to render as the integer id without colliding
/// with any model field literally named "id".
#[derive(Serialize)]
pub(crate) struct ListRowCtx {
    pub id: i64,
    #[serde(flatten)]
    pub values: HashMap<String, String>,
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
    let fields: Vec<ListField> = entry
        .fields
        .iter()
        .map(|f| ListField {
            name: f.name.to_string(),
            label: f.label.to_string(),
        })
        .collect();
    // Field-name positions used to convert each row's positional cells
    // (Vec<String>) into a name-keyed values map. Stays in lockstep with
    // `entry.fields` because `AdminModel::display_values` is generated
    // in the same field order as `FIELDS`.
    let field_names: Vec<&'static str> = entry.fields.iter().map(|f| f.name).collect();
    ListCtx {
        base: BaseContext::new(Some(identity), csrf_token, admin),
        page_title: entry.display_name.to_string(),
        entries: admin.entries().iter().filter(|e| !e.core).map(SidebarEntry::from).collect(),
        admin_name: entry.admin_name,
        display_name: entry.display_name,
        singular_name: entry.singular_name,
        fields,
        rows: rows
            .into_iter()
            .map(|r| {
                let mut values: HashMap<String, String> =
                    HashMap::with_capacity(field_names.len().saturating_sub(1));
                for (i, cell) in r.cells.into_iter().enumerate() {
                    if let Some(name) = field_names.get(i) {
                        // Skip the "id" key so the explicit `id: i64`
                        // struct field wins on serialization (otherwise
                        // a flatten-map "id" string would shadow it).
                        if *name != "id" {
                            values.insert((*name).to_string(), cell);
                        }
                    }
                }
                ListRowCtx { id: r.id, values }
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
    /// Phase 6 — fields grouped into logical sections. The form
    /// template iterates `sections` and within each its `fields`.
    /// Phase 1/b's flat `fields: Vec<FormField>` is gone; group_into
    /// the heuristic in `form_ctx` always emits at least one section
    /// when the model has any editable fields.
    pub sections: Vec<FormSection>,
    pub errors: Vec<String>,
    pub flash: Option<FlashCtx>,
}

/// Phase 5/d — one option in a `<select>` list. Both fields are
/// `String` because options come from runtime data: enum choices
/// (static strings copied), foreign-key rows (id → display label),
/// many-to-many memberships. The label and value can diverge — for
/// FK selects, `value` is the row id, `label` is the human-readable
/// display string.
#[derive(Serialize)]
pub(crate) struct SelectOption {
    pub value: String,
    pub label: String,
}

#[derive(Serialize)]
pub(crate) struct FormField {
    pub name: &'static str,
    /// Phase 1/b — humanised label sourced from
    /// `intelligence::field_ui_metadata` ("created_at" → "Created At")
    /// instead of the raw column name. `String` because the intelligence
    /// layer owns the buffer.
    pub label: String,
    pub widget: &'static str,
    pub input_type: &'static str,
    pub value: String,
    pub hint: Option<String>,
    pub placeholder: Option<String>,
    /// Phase 1/b — `true` for fields the form template should mark
    /// with the required-asterisk. Optional types and booleans never
    /// carry the marker (booleans always submit a value, optionals
    /// are explicitly nullable).
    pub required: bool,
    /// Phase 5/d — populated when `widget == "select"`. `None` for
    /// non-select widgets so serialisation doesn't carry an empty
    /// list per field.
    pub options: Option<Vec<SelectOption>>,
    /// Phase 5/d — `true` for many-to-many relations so the template
    /// emits `<select multiple>`. `false` for single-select / non-select
    /// widgets.
    pub multiple: bool,
    /// Phase 6 — grid-span hint. `1` (default) renders the field at
    /// half-width inside the section's `grid-cols-2`; `2` makes the
    /// field span both columns. Currently set to `2` for textareas,
    /// `1` everywhere else; the form template branches on this with
    /// `{% if field.span == 2 %}col-span-2{% endif %}`.
    pub span: u8,
}

/// Phase 6 — one logical group of fields on a form. `title: None`
/// renders without an `<h3>` (used for the default "core fields"
/// section). Sections preserve insertion order, and fields within a
/// section preserve the macro's `FIELDS` order.
#[derive(Serialize)]
pub(crate) struct FormSection {
    pub title: Option<&'static str>,
    pub fields: Vec<FormField>,
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
            // Phase 5/c+d — base widget + input_type from the centralized
            // mapping. Sees choices + relation, so enum / FK / M2M fields
            // resolve to ("select", "select" | "select-multiple") here.
            let (base_widget, input_type) = map_field_to_ui(f);
            // Phase 7a/2/critical-fix — String fields with content-y
            // names (body / description / notes / content / summary)
            // render as <textarea> instead of the base single-line
            // <input>. The mapping fn intentionally doesn't see the
            // field name, so this name-hint override stays in the
            // caller. Only fires when the base mapping landed on
            // "input" — a select-shaped field (enum/FK/M2M) doesn't
            // get rewritten to textarea even if its name happens to
            // be "body".
            let widget = if base_widget == "input"
                && matches!(
                    f.field_type,
                    super::types::FieldType::String | super::types::FieldType::OptionalString
                )
                && is_long_text_name(f.name)
            {
                "textarea"
            } else {
                base_widget
            };
            // Phase 1/b — bools always submit (checked = true, absent
            // = false), so they never carry a required-asterisk; every
            // other non-nullable field does.
            let required = !f.field_type.nullable()
                && !matches!(f.field_type, super::types::FieldType::Bool);
            // Phase 5/d — select options + multiple flag.
            //   - Enum (choices): one option per allowed value (raw
            //     string used as both value and label per "no
            //     invented content" rule).
            //   - FK / M2M (relation): mocked option list for now.
            //     A real DB-backed lookup is the next sub-phase per
            //     spec ("Mock for now (no DB query yet)").
            //   - Everything else: None / false.
            let (options, multiple) = if let Some(values) = f.choices {
                let mut opts: Vec<SelectOption> = Vec::with_capacity(values.len() + 1);
                // Phase 7 / F4 — nullable enum fields prepend a leading
                // empty option so the user can clear the selection back
                // to NULL. Required (non-nullable) enum fields skip
                // this; the HTML5 `required` attribute on `<select>`
                // (added in F3) blocks empty submission.
                if f.field_type.nullable() {
                    opts.push(SelectOption {
                        value: String::new(),
                        label: "—".to_string(),
                    });
                }
                opts.extend(values.iter().map(|v| SelectOption {
                    value: (*v).to_string(),
                    label: (*v).to_string(),
                }));
                (Some(opts), false)
            } else if let Some(rel) = &f.relation {
                (
                    Some(vec![
                        SelectOption { value: "1".into(), label: "Item 1".into() },
                        SelectOption { value: "2".into(), label: "Item 2".into() },
                    ]),
                    rel.multi,
                )
            } else {
                (None, false)
            };
            // Phase 6 — span hint. Long-text textareas span the full
            // grid (col-span-2); everything else takes one half.
            let span: u8 = if widget == "textarea" { 2 } else { 1 };
            FormField {
                name: f.name,
                label: ui.label,
                widget,
                input_type,
                value,
                hint: ui.hint,
                placeholder: ui.placeholder,
                required,
                options,
                multiple,
                span,
            }
        })
        .collect::<Vec<FormField>>();

    // Phase 6 — group fields into Default / Metadata / Advanced
    // sections via a deterministic name heuristic. Order within each
    // section is preserved from the macro's FIELDS order.
    let sections = group_fields_into_sections(fields);

    FormCtx {
        base: BaseContext::new(Some(identity), csrf_token, admin),
        page_title: match mode {
            "new" => format!("Add {}", entry.singular_name),
            _ => format!("Change {}", entry.singular_name),
        },
        entries: admin.entries().iter().filter(|e| !e.core).map(SidebarEntry::from).collect(),
        admin_name: entry.admin_name,
        display_name: entry.display_name,
        singular_name: entry.singular_name,
        mode,
        object_id,
        sections,
        errors,
        flash: None,
    }
}

/// Phase 6 — partition the form's flat field list into three logical
/// sections by name heuristic. Default (untitled) collects business
/// fields; Metadata collects audit-trail timestamps; Advanced collects
/// system identifiers. Empty sections are dropped, so a form with no
/// audit / id fields still renders as a single section.
fn group_fields_into_sections(fields: Vec<FormField>) -> Vec<FormSection> {
    let mut default_fields = Vec::new();
    let mut metadata_fields = Vec::new();
    let mut advanced_fields = Vec::new();

    for field in fields {
        match classify_field_section(field.name) {
            FieldSection::Default => default_fields.push(field),
            FieldSection::Metadata => metadata_fields.push(field),
            FieldSection::Advanced => advanced_fields.push(field),
        }
    }

    let mut sections: Vec<FormSection> = Vec::with_capacity(3);
    if !default_fields.is_empty() {
        sections.push(FormSection { title: None, fields: default_fields });
    }
    if !metadata_fields.is_empty() {
        sections.push(FormSection { title: Some("Metadata"), fields: metadata_fields });
    }
    if !advanced_fields.is_empty() {
        sections.push(FormSection { title: Some("Advanced"), fields: advanced_fields });
    }
    sections
}

/// Phase 6 — section bucket for a single field name. Substring match
/// for audit-trail words (so `created_at`, `updated_at`,
/// `creation_timestamp` all land in Metadata); exact match for system
/// identifiers (so `user_id`, `application_id` etc. stay in Default —
/// they're business-meaningful FKs, not "advanced" internals).
enum FieldSection {
    Default,
    Metadata,
    Advanced,
}

fn classify_field_section(name: &str) -> FieldSection {
    if name.contains("created") || name.contains("updated") || name.contains("timestamp") {
        FieldSection::Metadata
    } else if matches!(name, "id" | "uuid" | "slug") {
        FieldSection::Advanced
    } else {
        FieldSection::Default
    }
}

/// Phase 7a/2/critical-fix — names that imply multi-line content.
/// Used by `form_ctx` to upgrade a `String` / `OptionalString` field
/// to a `<textarea>` instead of a single-line `<input>`. Conservative
/// list; expand only when a real model needs it.
fn is_long_text_name(name: &str) -> bool {
    matches!(
        name,
        "body" | "description" | "notes" | "content" | "summary" | "bio" | "details"
    )
}

/// Phase 5/c+d — backend-driven field-to-UI mapping.
///
/// Returns the (`widget`, `input_type`) pair the form template should
/// render for a given `AdminField`. The signature takes `&AdminField`
/// (Phase 5/d change from the original `&FieldType`) so the function
/// can see relation + choices metadata without the caller needing
/// per-site logic. Resolution priority (top-down):
///
///   1. `field.choices.is_some()` → enum-style `<select>`.
///   2. `field.relation.is_some()` && `relation.multi` → `<select multiple>`.
///   3. `field.relation.is_some()` (belongs-to) → single `<select>`.
///   4. Fall through to `field.field_type` mapping (the Phase 5/c rules).
///
/// Adding a new `FieldType` variant remains a one-arm change in the
/// final `match`. The first three arms are additive: any field with
/// choices or a relation overrides the FieldType-based mapping.
///
/// Returns `&'static str` (not `String`) because `FormField.widget` /
/// `.input_type` are already `&'static str`; allocating per call would
/// force a downstream type change with no behavioural benefit.
///
/// The `String → ("input", "text")` rule remains the base for plain
/// strings. Long-text override (`textarea` for `body`/`description`/
/// etc.) lives in `form_ctx` because it depends on the field NAME, not
/// just type.
fn map_field_to_ui(field: &super::types::AdminField) -> (&'static str, &'static str) {
    // 1. Closed-list enum → `<select>`. Trumps any FieldType mapping.
    if field.choices.is_some() {
        return ("select", "select");
    }
    // 2. & 3. Relation-backed → `<select>`, with `select-multiple` for M2M.
    if let Some(rel) = &field.relation {
        if rel.multi {
            return ("select", "select-multiple");
        }
        return ("select", "select");
    }
    // 4. Fall through to the FieldType-based base mapping.
    use super::types::FieldType::*;
    match field.field_type {
        Bool => ("checkbox", "checkbox"),
        I32 | I64 | OptionalI64 => ("input", "number"),
        DateTime | OptionalDateTime => ("input", "datetime-local"),
        String | OptionalString => ("input", "text"),
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
        entries: admin.entries().iter().filter(|e| !e.core).map(SidebarEntry::from).collect(),
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
// "Coming in Phase 8" stub page (Phase 7a/0.5/e — Developer-only).
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub(crate) struct ComingSoonCtx {
    #[serde(flatten)]
    pub base: BaseContext,
    /// Phase 7a/2 — sidebar nav entries (Developer-tier sees this
    /// page; the sidebar should still render with full nav).
    pub entries: Vec<SidebarEntry>,
    pub page_title: String,
    pub feature_name: String,
    pub description: String,
}

// ---------------------------------------------------------------------------
// 403 Forbidden page (Phase 7a/0.5/b).
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub(crate) struct ForbiddenCtx {
    #[serde(flatten)]
    pub base: BaseContext,
    /// Phase 7a/2 — sidebar entries; even on a 403 the user is
    /// authenticated, so the sidebar should render with whichever
    /// surfaces they CAN reach.
    pub entries: Vec<SidebarEntry>,
    pub page_title: &'static str,
    /// The permission codename or URL the user tried to reach. Shown
    /// in the body when present so the operator can audit how the
    /// guard fired without trawling logs.
    pub attempted: Option<String>,
    /// The minimum role required by the page that rejected them.
    /// `None` for permission failures, `Some(label)` for role-tier
    /// failures.
    pub required_role: Option<&'static str>,
}

/// Build the 403 response body. Free function (not on `AdminCtx`)
/// so the unit tests can render the page with just `Templates` +
/// `Admin` — no `Db` required.
pub(crate) fn render_forbidden_body(
    admin: &Admin,
    templates: &crate::templates::Templates,
    identity: &Identity,
    csrf_token: String,
    attempted: Option<String>,
    required_role: Option<&'static str>,
) -> crate::error::Result<String> {
    let view = ForbiddenCtx {
        base: BaseContext::new(Some(identity), csrf_token, admin),
        entries: admin.entries().iter().filter(|e| !e.core).map(SidebarEntry::from).collect(),
        page_title: "Permission denied",
        attempted,
        required_role,
    };
    templates.render("admin/forbidden.html", &view)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::admin::FieldType;
    use crate::auth::Role;
    use crate::templates::Templates;

    /// Build a minimal AdminField for mapping tests. Phase 5/d shifted
    /// the mapping fn to take `&AdminField` (so it can see relation +
    /// choices); this helper hides the boilerplate.
    fn af(field_type: FieldType) -> crate::admin::AdminField {
        crate::admin::AdminField {
            name: "x",
            label: "x",
            field_type,
            editable: true,
            relation: None,
            choices: None,
        }
    }

    /// Phase 5/c — locks the FieldType→UI mapping. Adding a new
    /// FieldType variant requires updating this test along with the
    /// match in `map_field_to_ui`; that's the single place to encode
    /// "what UI does this field render as".
    #[test]
    fn maps_field_types_to_expected_widgets() {
        assert_eq!(map_field_to_ui(&af(FieldType::Bool)),             ("checkbox", "checkbox"));
        assert_eq!(map_field_to_ui(&af(FieldType::String)),           ("input", "text"));
        assert_eq!(map_field_to_ui(&af(FieldType::OptionalString)),   ("input", "text"));
        assert_eq!(map_field_to_ui(&af(FieldType::I32)),              ("input", "number"));
        assert_eq!(map_field_to_ui(&af(FieldType::I64)),              ("input", "number"));
        assert_eq!(map_field_to_ui(&af(FieldType::OptionalI64)),      ("input", "number"));
        assert_eq!(map_field_to_ui(&af(FieldType::DateTime)),         ("input", "datetime-local"));
        assert_eq!(map_field_to_ui(&af(FieldType::OptionalDateTime)), ("input", "datetime-local"));
    }

    /// Phase 5/d — a field with a non-empty `choices` slice resolves
    /// to a `<select>` regardless of its underlying FieldType. Locks
    /// resolution priority #1 in `map_field_to_ui`.
    #[test]
    fn enum_field_renders_select() {
        const VALUES: &[&str] = &["draft", "published", "archived"];
        let mut field = af(FieldType::String);
        field.choices = Some(VALUES);
        assert_eq!(map_field_to_ui(&field), ("select", "select"));

        // FieldType::I64 with choices also resolves to select — the
        // choices arm runs before the FieldType match.
        let mut numeric = af(FieldType::I64);
        numeric.choices = Some(VALUES);
        assert_eq!(map_field_to_ui(&numeric), ("select", "select"));
    }

    /// Phase 5/d — a relation with `multi: true` produces
    /// `("select", "select-multiple")`; default `multi: false` stays
    /// single-select. Locks resolution priority #2 vs #3 in
    /// `map_field_to_ui`.
    #[test]
    fn relation_multi_sets_multiple() {
        let mut single = af(FieldType::I64);
        single.relation = Some(crate::admin::AdminRelation {
            target_model: "posts",
            display_field: None,
            multi: false,
        });
        assert_eq!(map_field_to_ui(&single), ("select", "select"));

        let mut many = af(FieldType::I64);
        many.relation = Some(crate::admin::AdminRelation {
            target_model: "tags",
            display_field: None,
            multi: true,
        });
        assert_eq!(map_field_to_ui(&many), ("select", "select-multiple"));
    }

    fn fake_identity(role: Role) -> Identity {
        Identity {
            user_id: 1,
            email: "test@example.com".into(),
            role,
            is_active: true,
            is_demo: false,
            demo_label: None,
        }
    }

    #[test]
    fn render_forbidden_body_with_required_role() {
        let admin = Admin::new();
        let templates = Templates::new(None).expect("embedded templates");
        let ident = fake_identity(Role::Staff);

        let body = render_forbidden_body(
            &admin,
            &templates,
            &ident,
            "fake-csrf".into(),
            None,
            Some("Administrator"),
        )
        .expect("forbidden page renders");

        assert!(body.contains("Permission denied"), "page h1 missing");
        assert!(
            body.contains("Administrator"),
            "required_role hint should be in body"
        );
        assert!(
            body.contains("Return to dashboard"),
            "back link missing"
        );
        // Identity-bearing surfaces still render: the user-tools welcome
        // line should include the test email.
        assert!(body.contains("test@example.com"), "user-tools email missing");
    }

    #[test]
    fn user_new_form_has_five_role_options() {
        // Render `admin/user_new.html` directly with a hand-built
        // context — the actual `UserNewCtx` struct is private to
        // `builtin.rs`. Asserts the 5-option select replaces the
        // pre-7a/0.5/d 2-checkbox UI.
        let templates = Templates::new(None).expect("embedded templates");
        let ctx = serde_json::json!({
            "site_title": "RustIO administration",
            "site_header": "RustIO administration",
            "index_title": "Site administration",
            "footer_copyright": "RustIO test",
            "csrf_token": "fake",
            "is_demo_session": false,
            "demo_label": null,
            "page_title": "Add user",
            "entries": [],
            "email": "",
            "role": "staff",
            "errors": [],
            "identity": { "email": "admin@example.com", "is_admin": true },
        });
        let body = templates
            .render("admin/user_new.html", &ctx)
            .expect("user_new renders");

        for value in ["user", "staff", "supervisor", "administrator", "developer"] {
            assert!(
                body.contains(&format!("value=\"{value}\"")),
                "role option {value:?} missing"
            );
        }
        // The default `role: "staff"` should make Staff the selected
        // option. Look for `value="staff"` followed by whitespace then
        // `selected` (template's inline `{% if role == "staff" %}` may
        // emit variable spacing — be tolerant).
        let staff_idx = body.find("value=\"staff\"").expect("staff option");
        let after_staff = &body[staff_idx..staff_idx.saturating_add(80)];
        assert!(
            after_staff.contains("selected"),
            "Staff option should be selected; got: {after_staff:?}"
        );
        // The pre-7a/0.5/d checkbox names must NOT appear.
        assert!(
            !body.contains("name=\"is_staff\""),
            "old is_staff checkbox should be gone"
        );
        assert!(
            !body.contains("name=\"is_superuser\""),
            "old is_superuser checkbox should be gone"
        );
    }

    #[test]
    fn render_base_with_demo_banner() {
        let admin = Admin::new();
        let templates = Templates::new(None).expect("embedded templates");
        let demo_ident = Identity {
            user_id: 1,
            email: "staff@rustio.local".into(),
            role: Role::Staff,
            is_active: true,
            is_demo: true,
            demo_label: Some("Demo Staff".into()),
        };

        // Render the dashboard page (any page that extends base.html
        // works) and assert the banner is present with the label.
        let dash = dashboard_ctx(&demo_ident, &admin, vec![], "fake-csrf".into());
        let body = templates.render("admin/index.html", &dash).expect("dashboard renders");

        assert!(body.contains("DEMO USER"), "demo banner text missing");
        assert!(body.contains("Demo Staff"), "demo_label not in banner");
        assert!(
            body.contains("RUSTIO_DEMO_MODE"),
            "banner should reference the env flag"
        );
    }

    #[test]
    fn render_base_without_demo_banner_for_real_user() {
        let admin = Admin::new();
        let templates = Templates::new(None).expect("embedded templates");
        let real_ident = Identity {
            user_id: 1,
            email: "admin@example.com".into(),
            role: Role::Administrator,
            is_active: true,
            is_demo: false,
            demo_label: None,
        };

        let dash = dashboard_ctx(&real_ident, &admin, vec![], "fake-csrf".into());
        let body = templates.render("admin/index.html", &dash).expect("dashboard renders");

        assert!(
            !body.contains("DEMO USER"),
            "demo banner must NOT render for is_demo=false"
        );
        assert!(
            !body.contains("RUSTIO_DEMO_MODE"),
            "banner copy must be absent for real users"
        );
    }

    #[test]
    fn form_renders_required_marker_humanised_label_and_cancel() {
        // Phase 1/b — template-only assertion. Hand-built JSON ctx
        // mirrors the shape `form_ctx` produces; exercises the three
        // user-visible additions (humanised label, required-asterisk,
        // Cancel button) without depending on AdminEntry plumbing.
        let templates = Templates::new(None).expect("embedded templates");
        let ctx = serde_json::json!({
            "site_title": "RustIO administration",
            "site_header": "RustIO administration",
            "index_title": "Site administration",
            "footer_copyright": "RustIO test",
            "csrf_token": "fake",
            "is_demo_session": false,
            "demo_label": null,
            "page_title": "Add post",
            "entries": [],
            "admin_name": "posts",
            "display_name": "Posts",
            "singular_name": "Post",
            "mode": "new",
            "object_id": null,
            "errors": [],
            "identity": { "email": "admin@example.com", "is_admin": true, "is_developer": false },
            "sections": [
                {
                    "title": null,
                    "fields": [
                        {
                            "name": "title",
                            "label": "Title",
                            "widget": "text",
                            "input_type": "text",
                            "value": "",
                            "hint": null,
                            "placeholder": null,
                            "required": true,
                            "options": null,
                            "multiple": false,
                            "span": 1,
                        },
                        {
                            "name": "published",
                            "label": "Published",
                            "widget": "checkbox",
                            "input_type": "checkbox",
                            "value": "false",
                            "hint": null,
                            "placeholder": null,
                            "required": false,
                            "options": null,
                            "multiple": false,
                            "span": 1,
                        },
                    ],
                },
            ],
        });
        let body = templates
            .render("admin/form.html", &ctx)
            .expect("form renders");

        // Humanised label is rendered.
        assert!(body.contains(">Title"), "humanised Title label missing");

        // Required-asterisk: present for `title`, absent for `published`.
        // The label tag for `title` should contain the marker.
        let title_label_idx = body
            .find("for=\"id_title\"")
            .expect("title label present");
        let title_label_end = title_label_idx
            + body[title_label_idx..]
                .find("</label>")
                .expect("label closes");
        let title_label = &body[title_label_idx..title_label_end];
        assert!(
            title_label.contains("class=\"required\""),
            "title label should carry the required marker, got: {title_label:?}"
        );

        let published_label_idx = body
            .find("for=\"id_published\"")
            .expect("published label present");
        let published_label_end = published_label_idx
            + body[published_label_idx..]
                .find("</label>")
                .expect("label closes");
        let published_label = &body[published_label_idx..published_label_end];
        assert!(
            !published_label.contains("class=\"required\""),
            "non-required field should not carry the marker, got: {published_label:?}"
        );

        // Cancel button points at the list page for this admin.
        assert!(
            body.contains("href=\"/admin/posts/\"") && body.contains(">\n            Cancel"),
            "Cancel link to list page missing",
        );

        // Save button still there — regression guard.
        assert!(body.contains("name=\"_save\""), "Save button missing");
    }

    /// Phase 6 — `form_ctx` partitions editable fields into Default,
    /// Metadata (audit-trail names), and Advanced (system identifier
    /// names) sections. Empty buckets are dropped so a model with only
    /// business fields renders as a single section.
    #[test]
    fn fields_are_grouped_into_sections() {
        // Build an AdminEntry with one field per bucket. All editable
        // so each appears in the form. Names chosen to fire each
        // heuristic arm:
        //   - "title"        → Default
        //   - "creation_timestamp" → Metadata (substring "timestamp")
        //   - "uuid"         → Advanced (exact match)
        static MIXED_FIELDS: &[crate::admin::AdminField] = &[
            crate::admin::AdminField {
                name: "title",
                label: "title",
                field_type: FieldType::String,
                editable: true,
                relation: None,
                choices: None,
            },
            crate::admin::AdminField {
                name: "creation_timestamp",
                label: "creation_timestamp",
                field_type: FieldType::DateTime,
                editable: true,
                relation: None,
                choices: None,
            },
            crate::admin::AdminField {
                name: "uuid",
                label: "uuid",
                field_type: FieldType::String,
                editable: true,
                relation: None,
                choices: None,
            },
        ];
        let admin = Admin::new();
        let entry = AdminEntry::for_testing(
            "posts", "Posts", "Post", "posts", MIXED_FIELDS, false,
        );
        let ident = fake_identity(Role::Administrator);
        let ctx = form_ctx(&ident, &admin, &entry, "new", None, None, vec![], "csrf".into());

        // Expect three sections in fixed order: Default → Metadata → Advanced.
        assert_eq!(ctx.sections.len(), 3, "expected three sections, got {ctx_len:?}",
            ctx_len = ctx.sections.iter().map(|s| s.title).collect::<Vec<_>>());
        assert_eq!(ctx.sections[0].title, None,                "first section is the untitled default bucket");
        assert_eq!(ctx.sections[0].fields.len(), 1);
        assert_eq!(ctx.sections[0].fields[0].name, "title");
        assert_eq!(ctx.sections[1].title, Some("Metadata"),    "second section is Metadata");
        assert_eq!(ctx.sections[1].fields.len(), 1);
        assert_eq!(ctx.sections[1].fields[0].name, "creation_timestamp");
        assert_eq!(ctx.sections[2].title, Some("Advanced"),    "third section is Advanced");
        assert_eq!(ctx.sections[2].fields.len(), 1);
        assert_eq!(ctx.sections[2].fields[0].name, "uuid");

        // Common-case regression guard: a `user_id` field stays in
        // the Default section (FK with business meaning, not "advanced").
        static FK_FIELDS: &[crate::admin::AdminField] = &[
            crate::admin::AdminField {
                name: "title",
                label: "title",
                field_type: FieldType::String,
                editable: true,
                relation: None,
                choices: None,
            },
            crate::admin::AdminField {
                name: "user_id",
                label: "user_id",
                field_type: FieldType::I64,
                editable: true,
                relation: None,
                choices: None,
            },
        ];
        let entry2 = AdminEntry::for_testing(
            "posts", "Posts", "Post", "posts", FK_FIELDS, false,
        );
        let ctx2 = form_ctx(&ident, &admin, &entry2, "new", None, None, vec![], "csrf".into());
        assert_eq!(ctx2.sections.len(), 1, "FK fields must NOT go to Advanced — they're business-meaningful");
        assert_eq!(ctx2.sections[0].fields.len(), 2);
    }

    /// Phase 6 — a textarea-shaped field carries `span = 2`. The form
    /// template renders that field's wrapper with `col-span-2` so it
    /// fills the row instead of sharing it with a sibling. Locks
    /// both the form_ctx span computation AND the template's
    /// `{% if field.span == 2 %}col-span-2{% endif %}` branch.
    #[test]
    fn textarea_fields_span_full_width() {
        static TEXTAREA_FIELDS: &[crate::admin::AdminField] = &[
            // "body" hits the long-text-name heuristic → widget = "textarea".
            crate::admin::AdminField {
                name: "body",
                label: "body",
                field_type: FieldType::String,
                editable: true,
                relation: None,
                choices: None,
            },
            // Plain string field for contrast.
            crate::admin::AdminField {
                name: "title",
                label: "title",
                field_type: FieldType::String,
                editable: true,
                relation: None,
                choices: None,
            },
        ];
        let admin = Admin::new();
        let entry = AdminEntry::for_testing(
            "posts", "Posts", "Post", "posts", TEXTAREA_FIELDS, false,
        );
        let ident = fake_identity(Role::Administrator);
        let ctx = form_ctx(&ident, &admin, &entry, "new", None, None, vec![], "csrf".into());

        let body_field = ctx.sections[0]
            .fields
            .iter()
            .find(|f| f.name == "body")
            .expect("body field present");
        assert_eq!(body_field.widget, "textarea");
        assert_eq!(body_field.span, 2, "textarea must span both columns");

        let title_field = ctx.sections[0]
            .fields
            .iter()
            .find(|f| f.name == "title")
            .expect("title field present");
        assert_eq!(title_field.widget, "input");
        assert_eq!(title_field.span, 1, "plain input takes one column");

        // Template renders the col-span-2 wrapper for the textarea.
        let templates = Templates::new(None).expect("embedded templates");
        let body = templates
            .render("admin/form.html", &ctx)
            .expect("form renders");

        // The wrapper around the textarea field must include col-span-2.
        let body_wrapper_idx = body
            .find("class=\"col-span-2\"")
            .expect("col-span-2 wrapper present");
        let body_after = &body[body_wrapper_idx..];
        assert!(
            body_after.contains("name=\"body\""),
            "col-span-2 wrapper should contain the body field"
        );
    }

    /// Phase 1/c — shared list-page context fixture. Returns a JSON
    /// value with the empty-state shape (`rows: []`, `total_rows: 0`).
    /// Callers patch `search_query` / `filters` to flip between the
    /// true-empty and filtered-empty branches. Phase 5/a — `fields`
    /// replaces the old `columns` shape; row iteration in the template
    /// now uses `row[field.name]` to read each cell.
    fn empty_list_ctx_skeleton() -> serde_json::Value {
        serde_json::json!({
            "site_title": "RustIO administration",
            "site_header": "RustIO administration",
            "index_title": "Site administration",
            "footer_copyright": "RustIO test",
            "csrf_token": "fake",
            "is_demo_session": false,
            "demo_label": null,
            "page_title": "Posts",
            "entries": [],
            "admin_name": "posts",
            "display_name": "Posts",
            "singular_name": "Post",
            "fields": [
                { "name": "title",  "label": "title"  },
                { "name": "body",   "label": "body"   },
                { "name": "author", "label": "author" },
            ],
            "rows": [],
            "search_query": "",
            "filters": [],
            "page": 1,
            "total_pages": 1,
            "per_page": 25,
            "total_rows": 0,
            "bulk_actions_enabled": false,
            "identity": { "email": "admin@example.com", "is_admin": true, "is_developer": false },
        })
    }

    /// Phase 5/a — exercises the dynamic-row path: headers are driven
    /// by `fields[].label`, cells by `row[field.name]`. The empty-state
    /// tests above all hit the `{% else %}` branch and never iterate
    /// rows; this one renders a row so the new lookup path is locked
    /// in. Regression target: a future change that mistakenly drops
    /// the `flatten` on `ListRowCtx.values` would fail the cell
    /// assertions below.
    #[test]
    fn list_renders_rows_via_field_keyed_lookup() {
        let templates = Templates::new(None).expect("embedded templates");
        let mut ctx = empty_list_ctx_skeleton();
        ctx["rows"] = serde_json::json!([
            {
                "id": 7,
                "title": "Alpha",
                "body": "first body",
                "author": "alice",
            },
            {
                "id": 9,
                "title": "Beta",
                "body": "second body",
                "author": "bob",
            },
        ]);
        ctx["total_rows"] = serde_json::json!(2);
        let body = templates
            .render("admin/list.html", &ctx)
            .expect("list renders");

        // Header row: one <th> per field, label rendered.
        assert!(body.contains(">title</th>"), "title header missing");
        assert!(body.contains(">body</th>"),  "body header missing");
        assert!(body.contains(">author</th>"),"author header missing");

        // Row 7: first column wrapped in the edit anchor; subsequent
        // cells render as plain text.
        assert!(
            body.contains("href=\"/admin/posts/7/edit\">Alpha</a>"),
            "row 7 first-column edit-link missing"
        );
        assert!(body.contains("first body"), "row 7 body cell missing");
        assert!(body.contains("alice"),      "row 7 author cell missing");

        // Row 9 same.
        assert!(
            body.contains("href=\"/admin/posts/9/edit\">Beta</a>"),
            "row 9 first-column edit-link missing"
        );
        assert!(body.contains("second body"), "row 9 body cell missing");
        assert!(body.contains("bob"),         "row 9 author cell missing");

        // Empty-state copy must NOT appear when rows are present.
        assert!(
            !body.contains("No posts yet"),
            "true-empty copy must not render when rows present"
        );
        assert!(
            !body.contains("No results match your search"),
            "filtered-empty copy must not render when rows present"
        );
    }

    #[test]
    fn list_true_empty_renders_friendly_cta() {
        // No rows, no search, no active filter — show the "Create
        // your first …" CTA and the friendly heading.
        let templates = Templates::new(None).expect("embedded templates");
        let ctx = empty_list_ctx_skeleton();
        let body = templates
            .render("admin/list.html", &ctx)
            .expect("list renders");

        assert!(body.contains("No posts yet."), "true-empty heading missing");
        assert!(
            body.contains("Create your first post"),
            "true-empty CTA copy missing",
        );
        assert!(
            body.contains("href=\"/admin/posts/new\""),
            "true-empty CTA link missing",
        );
        // The filtered-empty wording must NOT appear in this branch.
        assert!(
            !body.contains("No results match your search"),
            "true-empty branch leaked filtered-empty copy",
        );
    }

    #[test]
    fn list_filtered_empty_omits_cta() {
        // Search query active, no rows — show "no results match" and
        // suppress the CTA so we're not nudging "create" when the
        // user is actively narrowing.
        let templates = Templates::new(None).expect("embedded templates");
        let mut ctx = empty_list_ctx_skeleton();
        ctx["search_query"] = serde_json::Value::String("nonsense".into());
        let body = templates
            .render("admin/list.html", &ctx)
            .expect("list renders");

        assert!(
            body.contains("No results match your search"),
            "filtered-empty copy missing",
        );
        assert!(
            !body.contains("Create your first post"),
            "filtered-empty branch must not show the create CTA",
        );
        assert!(
            !body.contains("No posts yet"),
            "filtered-empty branch must not show the true-empty heading",
        );
    }

    #[test]
    fn list_filter_only_empty_omits_cta() {
        // Search empty but a filter group has a `current` value —
        // still treated as filtered, not true empty.
        let templates = Templates::new(None).expect("embedded templates");
        let mut ctx = empty_list_ctx_skeleton();
        ctx["filters"] = serde_json::json!([
            {
                "field": "published",
                "label": "Published",
                "options": [],
                "current": "true",
            }
        ]);
        let body = templates
            .render("admin/list.html", &ctx)
            .expect("list renders");

        assert!(
            body.contains("No results match your search"),
            "filter-only empty should still show 'No results match' copy",
        );
        assert!(
            !body.contains("Create your first post"),
            "filter-only empty must not show the create CTA",
        );
    }

    #[test]
    fn render_forbidden_body_with_attempted_perm() {
        let admin = Admin::new();
        let templates = Templates::new(None).expect("embedded templates");
        let ident = fake_identity(Role::Staff);

        let body = render_forbidden_body(
            &admin,
            &templates,
            &ident,
            "fake-csrf".into(),
            Some("posts.delete_post".into()),
            None,
        )
        .expect("forbidden page renders");

        assert!(
            body.contains("posts.delete_post"),
            "attempted permission should appear in body"
        );
        // When `required_role` is None the section is hidden.
        assert!(
            !body.contains("This page requires"),
            "required_role section must be hidden when None"
        );
    }
}
