//! UI layer for the `/ops` operational console. Layout, visual
//! identity, and shared HTML helpers. Tailwind is loaded from a CDN;
//! one tiny custom stylesheet block below handles the font and
//! custom scrollbar. Brand colour is `teal` (Tailwind built-in).

#![allow(dead_code)]

use bytes::Bytes;
use chrono::{DateTime, Datelike, Utc};
use http_body_util::Full;
use hyper::StatusCode;
use rustio_core::auth::User;
use rustio_core::{html, Response};

// ═══════════════════════════════════════════════════════════════
// Minimal custom CSS — font import + scrollbar. Everything else
// comes from Tailwind utility classes on each element.
// ═══════════════════════════════════════════════════════════════

pub const CUSTOM_STYLES: &str = r#"
@import url('https://fonts.googleapis.com/css2?family=Inter:wght@300;400;500;600;700&display=swap');
body { font-family: 'Inter', system-ui, -apple-system, sans-serif; }
.custom-scrollbar::-webkit-scrollbar { height: 8px; width: 8px; }
.custom-scrollbar::-webkit-scrollbar-track { background: #f1f5f9; }
.custom-scrollbar::-webkit-scrollbar-thumb { background: #cbd5e1; border-radius: 4px; }
.custom-scrollbar::-webkit-scrollbar-thumb:hover { background: #94a3b8; }
/* Tailwind forms don't ship native details styling — keep the summary's default caret out. */
summary::-webkit-details-marker { display: none; }
summary { list-style: none; }
.kbd {
  font-family: ui-monospace, SF Mono, Menlo, monospace; font-size: 11px;
  padding: 1px 5px; margin: 0 2px; border: 1px solid #cbd5e1;
  border-bottom-width: 2px; border-radius: 4px;
  background: #f8fafc; color: #475569;
}
/* Timeline dots */
.timeline-dot { position: absolute; left: -7px; top: 6px; width: 12px; height: 12px;
  background: #0d9488; border: 2px solid #fff; border-radius: 9999px;
  box-shadow: 0 0 0 2px #e2e8f0; }
.timeline-dot.cancelled { background: #94a3b8; }
"#;

// ═══════════════════════════════════════════════════════════════
// Icon kit — inline SVGs. All stroked, 24×24 viewBox, use currentColor.
// ═══════════════════════════════════════════════════════════════

macro_rules! ic {
    ($body:expr, $size:expr) => {
        concat!(
            r#"<svg class="w-"#,
            $size,
            r#" h-"#,
            $size,
            r#"" fill="none" stroke="currentColor" viewBox="0 0 24 24" aria-hidden="true">"#,
            $body,
            r#"</svg>"#
        )
    };
}

pub const ICON_LOGO: &str = ic!(r#"<path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M19.428 15.428a2 2 0 00-1.022-.547l-2.387-.477a6 6 0 00-3.86.517l-.318.158a6 6 0 01-3.86.517L6.05 15.21a2 2 0 00-1.806.547M8 4h8l-1 1v5.172a2 2 0 00.586 1.414l5 5c1.26 1.26.367 3.414-1.415 3.414H4.828c-1.782 0-2.674-2.154-1.414-3.414l5-5A2 2 0 009 10.172V5L8 4z"/>"#, "8");
pub const ICON_DASHBOARD: &str = ic!(r#"<path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M3 12l2-2m0 0l7-7 7 7M5 10v10a1 1 0 001 1h3m10-11l2 2m-2-2v10a1 1 0 01-1 1h-3m-6 0a1 1 0 001-1v-4a1 1 0 011-1h2a1 1 0 011 1v4a1 1 0 001 1m-6 0h6"/>"#, "5");
pub const ICON_CALENDAR: &str = ic!(r#"<path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M8 7V3m8 4V3m-9 8h10M5 21h14a2 2 0 002-2V7a2 2 0 00-2-2H5a2 2 0 00-2 2v12a2 2 0 002 2z"/>"#, "5");
pub const ICON_USERS: &str = ic!(r#"<path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 4.354a4 4 0 110 5.292M15 21H3v-1a6 6 0 0112 0v1zm0 0h6v-1a6 6 0 00-9-5.197M13 7a4 4 0 11-8 0 4 4 0 018 0z"/>"#, "5");
pub const ICON_STETH: &str = ic!(r#"<path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M4.8 2.3A.3.3 0 1 0 5 2H4a2 2 0 0 0-2 2v5a6 6 0 0 0 6 6v0a6 6 0 0 0 6-6V4a2 2 0 0 0-2-2h-1a.2.2 0 1 0 .3.3"/><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M8 15v1a6 6 0 0 0 6 6v0a6 6 0 0 0 6-6v-4"/><circle cx="20" cy="10" r="2" stroke-width="2"/>"#, "5");
pub const ICON_CHART: &str = ic!(r#"<path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 19v-6a2 2 0 00-2-2H5a2 2 0 00-2 2v6a2 2 0 002 2h2a2 2 0 002-2zm0 0V9a2 2 0 012-2h2a2 2 0 012 2v10m-6 0a2 2 0 002 2h2a2 2 0 002-2m0 0V5a2 2 0 012-2h2a2 2 0 012 2v14a2 2 0 01-2 2h-2a2 2 0 01-2-2z"/>"#, "5");
pub const ICON_SEARCH_LG: &str = ic!(r#"<path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z"/>"#, "5");
pub const ICON_PLUS_SM: &str = ic!(r#"<path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 4v16m8-8H4"/>"#, "4");
pub const ICON_FILTER: &str = ic!(r#"<path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M3 4a1 1 0 011-1h16a1 1 0 011 1v2.586a1 1 0 01-.293.707l-6.414 6.414a1 1 0 00-.293.707V17l-4 4v-6.586a1 1 0 00-.293-.707L3.293 7.293A1 1 0 013 6.586V4z"/>"#, "4");
pub const ICON_CHEVRON_DOWN_SM: &str = ic!(r#"<path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M19 9l-7 7-7-7"/>"#, "4");
pub const ICON_COLUMNS: &str = ic!(r#"<path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M4 6a2 2 0 012-2h3v16H6a2 2 0 01-2-2V6zm11-2h3a2 2 0 012 2v12a2 2 0 01-2 2h-3V4z"/>"#, "4");
pub const ICON_BELL: &str = ic!(r#"<path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M15 17h5l-1.405-1.405A2.032 2.032 0 0118 14.158V11a6.002 6.002 0 00-4-5.659V5a2 2 0 10-4 0v.341C7.67 6.165 6 8.388 6 11v3.159c0 .538-.214 1.055-.595 1.436L4 17h5m6 0v1a3 3 0 11-6 0v-1m6 0H9"/>"#, "6");
pub const ICON_CLOCK: &str = ic!(r#"<path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 8v4l3 3m6-3a9 9 0 11-18 0 9 9 0 0118 0z"/>"#, "4");
pub const ICON_STATUS: &str = ic!(r#"<path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 5H7a2 2 0 00-2 2v12a2 2 0 002 2h10a2 2 0 002-2V7a2 2 0 00-2-2h-2M9 5a2 2 0 002 2h2a2 2 0 002-2M9 5a2 2 0 012-2h2a2 2 0 012 2"/>"#, "4");
pub const ICON_DOTS_VERT: &str = ic!(r#"<path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 5v.01M12 12v.01M12 19v.01M12 6a1 1 0 110-2 1 1 0 010 2zm0 7a1 1 0 110-2 1 1 0 010 2zm0 7a1 1 0 110-2 1 1 0 010 2z"/>"#, "5");
pub const ICON_ALERT_SM: &str = ic!(r#"<circle cx="12" cy="12" r="10" stroke-width="2"/><line x1="12" y1="8" x2="12" y2="12" stroke-width="2" stroke-linecap="round"/><line x1="12" y1="16" x2="12.01" y2="16" stroke-width="2" stroke-linecap="round"/>"#, "5");
pub const ICON_LOGOUT_SM: &str = ic!(r#"<path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M17 16l4-4m0 0l-4-4m4 4H7m6 4v1a3 3 0 01-3 3H6a3 3 0 01-3-3V7a3 3 0 013-3h4a3 3 0 013 3v1"/>"#, "5");
pub const ICON_ARROW_LEFT_SM: &str = ic!(r#"<path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M10 19l-7-7m0 0l7-7m-7 7h18"/>"#, "4");
pub const ICON_INBOX_LG: &str = ic!(r#"<path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M20 13V6a2 2 0 00-2-2H6a2 2 0 00-2 2v7m16 0v5a2 2 0 01-2 2H6a2 2 0 01-2-2v-5m16 0h-2.586a1 1 0 00-.707.293l-2.414 2.414a1 1 0 01-.707.293h-3.172a1 1 0 01-.707-.293l-2.414-2.414A1 1 0 006.586 13H4"/>"#, "8");
pub const ICON_CHECK_SM: &str = ic!(r#"<path stroke-linecap="round" stroke-linejoin="round" stroke-width="2.5" d="M5 13l4 4L19 7"/>"#, "4");
pub const ICON_X_SM: &str = ic!(r#"<path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M6 18L18 6M6 6l12 12"/>"#, "4");

// ═══════════════════════════════════════════════════════════════
// Shell
// ═══════════════════════════════════════════════════════════════

#[derive(Clone, Copy, PartialEq)]
pub enum Nav {
    Overview,
    Appointments,
    Patients,
    Doctors,
    Reports,
    Other,
}

pub struct Actor<'a> {
    pub user: &'a User,
    pub bearer: &'a str,
    pub csrf: &'a str,
}

/// Options passed into the shell for the header bar.
pub struct ShellOpts<'a> {
    /// Short page title rendered in the content header (e.g.
    /// "Appointment Center", "New appointment", "Appointment #42").
    pub header_title: &'a str,
    /// Right-aligned status badge in the header (e.g. "12 Today").
    /// Pass `""` to hide.
    pub header_badge: &'a str,
}

pub fn render_shell(
    title: &str,
    actor: &Actor<'_>,
    active: Nav,
    opts: &ShellOpts<'_>,
    content: &str,
    page_js: &str,
) -> String {
    let side = |label: &str, href: &str, icon: &str, this: Nav, enabled: bool| -> String {
        let is_active = this == active;
        let cls = if is_active {
            "flex items-center gap-3 px-3 py-2 bg-teal-600 text-white rounded-md font-medium shadow-sm"
        } else if enabled {
            "flex items-center gap-3 px-3 py-2 text-slate-300 hover:bg-slate-800 hover:text-white rounded-md transition-colors"
        } else {
            "flex items-center gap-3 px-3 py-2 text-slate-500 rounded-md opacity-50 cursor-not-allowed pointer-events-none"
        };
        format!(
            r#"<a class="{cls}" href="{href}">{icon}{label}</a>"#,
            cls = cls,
            href = href,
            icon = icon,
            label = escape_html(label),
        )
    };

    let avatar_initials = initials(&actor.user.email);
    let role_label = humanise_role(&actor.user.role);

    let badge_html = if opts.header_badge.is_empty() {
        String::new()
    } else {
        format!(
            r#"<span class="px-2.5 py-0.5 rounded-full bg-teal-50 text-teal-700 text-xs font-medium border border-teal-100">{}</span>"#,
            escape_html(opts.header_badge),
        )
    };

    format!(
        r##"<!doctype html>
<html lang="en" dir="ltr">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>{title} · MedFlow</title>
  <meta name="api-token" content="{token}">
  <script src="https://cdn.tailwindcss.com"></script>
  <style>{styles}</style>
</head>
<body class="bg-slate-50 text-slate-800 antialiased">
<div class="flex h-screen overflow-hidden">

  <aside class="w-64 bg-slate-900 text-slate-300 flex flex-col">
    <div class="h-16 flex items-center px-6 border-b border-slate-800">
      <a href="/ops/appointments" class="flex items-center gap-2 text-teal-400 hover:no-underline">
        {logo}
        <h1 class="text-xl font-bold tracking-tight">Med<span class="text-white">Flow</span></h1>
      </a>
    </div>
    <nav class="flex-1 px-4 py-6 space-y-1 overflow-y-auto">
      <p class="px-3 text-xs font-semibold text-slate-500 uppercase tracking-wider mb-2">Operations</p>
      {overview}
      {appts}
      <p class="px-3 text-xs font-semibold text-slate-500 uppercase tracking-wider mb-2 mt-5">Directory</p>
      {patients}
      {doctors}
      <p class="px-3 text-xs font-semibold text-slate-500 uppercase tracking-wider mb-2 mt-5">Insights</p>
      {reports}
    </nav>
    <div class="p-4 border-t border-slate-800 flex items-center gap-3">
      <div class="w-9 h-9 rounded-full bg-gradient-to-br from-teal-500 to-teal-700 text-white grid place-items-center font-semibold text-sm">{initials}</div>
      <div class="flex-1 min-w-0">
        <p class="text-sm text-white font-medium truncate" title="{email}">{email}</p>
        <p class="text-xs text-slate-500 uppercase tracking-wide">{role}</p>
      </div>
      <form method="post" action="/admin/logout" class="m-0">
        <input type="hidden" name="_csrf" value="{csrf}">
        <button type="submit" title="Sign out" class="p-1.5 text-slate-400 hover:text-white hover:bg-slate-800 rounded-md transition-colors">{logout_icon}</button>
      </form>
    </div>
  </aside>

  <main class="flex-1 flex flex-col overflow-hidden">
    <header class="h-16 bg-white shadow-sm flex items-center justify-between px-6 lg:px-8 border-b border-slate-200 z-10">
      <div class="flex items-center gap-4">
        <h2 class="text-xl font-semibold text-slate-800">{header_title}</h2>
        {badge}
      </div>
      <div class="flex items-center gap-4">
        <button type="button" class="p-2 text-slate-400 hover:text-teal-600 transition-colors relative" title="Notifications">
          {bell}
          <span class="absolute top-1 right-1.5 w-2.5 h-2.5 bg-red-500 rounded-full border-2 border-white"></span>
        </button>
        <div class="h-8 w-px bg-slate-200"></div>
        <div class="flex items-center gap-3">
          <div class="text-right hidden md:block">
            <p class="text-sm font-medium text-slate-700 leading-none">{role}</p>
            <p class="text-xs text-slate-500 mt-1">{email}</p>
          </div>
          <div class="w-9 h-9 rounded-full bg-gradient-to-br from-teal-500 to-teal-700 text-white grid place-items-center font-semibold text-sm border border-slate-200 shadow-sm">{initials}</div>
        </div>
      </div>
    </header>

    <div class="flex-1 overflow-y-auto p-6 lg:p-8 custom-scrollbar">
      {content}
    </div>
  </main>
</div>
<script>{page_js}</script>
</body>
</html>"##,
        title = escape_html(title),
        token = escape_html(actor.bearer),
        styles = CUSTOM_STYLES,
        logo = ICON_LOGO,
        overview = side("Dashboard Overview", "/admin", ICON_DASHBOARD, Nav::Overview, true),
        appts = side("Appointment Center", "/ops/appointments", ICON_CALENDAR, Nav::Appointments, true),
        patients = side("Patients", "/admin/patients", ICON_USERS, Nav::Patients, true),
        doctors = side("Doctors", "/admin/doctors", ICON_STETH, Nav::Doctors, true),
        reports = side("Analytics & Reports", "#", ICON_CHART, Nav::Reports, false),
        initials = escape_html(&avatar_initials),
        email = escape_html(&actor.user.email),
        role = escape_html(&role_label),
        csrf = escape_html(actor.csrf),
        logout_icon = ICON_LOGOUT_SM,
        header_title = escape_html(opts.header_title),
        badge = badge_html,
        bell = ICON_BELL,
        content = content,
        page_js = page_js,
    )
}

// ═══════════════════════════════════════════════════════════════
// Formatting helpers
// ═══════════════════════════════════════════════════════════════

pub fn humanise_role(role: &str) -> String {
    match role {
        "receptionist" => "Receptionist",
        "doctor" => "Doctor",
        "billing" => "Billing",
        "admin" => "System Admin",
        other => other,
    }
    .to_string()
}

pub fn humanise_status(s: &str) -> String {
    match s {
        "scheduled" => "Scheduled",
        "confirmed" => "Confirmed",
        "in_progress" => "In Progress",
        "completed" => "Completed",
        "cancelled" => "Cancelled",
        other => other,
    }
    .to_string()
}

/// Tailwind classes + dot colour for the status pill.
pub fn status_pill_classes(s: &str) -> &'static str {
    match s {
        "scheduled" => "bg-indigo-50 text-indigo-700 border-indigo-200",
        "confirmed" => "bg-cyan-50 text-cyan-700 border-cyan-200",
        "in_progress" => "bg-amber-50 text-amber-700 border-amber-200",
        "completed" => "bg-emerald-50 text-emerald-700 border-emerald-200",
        "cancelled" => "bg-slate-100 text-slate-700 border-slate-200",
        _ => "bg-slate-100 text-slate-700 border-slate-200",
    }
}
pub fn status_dot_color(s: &str) -> &'static str {
    match s {
        "scheduled" => "bg-indigo-500",
        "confirmed" => "bg-cyan-500",
        "in_progress" => "bg-amber-500",
        "completed" => "bg-emerald-500",
        "cancelled" => "bg-slate-400",
        _ => "bg-slate-400",
    }
}

pub fn pluralise(n: usize, singular: &str) -> String {
    if n == 1 {
        format!("1 {singular}")
    } else {
        format!("{n} {singular}s")
    }
}

pub fn initials(s: &str) -> String {
    let trimmed = s.split('@').next().unwrap_or(s).replace(['.', '_', '-'], " ");
    let mut out = String::new();
    for word in trimmed.split_whitespace().take(2) {
        if let Some(c) = word.chars().next() {
            out.push(c.to_ascii_uppercase());
        }
    }
    if out.is_empty() {
        out.push('?');
    }
    out
}

pub fn relative_past(ts: DateTime<Utc>) -> String {
    let delta = Utc::now() - ts;
    let secs = delta.num_seconds();
    if secs < 60 {
        "just now".to_string()
    } else if secs < 3600 {
        format!("{} min ago", delta.num_minutes())
    } else if secs < 86400 {
        format!("{} h ago", delta.num_hours())
    } else if delta.num_days() < 7 {
        format!("{} d ago", delta.num_days())
    } else {
        ts.format("%Y-%m-%d").to_string()
    }
}

pub fn iso_ymd(ts: DateTime<Utc>) -> String {
    format!("{:04}-{:02}-{:02}", ts.year(), ts.month(), ts.day())
}

pub fn short_time(ts: DateTime<Utc>) -> String {
    ts.format("%H:%M").to_string()
}

// ═══════════════════════════════════════════════════════════════
// Shared HTML helpers
// ═══════════════════════════════════════════════════════════════

pub fn escape_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
    out
}

pub fn redirect(to: &str) -> Response {
    hyper::Response::builder()
        .status(StatusCode::SEE_OTHER)
        .header("location", to)
        .body(Full::new(Bytes::new()))
        .expect("valid redirect")
}

pub fn forbidden_page(actor: &Actor<'_>, message: &str) -> Response {
    let content = format!(
        r#"<div class="bg-white rounded-xl shadow-sm border border-slate-200 p-12 text-center">
  <div class="mx-auto w-14 h-14 rounded-full bg-red-50 text-red-600 grid place-items-center mb-4">{icon}</div>
  <h3 class="text-lg font-semibold text-slate-800 mb-2">Not allowed</h3>
  <p class="text-sm text-slate-500 mb-5">{message}</p>
  <a href="/ops/appointments" class="inline-flex items-center gap-2 px-4 py-2 bg-teal-600 text-white rounded-lg text-sm font-medium hover:bg-teal-700 transition-colors shadow-sm">Back to appointments</a>
</div>"#,
        icon = ICON_ALERT_SM,
        message = escape_html(message),
    );
    let opts = ShellOpts {
        header_title: "Forbidden",
        header_badge: "",
    };
    let body = render_shell("Forbidden", actor, Nav::Other, &opts, &content, "");
    let mut resp = html(body);
    *resp.status_mut() = StatusCode::FORBIDDEN;
    resp
}

pub fn not_found_page(actor: &Actor<'_>, what: &str) -> Response {
    let content = format!(
        r#"<div class="bg-white rounded-xl shadow-sm border border-slate-200 p-12 text-center">
  <div class="mx-auto w-14 h-14 rounded-full bg-slate-100 text-slate-500 grid place-items-center mb-4">{icon}</div>
  <h3 class="text-lg font-semibold text-slate-800 mb-2">Not found</h3>
  <p class="text-sm text-slate-500 mb-5">{message}</p>
  <a href="/ops/appointments" class="inline-flex items-center gap-2 px-4 py-2 bg-teal-600 text-white rounded-lg text-sm font-medium hover:bg-teal-700 transition-colors shadow-sm">Back to appointments</a>
</div>"#,
        icon = ICON_INBOX_LG,
        message = escape_html(what),
    );
    let opts = ShellOpts {
        header_title: "Not found",
        header_badge: "",
    };
    let body = render_shell("Not found", actor, Nav::Other, &opts, &content, "");
    let mut resp = html(body);
    *resp.status_mut() = StatusCode::NOT_FOUND;
    resp
}
