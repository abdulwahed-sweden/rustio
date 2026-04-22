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

use crate::admin::admin_form_bridge::{AdminDataType, AdminUiField, AdminUiModel};
use crate::admin::auto_form::{AutoField, FieldOverride, FormBuilder, FormModel};
use crate::admin::form::{render_form, FieldConfig, FieldType, FormConfig};
use crate::admin::ui::{
    render_page_header, render_sidebar, render_table_shell, render_toolbar, render_topbar,
    BadgeVariant, Breadcrumb, FilterChip, PageAction, PageHeaderConfig, PaginationConfig,
    SearchConfig, SearchProminence, SidebarGroup, SidebarItem, TableCell, TableColumn, TableRow,
    TableShellConfig, TopbarConfig,
};

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
<script>{keyboard}{form_keyboard}</script>
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
    )
}

// ---------------------------------------------------------------
// Public entry point — handler body for /admin-new
// ---------------------------------------------------------------

/// Render the foundation page for `/admin-new`.
///
/// `submitted = None` is the GET path: the demo form seeds invalid
/// values to showcase validation UX. `submitted = Some(params)` is
/// the POST path: values come from the parsed form body, the form is
/// validated, and the success banner appears when nothing failed.
pub fn admin_index(submitted: Option<&HashMap<String, String>>) -> String {
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

    let page_header = render_page_header(&PageHeaderConfig {
        breadcrumbs,
        title: "Users".into(),
        subtitle: Some("auth.User · 142 records".into()),
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
        value: String::new(),
        action: "/admin-new".into(),
        filters: vec![
            FilterChip {
                label: "All".into(),
                count: Some("142".into()),
                active: true,
            },
            FilterChip {
                label: "Active".into(),
                count: Some("128".into()),
                active: false,
            },
            FilterChip {
                label: "Staff".into(),
                count: Some("11".into()),
                active: false,
            },
            FilterChip {
                label: "Inactive".into(),
                count: Some("3".into()),
                active: false,
            },
        ],
    };

    let toolbar = render_toolbar(&search_cfg);
    let table = render_table_shell(&sample_users_table());
    let foundation_note = r#"<p style="margin: 20px 0 0; font-family: var(--mono); font-size: 12px; color: var(--ink-subtle);">Foundation build · sample data · DB wiring deferred to the next step.</p>"#;
    let drawer = demo_admin_form(submitted);

    let content = format!("{page_header}{toolbar}{table}{foundation_note}{drawer}");

    render_layout(topbar, sidebar, content)
}

// ---------------------------------------------------------------
// AdminModel-bridge demo (rendered on /admin-new today)
// ---------------------------------------------------------------

/// Tag struct used purely as a type parameter for
/// `FormBuilder::from_admin_ui_model::<UserAdmin>()`. The bridge's
/// `AdminUiModel` carries the `Ui` suffix so it can't collide with
/// the framework's existing `crate::admin::AdminModel` trait.
struct UserAdmin;

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
            },
            AdminUiField {
                name: "email",
                label: "Email",
                data_type: AdminDataType::Email,
                required: true,
                readonly: false,
                is_relation: false,
                options: vec![],
            },
            AdminUiField {
                name: "is_active",
                label: "Active",
                data_type: AdminDataType::Boolean,
                required: false,
                readonly: false,
                is_relation: false,
                options: vec![],
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
            },
            AdminUiField {
                name: "salary_amount",
                label: "Salary",
                data_type: AdminDataType::Float,
                required: false,
                readonly: false,
                is_relation: false,
                options: vec![],
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

fn sample_users_table() -> TableShellConfig {
    let columns = vec![
        TableColumn::checkbox(),
        TableColumn::text("Username").sorted("↓"),
        TableColumn::text("Email"),
        TableColumn::text("Full name"),
        TableColumn::text("Status"),
        TableColumn::text("Date joined"),
        TableColumn::text("Last login"),
    ];

    struct SampleUser {
        selected: bool,
        username: &'static str,
        email: &'static str,
        full_name: &'static str,
        status_variant: BadgeVariant,
        status_label: &'static str,
        joined: &'static str,
        last_login: &'static str,
    }

    let sample: &[SampleUser] = &[
        SampleUser {
            selected: false,
            username: "amansour",
            email: "abdulwahed@rustio.dev",
            full_name: "Abdulwahed Mansour",
            status_variant: BadgeVariant::Rust,
            status_label: "SUPERUSER",
            joined: "2025-01-12",
            last_login: "2m ago",
        },
        SampleUser {
            selected: false,
            username: "l.chen",
            email: "lin.chen@rustio.dev",
            full_name: "Lin Chen",
            status_variant: BadgeVariant::Success,
            status_label: "ACTIVE",
            joined: "2025-02-04",
            last_login: "1h ago",
        },
        SampleUser {
            selected: true,
            username: "m.okafor",
            email: "maya.okafor@rustio.dev",
            full_name: "Maya Okafor",
            status_variant: BadgeVariant::Warn,
            status_label: "STAFF",
            joined: "2025-02-18",
            last_login: "3h ago",
        },
        SampleUser {
            selected: false,
            username: "r.ibarra",
            email: "ruben.ibarra@rustio.dev",
            full_name: "Rubén Ibarra",
            status_variant: BadgeVariant::Success,
            status_label: "ACTIVE",
            joined: "2025-03-02",
            last_login: "yesterday",
        },
        SampleUser {
            selected: false,
            username: "s.pohjola",
            email: "saana.pohjola@rustio.dev",
            full_name: "Saana Pohjola",
            status_variant: BadgeVariant::Muted,
            status_label: "INACTIVE",
            joined: "2025-03-15",
            last_login: "14d ago",
        },
        SampleUser {
            selected: false,
            username: "t.dube",
            email: "thandi.dube@rustio.dev",
            full_name: "Thandi Dube",
            status_variant: BadgeVariant::Success,
            status_label: "ACTIVE",
            joined: "2025-03-28",
            last_login: "5m ago",
        },
    ];

    let rows = sample
        .iter()
        .map(|u| TableRow {
            selected: u.selected,
            cells: vec![
                TableCell::Checkbox {
                    checked: u.selected,
                },
                TableCell::Primary(u.username.to_string()),
                TableCell::Mono(u.email.to_string()),
                TableCell::Plain(u.full_name.to_string()),
                TableCell::Badge {
                    variant: u.status_variant,
                    text: u.status_label.to_string(),
                },
                TableCell::Mono(u.joined.to_string()),
                TableCell::Mono(u.last_login.to_string()),
            ],
        })
        .collect();

    TableShellConfig {
        columns,
        rows,
        pagination: Some(PaginationConfig {
            showing_from: 1,
            showing_to: 6,
            total: 142,
            current_page: 1,
            page_count: 24,
        }),
    }
}
