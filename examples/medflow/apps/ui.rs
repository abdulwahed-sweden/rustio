//! Shared UI layer for the `/ops` operational pages.
//!
//! All visual concerns live here:
//!
//!   * one stylesheet ([`STYLES`]) — spacing, typography, colour,
//!     dark-mode, responsive breakpoints;
//!   * one shell ([`render_shell`]) — top bar, main, footer, page JS;
//!   * a small icon kit (inline SVG consts);
//!   * tiny formatting helpers (`humanise_status`, `pluralise`,
//!     `format_dt_smart`);
//!   * a real logout form helper ([`logout_form`]) that POSTs to
//!     `/admin/logout` with CSRF pulled from the request context.
//!
//! Nothing here touches `rustio-core`. The framework admin at
//! `/admin/*` keeps its own styling.

#![allow(dead_code)]

use bytes::Bytes;
use chrono::{DateTime, Utc};
use http_body_util::Full;
use hyper::StatusCode;
use rustio_core::auth::User;
use rustio_core::{html, Response};

// ═══════════════════════════════════════════════════════════════
// Stylesheet
// ═══════════════════════════════════════════════════════════════

/// Whole `/ops` stylesheet as a single const. Uses CSS custom
/// properties for every token so dark-mode and future themes only
/// reassign a handful of variables.
pub const STYLES: &str = r#"
/* ── Design tokens ──────────────────────────────────────── */
:root {
  /* Spacing scale (modular, every step is a logical unit) */
  --s-0:   0;
  --s-0-5: 2px;
  --s-1:   4px;
  --s-2:   8px;
  --s-3:   12px;
  --s-4:   16px;
  --s-5:   20px;
  --s-6:   24px;
  --s-7:   32px;
  --s-8:   48px;

  /* Typography scale */
  --text-xs:   0.76rem;
  --text-sm:   0.86rem;
  --text-base: 0.94rem;
  --text-lg:   1.08rem;
  --text-xl:   1.26rem;
  --text-2xl:  1.55rem;
  --lh-tight:  1.25;
  --lh-normal: 1.5;

  /* Colour tokens */
  --brand:      #2563eb;
  --brand-dark: #1d4ed8;
  --brand-soft: #eff6ff;

  --text:       #0f172a;
  --text-dim:   #475569;
  --text-mute:  #94a3b8;

  --surface:    #ffffff;
  --surface-2:  #f8fafc;
  --bg:         #f1f5f9;

  --border:     #e2e8f0;
  --border-strong: #cbd5e1;

  --danger:     #dc2626;
  --danger-bg:  #fef2f2;
  --danger-border: #fecaca;
  --success:    #16a34a;
  --success-bg: #f0fdf4;
  --success-border: #bbf7d0;
  --warning:    #d97706;

  --radius:     8px;
  --radius-lg:  12px;
  --radius-pill: 9999px;
  --shadow-sm:  0 1px 2px rgba(15, 23, 42, 0.04), 0 0 0 1px rgba(15, 23, 42, 0.04);
  --shadow:     0 2px 4px rgba(15, 23, 42, 0.04), 0 0 0 1px rgba(15, 23, 42, 0.04);
  --shadow-lg:  0 10px 25px -5px rgba(15, 23, 42, 0.10), 0 8px 10px -6px rgba(15, 23, 42, 0.05);

  --ring:       0 0 0 3px rgba(37, 99, 235, 0.25);

  --font: system-ui, -apple-system, "Segoe UI", Roboto, "Helvetica Neue", sans-serif;
  --font-mono: ui-monospace, "SF Mono", Menlo, Consolas, monospace;
}

/* Dark mode — re-assign tokens only. Everything else follows. */
@media (prefers-color-scheme: dark) {
  :root {
    --text:       #e2e8f0;
    --text-dim:   #94a3b8;
    --text-mute:  #64748b;
    --surface:    #1e293b;
    --surface-2:  #0f172a;
    --bg:         #0b1220;
    --border:     #334155;
    --border-strong: #475569;
    --brand-soft: rgba(96, 165, 250, 0.14);
    --danger-bg:  rgba(220, 38, 38, 0.12);
    --danger-border: rgba(220, 38, 38, 0.45);
    --success-bg: rgba(22, 163, 74, 0.12);
    --success-border: rgba(22, 163, 74, 0.45);
    --shadow-sm:  0 1px 2px rgba(0, 0, 0, 0.30), 0 0 0 1px rgba(255, 255, 255, 0.04);
    --shadow:     0 2px 4px rgba(0, 0, 0, 0.30), 0 0 0 1px rgba(255, 255, 255, 0.04);
  }
}

/* ── Reset + base ───────────────────────────────────────── */
*, *::before, *::after { box-sizing: border-box; }
html, body { margin: 0; padding: 0; }
body {
  font-family: var(--font);
  color: var(--text);
  background: var(--bg);
  line-height: var(--lh-normal);
  font-size: var(--text-base);
  -webkit-font-smoothing: antialiased;
  text-rendering: optimizeLegibility;
}
h1, h2, h3 { line-height: var(--lh-tight); margin: 0; letter-spacing: -0.015em; }
h1 { font-size: var(--text-2xl); font-weight: 600; }
h2 { font-size: var(--text-xl); font-weight: 600; }
h3 { font-size: var(--text-lg); font-weight: 600; }
p  { margin: 0 0 var(--s-3); }
a  { color: var(--brand); text-decoration: none; }
a:hover { color: var(--brand-dark); text-decoration: underline; }
small { color: var(--text-mute); font-size: var(--text-sm); }

/* Keyboard-only focus ring everywhere */
:where(a, button, [role="button"], select, input, textarea, summary):focus-visible {
  outline: none;
  box-shadow: var(--ring);
  border-color: var(--brand) !important;
}

/* ── Top bar ────────────────────────────────────────────── */
.topbar {
  background: var(--surface);
  border-bottom: 1px solid var(--border);
  padding: 0 var(--s-6);
  display: flex;
  align-items: center;
  height: 58px;
  gap: var(--s-5);
  position: sticky; top: 0; z-index: 10;
  backdrop-filter: saturate(180%) blur(8px);
}
.brand {
  display: inline-flex; align-items: center; gap: var(--s-2);
  font-weight: 700; font-size: var(--text-lg);
  color: var(--text); text-decoration: none;
  letter-spacing: -0.015em;
}
.brand:hover { text-decoration: none; }
.brand-mark {
  display: inline-flex; align-items: center; justify-content: center;
  width: 28px; height: 28px;
  background: var(--brand);
  color: #fff;
  border-radius: 7px;
  font-weight: 700; font-size: 15px;
}
nav.primary { display: flex; gap: var(--s-1); }
nav.primary a {
  padding: var(--s-1) var(--s-3);
  border-radius: var(--radius);
  color: var(--text-dim);
  font-weight: 500;
  font-size: var(--text-sm);
  text-decoration: none;
}
nav.primary a:hover { background: var(--bg); color: var(--text); text-decoration: none; }
nav.primary a.active { background: var(--brand-soft); color: var(--brand-dark); }
.userbox {
  margin-left: auto;
  display: flex; align-items: center; gap: var(--s-3);
  color: var(--text-dim); font-size: var(--text-sm);
}
.userbox .email { max-width: 180px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.role-pill {
  display: inline-flex; align-items: center;
  padding: 2px 10px;
  background: var(--brand-soft); color: var(--brand-dark);
  border-radius: var(--radius-pill);
  font-weight: 600; font-size: var(--text-xs);
  text-transform: capitalize;
  letter-spacing: 0.015em;
}
.logout-form {
  margin: 0;
}
.logout-form button {
  display: inline-flex; align-items: center; gap: var(--s-1);
  font: inherit; font-size: var(--text-sm);
  padding: var(--s-1) var(--s-2);
  background: transparent;
  border: 1px solid transparent;
  color: var(--text-mute);
  border-radius: var(--radius);
  cursor: pointer;
}
.logout-form button:hover { color: var(--danger); border-color: var(--danger-border); background: var(--danger-bg); }

/* ── Main + footer ──────────────────────────────────────── */
main { max-width: 1120px; margin: 0 auto; padding: var(--s-6) var(--s-6) var(--s-8); }
footer.site {
  max-width: 1120px; margin: 0 auto; padding: var(--s-5) var(--s-6) var(--s-6);
  display: flex; justify-content: space-between; align-items: center;
  color: var(--text-mute); font-size: var(--text-xs);
  border-top: 1px solid var(--border);
}

/* ── Page head ──────────────────────────────────────────── */
.page-head { display: flex; align-items: center; gap: var(--s-3); margin-bottom: var(--s-5); flex-wrap: wrap; }
.page-head h1 { font-size: var(--text-xl); }
.page-head .count { color: var(--text-mute); font-size: var(--text-sm); font-variant-numeric: tabular-nums; }
.page-head .spacer { flex: 1; }

/* ── Card ───────────────────────────────────────────────── */
.card {
  background: var(--surface);
  border: 1px solid var(--border);
  border-radius: var(--radius-lg);
  box-shadow: var(--shadow-sm);
  overflow: hidden;
}

/* ── Banners ────────────────────────────────────────────── */
.banner {
  display: flex; align-items: flex-start; gap: var(--s-2);
  padding: var(--s-2) var(--s-3);
  border-radius: var(--radius);
  border: 1px solid var(--danger-border);
  background: var(--danger-bg);
  color: var(--danger);
  margin-bottom: var(--s-4);
  font-size: var(--text-sm);
}
.banner-ok { border-color: var(--success-border); background: var(--success-bg); color: var(--success); }
.banner svg { flex-shrink: 0; margin-top: 2px; }

/* ── Buttons ────────────────────────────────────────────── */
.btn {
  display: inline-flex; align-items: center; gap: var(--s-1);
  font: inherit; font-weight: 500; font-size: var(--text-sm);
  padding: 6px 14px;
  border: 1px solid var(--border-strong);
  background: var(--surface);
  color: var(--text);
  border-radius: var(--radius);
  cursor: pointer;
  text-decoration: none;
  line-height: 1.2;
  transition: background-color 0.08s ease-out, border-color 0.08s ease-out, color 0.08s ease-out;
}
.btn:hover { background: var(--surface-2); color: var(--text); text-decoration: none; }
.btn:disabled, .btn[aria-disabled="true"] {
  opacity: 0.55; cursor: not-allowed; pointer-events: none;
}
.btn-primary { background: var(--brand); border-color: var(--brand); color: #fff; }
.btn-primary:hover { background: var(--brand-dark); border-color: var(--brand-dark); color: #fff; }
.btn-danger  { border-color: var(--danger-border); color: var(--danger); background: var(--surface); }
.btn-danger:hover { background: var(--danger-bg); color: var(--danger); }
.btn-ghost { border-color: transparent; }
.btn-sm { padding: 3px 10px; font-size: var(--text-xs); }
.btn-icon { padding: 5px 8px; }

/* ── Filter bar ─────────────────────────────────────────── */
.filter-bar {
  display: flex; align-items: center; gap: var(--s-2); flex-wrap: wrap;
  padding: var(--s-3);
  border-bottom: 1px solid var(--border);
  background: var(--surface-2);
}
.filter-bar .search {
  position: relative; flex: 1 1 220px; max-width: 360px;
}
.filter-bar .search svg {
  position: absolute; left: 10px; top: 50%; transform: translateY(-50%);
  color: var(--text-mute); pointer-events: none;
}
.filter-bar .search input {
  width: 100%; padding: 6px 10px 6px 32px;
  border: 1px solid var(--border-strong);
  border-radius: var(--radius);
  background: var(--surface);
  color: var(--text);
  font: inherit; font-size: var(--text-sm);
}
.filter-bar select {
  padding: 6px 10px;
  border: 1px solid var(--border-strong);
  border-radius: var(--radius);
  background: var(--surface);
  color: var(--text);
  font: inherit; font-size: var(--text-sm);
}
.filter-chips { display: flex; gap: var(--s-1); flex-wrap: wrap; }
.filter-chip {
  display: inline-flex; align-items: center; gap: var(--s-1);
  padding: 2px 8px;
  background: var(--brand-soft); color: var(--brand-dark);
  border-radius: var(--radius-pill);
  font-size: var(--text-xs); font-weight: 500;
}
.filter-chip a { color: inherit; padding: 0 2px; font-weight: 700; }
.filter-chip a:hover { text-decoration: none; }

/* ── Table ──────────────────────────────────────────────── */
table.grid { border-collapse: collapse; width: 100%; font-size: var(--text-sm); }
table.grid th, table.grid td { padding: var(--s-3) var(--s-3); text-align: left; vertical-align: middle; border-bottom: 1px solid var(--border); }
table.grid thead th {
  background: var(--surface-2);
  font-weight: 600; color: var(--text-dim);
  font-size: var(--text-xs);
  text-transform: uppercase;
  letter-spacing: 0.05em;
  border-bottom: 1px solid var(--border-strong);
  white-space: nowrap;
}
table.grid thead th a {
  color: inherit; text-decoration: none;
  display: inline-flex; align-items: center; gap: 2px;
}
table.grid thead th a:hover { color: var(--text); text-decoration: none; }
table.grid tbody tr:hover { background: var(--surface-2); }
table.grid tbody tr:last-child td { border-bottom: none; }
td.empty { padding: var(--s-7); text-align: center; color: var(--text-mute); }
td.id-col { color: var(--text-mute); font-family: var(--font-mono); font-size: var(--text-xs); }
td.date-col { font-variant-numeric: tabular-nums; white-space: nowrap; }
td.date-col .relative { color: var(--text-mute); font-size: var(--text-xs); display: block; }
.today-flag {
  display: inline-block; padding: 1px 6px; margin-right: var(--s-1);
  background: var(--warning); color: #fff;
  border-radius: var(--radius-pill);
  font-size: var(--text-xs); font-weight: 600;
}

/* ── Status pills ───────────────────────────────────────── */
.pill {
  display: inline-flex; align-items: center; gap: var(--s-1);
  padding: 2px 10px; border-radius: var(--radius-pill);
  font-size: var(--text-xs); font-weight: 500;
  border: 1px solid transparent;
}
.pill::before { content: ""; width: 7px; height: 7px; border-radius: var(--radius-pill); }
.pill-scheduled   { background: #eef2ff; color: #3730a3; border-color: #c7d2fe; }
.pill-scheduled::before   { background: #6366f1; }
.pill-confirmed   { background: #ecfeff; color: #155e75; border-color: #a5f3fc; }
.pill-confirmed::before   { background: #06b6d4; }
.pill-in_progress { background: #fef3c7; color: #92400e; border-color: #fde68a; }
.pill-in_progress::before { background: #f59e0b; }
.pill-completed   { background: #dcfce7; color: #166534; border-color: #86efac; }
.pill-completed::before   { background: #22c55e; }
.pill-cancelled   { background: #f3f4f6; color: #475569; border-color: #cbd5e1; }
.pill-cancelled::before   { background: #94a3b8; }

/* ── Empty state ────────────────────────────────────────── */
.empty-state { padding: var(--s-8) var(--s-6); text-align: center; color: var(--text-mute); }
.empty-state .glyph {
  display: inline-flex; align-items: center; justify-content: center;
  width: 44px; height: 44px;
  border-radius: var(--radius-pill);
  background: var(--bg); color: var(--text-mute);
  margin-bottom: var(--s-3);
}
.empty-state h3 { color: var(--text-dim); margin: 0 0 var(--s-1); font-size: var(--text-base); }
.empty-state p  { margin: 0 0 var(--s-4); }

/* ── Forms ──────────────────────────────────────────────── */
form.form { display: flex; flex-direction: column; gap: var(--s-5); padding: var(--s-6); }
fieldset.group {
  border: none; margin: 0; padding: 0;
  display: grid;
  grid-template-columns: 180px minmax(0, 1fr);
  column-gap: var(--s-5);
  row-gap: var(--s-3);
  align-items: start;
}
fieldset.group + fieldset.group { border-top: 1px solid var(--border); padding-top: var(--s-5); }
fieldset.group > legend {
  grid-column: 1 / -1;
  font-size: var(--text-xs);
  font-weight: 600;
  color: var(--text-dim);
  text-transform: uppercase;
  letter-spacing: 0.06em;
  padding: 0; margin-bottom: var(--s-1);
}
label.field {
  font-weight: 500; color: var(--text-dim);
  font-size: var(--text-sm);
  padding-top: 9px;
}
label.field .req { color: var(--danger); margin-left: 2px; }
label.field .opt { color: var(--text-mute); font-weight: 400; font-size: var(--text-xs); margin-left: 6px; text-transform: lowercase; }

.input {
  font: inherit; font-size: var(--text-sm);
  padding: 8px 10px;
  border: 1px solid var(--border-strong);
  border-radius: var(--radius);
  background: var(--surface);
  color: var(--text);
  width: 100%;
  max-width: 420px;
  transition: border-color 0.08s, box-shadow 0.08s;
}
.input:hover { border-color: var(--text-mute); }
.input::placeholder { color: var(--text-mute); }
.input[type="number"] { max-width: 200px; font-variant-numeric: tabular-nums; }
.input[type="datetime-local"] { max-width: 280px; font-variant-numeric: tabular-nums; }
textarea.input { resize: vertical; min-height: 4.8em; font-family: inherit; }
.input.invalid { border-color: var(--danger); }
.input.invalid:focus-visible { box-shadow: 0 0 0 3px rgba(220, 38, 38, 0.22); }

.field-hint { color: var(--text-mute); font-size: var(--text-xs); margin-top: calc(-1 * var(--s-1)); }
.field-error {
  color: var(--danger); font-size: var(--text-xs); margin-top: calc(-1 * var(--s-1));
  display: flex; align-items: center; gap: var(--s-1);
}
.field-error:empty { display: none; }

.char-counter { color: var(--text-mute); font-size: var(--text-xs); font-variant-numeric: tabular-nums; margin-top: calc(-1 * var(--s-1)); }
.char-counter.over { color: var(--danger); font-weight: 500; }

.duration-combo { display: flex; flex-wrap: wrap; gap: var(--s-2); max-width: 420px; }
.duration-combo select { flex: 1 1 180px; max-width: 220px; }
.duration-combo input  { flex: 0 1 120px; max-width: 140px; }

.summary-box {
  grid-column: 1 / -1;
  padding: var(--s-3) var(--s-4);
  background: var(--surface-2);
  border: 1px solid var(--border);
  border-radius: var(--radius);
  font-size: var(--text-sm);
  color: var(--text-dim);
}
.summary-box strong { color: var(--text); }

.form-footer {
  display: flex; align-items: center; gap: var(--s-2);
  padding-top: var(--s-4);
  border-top: 1px solid var(--border);
  flex-wrap: wrap;
}
.form-footer .shortcut {
  margin-left: auto; color: var(--text-mute); font-size: var(--text-xs);
}
.form-footer kbd {
  font-family: var(--font-mono); font-size: var(--text-xs);
  padding: 1px 5px; margin: 0 2px;
  border: 1px solid var(--border-strong);
  border-bottom-width: 2px;
  border-radius: 4px;
  background: var(--surface);
  color: var(--text-dim);
}

/* ── Responsive — mobile ────────────────────────────────── */
@media (max-width: 720px) {
  main   { padding: var(--s-4); }
  footer.site { padding: var(--s-3) var(--s-4); flex-direction: column; gap: var(--s-1); align-items: flex-start; }
  .topbar { padding: 0 var(--s-3); gap: var(--s-2); }
  .topbar .brand span:not(.brand-mark) { display: none; }
  nav.primary { display: none; }
  .userbox .email { display: none; }
  fieldset.group { grid-template-columns: 1fr; row-gap: var(--s-1); }
  label.field { padding-top: 0; }
  .input { max-width: 100% !important; }
  table.grid th, table.grid td { padding: var(--s-2) var(--s-2); font-size: var(--text-xs); }
  .filter-bar .search { max-width: 100%; }
}
"#;

// ═══════════════════════════════════════════════════════════════
// Inline SVG icon kit
// ═══════════════════════════════════════════════════════════════

macro_rules! icon {
    ($body:expr) => {
        concat!(
            r#"<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">"#,
            $body,
            r#"</svg>"#
        )
    };
}

pub const ICON_PLUS: &str = icon!(r#"<path d="M12 5v14M5 12h14"/>"#);
pub const ICON_SEARCH: &str = icon!(r#"<circle cx="11" cy="11" r="8"/><path d="m21 21-4.3-4.3"/>"#);
pub const ICON_LOGOUT: &str = icon!(r#"<path d="M15 3h4a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2h-4"/><polyline points="10 17 15 12 10 7"/><line x1="15" y1="12" x2="3" y2="12"/>"#);
pub const ICON_CALENDAR: &str = icon!(r#"<rect x="3" y="4" width="18" height="18" rx="2" ry="2"/><line x1="16" y1="2" x2="16" y2="6"/><line x1="8" y1="2" x2="8" y2="6"/><line x1="3" y1="10" x2="21" y2="10"/>"#);
pub const ICON_ALERT: &str = icon!(r#"<circle cx="12" cy="12" r="10"/><line x1="12" y1="8" x2="12" y2="12"/><line x1="12" y1="16" x2="12.01" y2="16"/>"#);
pub const ICON_INBOX: &str = icon!(r#"<polyline points="22 12 16 12 14 15 10 15 8 12 2 12"/><path d="M5.45 5.11 2 12v6a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2v-6l-3.45-6.89A2 2 0 0 0 16.76 4H7.24a2 2 0 0 0-1.79 1.11z"/>"#);

// ═══════════════════════════════════════════════════════════════
// Shell
// ═══════════════════════════════════════════════════════════════

pub enum Nav {
    Appointments,
    Other,
}

impl Nav {
    fn appointments_class(&self) -> &'static str {
        match self {
            Nav::Appointments => "active",
            Nav::Other => "",
        }
    }
}

/// The signed-in actor's snapshot used by the shell. `bearer` is
/// exposed to page JS as a `<meta>` tag; `csrf` is baked into the
/// logout form so the admin's CSRF verifier accepts it.
pub struct Actor<'a> {
    pub user: &'a User,
    pub bearer: &'a str,
    pub csrf: &'a str,
}

/// Render the full page: `<head>` + top bar + caller-supplied
/// `<main>` inner + footer + caller-supplied page JS.
pub fn render_shell(
    title: &str,
    actor: &Actor<'_>,
    active: Nav,
    inner: &str,
    page_js: &str,
) -> String {
    let role_label = humanise_role(&actor.user.role);
    format!(
        r##"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>{title} · MedFlow</title>
  <meta name="api-token" content="{token}">
  <meta name="color-scheme" content="light dark">
  <style>{styles}</style>
</head>
<body>
  <header class="topbar">
    <a class="brand" href="/ops/appointments">
      <span class="brand-mark">M</span>
      <span>MedFlow</span>
    </a>
    <nav class="primary">
      <a class="{nav_appts}" href="/ops/appointments">Appointments</a>
    </nav>
    <div class="userbox">
      <span class="email" title="{email}">{email}</span>
      <span class="role-pill">{role}</span>
      <form class="logout-form" method="post" action="/admin/logout">
        <input type="hidden" name="_csrf" value="{csrf}">
        <button type="submit" title="Sign out">{logout_icon}<span>Sign out</span></button>
      </form>
    </div>
  </header>
  <main>{inner}</main>
  <footer class="site">
    <span>MedFlow · operational console</span>
    <span>Framework admin at <a href="/admin">/admin</a></span>
  </footer>
  <script>{page_js}</script>
</body>
</html>"##,
        title = escape_html(title),
        token = escape_html(actor.bearer),
        styles = STYLES,
        nav_appts = active.appointments_class(),
        email = escape_html(&actor.user.email),
        role = escape_html(&role_label),
        csrf = escape_html(actor.csrf),
        logout_icon = ICON_LOGOUT,
        inner = inner,
        page_js = page_js,
    )
}

pub fn humanise_role(role: &str) -> String {
    match role {
        "receptionist" => "Receptionist",
        "doctor" => "Doctor",
        "billing" => "Billing",
        "admin" => "Admin",
        other => other,
    }
    .to_string()
}

pub fn humanise_status(s: &str) -> String {
    match s {
        "scheduled" => "Scheduled",
        "confirmed" => "Confirmed",
        "in_progress" => "In progress",
        "completed" => "Completed",
        "cancelled" => "Cancelled",
        other => other,
    }
    .to_string()
}

pub fn pluralise(n: usize, singular: &str) -> String {
    if n == 1 {
        format!("1 {singular}")
    } else {
        format!("{n} {singular}s")
    }
}

/// Format a timestamp with a scanning-friendly main line plus a
/// coloured relative marker: "Today", "Tomorrow", "Yesterday", or
/// the date for anything further. The main line is always ISO-ish
/// so humans and machines both parse it.
pub fn format_dt_smart(ts: DateTime<Utc>) -> (String, Option<String>) {
    let now = Utc::now();
    let today = now.date_naive();
    let ts_date = ts.date_naive();
    let days = (ts_date - today).num_days();

    let main = ts.format("%Y-%m-%d %H:%M").to_string();
    let relative = match days {
        0 => Some("Today".to_string()),
        1 => Some("Tomorrow".to_string()),
        -1 => Some("Yesterday".to_string()),
        d if d > 1 && d <= 7 => Some(format!("in {d} days")),
        d if d < -1 && d >= -7 => Some(format!("{} days ago", -d)),
        _ => None,
    };
    (main, relative)
}

// ═══════════════════════════════════════════════════════════════
// Small helpers
// ═══════════════════════════════════════════════════════════════

/// HTML-entity escape. Every value that reaches a page (names,
/// emails, the bearer token, any server-returned message) passes
/// through this.
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

/// 303 redirect — `rustio_core::http` has no public redirect helper.
pub fn redirect(to: &str) -> Response {
    hyper::Response::builder()
        .status(StatusCode::SEE_OTHER)
        .header("location", to)
        .body(Full::new(Bytes::new()))
        .expect("valid redirect")
}

/// 403 page rendered inside the shell so it feels like a refusal,
/// not a crash.
pub fn forbidden_page(actor: &Actor<'_>, message: &str) -> Response {
    let inner = format!(
        r#"<section class="card">
  <div class="empty-state">
    <span class="glyph">{icon}</span>
    <h3>Not allowed</h3>
    <p>{message}</p>
    <a class="btn" href="/ops/appointments">← Back to appointments</a>
  </div>
</section>"#,
        icon = ICON_ALERT,
        message = escape_html(message),
    );
    let body = render_shell("Forbidden", actor, Nav::Other, &inner, "");
    let mut resp = html(body);
    *resp.status_mut() = StatusCode::FORBIDDEN;
    resp
}
