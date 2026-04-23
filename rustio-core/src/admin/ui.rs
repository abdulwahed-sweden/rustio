//! Admin UI primitives (v3, 2026-04-23 full-UI-reset).
//!
//! Minimal set of typed helpers for the shell components every
//! admin page renders: topbar, sidebar, page header. Class names
//! here match [`components.css`] one-to-one; no post-hoc string
//! manipulation downstream. When a new shell element is needed,
//! add a helper here rather than inlining markup in `layout.rs`.

use std::fmt::Write as _;

// =============================================================
// Topbar
// =============================================================

#[derive(Debug, Clone)]
pub struct TopbarConfig {
    /// Human-readable label for the current section (e.g. "Dashboard",
    /// "Users"). Rendered next to the user chip at the right.
    pub title: String,
    pub user_initials: String,
    pub user_email: String,
    /// Per-session CSRF token for the topbar's logout form. `None`
    /// downgrades the Sign-out button to a GET link that lands on the
    /// legacy confirmation page (still works; the POST is just a
    /// smoother one-click path).
    pub csrf_token: Option<String>,
}

pub fn render_topbar(cfg: &TopbarConfig) -> String {
    let logout = match &cfg.csrf_token {
        Some(t) => format!(
            r#"<form method="post" action="/admin/logout" class="topbar-logout-form"><input type="hidden" name="_csrf" value="{}"><button type="submit" class="topbar-logout-btn">Sign out</button></form>"#,
            html_escape(t)
        ),
        None => String::from(r#"<a class="topbar-logout-btn" href="/admin/logout">Sign out</a>"#),
    };
    format!(
        r#"<header class="topbar">
  <div class="topbar-title">{title}</div>
  <div class="topbar-spacer"></div>
  <div class="topbar-actions">
    {logout}
    <div class="topbar-user">
      <div class="user-avatar" aria-hidden="true">{initials}</div>
      <div class="user-meta"><span class="sr-only">Signed in as </span><span>{email}</span></div>
    </div>
  </div>
</header>"#,
        title = html_escape(&cfg.title),
        logout = logout,
        initials = html_escape(&cfg.user_initials),
        email = html_escape(&cfg.user_email),
    )
}

// =============================================================
// Sidebar
// =============================================================

#[derive(Debug, Clone)]
pub struct SidebarGroup {
    pub label: Option<String>,
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
    let mut s = String::from(
        r#"<aside class="sidebar">
  <a class="sidebar-brand" href="/admin">
    <span class="sidebar-brand-mark">R</span>
    RustIO
  </a>
  <nav class="sidebar-nav">"#,
    );
    for group in groups {
        s.push_str(r#"<div class="sidebar-section">"#);
        if let Some(label) = &group.label {
            let _ = write!(
                s,
                r#"<div class="sidebar-section-label">{}</div>"#,
                html_escape(label)
            );
        }
        for item in &group.items {
            let active_attr = if item.active {
                r#" aria-current="page""#
            } else {
                ""
            };
            let _ = write!(
                s,
                r#"<a class="sidebar-link" href="{href}"{active}><span>{label}</span>"#,
                href = html_escape(&item.href),
                active = active_attr,
                label = html_escape(&item.label),
            );
            if let Some(count) = &item.count {
                let _ = write!(
                    s,
                    r#"<span class="sidebar-link-count">{}</span>"#,
                    html_escape(count)
                );
            }
            s.push_str("</a>");
        }
        s.push_str("</div>");
    }
    s.push_str("</nav></aside>");
    s
}

// =============================================================
// Page header
// =============================================================

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
    pub eyebrow: Option<String>,
    pub title: String,
    pub subtitle: Option<String>,
    pub actions: Vec<PageAction>,
    /// Unused under the new design — kept for API compatibility with
    /// callers that still pass breadcrumbs. Nothing is rendered; the
    /// eyebrow + topbar title replace breadcrumb navigation.
    pub breadcrumbs: Vec<Breadcrumb>,
}

pub fn render_page_header(cfg: &PageHeaderConfig) -> String {
    let mut s = String::from(r#"<section class="page-header"><div class="page-header-text">"#);
    if let Some(eyebrow) = &cfg.eyebrow {
        let _ = write!(s, r#"<p class="page-eyebrow">{}</p>"#, html_escape(eyebrow));
    }
    let _ = write!(
        s,
        r#"<h1 class="page-title">{}</h1>"#,
        html_escape(&cfg.title)
    );
    if let Some(sub) = &cfg.subtitle {
        let _ = write!(s, r#"<p class="page-subtitle">{}</p>"#, html_escape(sub));
    }
    s.push_str("</div>");
    if !cfg.actions.is_empty() {
        s.push_str(r#"<div class="page-actions">"#);
        for action in &cfg.actions {
            let cls = if action.primary {
                "btn btn-primary"
            } else {
                "btn btn-secondary"
            };
            match &action.href {
                Some(href) => {
                    let _ = write!(
                        s,
                        r#"<a class="{cls}" href="{href}">{label}</a>"#,
                        href = html_escape(href),
                        label = html_escape(&action.label),
                    );
                }
                None => {
                    let _ = write!(
                        s,
                        r#"<button type="button" class="{cls}">{label}</button>"#,
                        label = html_escape(&action.label),
                    );
                }
            }
        }
        s.push_str("</div>");
    }
    s.push_str("</section>");
    s
}

// =============================================================
// Utilities
// =============================================================

/// HTML-escape user-supplied text for interpolation into attributes
/// and text content. Handles `&`, `<`, `>`, `"`, `'`.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_escape_handles_specials() {
        assert_eq!(
            html_escape("a & b < c > d \"e\" 'f'"),
            "a &amp; b &lt; c &gt; d &quot;e&quot; &#39;f&#39;"
        );
    }

    #[test]
    fn topbar_renders_sign_out_form_when_csrf_present() {
        let html = render_topbar(&TopbarConfig {
            title: "Users".into(),
            user_initials: "AM".into(),
            user_email: "a@b.co".into(),
            csrf_token: Some("TOKEN".into()),
        });
        assert!(html.contains(r#"action="/admin/logout""#));
        assert!(html.contains(r#"name="_csrf" value="TOKEN""#));
    }

    #[test]
    fn sidebar_marks_active_item_with_aria_current() {
        let html = render_sidebar(&[SidebarGroup {
            label: Some("Models".into()),
            items: vec![SidebarItem {
                label: "Users".into(),
                count: Some("42".into()),
                href: "/admin/users".into(),
                active: true,
            }],
        }]);
        assert!(html.contains(r#"aria-current="page""#));
        assert!(html.contains(r#"<span>Users</span>"#));
        assert!(html.contains(r#"class="sidebar-link-count">42"#));
    }
}
