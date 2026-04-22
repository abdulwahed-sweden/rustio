//! Admin-new UI component renderers.
//!
//! Each helper emits HTML whose class names come straight from the
//! approved design extraction (`assets/admin-new/theme.css` +
//! `components.css`). Callers pass typed configuration structs in and
//! get back ready-to-embed `String`s. No DB access lives here — the
//! foundation step is purely structural.
//!
//! Search is modelled explicitly: [`SearchConfig`] + [`SearchProminence`]
//! declare per-page intent, and every admin page must make a deliberate
//! choice (`Hidden`, `Standard`, or `Primary`). Same component DNA in
//! all three modes — placement and weight differ, markup does not.

use std::fmt::Write as _;

// ---------------------------------------------------------------
// Search configuration
// ---------------------------------------------------------------

/// How visible the search control should be on a page.
///
/// - `Hidden`: no search UI is rendered at all.
/// - `Standard`: search appears inside the toolbar alongside filter
///   chips. Visually secondary but still clear.
/// - `Primary`: search is the dominant control on the page, rendered
///   above the toolbar with more visual weight. Still the same
///   component — prominence is a CSS variant, not a separate widget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchProminence {
    Hidden,
    Standard,
    Primary,
}

/// Declarative search configuration.
///
/// `label` and `placeholder` are human-readable strings. Vague
/// placeholders like `"Search..."` are a code-review red flag — the
/// placeholder should always name what the user can type (e.g.
/// `"Search users by username, email, or full name"`).
#[derive(Debug, Clone)]
pub struct SearchConfig {
    pub enabled: bool,
    pub prominence: SearchProminence,
    pub label: String,
    pub placeholder: String,
    pub keyboard_enabled: bool,
    /// Initial user query, echoed back into the input value.
    pub value: String,
    /// Form action URL. GET submits with query param `q`.
    pub action: String,
    pub filters: Vec<FilterChip>,
}

impl SearchConfig {
    pub fn hidden() -> Self {
        Self {
            enabled: false,
            prominence: SearchProminence::Hidden,
            label: String::new(),
            placeholder: String::new(),
            keyboard_enabled: false,
            value: String::new(),
            action: String::new(),
            filters: Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct FilterChip {
    pub label: String,
    pub count: Option<String>,
    pub active: bool,
}

// ---------------------------------------------------------------
// Topbar
// ---------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct TopbarConfig {
    pub brand: String,
    pub brand_mark: String,
    pub env_label: String,
    pub user_initials: String,
    pub user_email: String,
}

pub fn render_topbar(cfg: &TopbarConfig) -> String {
    format!(
        r#"<div class="topbar">
  <div class="brand">
    <div class="brand-mark">{mark}</div>
    {brand}
    <span class="brand-sep">/</span>
    <span class="brand-env">{env}</span>
  </div>
  <div class="topbar-spacer"></div>
  <div class="topbar-actions">
    <a href="/docs">Docs</a>
    <a href="/admin">Classic admin</a>
    <div class="user-chip">
      <div class="user-avatar">{initials}</div>
      <span>{email}</span>
    </div>
  </div>
</div>"#,
        mark = html_escape(&cfg.brand_mark),
        brand = html_escape(&cfg.brand),
        env = html_escape(&cfg.env_label),
        initials = html_escape(&cfg.user_initials),
        email = html_escape(&cfg.user_email),
    )
}

// ---------------------------------------------------------------
// Sidebar
// ---------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct SidebarGroup {
    pub label: String,
    pub items: Vec<SidebarItem>,
}

#[derive(Debug, Clone)]
pub struct SidebarItem {
    pub label: String,
    pub count: Option<String>,
    pub href: String,
    pub active: bool,
}

pub fn render_sidebar(groups: &[SidebarGroup]) -> String {
    let mut s = String::from(r#"<aside class="sidebar">"#);
    for group in groups {
        s.push_str(r#"<div class="sidebar-group">"#);
        let _ = write!(
            s,
            r#"<div class="sidebar-label">{}</div>"#,
            html_escape(&group.label)
        );
        for item in &group.items {
            let active_cls = if item.active { " active" } else { "" };
            let _ = write!(
                s,
                r#"<a class="sidebar-item{cls}" href="{href}">{label}"#,
                cls = active_cls,
                href = html_escape(&item.href),
                label = html_escape(&item.label),
            );
            if let Some(count) = &item.count {
                let _ = write!(
                    s,
                    r#" <span class="sidebar-count">{}</span>"#,
                    html_escape(count)
                );
            }
            s.push_str("</a>");
        }
        s.push_str("</div>");
    }
    s.push_str("</aside>");
    s
}

// ---------------------------------------------------------------
// Page header
// ---------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct Breadcrumb {
    pub label: String,
    pub href: Option<String>,
}

#[derive(Debug, Clone)]
pub struct PageAction {
    pub label: String,
    pub href: Option<String>,
    pub primary: bool,
}

#[derive(Debug, Clone)]
pub struct PageHeaderConfig {
    pub breadcrumbs: Vec<Breadcrumb>,
    pub title: String,
    pub subtitle: Option<String>,
    pub actions: Vec<PageAction>,
}

pub fn render_page_header(cfg: &PageHeaderConfig) -> String {
    let mut s = String::new();
    if !cfg.breadcrumbs.is_empty() {
        s.push_str(r#"<div class="breadcrumbs">"#);
        let last = cfg.breadcrumbs.len() - 1;
        for (i, crumb) in cfg.breadcrumbs.iter().enumerate() {
            if i > 0 {
                s.push_str(r#"<span class="sep">›</span>"#);
            }
            match (&crumb.href, i == last) {
                (Some(href), false) => {
                    let _ = write!(
                        s,
                        r#"<a href="{}">{}</a>"#,
                        html_escape(href),
                        html_escape(&crumb.label)
                    );
                }
                _ => {
                    let _ = write!(s, "<span>{}</span>", html_escape(&crumb.label));
                }
            }
        }
        s.push_str("</div>");
    }
    s.push_str(r#"<div class="page-header"><div>"#);
    let _ = write!(
        s,
        r#"<h1 class="page-title">{}</h1>"#,
        html_escape(&cfg.title)
    );
    if let Some(sub) = &cfg.subtitle {
        let _ = write!(
            s,
            r#"<div class="page-subtitle">{}</div>"#,
            html_escape(sub)
        );
    }
    s.push_str("</div>");
    if !cfg.actions.is_empty() {
        s.push_str(r#"<div class="btn-group">"#);
        for action in &cfg.actions {
            let cls = if action.primary {
                "btn btn-primary"
            } else {
                "btn"
            };
            match &action.href {
                Some(href) => {
                    let _ = write!(
                        s,
                        r#"<a class="{cls}" href="{href}">{label}</a>"#,
                        cls = cls,
                        href = html_escape(href),
                        label = html_escape(&action.label),
                    );
                }
                None => {
                    let _ = write!(
                        s,
                        r#"<button type="button" class="{cls}">{label}</button>"#,
                        cls = cls,
                        label = html_escape(&action.label),
                    );
                }
            }
        }
        s.push_str("</div>");
    }
    s.push_str("</div>");
    s
}

// ---------------------------------------------------------------
// Toolbar + search + filter chips
// ---------------------------------------------------------------

/// Render the search-and-filter region for a page. Dispatches on
/// [`SearchProminence`]:
///
/// - `Hidden`: empty string.
/// - `Standard`: toolbar row containing the search input followed by
///   filter chips.
/// - `Primary`: search rendered above the toolbar with additional
///   weight; filter chips still sit in the toolbar below.
pub fn render_toolbar(search: &SearchConfig) -> String {
    if !search.enabled || matches!(search.prominence, SearchProminence::Hidden) {
        return String::new();
    }
    match search.prominence {
        SearchProminence::Hidden => String::new(),
        SearchProminence::Standard => {
            let mut s = String::from(r#"<div class="toolbar">"#);
            s.push_str(&render_search(search));
            for chip in &search.filters {
                s.push_str(&render_filter_chip(chip));
            }
            s.push_str("</div>");
            s.push_str(&render_keyboard_hint(search));
            s
        }
        SearchProminence::Primary => {
            let mut s = String::from(r#"<div class="search-primary-wrap">"#);
            s.push_str(&render_search(search));
            s.push_str("</div>");
            s.push_str(&render_keyboard_hint(search));
            if !search.filters.is_empty() {
                s.push_str(r#"<div class="toolbar toolbar-filters-only">"#);
                for chip in &search.filters {
                    s.push_str(&render_filter_chip(chip));
                }
                s.push_str("</div>");
            }
            s
        }
    }
}

pub fn render_search(search: &SearchConfig) -> String {
    if !search.enabled || matches!(search.prominence, SearchProminence::Hidden) {
        return String::new();
    }
    let wrapper_cls = match search.prominence {
        SearchProminence::Primary => "search search-primary",
        _ => "search",
    };
    let action = if search.action.is_empty() {
        "#".to_string()
    } else {
        html_escape(&search.action)
    };
    format!(
        r#"<form class="search-form" role="search" method="get" action="{action}" aria-label="{label}">
  <div class="{wrapper_cls}">
    <svg class="search-icon" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
      <circle cx="11" cy="11" r="8"></circle>
      <line x1="21" y1="21" x2="16.65" y2="16.65"></line>
    </svg>
    <label class="sr-only" for="admin-new-search">{label}</label>
    <input id="admin-new-search" name="q" type="search" value="{value}" placeholder="{placeholder}" data-role="search-input" autocomplete="off">
    <button type="submit" class="search-submit" aria-label="Submit search">Search <kbd>⏎</kbd></button>
  </div>
</form>"#,
        action = action,
        wrapper_cls = wrapper_cls,
        label = html_escape(&search.label),
        value = html_escape(&search.value),
        placeholder = html_escape(&search.placeholder),
    )
}

pub fn render_filter_chip(chip: &FilterChip) -> String {
    let active_cls = if chip.active { " active" } else { "" };
    let mut s = format!(
        r#"<button type="button" class="filter-chip{cls}">{label}"#,
        cls = active_cls,
        label = html_escape(&chip.label),
    );
    if let Some(count) = &chip.count {
        let _ = write!(s, r#" <span class="count">{}</span>"#, html_escape(count));
    }
    s.push_str("</button>");
    s
}

fn render_keyboard_hint(search: &SearchConfig) -> String {
    if !search.keyboard_enabled {
        return String::new();
    }
    String::from(
        r#"<div class="search-hint">Press <kbd class="kbd-inline">/</kbd> or <kbd class="kbd-inline">⌘K</kbd> to search instantly · <kbd class="kbd-inline">Esc</kbd> to exit</div>"#,
    )
}

// ---------------------------------------------------------------
// Table shell
// ---------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct TableColumn {
    pub label: String,
    pub sorted: bool,
    pub sort_arrow: Option<String>,
    pub width: Option<String>,
    pub is_checkbox: bool,
}

impl TableColumn {
    pub fn text(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            sorted: false,
            sort_arrow: None,
            width: None,
            is_checkbox: false,
        }
    }

    pub fn checkbox() -> Self {
        Self {
            label: String::new(),
            sorted: false,
            sort_arrow: None,
            width: Some("36px".into()),
            is_checkbox: true,
        }
    }

    pub fn sorted(mut self, arrow: impl Into<String>) -> Self {
        self.sorted = true;
        self.sort_arrow = Some(arrow.into());
        self
    }
}

#[derive(Debug, Clone)]
pub struct TableRow {
    pub selected: bool,
    pub cells: Vec<TableCell>,
}

#[derive(Debug, Clone)]
pub enum TableCell {
    Plain(String),
    Mono(String),
    Primary(String),
    Badge { variant: BadgeVariant, text: String },
    Checkbox { checked: bool },
}

#[derive(Debug, Clone, Copy)]
pub enum BadgeVariant {
    Success,
    Warn,
    Danger,
    Muted,
    Rust,
}

impl BadgeVariant {
    fn class(self) -> &'static str {
        match self {
            BadgeVariant::Success => "badge-success",
            BadgeVariant::Warn => "badge-warn",
            BadgeVariant::Danger => "badge-danger",
            BadgeVariant::Muted => "badge-muted",
            BadgeVariant::Rust => "badge-rust",
        }
    }
}

#[derive(Debug, Clone)]
pub struct PaginationConfig {
    pub showing_from: usize,
    pub showing_to: usize,
    pub total: usize,
    pub current_page: usize,
    pub page_count: usize,
}

#[derive(Debug, Clone)]
pub struct TableShellConfig {
    pub columns: Vec<TableColumn>,
    pub rows: Vec<TableRow>,
    pub pagination: Option<PaginationConfig>,
}

pub fn render_table_shell(cfg: &TableShellConfig) -> String {
    let mut s = String::from(r#"<div class="table-wrap"><table><thead><tr>"#);
    for col in &cfg.columns {
        let width_attr = col
            .width
            .as_ref()
            .map(|w| format!(r#" style="width: {};""#, w))
            .unwrap_or_default();
        if col.is_checkbox {
            s.push_str(
                r#"<th style="width: 36px; cursor: default;"><span class="checkbox"></span></th>"#,
            );
        } else {
            let sort_cls = if col.sorted { " class=\"sorted\"" } else { "" };
            let _ = write!(
                s,
                r#"<th{sort}{w}>{label}"#,
                sort = sort_cls,
                w = width_attr,
                label = html_escape(&col.label),
            );
            if let Some(arrow) = &col.sort_arrow {
                let _ = write!(
                    s,
                    r#" <span class="sort-arrow">{}</span>"#,
                    html_escape(arrow)
                );
            }
            s.push_str("</th>");
        }
    }
    s.push_str("</tr></thead><tbody>");
    for row in &cfg.rows {
        let cls = if row.selected {
            r#" class="selected""#
        } else {
            ""
        };
        let _ = write!(s, "<tr{}>", cls);
        for cell in &row.cells {
            s.push_str(&render_cell(cell));
        }
        s.push_str("</tr>");
    }
    s.push_str("</tbody></table>");
    if let Some(pag) = &cfg.pagination {
        s.push_str(&render_pagination(pag));
    }
    s.push_str("</div>");
    s
}

fn render_cell(cell: &TableCell) -> String {
    match cell {
        TableCell::Plain(v) => format!("<td>{}</td>", html_escape(v)),
        TableCell::Mono(v) => format!(r#"<td class="mono">{}</td>"#, html_escape(v)),
        TableCell::Primary(v) => format!(r#"<td class="primary-col mono">{}</td>"#, html_escape(v)),
        TableCell::Badge { variant, text } => format!(
            r#"<td><span class="badge {cls}">{text}</span></td>"#,
            cls = variant.class(),
            text = html_escape(text),
        ),
        TableCell::Checkbox { checked } => {
            let cls = if *checked {
                "checkbox checked"
            } else {
                "checkbox"
            };
            format!(r#"<td><span class="{}"></span></td>"#, cls)
        }
    }
}

pub fn render_pagination(cfg: &PaginationConfig) -> String {
    let mut s = format!(
        r#"<div class="pagination"><div>Showing <span>{f}</span>–<span>{t}</span> of <span>{n}</span></div><div class="pagination-controls">"#,
        f = cfg.showing_from,
        t = cfg.showing_to,
        n = cfg.total,
    );
    let prev_disabled = if cfg.current_page <= 1 {
        " disabled"
    } else {
        ""
    };
    let _ = write!(
        s,
        r#"<button type="button" class="page-btn"{}>‹ Prev</button>"#,
        prev_disabled
    );
    for p in 1..=cfg.page_count {
        let active = if p == cfg.current_page { " active" } else { "" };
        let _ = write!(
            s,
            r#"<button type="button" class="page-btn{}">{}</button>"#,
            active, p
        );
    }
    let next_disabled = if cfg.current_page >= cfg.page_count {
        " disabled"
    } else {
        ""
    };
    let _ = write!(
        s,
        r#"<button type="button" class="page-btn"{}>Next ›</button>"#,
        next_disabled
    );
    s.push_str("</div></div>");
    s
}

// ---------------------------------------------------------------
// HTML escape
// ---------------------------------------------------------------

pub fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}
