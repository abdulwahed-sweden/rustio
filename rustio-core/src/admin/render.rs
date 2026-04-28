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
use super::types::{Admin, AdminEntry, AdminField, EditRow, ListRow};
use crate::auth::Identity;
use crate::error::Result;
use crate::orm::Db;

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
    /// Phase 6.2 — login form fields (email + password) rendered via
    /// the shared `_form_field.html` include. Page chrome (card,
    /// hidden sidebar/breadcrumbs) stays bespoke.
    pub sections: Vec<FormSection>,
}

/// Phase 6.2 — pre-built FormField list for the login form. Static
/// because the values never change between requests; built once and
/// cloned into LoginCtx.sections.
pub(crate) fn login_form_sections() -> Vec<FormSection> {
    vec![FormSection {
        title: None,
        fields: vec![
            FormField {
                name: "email",
                label: "Email".to_string(),
                widget: "input",
                input_type: "email",
                value: String::new(),
                hint: None,
                placeholder: None,
                required: true,
                options: None,
                multiple: false,
                span: 1,
                autocomplete: Some("username"),
                autofocus: true,
                disabled: false,
                maxlength: None,
                searchable: false,
                has_more: false,
                search_url: None,
                errors: vec![],
                target_model: None,
            },
            FormField {
                name: "password",
                label: "Password".to_string(),
                widget: "input",
                input_type: "password",
                value: String::new(),
                hint: None,
                placeholder: None,
                required: true,
                options: None,
                multiple: false,
                span: 1,
                autocomplete: Some("current-password"),
                autofocus: false,
                disabled: false,
                maxlength: None,
                searchable: false,
                has_more: false,
                search_url: None,
                errors: vec![],
                target_model: None,
            },
        ],
    }]
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
        entries: admin
            .entries()
            .iter()
            .filter(|e| !e.core)
            .map(SidebarEntry::from)
            .collect(),
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
        entries: admin
            .entries()
            .iter()
            .filter(|e| !e.core)
            .map(SidebarEntry::from)
            .collect(),
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
#[derive(Serialize, Clone)]
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
    /// Phase 6.2 — HTML5 `autocomplete` token. `Some("current-password")`
    /// / `Some("new-password")` / `Some("username")` / `Some("email")` /
    /// `Some("off")`. `None` skips the attribute. Surfaced for password
    /// manager hints on bespoke forms (login / password-change /
    /// user-new); generic AdminEntry-driven forms default to `None`.
    pub autocomplete: Option<&'static str>,
    /// Phase 6.2 — `true` emits the HTML5 `autofocus` attribute. Set on
    /// the first user-editable field of a bespoke form so the cursor
    /// lands there; defaults to `false` for AdminEntry-driven forms.
    pub autofocus: bool,
    /// Phase 6.2 — `true` emits HTML5 `disabled`. Used for read-only
    /// displays inside edit forms (user_edit's email field).
    pub disabled: bool,
    /// Phase 6.2 — HTML5 `maxlength` attribute. `Some(150)` for group
    /// names; `None` skips. Surfaced for length-limited free-text fields
    /// on bespoke forms.
    pub maxlength: Option<u16>,
    /// Phase 7.2 — `true` for select fields backed by a relation
    /// (FK / M2M) so the form template wraps the `<select>` with a
    /// client-side filter input. `false` for everything else,
    /// including enum-style closed-list selects (which are typically
    /// short enough to need no filter).
    pub searchable: bool,
    /// Phase 7.2 — `true` when the relation has more rows than the
    /// resolver's truncation limit (currently 50). Drives a hint
    /// message under the search input ("Showing first 50 results.
    /// Keep typing to filter.").
    pub has_more: bool,
    /// Phase 7.3 — when present, JS upgrades the client-side filter
    /// to a remote-search call against this URL. `Some("/admin/search/User")`
    /// for FK / M2M fields whose target resolves; `None` for enums,
    /// non-relation fields, and bespoke-handler-built fields. The
    /// plain `<select>` keeps working with the truncated 50-row
    /// initial set when JS is disabled or the URL is `None`.
    pub search_url: Option<String>,
    /// Phase 7.5 — per-field validation errors. Default empty. The
    /// generic admin path (`do_create` / `do_update`) leaves this
    /// untouched because `AdminOps::create / update` returns flat
    /// `Vec<String>`; those errors stay in `FormCtx.errors`. Bespoke
    /// handlers (user_new / user_edit / group_new / password_change)
    /// build a parallel `HashMap<String, Vec<String>>` while pushing
    /// global errors and pass it through `apply_field_errors`.
    pub errors: Vec<String>,
    /// Phase 10 — display name of the target model for relation
    /// fields. `Some("User")` when this field carries an
    /// `AdminRelation`; `None` otherwise. The form template uses it
    /// for the "Select <Model>…" placeholder and the
    /// "No <Model> available" empty-options message. Mirrors
    /// `search_url`'s relation-derived nature.
    pub target_model: Option<String>,
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
    // Phase 7 — pre-fetched FK / M2M options keyed by field name. The
    // caller (an async show_* handler) builds this via
    // `resolve_relation_options` before invoking this sync builder.
    // A missing entry or empty Vec produces an empty `<select>` — the
    // pre-Phase-7 mock pair is gone.
    relation_options: HashMap<&'static str, (Vec<SelectOption>, bool)>,
    // Phase 7.5 — per-field validation errors keyed by field name.
    // Populated by bespoke validators that already know which field a
    // given error belongs to. The generic AdminEntry path (this fn's
    // primary caller, `do_create` / `do_update`) passes `HashMap::new()`
    // because `AdminOps::create / update` returns flat `Vec<String>`;
    // those errors stay in the global block above.
    field_errors: HashMap<String, Vec<String>>,
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
            let required =
                !f.field_type.nullable() && !matches!(f.field_type, super::types::FieldType::Bool);
            // Phase 5/d — select options + multiple flag.
            //   - Enum (choices): one option per allowed value (raw
            //     string used as both value and label per "no
            //     invented content" rule).
            //   - FK / M2M (relation): mocked option list for now.
            //     A real DB-backed lookup is the next sub-phase per
            //     spec ("Mock for now (no DB query yet)").
            //   - Everything else: None / false.
            // Phase 7.2 — per-field tuple now carries four signals so
            // FormField can drive the searchable filter UI. Order:
            //   options, multiple, searchable, has_more.
            //   - choices (enum): closed list, never searchable.
            //   - relation (FK / M2M): always searchable; has_more
            //     comes from the resolver's truncation flag.
            //   - other: no select, all defaults false.
            let (mut options, multiple, mut searchable, mut has_more) =
                if let Some(values) = f.choices {
                    let mut opts: Vec<SelectOption> = Vec::with_capacity(values.len() + 1);
                    // Phase 7 / F4 — nullable enum fields prepend a leading
                    // empty option so the user can clear the selection back
                    // to NULL.
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
                    (Some(opts), false, false, false)
                } else if let Some(rel) = &f.relation {
                    // Phase 7.2 — `(options, has_more)` from the resolver;
                    // missing entry or empty Vec → empty select. searchable
                    // is always true for relation-backed selects.
                    let (opts, has_more) =
                        relation_options.get(f.name).cloned().unwrap_or_default();
                    (Some(opts), rel.multi, true, has_more)
                } else {
                    (None, false, false, false)
                };

            // Phase 10 — synthesise a status select when no enum is
            // declared. UI-only — the underlying field stays a String
            // (no schema change). Triggers when:
            //   - field.name == "status"
            //   - no `choices` (closed enum list)
            //   - no `relation` (would already be a select)
            // The select is small + closed → not searchable, no
            // truncation flag.
            let mut widget = widget;
            if f.name == "status" && options.is_none() {
                options = Some(vec![
                    SelectOption {
                        value: "draft".to_string(),
                        label: "draft".to_string(),
                    },
                    SelectOption {
                        value: "published".to_string(),
                        label: "published".to_string(),
                    },
                ]);
                searchable = false;
                has_more = false;
                widget = "select";
            }
            // Phase 6 — span hint. Long-text textareas span the full
            // grid (col-span-2); everything else takes one half.
            let span: u8 = if widget == "textarea" { 2 } else { 1 };
            // Phase 7.3 — remote-search URL only for relation-backed
            // fields. Enum / closed-list selects keep `None` (their
            // option set is in-page; nothing to fetch).
            let search_url = f
                .relation
                .as_ref()
                .map(|rel| format!("/admin/search/{}", rel.target_model));
            // Phase 10 — relation fields gain a "Select <Model>…"
            // placeholder so the empty-select state reads as a
            // prompt, not a blank line. The template also uses the
            // target_model below for the empty-options message
            // ("No <Model> available"). For non-relation fields the
            // placeholder + target_model come from `intelligence`.
            let target_model = f.relation.as_ref().map(|rel| rel.target_model.to_string());
            let placeholder = if let Some(rel) = &f.relation {
                Some(format!("Select {}…", rel.target_model))
            } else {
                ui.placeholder
            };
            FormField {
                name: f.name,
                label: ui.label,
                widget,
                input_type,
                value,
                hint: ui.hint,
                placeholder,
                required,
                options,
                multiple,
                span,
                // Phase 6.2 — UX attributes are surfaced only by
                // bespoke handlers that hand-build FormField. The
                // AdminEntry-driven path defaults them off; the
                // template's `{% if field.autofocus %}` etc. checks
                // produce no markup.
                autocomplete: None,
                autofocus: false,
                disabled: false,
                maxlength: None,
                searchable,
                has_more,
                search_url,
                // Phase 7.5 — per-field errors from the caller's map.
                // Generic do_create / do_update pass an empty map, so
                // every field on those paths starts with an empty Vec.
                errors: field_errors.get(f.name).cloned().unwrap_or_default(),
                target_model,
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
        entries: admin
            .entries()
            .iter()
            .filter(|e| !e.core)
            .map(SidebarEntry::from)
            .collect(),
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

/// Phase 7.5 — apply a per-field error map to an existing
/// `Vec<FormSection>` in place. Used by bespoke validators
/// (do_new_user, do_new_group, render_user_edit_with_errors,
/// do_password_change) that already know which field a given error
/// belongs to: the validator builds a `HashMap<String, Vec<String>>`
/// while pushing global errors, calls a section helper to produce
/// the flat sections, then walks them with this fn to attach the
/// keyed errors. Keeps the section-builder signatures unchanged.
pub(crate) fn apply_field_errors(
    sections: &mut [FormSection],
    field_errors: &HashMap<String, Vec<String>>,
) {
    for section in sections.iter_mut() {
        for field in section.fields.iter_mut() {
            if let Some(errs) = field_errors.get(field.name) {
                field.errors = errs.clone();
            }
        }
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

    // Phase 10 — section rename: Default → "General", Metadata →
    // "System". Advanced stays. Empty sections are still dropped.
    let mut sections: Vec<FormSection> = Vec::with_capacity(3);
    if !default_fields.is_empty() {
        sections.push(FormSection {
            title: Some("General"),
            fields: default_fields,
        });
    }
    if !metadata_fields.is_empty() {
        sections.push(FormSection {
            title: Some("System"),
            fields: metadata_fields,
        });
    }
    if !advanced_fields.is_empty() {
        sections.push(FormSection {
            title: Some("Advanced"),
            fields: advanced_fields,
        });
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
/// Phase 7.2 — initial-render row cap for FK / M2M selects. A 1000-row
/// relation isn't usable as a flat `<select>`; the resolver truncates
/// to this many entries and the FormField's `has_more` flag drives a
/// "keep typing to filter" hint in the template. Searchable filtering
/// runs against the truncated set client-side; future phases can wire
/// up an XHR endpoint for typeahead beyond the cap.
pub(crate) const FK_OPTIONS_LIMIT: usize = 50;

/// Phase 7 — fetch real `<select>` options for every FK / M2M field on
/// an `AdminEntry`, keyed by the field's name. Async because
/// `AdminOps::list` is the canonical row-fetch API and is itself
/// async; the caller (a show_* handler) is already async and awaits
/// this once per page render before invoking the sync `form_ctx`.
///
/// Phase 7.2 — return value is `(Vec<SelectOption>, bool)` per key. The
/// bool is `has_more`: `true` when the relation had more rows than
/// `FK_OPTIONS_LIMIT` and the option list was truncated. Empty target
/// lists, missing target models, and non-relation fields all produce
/// a benign empty entry (`(vec![], false)`) — never a panic.
///
/// The label for each option follows the resolution ladder:
///   1. `relation.display_field` if present and the column exists on
///      the target.
///   2. `"name"` column if present.
///   3. `"title"` column if present.
///   4. Stringified id (`row.id.to_string()`).
pub(crate) async fn resolve_relation_options(
    admin: &Admin,
    entry: &AdminEntry,
    db: &Db,
) -> Result<HashMap<&'static str, (Vec<SelectOption>, bool)>> {
    let mut out: HashMap<&'static str, (Vec<SelectOption>, bool)> = HashMap::new();
    for f in entry.fields.iter() {
        let Some(rel) = &f.relation else {
            continue;
        };
        // The macro emits `target_model` from the
        // `#[rustio(belongs_to = "User")]` attribute — that's the
        // singular struct name. Match against any of the AdminEntry
        // identifiers so handlers don't have to think about which.
        let target = admin.entries().iter().find(|e| {
            e.singular_name == rel.target_model
                || e.admin_name == rel.target_model
                || e.display_name == rel.target_model
        });
        let Some(target) = target else {
            // Unknown target — emit an empty list so the form still
            // renders with a `<select>` and an explicit empty state.
            out.insert(f.name, (Vec::new(), false));
            continue;
        };
        let rows = target.ops.list(db).await?;
        let total = rows.len();
        let display_idx = pick_display_index(target.fields, rel.display_field);
        let mut opts: Vec<SelectOption> = rows
            .into_iter()
            .map(|r| {
                let label = display_idx
                    .and_then(|i| r.cells.get(i).cloned())
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| r.id.to_string());
                SelectOption {
                    value: r.id.to_string(),
                    label,
                }
            })
            .collect();
        let has_more = total > FK_OPTIONS_LIMIT;
        opts.truncate(FK_OPTIONS_LIMIT);
        out.insert(f.name, (opts, has_more));
    }
    Ok(out)
}

/// Phase 7.3 — case-insensitive substring filter for SelectOption
/// labels. Hoisted out of the search handler so the filter logic is
/// unit-testable without DB / route plumbing.
pub(crate) fn filter_options(
    opts: Vec<SelectOption>,
    query: &str,
    limit: usize,
) -> Vec<SelectOption> {
    let needle = query.to_lowercase();
    opts.into_iter()
        .filter(|o| o.label.to_lowercase().contains(&needle))
        .take(limit)
        .collect()
}

/// Phase 7.3 — remote-search row cap. Capped lower than the initial
/// page render (Phase 7.2's `FK_OPTIONS_LIMIT = 50`) because each
/// search hit is a network round-trip that the user actively
/// initiated; 20 is the conventional typeahead size.
pub(crate) const SEARCH_RESULT_LIMIT: usize = 20;

/// Phase 7.3 — backend for the `/admin/search/:model` endpoint.
/// Resolves the target AdminEntry by model name (singular / slug /
/// display), fetches its rows via `AdminOps::list`, builds
/// SelectOption labels via the same display-field ladder as
/// `resolve_relation_options` (with `display_field=None` because the
/// search URL doesn't currently encode it), and filters by the
/// query. Returns an empty Vec if the model is unknown or no rows
/// match — never an error, never a panic.
pub(crate) async fn search_options(
    admin: &Admin,
    db: &Db,
    model: &str,
    query: &str,
) -> Result<Vec<SelectOption>> {
    let target = admin
        .entries()
        .iter()
        .find(|e| e.singular_name == model || e.admin_name == model || e.display_name == model);
    let Some(target) = target else {
        return Ok(Vec::new());
    };
    // Phase 7.6 — a transient DB blip during FK lookup must NOT 500
    // the search endpoint. Swallow the error, log it, and return an
    // empty result; the client falls back to the in-page truncated
    // option set, which is still submittable.
    let rows = match target.ops.list(db).await {
        Ok(r) => r,
        Err(e) => {
            log::warn!(
                "search_options: list({model}) failed, returning empty result: {e}"
            );
            return Ok(Vec::new());
        }
    };
    let display_idx = pick_display_index(target.fields, None);
    let opts: Vec<SelectOption> = rows
        .into_iter()
        .map(|r| {
            let label = display_idx
                .and_then(|i| r.cells.get(i).cloned())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| r.id.to_string());
            SelectOption {
                value: r.id.to_string(),
                label,
            }
        })
        .collect();
    Ok(filter_options(opts, query, SEARCH_RESULT_LIMIT))
}

/// Phase 7.6 — cap a search query at `MAX_SEARCH_QUERY_CHARS` so a
/// pathologically long `?q=` doesn't peg a worker. Slicing happens
/// on char boundaries (not bytes) so multi-byte codepoints don't
/// panic.
pub(crate) const MAX_SEARCH_QUERY_CHARS: usize = 200;

pub(crate) fn truncate_query(raw: &str) -> String {
    raw.chars().take(MAX_SEARCH_QUERY_CHARS).collect()
}

/// Phase 7 — pick the index in `fields` whose name matches the
/// preferred display column, with a `name` → `title` fallback. Returns
/// `None` if neither matches; callers fall back to the row id.
fn pick_display_index(fields: &[AdminField], display_field: Option<&str>) -> Option<usize> {
    if let Some(preferred) = display_field {
        if let Some(i) = fields.iter().position(|f| f.name == preferred) {
            return Some(i);
        }
    }
    for fallback in ["name", "title"] {
        if let Some(i) = fields.iter().position(|f| f.name == fallback) {
            return Some(i);
        }
    }
    None
}

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
        entries: admin
            .entries()
            .iter()
            .filter(|e| !e.core)
            .map(SidebarEntry::from)
            .collect(),
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
        entries: admin
            .entries()
            .iter()
            .filter(|e| !e.core)
            .map(SidebarEntry::from)
            .collect(),
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
    /// Phase 6.2 — three password fields rendered through the shared
    /// FormField include. Page chrome (the success branch, the page
    /// header card) stays bespoke.
    pub sections: Vec<FormSection>,
}

/// Phase 6.2 — role options for user_new / user_edit. Labels carry the
/// privilege descriptions shown in the existing dropdowns; values are
/// the role slugs the auth layer expects.
pub(crate) fn role_select_options() -> Vec<SelectOption> {
    vec![
        SelectOption {
            value: "user".to_string(),
            label: "User (no admin access)".to_string(),
        },
        SelectOption {
            value: "staff".to_string(),
            label: "Staff (admin access; per-model group permissions)".to_string(),
        },
        SelectOption {
            value: "supervisor".to_string(),
            label: "Supervisor (view + edit; no destructive ops)".to_string(),
        },
        SelectOption {
            value: "administrator".to_string(),
            label: "Administrator (full coverage; bypasses group checks)".to_string(),
        },
        SelectOption {
            value: "developer".to_string(),
            label: "Developer (schema browser + execution logs + SQL console)".to_string(),
        },
    ]
}

/// Phase 6.2 — FormField list for the user_new form. Two sections:
/// Identity (email + password) and Role (the 5-option select). The
/// caller passes the current values so re-render after validation
/// failure preserves them; new-form callers pass empty/staff defaults.
pub(crate) fn user_new_form_sections(email: &str, role: &str) -> Vec<FormSection> {
    vec![
        FormSection {
            title: Some("Identity"),
            fields: vec![
                FormField {
                    name: "email",
                    label: "Email".to_string(),
                    widget: "input",
                    input_type: "email",
                    value: email.to_string(),
                    hint: Some("Must be unique across all users.".to_string()),
                    placeholder: None,
                    required: true,
                    options: None,
                    multiple: false,
                    span: 2,
                    autocomplete: Some("off"),
                    autofocus: true,
                    disabled: false,
                    maxlength: None,
                    searchable: false,
                    has_more: false,
                    search_url: None,
                    errors: vec![],
                    target_model: None,
                },
                FormField {
                    name: "password",
                    label: "Password".to_string(),
                    widget: "input",
                    input_type: "password",
                    value: String::new(),
                    hint: Some(
                        "At least 8 characters. The user can change it later via Change password."
                            .to_string(),
                    ),
                    placeholder: None,
                    required: true,
                    options: None,
                    multiple: false,
                    span: 2,
                    autocomplete: Some("new-password"),
                    autofocus: false,
                    disabled: false,
                    maxlength: None,
                    searchable: false,
                    has_more: false,
                    search_url: None,
                    errors: vec![],
                    target_model: None,
                },
            ],
        },
        FormSection {
            title: Some("Role"),
            fields: vec![FormField {
                name: "role",
                label: "Role".to_string(),
                widget: "select",
                input_type: "select",
                value: role.to_string(),
                hint: Some(
                    "Higher roles include all lower-role capabilities. Group memberships are assigned on the next page after save."
                        .to_string(),
                ),
                placeholder: None,
                required: true,
                options: Some(role_select_options()),
                multiple: false,
                span: 2,
                autocomplete: None,
                autofocus: false,
                disabled: false,
                maxlength: None,
                searchable: false,
                has_more: false,
                search_url: None,
                errors: vec![],
                target_model: None,
            }],
        },
    ]
}

/// Phase 6.2 — General section for group_new / group_edit. Two
/// fields: name (text, required, 150-char max) and description
/// (textarea). Caller passes the current values so re-render after
/// validation failure preserves them.
pub(crate) fn group_form_sections(name: &str, description: &str) -> Vec<FormSection> {
    vec![FormSection {
        title: Some("General"),
        fields: vec![
            FormField {
                name: "name",
                label: "Name".to_string(),
                widget: "input",
                input_type: "text",
                value: name.to_string(),
                hint: Some(
                    "A short identifier — letters, digits, dots and dashes only. Example: editors."
                        .to_string(),
                ),
                placeholder: None,
                required: true,
                options: None,
                multiple: false,
                span: 2,
                autocomplete: Some("off"),
                autofocus: true,
                disabled: false,
                maxlength: Some(150),
                searchable: false,
                has_more: false,
                search_url: None,
                errors: vec![],
                target_model: None,
            },
            FormField {
                name: "description",
                label: "Description".to_string(),
                widget: "textarea",
                input_type: "text",
                value: description.to_string(),
                hint: Some("Optional. What this group is for.".to_string()),
                placeholder: None,
                required: false,
                options: None,
                multiple: false,
                span: 2,
                autocomplete: None,
                autofocus: false,
                disabled: false,
                maxlength: None,
                searchable: false,
                has_more: false,
                search_url: None,
                errors: vec![],
                target_model: None,
            },
        ],
    }]
}

/// Phase 6.2 — Identity section for user_edit. Email is disabled
/// (read-only display); role is the select; is_active is the checkbox.
/// Built per render so values reflect the current row.
pub(crate) fn user_edit_identity_sections(
    email: &str,
    role: &str,
    is_active: bool,
) -> Vec<FormSection> {
    vec![FormSection {
        title: Some("Identity"),
        fields: vec![
            FormField {
                name: "email",
                label: "Email".to_string(),
                widget: "input",
                input_type: "email",
                value: email.to_string(),
                hint: Some(
                    "Email changes aren't exposed here — they require a full user update."
                        .to_string(),
                ),
                placeholder: None,
                required: false,
                options: None,
                multiple: false,
                span: 2,
                autocomplete: None,
                autofocus: false,
                disabled: true,
                maxlength: None,
                searchable: false,
                has_more: false,
                search_url: None,
                errors: vec![],
                target_model: None,
            },
            FormField {
                name: "role",
                label: "Role".to_string(),
                widget: "select",
                input_type: "select",
                value: role.to_string(),
                hint: None,
                placeholder: None,
                required: true,
                options: Some(role_select_options()),
                multiple: false,
                span: 2,
                autocomplete: None,
                autofocus: false,
                disabled: false,
                maxlength: None,
                searchable: false,
                has_more: false,
                search_url: None,
                errors: vec![],
                target_model: None,
            },
            FormField {
                name: "is_active",
                label: "Active".to_string(),
                widget: "checkbox",
                input_type: "checkbox",
                value: if is_active {
                    "true".to_string()
                } else {
                    "false".to_string()
                },
                hint: Some("Inactive users cannot sign in or hold sessions.".to_string()),
                placeholder: None,
                required: false,
                options: None,
                multiple: false,
                span: 2,
                autocomplete: None,
                autofocus: false,
                disabled: false,
                maxlength: None,
                searchable: false,
                has_more: false,
                search_url: None,
                errors: vec![],
                target_model: None,
            },
        ],
    }]
}

/// Phase 6.2 — Reset password section for user_edit. Single optional
/// field; leaving it blank keeps the existing password.
pub(crate) fn user_edit_password_sections() -> Vec<FormSection> {
    vec![FormSection {
        title: Some("Reset password (optional)"),
        fields: vec![FormField {
            name: "new_password",
            label: "New password".to_string(),
            widget: "input",
            input_type: "password",
            value: String::new(),
            hint: Some("Leave blank to keep the current password unchanged.".to_string()),
            placeholder: None,
            required: false,
            options: None,
            multiple: false,
            span: 2,
            autocomplete: Some("new-password"),
            autofocus: false,
            disabled: false,
            maxlength: None,
            searchable: false,
            has_more: false,
            search_url: None,
            errors: vec![],
            target_model: None,
        }],
    }]
}

/// Phase 6.2 — pre-built FormField list for the password-change form.
/// Static; the values are always empty (we never echo passwords back).
pub(crate) fn password_change_form_sections() -> Vec<FormSection> {
    vec![FormSection {
        title: None,
        fields: vec![
            FormField {
                name: "old_password",
                label: "Old password".to_string(),
                widget: "input",
                input_type: "password",
                value: String::new(),
                hint: None,
                placeholder: None,
                required: true,
                options: None,
                multiple: false,
                span: 2,
                autocomplete: Some("current-password"),
                autofocus: true,
                disabled: false,
                maxlength: None,
                searchable: false,
                has_more: false,
                search_url: None,
                errors: vec![],
                target_model: None,
            },
            FormField {
                name: "new_password1",
                label: "New password".to_string(),
                widget: "input",
                input_type: "password",
                value: String::new(),
                hint: Some("Your password must contain at least 8 characters.".to_string()),
                placeholder: None,
                required: true,
                options: None,
                multiple: false,
                span: 2,
                autocomplete: Some("new-password"),
                autofocus: false,
                disabled: false,
                maxlength: None,
                searchable: false,
                has_more: false,
                search_url: None,
                errors: vec![],
                target_model: None,
            },
            FormField {
                name: "new_password2",
                label: "Confirm".to_string(),
                widget: "input",
                input_type: "password",
                value: String::new(),
                hint: None,
                placeholder: None,
                required: true,
                options: None,
                multiple: false,
                span: 2,
                autocomplete: Some("new-password"),
                autofocus: false,
                disabled: false,
                maxlength: None,
                searchable: false,
                has_more: false,
                search_url: None,
                errors: vec![],
                target_model: None,
            },
        ],
    }]
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
        assert_eq!(
            map_field_to_ui(&af(FieldType::Bool)),
            ("checkbox", "checkbox")
        );
        assert_eq!(map_field_to_ui(&af(FieldType::String)), ("input", "text"));
        assert_eq!(
            map_field_to_ui(&af(FieldType::OptionalString)),
            ("input", "text")
        );
        assert_eq!(map_field_to_ui(&af(FieldType::I32)), ("input", "number"));
        assert_eq!(map_field_to_ui(&af(FieldType::I64)), ("input", "number"));
        assert_eq!(
            map_field_to_ui(&af(FieldType::OptionalI64)),
            ("input", "number")
        );
        assert_eq!(
            map_field_to_ui(&af(FieldType::DateTime)),
            ("input", "datetime-local")
        );
        assert_eq!(
            map_field_to_ui(&af(FieldType::OptionalDateTime)),
            ("input", "datetime-local")
        );
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

    /// Phase 7 — FK fields render with the real options the handler
    /// pre-fetched into `relation_options`, NOT the pre-Phase-7 mock
    /// pair. Test passes a hand-built map keyed by field name; asserts
    /// the rendered FormField carries the same options through to the
    /// HTML.
    #[test]
    fn fk_field_renders_real_options() {
        // AdminEntry with one editable FK column (`author_id`) plus a
        // plain text title for contrast. AdminRelation points at the
        // synthetic User entry; the test bypasses the resolver and
        // injects options directly via the relation_options map.
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
                name: "author_id",
                label: "author_id",
                field_type: FieldType::I64,
                editable: true,
                relation: Some(crate::admin::AdminRelation {
                    target_model: "User",
                    display_field: Some("email"),
                    multi: false,
                }),
                choices: None,
            },
        ];
        let admin = Admin::new();
        let entry = AdminEntry::for_testing("posts", "Posts", "Post", "posts", FK_FIELDS, false);
        let ident = fake_identity(Role::Administrator);

        let mut relation_options: HashMap<&'static str, (Vec<SelectOption>, bool)> = HashMap::new();
        relation_options.insert(
            "author_id",
            (
                vec![
                    SelectOption {
                        value: "1".to_string(),
                        label: "alice@example.com".to_string(),
                    },
                    SelectOption {
                        value: "2".to_string(),
                        label: "bob@example.com".to_string(),
                    },
                    SelectOption {
                        value: "3".to_string(),
                        label: "charlie@example.com".to_string(),
                    },
                ],
                false,
            ),
        );

        let ctx = form_ctx(
            &ident,
            &admin,
            &entry,
            "new",
            None,
            None,
            vec![],
            "csrf".into(),
            relation_options,
            HashMap::new(),
        );

        // Locate the author_id field in the resolved sections — it
        // should land in the Default bucket (FK column, name doesn't
        // hit Metadata or Advanced heuristics).
        let author_field = ctx
            .sections
            .iter()
            .flat_map(|s| s.fields.iter())
            .find(|f| f.name == "author_id")
            .expect("author_id field present");
        assert_eq!(author_field.widget, "select");
        let opts = author_field
            .options
            .as_ref()
            .expect("author_id field has options");
        assert_eq!(opts.len(), 3, "real options length should reflect input");
        assert_eq!(opts[0].value, "1");
        assert_eq!(opts[0].label, "alice@example.com");
        assert_eq!(opts[2].label, "charlie@example.com");
        // The legacy mock label MUST NOT appear.
        assert!(
            !opts
                .iter()
                .any(|o| o.label == "Item 1" || o.label == "Item 2"),
            "Phase 7 mock pair must be gone; got: {opts:?}",
            opts = opts.iter().map(|o| &o.label).collect::<Vec<_>>()
        );

        // Render the template and confirm the labels surface in the
        // produced HTML — closes the loop end-to-end.
        let templates = Templates::new(None).expect("embedded templates");
        let body = templates
            .render("admin/form.html", &ctx)
            .expect("form renders");
        assert!(
            body.contains("alice@example.com"),
            "alice option missing in HTML"
        );
        assert!(
            body.contains("bob@example.com"),
            "bob option missing in HTML"
        );
        assert!(
            body.contains("charlie@example.com"),
            "charlie option missing in HTML"
        );
        assert!(
            !body.contains("Item 1"),
            "rendered HTML must not contain the Phase 7 mock label"
        );
    }

    /// Phase 7.3 — `filter_options` is the testable core of the
    /// `/admin/search/:model` endpoint. Locks the contract:
    ///   - case-insensitive substring match against `label`
    ///   - results capped at the requested `limit`
    ///   - empty query returns everything (callers gate length;
    ///     the endpoint short-circuits empty in `show_search`)
    #[test]
    fn remote_search_returns_results() {
        let opts = vec![
            SelectOption {
                value: "1".to_string(),
                label: "alice@example.com".to_string(),
            },
            SelectOption {
                value: "2".to_string(),
                label: "bob@example.com".to_string(),
            },
            SelectOption {
                value: "3".to_string(),
                label: "Alice Cooper".to_string(),
            },
            SelectOption {
                value: "4".to_string(),
                label: "carol@acme.io".to_string(),
            },
        ];

        // Case-insensitive: "alice" matches alice@example.com AND
        // "Alice Cooper".
        let r = filter_options(opts.clone(), "alice", 20);
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].value, "1");
        assert_eq!(r[1].value, "3");

        // Mixed case in query: also case-insensitive.
        let r = filter_options(opts.clone(), "BoB", 20);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].value, "2");

        // Limit honoured.
        let r = filter_options(opts.clone(), "a", 2);
        assert_eq!(r.len(), 2, "limit=2 caps the result vec");

        // No matches → empty.
        let r = filter_options(opts.clone(), "zzznoexist", 20);
        assert!(r.is_empty());

        // The legacy mock label MUST never appear (regression guard).
        let r = filter_options(opts, "Item", 20);
        assert!(r.is_empty(), "no row labelled 'Item' — legacy mock is gone");
    }

    /// Phase 7.3 — FK fields gain a `search_url` pointing at the
    /// `/admin/search/<TargetModel>` endpoint. Locks both the URL
    /// shape and the template's `data-search-url` attribute.
    #[test]
    fn fk_field_carries_search_url() {
        static FK_FIELDS: &[crate::admin::AdminField] = &[crate::admin::AdminField {
            name: "author_id",
            label: "author_id",
            field_type: FieldType::I64,
            editable: true,
            relation: Some(crate::admin::AdminRelation {
                target_model: "User",
                display_field: Some("email"),
                multi: false,
            }),
            choices: None,
        }];
        let admin = Admin::new();
        let entry = AdminEntry::for_testing("posts", "Posts", "Post", "posts", FK_FIELDS, false);
        let ident = fake_identity(Role::Administrator);
        let mut relation_options: HashMap<&'static str, (Vec<SelectOption>, bool)> = HashMap::new();
        relation_options.insert(
            "author_id",
            (
                vec![SelectOption {
                    value: "1".into(),
                    label: "alice@example.com".into(),
                }],
                false,
            ),
        );
        let ctx = form_ctx(
            &ident,
            &admin,
            &entry,
            "new",
            None,
            None,
            vec![],
            "csrf".into(),
            relation_options,
            HashMap::new(),
        );

        let author = ctx
            .sections
            .iter()
            .flat_map(|s| s.fields.iter())
            .find(|f| f.name == "author_id")
            .expect("author_id field");
        assert_eq!(
            author.search_url.as_deref(),
            Some("/admin/search/User"),
            "FK fields must carry the JSON search endpoint URL"
        );

        // Rendered template surfaces the URL as a data attribute.
        // minijinja autoescapes `/` to `&#x2f;` in attribute values
        // (OWASP-safe — browsers decode it back when reading the DOM,
        // and `input.dataset.searchUrl` returns the unescaped string,
        // which is what the JS fetch handler uses).
        let templates = Templates::new(None).expect("embedded templates");
        let body = templates
            .render("admin/form.html", &ctx)
            .expect("form renders");
        assert!(
            body.contains("data-search-url=\"&#x2f;admin&#x2f;search&#x2f;User\""),
            "search_url must surface as data-search-url on the search input"
        );
    }

    /// Phase 7.2 — searchable FK selects render the search-input
    /// scaffolding (placeholder, `data-target`, `aria-controls`) plus
    /// the underlying `<select>`. The selected option keeps its
    /// `selected` marker so the JS filter (which exempts the selected
    /// option from hiding) never accidentally drops the current
    /// value. Locks both the FormField shape and the template wrap.
    #[test]
    fn searchable_select_filters_options() {
        // FK column with three real options. `field.value = "2"` so
        // the second option is the selected one — must persist.
        static FK_FIELDS: &[crate::admin::AdminField] = &[crate::admin::AdminField {
            name: "author_id",
            label: "author_id",
            field_type: FieldType::I64,
            editable: true,
            relation: Some(crate::admin::AdminRelation {
                target_model: "User",
                display_field: Some("email"),
                multi: false,
            }),
            choices: None,
        }];
        let admin = Admin::new();
        let entry = AdminEntry::for_testing("posts", "Posts", "Post", "posts", FK_FIELDS, false);
        let ident = fake_identity(Role::Administrator);
        let mut relation_options: HashMap<&'static str, (Vec<SelectOption>, bool)> = HashMap::new();
        relation_options.insert(
            "author_id",
            (
                vec![
                    SelectOption {
                        value: "1".to_string(),
                        label: "alice@example.com".to_string(),
                    },
                    SelectOption {
                        value: "2".to_string(),
                        label: "bob@example.com".to_string(),
                    },
                    SelectOption {
                        value: "3".to_string(),
                        label: "charlie@example.com".to_string(),
                    },
                ],
                false, // has_more=false (3 < 50)
            ),
        );

        // Edit-mode render with the existing row carrying author_id="2".
        let existing = EditRow {
            id: 7,
            values: vec![("author_id".to_string(), "2".to_string())],
        };
        let ctx = form_ctx(
            &ident,
            &admin,
            &entry,
            "edit",
            Some(7),
            Some(&existing),
            vec![],
            "csrf".into(),
            relation_options,
            HashMap::new(),
        );

        let author_field = ctx
            .sections
            .iter()
            .flat_map(|s| s.fields.iter())
            .find(|f| f.name == "author_id")
            .expect("author_id field present");
        assert_eq!(author_field.widget, "select");
        assert!(
            author_field.searchable,
            "FK fields must default to searchable=true"
        );
        assert!(
            !author_field.has_more,
            "3 options is below the 50-row truncation threshold"
        );
        assert_eq!(
            author_field.value, "2",
            "edit mode must surface the existing author_id value"
        );

        let templates = Templates::new(None).expect("embedded templates");
        let body = templates
            .render("admin/form.html", &ctx)
            .expect("form renders");

        // The search input scaffolding is present.
        assert!(
            body.contains("data-search-input"),
            "search input marker missing"
        );
        assert!(
            body.contains("data-target=\"id_author_id\""),
            "search input must wire to the select via data-target"
        );
        assert!(
            body.contains("aria-controls=\"id_author_id\""),
            "search input must announce its target via aria-controls"
        );
        assert!(
            body.contains("placeholder=\"Search…\""),
            "search input must carry the placeholder copy"
        );

        // The selected value persists — bob@example.com (value=2) has
        // the `selected` attribute on its <option>.
        let bob_idx = body
            .find("value=\"2\"")
            .expect("option with value=2 must render");
        let after_bob = &body[bob_idx..bob_idx.saturating_add(120)];
        assert!(
            after_bob.contains("selected"),
            "selected option must carry `selected`; got: {after_bob:?}"
        );

        // No has_more hint when below the threshold.
        assert!(
            !body.contains("Showing first 50 results"),
            "has_more hint must not appear when has_more=false"
        );
    }

    /// Phase 7.2 — when the relation has more rows than the resolver's
    /// truncation cap, FormField.has_more flips to true and the
    /// template renders the "Showing first 50 results" hint paragraph.
    #[test]
    fn searchable_select_renders_has_more_hint() {
        static FK_FIELDS: &[crate::admin::AdminField] = &[crate::admin::AdminField {
            name: "author_id",
            label: "author_id",
            field_type: FieldType::I64,
            editable: true,
            relation: Some(crate::admin::AdminRelation {
                target_model: "User",
                display_field: None,
                multi: false,
            }),
            choices: None,
        }];
        let admin = Admin::new();
        let entry = AdminEntry::for_testing("posts", "Posts", "Post", "posts", FK_FIELDS, false);
        let ident = fake_identity(Role::Administrator);
        let mut relation_options: HashMap<&'static str, (Vec<SelectOption>, bool)> = HashMap::new();
        // Just one option for the test, but flip has_more = true to
        // simulate a relation that exceeded the resolver's cap.
        relation_options.insert(
            "author_id",
            (
                vec![SelectOption {
                    value: "1".into(),
                    label: "first".into(),
                }],
                true,
            ),
        );
        let ctx = form_ctx(
            &ident,
            &admin,
            &entry,
            "new",
            None,
            None,
            vec![],
            "csrf".into(),
            relation_options,
            HashMap::new(),
        );
        let templates = Templates::new(None).expect("embedded templates");
        let body = templates
            .render("admin/form.html", &ctx)
            .expect("form renders");
        assert!(
            body.contains("Showing first 50 results"),
            "has_more hint copy missing"
        );
    }

    /// Phase 7 — FK field WITHOUT a matching entry in relation_options
    /// renders an empty `<select>` rather than the legacy mock. Locks
    /// the empty-state contract in `form_ctx`.
    #[test]
    fn fk_field_with_no_options_renders_empty_select() {
        static FK_FIELDS: &[crate::admin::AdminField] = &[crate::admin::AdminField {
            name: "author_id",
            label: "author_id",
            field_type: FieldType::I64,
            editable: true,
            relation: Some(crate::admin::AdminRelation {
                target_model: "User",
                display_field: None,
                multi: false,
            }),
            choices: None,
        }];
        let admin = Admin::new();
        let entry = AdminEntry::for_testing("posts", "Posts", "Post", "posts", FK_FIELDS, false);
        let ident = fake_identity(Role::Administrator);
        let ctx = form_ctx(
            &ident,
            &admin,
            &entry,
            "new",
            None,
            None,
            vec![],
            "csrf".into(),
            HashMap::new(),
            HashMap::new(),
        );
        let author_field = ctx
            .sections
            .iter()
            .flat_map(|s| s.fields.iter())
            .find(|f| f.name == "author_id")
            .expect("author_id field present");
        assert_eq!(author_field.widget, "select");
        let opts = author_field.options.as_ref().expect("options is Some");
        assert!(opts.is_empty(), "no relation_options entry → empty select");
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
        assert!(body.contains("Return to dashboard"), "back link missing");
        // Identity-bearing surfaces still render: the user-tools welcome
        // line should include the test email.
        assert!(
            body.contains("test@example.com"),
            "user-tools email missing"
        );
    }

    /// Phase 6.2 — `user_edit.html` now renders its Identity and
    /// Reset-password sections through the shared FormField include,
    /// while keeping the group-membership checkbox list as a custom
    /// block (per Phase 6.2 spec correction #1: groups stay as
    /// checkbox-list, NOT multi-select). This locks all three sides:
    ///   - role select renders with its 5 options
    ///   - is_active checkbox renders with the right name + checked state
    ///   - group_<id> checkbox list renders with all_groups + user_groups
    ///     (custom block, not a `<select multiple>`)
    #[test]
    fn user_edit_renders_dynamic_form() {
        let templates = Templates::new(None).expect("embedded templates");
        let identity_sections = user_edit_identity_sections("alice@example.com", "staff", true);
        let password_sections = user_edit_password_sections();
        let ctx = serde_json::json!({
            "site_title": "RustIO administration",
            "site_header": "RustIO administration",
            "index_title": "Site administration",
            "footer_copyright": "RustIO test",
            "csrf_token": "fake",
            "is_demo_session": false,
            "demo_label": null,
            "page_title": "Edit user",
            "entries": [],
            "user_id": 42,
            "email": "alice@example.com",
            "role": "staff",
            "is_active": true,
            "errors": [],
            "is_last_developer": false,
            "all_groups": [
                { "id": 1, "name": "editors", "description": "Edit posts" },
                { "id": 2, "name": "moderators", "description": "Moderate comments" },
            ],
            "user_groups": [1],
            "identity_sections": identity_sections,
            "password_sections": password_sections,
            "identity": { "email": "admin@example.com", "is_admin": true, "is_developer": false },
        });
        let body = templates
            .render("admin/user_edit.html", &ctx)
            .expect("user_edit renders");

        // Role select with all five options.
        assert!(body.contains("name=\"role\""), "role select missing");
        for value in ["user", "staff", "supervisor", "administrator", "developer"] {
            assert!(
                body.contains(&format!("value=\"{value}\"")),
                "role option {value:?} missing"
            );
        }
        // Staff is the current role → that option carries `selected`.
        let staff_idx = body.find("value=\"staff\"").expect("staff option");
        let after_staff = &body[staff_idx..staff_idx.saturating_add(80)];
        assert!(
            after_staff.contains("selected"),
            "Staff option should be selected; got: {after_staff:?}"
        );

        // is_active checkbox renders with name + checked (is_active=true
        // → FormField.value="true" → template sets the checked attr).
        assert!(
            body.contains("name=\"is_active\""),
            "is_active checkbox missing"
        );
        let active_idx = body.find("name=\"is_active\"").expect("is_active checkbox");
        let after_active = &body[active_idx..active_idx.saturating_add(160)];
        assert!(
            after_active.contains("checked"),
            "is_active should be checked when is_active=true; got: {after_active:?}"
        );

        // Group memberships render as the custom checkbox list — NOT a
        // <select multiple>. Each enabled group shows up as
        // `name="group_<id>"` and group #1 (in user_groups) is checked.
        assert!(
            !body.contains("<select multiple"),
            "groups must NOT render as select-multiple — checkbox list is the contract"
        );
        assert!(
            body.contains("name=\"group_1\""),
            "group_1 checkbox missing"
        );
        assert!(
            body.contains("name=\"group_2\""),
            "group_2 checkbox missing"
        );
        let g1_idx = body.find("name=\"group_1\"").expect("group_1 checkbox");
        let after_g1 = &body[g1_idx..g1_idx.saturating_add(100)];
        assert!(
            after_g1.contains("checked"),
            "group_1 should be checked (1 ∈ user_groups); got: {after_g1:?}"
        );
        // The per-group description still surfaces — that's the UX
        // feature the multi-select migration would have lost.
        assert!(
            body.contains("Edit posts"),
            "group description must render alongside the checkbox label"
        );

        // Email field is disabled (read-only display).
        assert!(
            body.contains("name=\"email\""),
            "email input must still be present"
        );
        let email_idx = body.find("name=\"email\"").expect("email input");
        let after_email = &body[email_idx..email_idx.saturating_add(200)];
        assert!(
            after_email.contains("disabled"),
            "email input must carry HTML disabled attribute; got: {after_email:?}"
        );
    }

    #[test]
    fn user_new_form_has_five_role_options() {
        // Phase 6.2 — user_new.html renders the role select via the
        // shared FormField include. Build the JSON ctx through the
        // public `user_new_form_sections` builder so the test exercises
        // the same path the handler uses; assertions on the rendered
        // markup are unchanged (5 options, Staff selected, no legacy
        // `is_staff`/`is_superuser` checkbox names).
        let templates = Templates::new(None).expect("embedded templates");
        let sections = user_new_form_sections("", "staff");
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
            "sections": sections,
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
        // Default role: "staff" → that option carries `selected`.
        let staff_idx = body.find("value=\"staff\"").expect("staff option");
        let after_staff = &body[staff_idx..staff_idx.saturating_add(80)];
        assert!(
            after_staff.contains("selected"),
            "Staff option should be selected; got: {after_staff:?}"
        );
        // Pre-7a/0.5/d checkbox names must NOT appear.
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
        let body = templates
            .render("admin/index.html", &dash)
            .expect("dashboard renders");

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
        let body = templates
            .render("admin/index.html", &dash)
            .expect("dashboard renders");

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
                            "autocomplete": null,
                            "autofocus": false,
                            "disabled": false,
                            "maxlength": null,
                            "searchable": false,
                            "has_more": false,
                            "search_url": null,
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
                            "autocomplete": null,
                            "autofocus": false,
                            "disabled": false,
                            "maxlength": null,
                            "searchable": false,
                            "has_more": false,
                            "search_url": null,
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
        let title_label_idx = body.find("for=\"id_title\"").expect("title label present");
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
        let entry = AdminEntry::for_testing("posts", "Posts", "Post", "posts", MIXED_FIELDS, false);
        let ident = fake_identity(Role::Administrator);
        let ctx = form_ctx(
            &ident,
            &admin,
            &entry,
            "new",
            None,
            None,
            vec![],
            "csrf".into(),
            HashMap::new(),
            HashMap::new(),
        );

        // Phase 10 — sections were renamed: Default → "General",
        // Metadata → "System", Advanced unchanged. Order is still
        // General → System → Advanced.
        assert_eq!(
            ctx.sections.len(),
            3,
            "expected three sections, got {ctx_len:?}",
            ctx_len = ctx.sections.iter().map(|s| s.title).collect::<Vec<_>>()
        );
        assert_eq!(
            ctx.sections[0].title,
            Some("General"),
            "first section is the General (formerly default) bucket"
        );
        assert_eq!(ctx.sections[0].fields.len(), 1);
        assert_eq!(ctx.sections[0].fields[0].name, "title");
        assert_eq!(
            ctx.sections[1].title,
            Some("System"),
            "second section is System (formerly Metadata)"
        );
        assert_eq!(ctx.sections[1].fields.len(), 1);
        assert_eq!(ctx.sections[1].fields[0].name, "creation_timestamp");
        assert_eq!(
            ctx.sections[2].title,
            Some("Advanced"),
            "third section is Advanced"
        );
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
        let entry2 = AdminEntry::for_testing("posts", "Posts", "Post", "posts", FK_FIELDS, false);
        let ctx2 = form_ctx(
            &ident,
            &admin,
            &entry2,
            "new",
            None,
            None,
            vec![],
            "csrf".into(),
            HashMap::new(),
            HashMap::new(),
        );
        assert_eq!(
            ctx2.sections.len(),
            1,
            "FK fields must NOT go to Advanced — they're business-meaningful"
        );
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
        let entry =
            AdminEntry::for_testing("posts", "Posts", "Post", "posts", TEXTAREA_FIELDS, false);
        let ident = fake_identity(Role::Administrator);
        let ctx = form_ctx(
            &ident,
            &admin,
            &entry,
            "new",
            None,
            None,
            vec![],
            "csrf".into(),
            HashMap::new(),
            HashMap::new(),
        );

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
        assert!(body.contains(">body</th>"), "body header missing");
        assert!(body.contains(">author</th>"), "author header missing");

        // Row 7: first column wrapped in the edit anchor; subsequent
        // cells render as plain text.
        assert!(
            body.contains("href=\"/admin/posts/7/edit\">Alpha</a>"),
            "row 7 first-column edit-link missing"
        );
        assert!(body.contains("first body"), "row 7 body cell missing");
        assert!(body.contains("alice"), "row 7 author cell missing");

        // Row 9 same.
        assert!(
            body.contains("href=\"/admin/posts/9/edit\">Beta</a>"),
            "row 9 first-column edit-link missing"
        );
        assert!(body.contains("second body"), "row 9 body cell missing");
        assert!(body.contains("bob"), "row 9 author cell missing");

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

    /// Phase 7.5 — fixture for the inline-field-error tests. Builds a
    /// FormCtx-shaped JSON literal with one section containing two
    /// FormFields: `email` carries an error, `password` does not.
    /// Both pass through the same `_form_field.html` include, so a
    /// single render exercises the error / no-error paths together.
    fn form_with_field_errors_ctx() -> serde_json::Value {
        serde_json::json!({
            "site_title": "RustIO administration",
            "site_header": "RustIO administration",
            "index_title": "Site administration",
            "footer_copyright": "RustIO test",
            "csrf_token": "fake",
            "is_demo_session": false,
            "demo_label": null,
            "page_title": "Add user",
            "entries": [],
            "admin_name": "users",
            "display_name": "Users",
            "singular_name": "User",
            "mode": "new",
            "object_id": null,
            "errors": ["Email is required."],
            "flash": null,
            "identity": { "email": "admin@example.com", "is_admin": true, "is_developer": false },
            "sections": [
                {
                    "title": null,
                    "fields": [
                        {
                            "name": "email",
                            "label": "Email",
                            "widget": "input",
                            "input_type": "email",
                            "value": "",
                            "hint": null,
                            "placeholder": null,
                            "required": true,
                            "options": null,
                            "multiple": false,
                            "span": 1,
                            "autocomplete": null,
                            "autofocus": false,
                            "disabled": false,
                            "maxlength": null,
                            "searchable": false,
                            "has_more": false,
                            "search_url": null,
                            "errors": ["Email is required."],
                        },
                        {
                            "name": "password",
                            "label": "Password",
                            "widget": "input",
                            "input_type": "password",
                            "value": "",
                            "hint": null,
                            "placeholder": null,
                            "required": true,
                            "options": null,
                            "multiple": false,
                            "span": 1,
                            "autocomplete": null,
                            "autofocus": false,
                            "disabled": false,
                            "maxlength": null,
                            "searchable": false,
                            "has_more": false,
                            "search_url": null,
                            "errors": [],
                        },
                    ],
                },
            ],
        })
    }

    /// Phase 7.5 — Step 1 plumbing renders inline error copy under the
    /// field that owns it. Locks the (validator → field_errors map →
    /// `apply_field_errors` → `FormField.errors` → `_form_field.html`
    /// error block) end-to-end pipeline.
    #[test]
    fn field_errors_render_under_inputs() {
        let templates = Templates::new(None).expect("embedded templates");
        let body = templates
            .render("admin/form.html", &form_with_field_errors_ctx())
            .expect("form renders");

        // The error block lives under the email input with the
        // canonical id `error_<name>`.
        assert!(
            body.contains(r#"id="error_email""#),
            "expected <p id=\"error_email\"> error block, body fragment: {}",
            &body[..body.len().min(500)]
        );
        assert!(
            body.contains("Email is required."),
            "error message must surface under the email field"
        );
        // `password` has no errors → no error block for it. The
        // global error banner above the form may carry the same
        // string, so we narrow the assertion to the per-field id.
        assert!(
            !body.contains(r#"id="error_password""#),
            "no error for password → no error block expected"
        );
    }

    /// Phase 7.5 — `aria-invalid` toggles per-field. Errors → `"true"`,
    /// no errors → `"false"`. Locks the aria attribute the screen
    /// reader uses to announce a field as invalid.
    #[test]
    fn input_has_aria_invalid_when_error() {
        let templates = Templates::new(None).expect("embedded templates");
        let body = templates
            .render("admin/form.html", &form_with_field_errors_ctx())
            .expect("form renders");

        // Locate the email input, slice its tag, assert
        // aria-invalid="true".
        let email_idx = body.find(r#"name="email""#).expect("email input present");
        let email_start = body[..email_idx].rfind('<').expect("tag start");
        let email_end = email_idx + body[email_idx..].find('>').expect("tag end");
        let email_tag = &body[email_start..email_end];
        assert!(
            email_tag.contains(r#"aria-invalid="true""#),
            "email input must carry aria-invalid=\"true\", got: {email_tag}"
        );

        // Same slice trick for the password input — should be "false".
        let pw_idx = body
            .find(r#"name="password""#)
            .expect("password input present");
        let pw_start = body[..pw_idx].rfind('<').expect("tag start");
        let pw_end = pw_idx + body[pw_idx..].find('>').expect("tag end");
        let pw_tag = &body[pw_start..pw_end];
        assert!(
            pw_tag.contains(r#"aria-invalid="false""#),
            "password input must carry aria-invalid=\"false\", got: {pw_tag}"
        );
    }

    /// Phase 7.5 — `aria-describedby` on an erroring input must point
    /// at the id of the error block under it. Renders the same
    /// fixture and asserts both anchors exist with matching ids; if
    /// the template ever drifts (different id prefix on one side),
    /// screen readers stop announcing the error and this fails.
    #[test]
    fn aria_describedby_links_correctly() {
        let templates = Templates::new(None).expect("embedded templates");
        let body = templates
            .render("admin/form.html", &form_with_field_errors_ctx())
            .expect("form renders");

        // Input side: aria-describedby="error_email" only present
        // when errors exist.
        assert!(
            body.contains(r#"aria-describedby="error_email""#),
            "email input missing aria-describedby anchor"
        );
        // Error block side: the id the input points at must exist.
        assert!(
            body.contains(r#"id="error_email""#),
            "error block id must match aria-describedby target"
        );
        // No errors → no aria-describedby on the password input.
        let pw_idx = body
            .find(r#"name="password""#)
            .expect("password input present");
        let pw_end = pw_idx + body[pw_idx..].find('>').expect("tag close");
        let pw_tag = &body[pw_idx..pw_end];
        assert!(
            !pw_tag.contains("aria-describedby"),
            "password (no errors) must NOT carry aria-describedby"
        );
    }

    /// Phase 7.5 — the FK search-input Esc handler calls
    /// `e.stopPropagation()` so the global Esc-to-cancel listener
    /// doesn't navigate away when the user just wants to clear the
    /// search box. This test renders any page that extends base
    /// (login is the smallest) and locks both contract halves: the
    /// stopPropagation in the search-input branch AND the
    /// `[data-cancel]` selector in the global handler. If either
    /// drifts, Esc-to-clear regresses into Esc-to-cancel.
    #[test]
    fn search_escape_does_not_trigger_cancel() {
        let templates = Templates::new(None).expect("embedded templates");
        let body = templates
            .render(
                "admin/login.html",
                &serde_json::json!({
                    "site_title": "RustIO administration",
                    "site_header": "RustIO administration",
                    "index_title": "Site administration",
                    "footer_copyright": "RustIO test",
                    "csrf_token": "fake",
                    "is_demo_session": false,
                    "demo_label": null,
                    "page_title": "Sign in",
                    "error": null,
                    "identity": null,
                    "sections": [],
                }),
            )
            .expect("login renders");

        assert!(
            body.contains("e.stopPropagation()"),
            "search-input Esc handler must call stopPropagation()"
        );
        assert!(
            body.contains(r#"querySelector("[data-cancel]")"#),
            "global Esc handler must target [data-cancel] anchors"
        );
        // The global handler bails out when the keystroke originates
        // in an input/textarea, so individual element listeners
        // (search clear, etc.) get first crack.
        assert!(
            body.contains(r#"!e.target.matches("input, textarea")"#),
            "global Esc must be guarded against input/textarea targets"
        );
    }

    /// Phase 7.5 — empty-state regression. Phase 1/c shipped the
    /// "Create your first …" CTA on a true-empty list page; this
    /// test pins the contract so a future template churn can't
    /// silently drop the CTA. Pairs with
    /// `list_true_empty_renders_friendly_cta` above (which checks
    /// the heading copy); this one focuses on the button + href.
    #[test]
    fn empty_state_has_add_button() {
        let templates = Templates::new(None).expect("embedded templates");
        let ctx = empty_list_ctx_skeleton();
        let body = templates
            .render("admin/list.html", &ctx)
            .expect("list renders");

        assert!(
            body.contains("btn-primary"),
            "empty-state CTA must use btn-primary"
        );
        assert!(
            body.contains(r#"href="/admin/posts/new""#),
            "empty-state CTA must link to /admin/<name>/new"
        );
    }

    /// Phase 7.6 — a transient DB blip during FK lookup must NOT 500
    /// the search endpoint. Builds an Admin with one entry whose
    /// `ops.list()` returns a synthetic Err and calls
    /// `search_options`; asserts the function swallows the error and
    /// returns an empty Vec. The endpoint stays callable; the client
    /// falls back to its truncated in-page option set.
    ///
    /// `Db::for_testing_no_connection` builds a lazy pool that never
    /// opens a real connection — `FailingOps::list` returns Err before
    /// the db is dereferenced, so no network round-trip happens.
    #[tokio::test]
    async fn search_db_failure_safe() {
        static AUTHOR_FIELDS: &[crate::admin::AdminField] = &[];
        let mut admin = Admin::new();
        admin
            .entries
            .push(crate::admin::types::AdminEntry::for_testing_failing_list(
                "authors", "Authors", "Author", "authors", AUTHOR_FIELDS,
            ));
        let db = crate::orm::Db::for_testing_no_connection();

        // FailingOps path: list() returns Err → search_options swallows
        // → empty Vec.
        let opts = search_options(&admin, &db, "Author", "alice")
            .await
            .expect("search_options must NOT bubble the list() Err");
        assert!(
            opts.is_empty(),
            "FailingOps list() should fall through to empty Vec, got {n} options",
            n = opts.len()
        );

        // Unknown-model fast path — early return at the top of
        // `search_options`, never reaches list().
        let opts = search_options(&admin, &db, "DoesNotExist", "alice")
            .await
            .expect("unknown model must NOT error");
        assert!(opts.is_empty(), "unknown model should return empty Vec");
    }

    /// Phase 7.6 — a pathologically long query string (here 100KB)
    /// must be truncated at MAX_SEARCH_QUERY_CHARS. Verifies the
    /// helper is char-boundary-safe (multi-byte input doesn't panic)
    /// and bounds the work `filter_options` would otherwise do on
    /// an unbounded string.
    #[test]
    fn search_query_truncated() {
        // ASCII path: 100,000 chars in, MAX_SEARCH_QUERY_CHARS chars
        // out (`a` is 1 byte, so chars == bytes here).
        let huge = "a".repeat(100_000);
        let truncated = truncate_query(&huge);
        assert_eq!(
            truncated.chars().count(),
            MAX_SEARCH_QUERY_CHARS,
            "ASCII query must truncate to MAX_SEARCH_QUERY_CHARS chars"
        );
        assert_eq!(
            truncated.len(),
            MAX_SEARCH_QUERY_CHARS,
            "ASCII path: byte count == char count"
        );

        // Multi-byte path: a 4-byte emoji repeated 1,000 times. Chars
        // truncate at 200, bytes at 800. The slice must NOT panic on
        // a non-char-boundary cut.
        let emoji = "\u{1F600}".repeat(1_000); // grinning face emoji
        let truncated = truncate_query(&emoji);
        assert_eq!(
            truncated.chars().count(),
            MAX_SEARCH_QUERY_CHARS,
            "multi-byte query must truncate by char count, not bytes"
        );
        assert_eq!(
            truncated.len(),
            MAX_SEARCH_QUERY_CHARS * 4,
            "byte count = chars * UTF-8 width"
        );

        // Short queries pass through untouched.
        assert_eq!(truncate_query("alice"), "alice");
        assert_eq!(truncate_query(""), "");
    }

    // ----- Phase 10 — UX widget tests ----------------------------

    /// Phase 10.A — `slug`-named String field gains the placeholder
    /// "my-post-title" and the hint "URL-friendly identifier" via
    /// `intelligence::field_ui_metadata`. Locks the name-based UI
    /// override so it doesn't drift back to a role-classifier
    /// default (which would emit no placeholder for a plain
    /// `PlainText` slug).
    #[test]
    fn slug_field_has_placeholder_and_hint() {
        static SLUG_FIELDS: &[crate::admin::AdminField] = &[
            crate::admin::AdminField {
                name: "slug",
                label: "slug",
                field_type: FieldType::String,
                editable: true,
                relation: None,
                choices: None,
            },
        ];
        let admin = Admin::new();
        let entry = AdminEntry::for_testing("posts", "Posts", "Post", "posts", SLUG_FIELDS, false);
        let ident = fake_identity(Role::Administrator);
        let ctx = form_ctx(
            &ident,
            &admin,
            &entry,
            "new",
            None,
            None,
            vec![],
            "csrf".into(),
            HashMap::new(),
            HashMap::new(),
        );
        let slug = ctx
            .sections
            .iter()
            .flat_map(|s| s.fields.iter())
            .find(|f| f.name == "slug")
            .expect("slug field present");
        assert_eq!(slug.placeholder.as_deref(), Some("my-post-title"));
        assert_eq!(slug.hint.as_deref(), Some("URL-friendly identifier"));
    }

    /// Phase 10.B — a `status`-named String field with no `choices`
    /// and no relation gets synthesised select options
    /// `["draft", "published"]` and widget `"select"`. Schema is
    /// unchanged (the underlying field stays `String`); this is a
    /// pure UI hint.
    #[test]
    fn status_field_renders_select_with_synthesized_options() {
        static STATUS_FIELDS: &[crate::admin::AdminField] = &[
            crate::admin::AdminField {
                name: "status",
                label: "status",
                field_type: FieldType::String,
                editable: true,
                relation: None,
                choices: None,
            },
        ];
        let admin = Admin::new();
        let entry =
            AdminEntry::for_testing("posts", "Posts", "Post", "posts", STATUS_FIELDS, false);
        let ident = fake_identity(Role::Administrator);
        let ctx = form_ctx(
            &ident,
            &admin,
            &entry,
            "new",
            None,
            None,
            vec![],
            "csrf".into(),
            HashMap::new(),
            HashMap::new(),
        );
        let status = ctx
            .sections
            .iter()
            .flat_map(|s| s.fields.iter())
            .find(|f| f.name == "status")
            .expect("status field present");
        assert_eq!(status.widget, "select");
        let opts = status
            .options
            .as_ref()
            .expect("status field has synthesised options");
        let labels: Vec<&str> = opts.iter().map(|o| o.label.as_str()).collect();
        assert_eq!(labels, vec!["draft", "published"]);
    }

    /// Phase 10.C — a relation field gains a "Select <Model>…"
    /// placeholder and `target_model: Some("<Model>")` so the form
    /// template can render the empty-options message
    /// "No <Model> available". Locks the FK UX additions.
    #[test]
    fn fk_field_has_select_model_placeholder_and_target_model() {
        static FK_FIELDS: &[crate::admin::AdminField] = &[
            crate::admin::AdminField {
                name: "author_id",
                label: "author_id",
                field_type: FieldType::I64,
                editable: true,
                relation: Some(crate::admin::AdminRelation {
                    target_model: "User",
                    display_field: Some("email"),
                    multi: false,
                }),
                choices: None,
            },
        ];
        let admin = Admin::new();
        let entry = AdminEntry::for_testing("posts", "Posts", "Post", "posts", FK_FIELDS, false);
        let ident = fake_identity(Role::Administrator);
        let ctx = form_ctx(
            &ident,
            &admin,
            &entry,
            "new",
            None,
            None,
            vec![],
            "csrf".into(),
            HashMap::new(),
            HashMap::new(),
        );
        let fk = ctx
            .sections
            .iter()
            .flat_map(|s| s.fields.iter())
            .find(|f| f.name == "author_id")
            .expect("author_id field present");
        assert_eq!(fk.placeholder.as_deref(), Some("Select User…"));
        assert_eq!(fk.target_model.as_deref(), Some("User"));
    }
}
