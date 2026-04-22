//! UI for the `/ops` operational console. One stylesheet, one shell
//! (dark left sidebar + light content area), a small icon kit, and
//! shared formatting helpers. Every `/ops/*` page renders through
//! [`render_shell`] so the visual identity stays consistent.

#![allow(dead_code)]

use bytes::Bytes;
use chrono::{DateTime, Datelike, Utc};
use http_body_util::Full;
use hyper::StatusCode;
use rustio_core::auth::User;
use rustio_core::{html, Response};

// ═══════════════════════════════════════════════════════════════
// Stylesheet
// ═══════════════════════════════════════════════════════════════

pub const STYLES: &str = r#"
/* Tokens */
:root {
  --s-1: 4px; --s-2: 8px; --s-3: 12px; --s-4: 16px; --s-5: 20px; --s-6: 24px; --s-7: 32px; --s-8: 48px; --s-9: 64px;
  --r-sm: 6px; --r: 10px; --r-lg: 14px; --r-pill: 9999px;

  --font: "Inter", -apple-system, BlinkMacSystemFont, "Segoe UI", system-ui, sans-serif;
  --font-mono: "SF Mono", ui-monospace, Menlo, Consolas, monospace;
  --text-2xs: 0.72rem; --text-xs: 0.78rem; --text-sm: 0.86rem; --text-base: 0.94rem;
  --text-lg: 1.06rem; --text-xl: 1.25rem; --text-2xl: 1.55rem; --text-3xl: 2rem;

  --teal-50:  #f0fdfa;
  --teal-100: #ccfbf1;
  --teal-500: #14b8a6;
  --teal-600: #0d9488;
  --teal-700: #0f766e;

  --slate-50: #f8fafc;
  --slate-100: #f1f5f9;
  --slate-200: #e2e8f0;
  --slate-300: #cbd5e1;
  --slate-400: #94a3b8;
  --slate-500: #64748b;
  --slate-600: #475569;
  --slate-700: #334155;
  --slate-800: #1e293b;
  --slate-900: #0f172a;

  --primary:      var(--teal-600);
  --primary-dark: var(--teal-700);
  --primary-soft: var(--teal-50);

  --bg:       var(--slate-50);
  --surface:  #ffffff;
  --surface-2: var(--slate-100);
  --text:     var(--slate-900);
  --text-dim: var(--slate-600);
  --text-mute: var(--slate-400);
  --border:   var(--slate-200);
  --border-strong: var(--slate-300);

  --danger:   #dc2626;
  --danger-bg: #fef2f2;
  --danger-border: #fecaca;
  --warning:  #d97706;
  --warning-bg: #fffbeb;
  --success:  #059669;
  --success-bg: #ecfdf5;

  --sidebar-bg:   var(--slate-900);
  --sidebar-item: var(--slate-400);
  --sidebar-hover-bg: var(--slate-800);
  --sidebar-hover:    #ffffff;
  --sidebar-active-bg: rgba(13, 148, 136, 0.18);
  --sidebar-active:   #5eead4;
  --sidebar-border:   var(--slate-800);

  --shadow-xs: 0 1px 2px rgba(15, 23, 42, 0.04);
  --shadow:    0 2px 4px rgba(15, 23, 42, 0.04), 0 0 0 1px rgba(15, 23, 42, 0.04);
  --shadow-lg: 0 12px 24px -8px rgba(15, 23, 42, 0.12), 0 0 0 1px rgba(15, 23, 42, 0.04);

  --ring: 0 0 0 3px rgba(13, 148, 136, 0.25);
}

/* Reset + base */
*, *::before, *::after { box-sizing: border-box; }
html, body { margin: 0; padding: 0; height: 100%; }
body {
  font-family: var(--font);
  color: var(--text);
  background: var(--bg);
  line-height: 1.5;
  font-size: var(--text-base);
  -webkit-font-smoothing: antialiased;
  font-feature-settings: "cv11", "ss03";
}
h1, h2, h3 { margin: 0; letter-spacing: -0.02em; line-height: 1.2; }
h1 { font-size: var(--text-2xl); font-weight: 700; }
h2 { font-size: var(--text-xl); font-weight: 600; }
h3 { font-size: var(--text-base); font-weight: 600; }
a { color: var(--primary); text-decoration: none; }
a:hover { color: var(--primary-dark); text-decoration: underline; }
small { color: var(--text-mute); font-size: var(--text-sm); }

:where(a, button, select, input, textarea, summary):focus-visible {
  outline: none;
  box-shadow: var(--ring);
  border-color: var(--primary) !important;
}

/* ───────── Layout: sidebar + content ───────── */
.layout { display: flex; min-height: 100vh; }
.sidebar {
  flex: 0 0 240px;
  background: var(--sidebar-bg);
  color: var(--sidebar-item);
  display: flex; flex-direction: column;
  padding: var(--s-6) var(--s-4);
  border-right: 1px solid var(--sidebar-border);
  position: sticky; top: 0; height: 100vh;
}
.sidebar .brand {
  display: flex; align-items: center; gap: var(--s-3);
  padding: 0 var(--s-3) var(--s-6);
  color: #ffffff; text-decoration: none; font-weight: 700;
  font-size: var(--text-lg); letter-spacing: -0.02em;
}
.sidebar .brand:hover { text-decoration: none; color: #fff; }
.sidebar .brand-mark {
  width: 32px; height: 32px;
  background: linear-gradient(135deg, var(--teal-500), var(--teal-700));
  color: #fff;
  border-radius: 8px;
  display: grid; place-items: center;
  font-size: 15px; font-weight: 700;
  box-shadow: 0 0 0 1px rgba(255, 255, 255, 0.08), 0 4px 8px rgba(13, 148, 136, 0.35);
}
.sidebar .brand small { display: block; color: var(--slate-400); font-size: var(--text-2xs); font-weight: 500; text-transform: uppercase; letter-spacing: 0.08em; margin-top: 2px; }

.sidebar-group-label {
  padding: 0 var(--s-3); margin: var(--s-4) 0 var(--s-2);
  font-size: var(--text-2xs); text-transform: uppercase;
  letter-spacing: 0.1em; color: var(--slate-500); font-weight: 600;
}
.sidebar nav { display: flex; flex-direction: column; gap: 2px; }
.sidebar nav a {
  display: flex; align-items: center; gap: var(--s-3);
  padding: 8px var(--s-3);
  color: var(--sidebar-item);
  font-size: var(--text-sm); font-weight: 500;
  border-radius: var(--r-sm);
}
.sidebar nav a:hover { background: var(--sidebar-hover-bg); color: var(--sidebar-hover); text-decoration: none; }
.sidebar nav a.active {
  background: var(--sidebar-active-bg);
  color: var(--sidebar-active);
  box-shadow: inset 2px 0 0 var(--sidebar-active);
}
.sidebar nav a.disabled { opacity: 0.45; cursor: not-allowed; pointer-events: none; }
.sidebar nav a svg { opacity: 0.9; }

.sidebar-footer {
  margin-top: auto;
  padding: var(--s-3);
  border-top: 1px solid var(--sidebar-border);
  display: flex; align-items: center; gap: var(--s-3);
  color: var(--slate-300);
}
.sidebar-footer .avatar-dot {
  width: 32px; height: 32px; border-radius: var(--r-pill);
  background: linear-gradient(135deg, var(--teal-500), var(--teal-700));
  color: #fff; display: grid; place-items: center;
  font-weight: 700; font-size: var(--text-sm);
  flex-shrink: 0;
}
.sidebar-footer .u { flex: 1 1 auto; min-width: 0; }
.sidebar-footer .u strong { color: #fff; font-size: var(--text-sm); display: block; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.sidebar-footer .u small { color: var(--slate-500); font-size: var(--text-2xs); text-transform: uppercase; letter-spacing: 0.06em; }
.sidebar-footer form { margin: 0; }
.sidebar-footer form button {
  background: transparent; border: 1px solid transparent; color: var(--slate-400);
  padding: 6px; border-radius: var(--r-sm); cursor: pointer; font: inherit;
}
.sidebar-footer form button:hover { background: var(--sidebar-hover-bg); color: #fff; border-color: var(--sidebar-border); }

.content {
  flex: 1 1 auto;
  min-width: 0;
  display: flex; flex-direction: column;
}
.page-hero {
  background: var(--surface);
  border-bottom: 1px solid var(--border);
  padding: var(--s-6) var(--s-7);
}
.page-hero .breadcrumb {
  font-size: var(--text-xs); color: var(--text-mute);
  display: flex; gap: var(--s-1); align-items: center;
  margin-bottom: var(--s-3);
}
.page-hero .breadcrumb a { color: var(--text-dim); }
.page-hero .row {
  display: flex; align-items: flex-end; gap: var(--s-4); flex-wrap: wrap;
}
.page-hero .title { flex: 1 1 auto; min-width: 0; }
.page-hero h1 { font-size: var(--text-3xl); margin-bottom: 2px; }
.page-hero .subtitle { color: var(--text-dim); font-size: var(--text-sm); }
.page-hero .actions { display: flex; align-items: center; gap: var(--s-2); }

main.page {
  flex: 1 1 auto;
  padding: var(--s-6) var(--s-7);
  width: 100%;
  max-width: 1280px;
}
footer.site {
  padding: var(--s-4) var(--s-7);
  color: var(--text-mute); font-size: var(--text-xs);
  border-top: 1px solid var(--border);
  display: flex; justify-content: space-between;
}

/* ───────── Stat cards strip ───────── */
.stats {
  display: grid;
  grid-template-columns: repeat(4, 1fr);
  gap: var(--s-4);
  margin-bottom: var(--s-6);
}
.stat {
  background: var(--surface);
  border: 1px solid var(--border);
  border-radius: var(--r-lg);
  padding: var(--s-4) var(--s-5);
  display: flex; flex-direction: column; gap: 2px;
  position: relative;
  overflow: hidden;
}
.stat::before {
  content: "";
  position: absolute; left: 0; top: 0; bottom: 0; width: 3px;
  background: var(--primary);
}
.stat.stat-warn::before { background: var(--warning); }
.stat.stat-ok::before   { background: var(--success); }
.stat.stat-mute::before { background: var(--slate-400); }
.stat .label { color: var(--text-mute); font-size: var(--text-xs); text-transform: uppercase; letter-spacing: 0.06em; font-weight: 600; }
.stat .value { font-size: var(--text-2xl); font-weight: 700; color: var(--text); font-variant-numeric: tabular-nums; letter-spacing: -0.02em; }
.stat .sub { font-size: var(--text-xs); color: var(--text-mute); }

/* ───────── Card + toolbar ───────── */
.card {
  background: var(--surface);
  border: 1px solid var(--border);
  border-radius: var(--r-lg);
  box-shadow: var(--shadow-xs);
  overflow: hidden;
}
.toolbar {
  padding: var(--s-3) var(--s-4);
  border-bottom: 1px solid var(--border);
  background: var(--surface);
  display: flex; align-items: center; gap: var(--s-3); flex-wrap: wrap;
}
.tabs { display: inline-flex; background: var(--surface-2); padding: 3px; border-radius: var(--r); }
.tabs a {
  padding: 5px 12px;
  font-size: var(--text-xs); font-weight: 500;
  color: var(--text-dim); border-radius: var(--r-sm);
}
.tabs a:hover { text-decoration: none; color: var(--text); }
.tabs a.active { background: var(--surface); color: var(--text); box-shadow: var(--shadow-xs); }
.tabs a .badge {
  display: inline-block; margin-left: 6px;
  background: var(--surface-2); color: var(--text-mute);
  padding: 0 6px; border-radius: var(--r-pill);
  font-size: var(--text-2xs); font-weight: 600;
  min-width: 18px; text-align: center;
}
.tabs a.active .badge { background: var(--primary-soft); color: var(--primary-dark); }

.toolbar .search {
  position: relative; flex: 0 1 260px; min-width: 160px;
}
.toolbar .search svg {
  position: absolute; left: 10px; top: 50%; transform: translateY(-50%);
  color: var(--text-mute); pointer-events: none;
}
.toolbar .search input {
  width: 100%; padding: 7px 10px 7px 32px;
  border: 1px solid var(--border-strong);
  border-radius: var(--r-sm);
  background: var(--surface);
  color: var(--text);
  font: inherit; font-size: var(--text-sm);
}
.toolbar select {
  padding: 7px 28px 7px 12px;
  border: 1px solid var(--border-strong);
  border-radius: var(--r-sm);
  background: var(--surface) url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='12' height='12' viewBox='0 0 24 24' fill='none' stroke='%2364748b' stroke-width='2' stroke-linecap='round' stroke-linejoin='round'%3E%3Cpolyline points='6 9 12 15 18 9'%3E%3C/polyline%3E%3C/svg%3E") no-repeat right 10px center;
  color: var(--text); font: inherit; font-size: var(--text-sm);
  appearance: none; -webkit-appearance: none;
}
.toolbar .spacer { flex: 1 1 auto; }

/* ───────── Buttons ───────── */
.btn {
  display: inline-flex; align-items: center; gap: 6px;
  font: inherit; font-weight: 500; font-size: var(--text-sm);
  padding: 7px 14px;
  border: 1px solid var(--border-strong);
  background: var(--surface);
  color: var(--text);
  border-radius: var(--r-sm);
  cursor: pointer;
  text-decoration: none;
  line-height: 1.2;
  transition: background-color 0.12s ease, border-color 0.12s ease, color 0.12s ease;
  white-space: nowrap;
}
.btn:hover { background: var(--surface-2); color: var(--text); text-decoration: none; }
.btn:disabled, .btn[aria-disabled="true"] { opacity: 0.55; cursor: not-allowed; pointer-events: none; }
.btn-primary { background: var(--primary); border-color: var(--primary); color: #fff; box-shadow: 0 1px 0 rgba(13, 148, 136, 0.3); }
.btn-primary:hover { background: var(--primary-dark); border-color: var(--primary-dark); color: #fff; }
.btn-danger  { border-color: var(--danger-border); color: var(--danger); background: var(--surface); }
.btn-danger:hover { background: var(--danger-bg); color: var(--danger); }
.btn-ghost { border-color: transparent; color: var(--text-dim); background: transparent; }
.btn-ghost:hover { background: var(--surface-2); color: var(--text); }
.btn-sm { padding: 3px 10px; font-size: var(--text-xs); }
.btn-lg { padding: 10px 18px; font-size: var(--text-base); }

/* ───────── Appointment rows ───────── */
table.rows { width: 100%; border-collapse: collapse; }
table.rows tr.day-row td {
  background: var(--surface-2);
  padding: var(--s-2) var(--s-4);
  font-size: var(--text-xs); font-weight: 600;
  color: var(--text-dim);
  text-transform: uppercase; letter-spacing: 0.06em;
  border-top: 1px solid var(--border);
  border-bottom: 1px solid var(--border);
}
table.rows tr.day-row .pill-today {
  margin-right: var(--s-2);
  background: var(--warning); color: #fff;
  padding: 2px 8px; border-radius: var(--r-pill);
  font-size: var(--text-2xs); font-weight: 700;
  letter-spacing: 0.04em;
}
table.rows tr.appt { transition: background 0.08s ease; }
table.rows tr.appt:hover { background: var(--surface-2); }
table.rows tr.appt td {
  padding: var(--s-3) var(--s-4);
  border-bottom: 1px solid var(--border);
  vertical-align: middle;
}
table.rows tr.appt td.time {
  width: 82px;
}
.time-chip {
  display: flex; flex-direction: column; align-items: center;
  background: var(--surface-2);
  border: 1px solid var(--border);
  border-radius: var(--r-sm);
  padding: 5px 8px; min-width: 64px;
}
.time-chip .hh { font-size: var(--text-lg); font-weight: 700; color: var(--text); font-variant-numeric: tabular-nums; line-height: 1; letter-spacing: -0.015em; }
.time-chip .md { font-size: var(--text-2xs); color: var(--text-mute); margin-top: 2px; text-transform: uppercase; letter-spacing: 0.05em; }
.person { display: flex; align-items: center; gap: 10px; }
.person .avatar {
  width: 32px; height: 32px;
  border-radius: var(--r-pill);
  color: #fff; font-weight: 600; font-size: var(--text-sm);
  display: grid; place-items: center;
  flex-shrink: 0; letter-spacing: 0.02em;
}
.person .meta { display: flex; flex-direction: column; min-width: 0; }
.person .meta .name { font-weight: 600; color: var(--text); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.person .meta small { color: var(--text-mute); font-size: var(--text-xs); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.dept-badge {
  display: inline-block;
  padding: 2px 8px; background: var(--surface-2); color: var(--text-dim);
  border: 1px solid var(--border); border-radius: var(--r-pill);
  font-size: var(--text-xs); font-weight: 500;
}

/* Status pills */
.pill {
  display: inline-flex; align-items: center; gap: 5px;
  padding: 3px 10px; border-radius: var(--r-pill);
  font-size: var(--text-xs); font-weight: 600;
  border: 1px solid transparent;
  letter-spacing: 0.01em;
}
.pill::before { content: ""; width: 6px; height: 6px; border-radius: var(--r-pill); }
.pill-scheduled   { background: #eef2ff; color: #3730a3; border-color: #c7d2fe; }
.pill-scheduled::before   { background: #6366f1; }
.pill-confirmed   { background: #ecfeff; color: #155e75; border-color: #a5f3fc; }
.pill-confirmed::before   { background: #06b6d4; }
.pill-in_progress { background: #fef3c7; color: #92400e; border-color: #fde68a; }
.pill-in_progress::before { background: #f59e0b; }
.pill-completed   { background: #dcfce7; color: #166534; border-color: #86efac; }
.pill-completed::before   { background: #22c55e; }
.pill-cancelled   { background: #f1f5f9; color: #475569; border-color: #cbd5e1; }
.pill-cancelled::before   { background: #94a3b8; }

.action-group { display: flex; gap: 6px; justify-content: flex-end; }
.action-group .btn { white-space: nowrap; }

/* ───────── Empty states ───────── */
.empty-state { padding: var(--s-8) var(--s-6); text-align: center; color: var(--text-mute); }
.empty-state .glyph {
  display: inline-grid; place-items: center;
  width: 56px; height: 56px; border-radius: var(--r-pill);
  background: var(--primary-soft); color: var(--primary);
  margin-bottom: var(--s-3);
}
.empty-state h3 { color: var(--text-dim); margin: 0 0 var(--s-1); font-size: var(--text-lg); }
.empty-state p  { margin: 0 0 var(--s-4); }

/* ───────── Banners ───────── */
.banner {
  display: flex; align-items: flex-start; gap: var(--s-2);
  padding: var(--s-3) var(--s-4);
  border-radius: var(--r);
  border: 1px solid var(--danger-border);
  background: var(--danger-bg);
  color: var(--danger);
  margin-bottom: var(--s-4);
  font-size: var(--text-sm);
}
.banner svg { flex-shrink: 0; margin-top: 1px; }
.banner-ok { border-color: #bbf7d0; background: var(--success-bg); color: var(--success); }
.banner-warn { border-color: #fde68a; background: var(--warning-bg); color: #92400e; }

/* ───────── Filter chips ───────── */
.filter-chips { display: flex; gap: var(--s-1); flex-wrap: wrap; }
.filter-chip {
  display: inline-flex; align-items: center; gap: 6px;
  padding: 2px 8px 2px 10px;
  background: var(--primary-soft); color: var(--primary-dark);
  border-radius: var(--r-pill);
  font-size: var(--text-xs); font-weight: 500;
}
.filter-chip a { color: inherit; padding: 0 2px; font-weight: 700; }
.filter-chip a:hover { text-decoration: none; }

/* ───────── Form layout (2-col with live preview) ───────── */
.form-layout {
  display: grid;
  grid-template-columns: minmax(0, 1fr) 340px;
  gap: var(--s-6);
  align-items: start;
}
form.form { display: flex; flex-direction: column; gap: var(--s-5); padding: var(--s-6); }
fieldset.group {
  border: none; margin: 0; padding: 0;
  display: grid;
  grid-template-columns: 160px minmax(0, 1fr);
  column-gap: var(--s-5);
  row-gap: var(--s-3);
  align-items: start;
}
fieldset.group + fieldset.group { border-top: 1px solid var(--border); padding-top: var(--s-5); }
fieldset.group > legend {
  grid-column: 1 / -1;
  font-size: var(--text-xs);
  font-weight: 700;
  color: var(--primary-dark);
  text-transform: uppercase;
  letter-spacing: 0.08em;
  margin-bottom: var(--s-1);
  padding: 0;
}
label.field { font-weight: 500; color: var(--text-dim); font-size: var(--text-sm); padding-top: 9px; }
label.field .req { color: var(--danger); margin-left: 2px; }
label.field .opt { color: var(--text-mute); font-weight: 400; font-size: var(--text-xs); margin-left: 6px; }

.input {
  font: inherit; font-size: var(--text-sm);
  padding: 8px 12px;
  border: 1px solid var(--border-strong);
  border-radius: var(--r-sm);
  background: var(--surface);
  color: var(--text);
  width: 100%;
  max-width: 440px;
  transition: border-color 0.08s, box-shadow 0.08s;
}
.input:hover { border-color: var(--text-mute); }
.input::placeholder { color: var(--text-mute); }
.input[type="number"] { max-width: 180px; font-variant-numeric: tabular-nums; }
.input[type="datetime-local"] { max-width: 240px; font-variant-numeric: tabular-nums; }
textarea.input { resize: vertical; min-height: 5em; font-family: inherit; }
.input.invalid { border-color: var(--danger); }
.input.invalid:focus-visible { box-shadow: 0 0 0 3px rgba(220, 38, 38, 0.22); }

.field-hint { color: var(--text-mute); font-size: var(--text-xs); margin-top: -6px; }
.field-error { color: var(--danger); font-size: var(--text-xs); margin-top: -6px; }
.field-error:empty { display: none; }
.char-counter { color: var(--text-mute); font-size: var(--text-2xs); font-variant-numeric: tabular-nums; text-align: right; max-width: 440px; }
.char-counter.over { color: var(--danger); font-weight: 600; }

.duration-combo { display: flex; flex-wrap: wrap; gap: var(--s-2); max-width: 440px; }
.duration-combo select { flex: 1 1 200px; max-width: 240px; }
.duration-combo input  { flex: 0 1 120px; max-width: 140px; }

/* Form preview sidebar */
.preview-card {
  position: sticky; top: var(--s-4);
  background: var(--surface);
  border: 1px solid var(--border);
  border-radius: var(--r-lg);
  padding: var(--s-5);
  box-shadow: var(--shadow-xs);
}
.preview-card h3 {
  font-size: var(--text-2xs); font-weight: 700;
  color: var(--text-mute); text-transform: uppercase; letter-spacing: 0.08em;
  margin-bottom: var(--s-3);
}
.preview-card .row { display: flex; gap: var(--s-3); margin-bottom: var(--s-3); }
.preview-card .row .key { flex: 0 0 72px; font-size: var(--text-xs); color: var(--text-mute); font-weight: 500; text-transform: uppercase; letter-spacing: 0.04em; padding-top: 1px; }
.preview-card .row .val { flex: 1 1 auto; font-size: var(--text-sm); color: var(--text); }
.preview-card .row .val .soft { color: var(--text-mute); font-style: italic; }
.preview-card .big-time {
  background: linear-gradient(135deg, var(--primary), var(--primary-dark));
  color: #fff;
  border-radius: var(--r);
  padding: var(--s-4);
  text-align: center; margin-bottom: var(--s-4);
}
.preview-card .big-time .hh { font-size: var(--text-2xl); font-weight: 700; font-variant-numeric: tabular-nums; display: block; letter-spacing: -0.02em; }
.preview-card .big-time .md { font-size: var(--text-xs); opacity: 0.9; letter-spacing: 0.04em; text-transform: uppercase; margin-top: 2px; display: block; }
.preview-card .big-time.empty { background: var(--surface-2); color: var(--text-mute); border: 1px dashed var(--border-strong); }
.preview-card .big-time.empty .hh { color: var(--text-mute); font-size: var(--text-lg); font-weight: 500; }
.preview-card .cta { margin-top: var(--s-4); display: flex; flex-direction: column; gap: var(--s-2); }
.preview-card .shortcut { color: var(--text-mute); font-size: var(--text-2xs); text-align: center; }
.preview-card kbd {
  font-family: var(--font-mono); font-size: var(--text-2xs);
  padding: 1px 4px; border: 1px solid var(--border-strong); border-bottom-width: 2px;
  border-radius: 3px; background: var(--surface); color: var(--text-dim);
}

/* ───────── Detail page ───────── */
.detail-grid { display: grid; grid-template-columns: minmax(0, 1fr) 320px; gap: var(--s-6); align-items: start; }
.info-grid { display: grid; grid-template-columns: repeat(2, 1fr); gap: var(--s-4) var(--s-6); padding: var(--s-5); }
.info-grid .item { display: flex; flex-direction: column; gap: 2px; }
.info-grid .item .k { font-size: var(--text-xs); color: var(--text-mute); text-transform: uppercase; letter-spacing: 0.06em; font-weight: 600; }
.info-grid .item .v { font-size: var(--text-base); color: var(--text); font-weight: 500; }
.info-grid .item.wide { grid-column: 1 / -1; }

.timeline { padding: var(--s-5); }
.timeline-event {
  position: relative; padding-left: var(--s-6); padding-bottom: var(--s-4);
  border-left: 2px solid var(--border); margin-left: 6px;
}
.timeline-event:last-child { border-left-color: transparent; padding-bottom: 0; }
.timeline-event::before {
  content: ""; position: absolute;
  left: -7px; top: 4px;
  width: 12px; height: 12px;
  background: var(--primary); border: 2px solid var(--surface);
  border-radius: var(--r-pill);
  box-shadow: 0 0 0 2px var(--border);
}
.timeline-event.cancelled::before { background: var(--slate-400); }
.timeline-event .what { font-size: var(--text-sm); color: var(--text); font-weight: 500; }
.timeline-event .when { font-size: var(--text-xs); color: var(--text-mute); margin-top: 2px; }
.timeline-event .what .arrow { color: var(--text-mute); margin: 0 4px; }

.section-head {
  display: flex; align-items: center; gap: var(--s-3);
  padding: var(--s-3) var(--s-5);
  border-bottom: 1px solid var(--border);
  background: var(--surface-2);
}
.section-head h3 { color: var(--text-dim); font-size: var(--text-xs); text-transform: uppercase; letter-spacing: 0.08em; font-weight: 700; }
.section-head .count-pill { background: var(--surface); color: var(--text-mute); font-size: var(--text-2xs); padding: 1px 7px; border-radius: var(--r-pill); border: 1px solid var(--border); font-weight: 600; }

/* ───────── Responsive ───────── */
@media (max-width: 960px) {
  .stats { grid-template-columns: repeat(2, 1fr); }
  .detail-grid, .form-layout { grid-template-columns: 1fr; }
  .preview-card { position: static; }
}
@media (max-width: 720px) {
  .layout { flex-direction: column; }
  .sidebar {
    flex: 0 0 auto; width: 100%; height: auto; position: sticky; top: 0;
    flex-direction: row; align-items: center; gap: var(--s-3);
    padding: var(--s-3) var(--s-4);
  }
  .sidebar .brand { padding: 0; }
  .sidebar-group-label, .sidebar nav, .sidebar-footer { display: none; }
  .sidebar .mobile-user {
    margin-left: auto; display: flex; align-items: center; gap: var(--s-2);
    color: var(--slate-300); font-size: var(--text-xs);
  }
  .sidebar .mobile-user .role-pill {
    background: var(--sidebar-active-bg); color: var(--sidebar-active);
    padding: 2px 8px; border-radius: var(--r-pill); font-size: var(--text-2xs); font-weight: 600;
  }
  .page-hero, main.page, footer.site { padding-left: var(--s-4); padding-right: var(--s-4); }
  .page-hero h1 { font-size: var(--text-2xl); }
  fieldset.group { grid-template-columns: 1fr; row-gap: var(--s-1); }
  label.field { padding-top: 0; }
  .input { max-width: 100% !important; }
  table.rows tr.appt td.actions-cell { display: none; }
  .info-grid { grid-template-columns: 1fr; padding: var(--s-4); }
}
"#;

// ═══════════════════════════════════════════════════════════════
// Icon kit (inline SVG)
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
pub const ICON_LOGOUT: &str = icon!(r#"<path d="M9 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h4"/><polyline points="16 17 21 12 16 7"/><line x1="21" y1="12" x2="9" y2="12"/>"#);
pub const ICON_CALENDAR: &str = icon!(r#"<rect x="3" y="4" width="18" height="18" rx="2" ry="2"/><line x1="16" y1="2" x2="16" y2="6"/><line x1="8" y1="2" x2="8" y2="6"/><line x1="3" y1="10" x2="21" y2="10"/>"#);
pub const ICON_ALERT: &str = icon!(r#"<circle cx="12" cy="12" r="10"/><line x1="12" y1="8" x2="12" y2="12"/><line x1="12" y1="16" x2="12.01" y2="16"/>"#);
pub const ICON_INBOX: &str = icon!(r#"<polyline points="22 12 16 12 14 15 10 15 8 12 2 12"/><path d="M5.45 5.11 2 12v6a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2v-6l-3.45-6.89A2 2 0 0 0 16.76 4H7.24a2 2 0 0 0-1.79 1.11z"/>"#);
pub const ICON_HOME: &str = icon!(r#"<path d="m3 9 9-7 9 7v11a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/><polyline points="9 22 9 12 15 12 15 22"/>"#);
pub const ICON_USERS: &str = icon!(r#"<path d="M17 21v-2a4 4 0 0 0-4-4H5a4 4 0 0 0-4 4v2"/><circle cx="9" cy="7" r="4"/><path d="M23 21v-2a4 4 0 0 0-3-3.87"/><path d="M16 3.13a4 4 0 0 1 0 7.75"/>"#);
pub const ICON_STETH: &str = icon!(r#"<path d="M4.8 2.3A.3.3 0 1 0 5 2H4a2 2 0 0 0-2 2v5a6 6 0 0 0 6 6v0a6 6 0 0 0 6-6V4a2 2 0 0 0-2-2h-1a.2.2 0 1 0 .3.3"/><path d="M8 15v1a6 6 0 0 0 6 6v0a6 6 0 0 0 6-6v-4"/><circle cx="20" cy="10" r="2"/>"#);
pub const ICON_CHART: &str = icon!(r#"<line x1="18" y1="20" x2="18" y2="10"/><line x1="12" y1="20" x2="12" y2="4"/><line x1="6" y1="20" x2="6" y2="14"/>"#);
pub const ICON_SETTINGS: &str = icon!(r#"<circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1 0 2.83 2 2 0 0 1-2.83 0l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-2 2 2 2 0 0 1-2-2v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83 0 2 2 0 0 1 0-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1-2-2 2 2 0 0 1 2-2h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 0-2.83 2 2 0 0 1 2.83 0l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 2-2 2 2 0 0 1 2 2v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 0 2 2 0 0 1 0 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 2 2 2 2 0 0 1-2 2h-.09a1.65 1.65 0 0 0-1.51 1z"/>"#);
pub const ICON_CLOCK: &str = icon!(r#"<circle cx="12" cy="12" r="10"/><polyline points="12 6 12 12 16 14"/>"#);
pub const ICON_CHEVRON_RIGHT: &str = icon!(r#"<polyline points="9 18 15 12 9 6"/>"#);
pub const ICON_ARROW_LEFT: &str = icon!(r#"<line x1="19" y1="12" x2="5" y2="12"/><polyline points="12 19 5 12 12 5"/>"#);
pub const ICON_CHECK: &str = icon!(r#"<polyline points="20 6 9 17 4 12"/>"#);
pub const ICON_X: &str = icon!(r#"<line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/>"#);

// ═══════════════════════════════════════════════════════════════
// Shell
// ═══════════════════════════════════════════════════════════════

#[derive(Clone, Copy)]
pub enum Nav {
    Overview,
    Appointments,
    Patients,
    Doctors,
    Reports,
    Settings,
    Other,
}

pub struct Actor<'a> {
    pub user: &'a User,
    pub bearer: &'a str,
    pub csrf: &'a str,
}

pub fn render_shell(
    title: &str,
    actor: &Actor<'_>,
    active: Nav,
    hero: &str,
    inner: &str,
    page_js: &str,
) -> String {
    let role_label = humanise_role(&actor.user.role);
    let avatar_initials = initials(&actor.user.email);

    let item = |name: &str, href: &str, icon: &str, this: Nav, enabled: bool| -> String {
        let is_active = matches!(
            (this, active),
            (Nav::Overview, Nav::Overview)
                | (Nav::Appointments, Nav::Appointments)
                | (Nav::Patients, Nav::Patients)
                | (Nav::Doctors, Nav::Doctors)
                | (Nav::Reports, Nav::Reports)
                | (Nav::Settings, Nav::Settings)
        );
        let class = if is_active {
            " active"
        } else if !enabled {
            " disabled"
        } else {
            ""
        };
        format!(
            r#"<a class="{cls}" href="{href}">{icon}<span>{name}</span></a>"#,
            cls = class.trim(),
            href = href,
            icon = icon,
            name = escape_html(name),
        )
    };

    let sidebar_items = format!(
        r#"<div class="sidebar-group-label">Workflow</div>
<nav>
  {home}
  {appts}
</nav>
<div class="sidebar-group-label">Directory</div>
<nav>
  {patients}
  {doctors}
</nav>
<div class="sidebar-group-label">Other</div>
<nav>
  {reports}
  {settings}
</nav>"#,
        home = item("Overview", "/admin", ICON_HOME, Nav::Overview, true),
        appts = item("Appointments", "/ops/appointments", ICON_CALENDAR, Nav::Appointments, true),
        patients = item("Patients", "/admin/patients", ICON_USERS, Nav::Patients, true),
        doctors = item("Doctors", "/admin/doctors", ICON_STETH, Nav::Doctors, true),
        reports = item("Reports", "#", ICON_CHART, Nav::Reports, false),
        settings = item("Settings", "/admin", ICON_SETTINGS, Nav::Settings, false),
    );

    format!(
        r##"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>{title} · MedFlow</title>
  <meta name="api-token" content="{token}">
  <meta name="color-scheme" content="light">
  <style>{styles}</style>
</head>
<body>
  <div class="layout">
    <aside class="sidebar" aria-label="Primary navigation">
      <a class="brand" href="/ops/appointments">
        <span class="brand-mark">M</span>
        <span>MedFlow<small>Operations</small></span>
      </a>
      {sidebar_items}
      <div class="sidebar-footer">
        <div class="avatar-dot">{initials}</div>
        <div class="u">
          <strong title="{email}">{email}</strong>
          <small>{role}</small>
        </div>
        <form method="post" action="/admin/logout">
          <input type="hidden" name="_csrf" value="{csrf}">
          <button type="submit" title="Sign out" aria-label="Sign out">{logout_icon}</button>
        </form>
      </div>
      <div class="mobile-user">
        <span>{email}</span>
        <span class="role-pill">{role}</span>
      </div>
    </aside>
    <div class="content">
      {hero}
      <main class="page">{inner}</main>
      <footer class="site">
        <span>MedFlow · operational console</span>
        <span>Framework admin at <a href="/admin">/admin</a></span>
      </footer>
    </div>
  </div>
  <script>{page_js}</script>
</body>
</html>"##,
        title = escape_html(title),
        token = escape_html(actor.bearer),
        styles = STYLES,
        sidebar_items = sidebar_items,
        email = escape_html(&actor.user.email),
        role = escape_html(&role_label),
        initials = escape_html(&avatar_initials),
        csrf = escape_html(actor.csrf),
        logout_icon = ICON_LOGOUT,
        hero = hero,
        inner = inner,
        page_js = page_js,
    )
}

pub fn page_hero(
    breadcrumb_html: &str,
    title: &str,
    subtitle: &str,
    actions_html: &str,
) -> String {
    let bc = if breadcrumb_html.is_empty() {
        String::new()
    } else {
        format!(r#"<div class="breadcrumb">{}</div>"#, breadcrumb_html)
    };
    format!(
        r#"<section class="page-hero">
{bc}
  <div class="row">
    <div class="title">
      <h1>{title}</h1>
      <div class="subtitle">{subtitle}</div>
    </div>
    <div class="actions">{actions}</div>
  </div>
</section>"#,
        bc = bc,
        title = escape_html(title),
        subtitle = escape_html(subtitle),
        actions = actions_html,
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

/// First letters of up to two words, uppercase.
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

/// Deterministic pastel colour for an avatar circle.
pub fn avatar_color(seed: &str) -> &'static str {
    const PALETTE: &[&str] = &[
        "#0d9488", // teal-600
        "#2563eb", // blue-600
        "#7c3aed", // violet-600
        "#db2777", // pink-600
        "#ea580c", // orange-600
        "#ca8a04", // yellow-600
        "#16a34a", // green-600
        "#0891b2", // cyan-600
    ];
    let hash: u32 = seed.bytes().fold(0u32, |a, b| a.wrapping_add(b as u32).wrapping_mul(31));
    PALETTE[(hash as usize) % PALETTE.len()]
}

/// Split a UTC timestamp into (HH:MM, "Apr 22") for the time chip.
pub fn time_chip(ts: DateTime<Utc>) -> (String, String) {
    (ts.format("%H:%M").to_string(), ts.format("%b %-d").to_string())
}

/// Heading used between day groups: ("Today", "Wednesday, April 22"),
/// ("Tomorrow", "Thursday, April 23"), or (None, full date).
pub fn day_heading(ts: DateTime<Utc>) -> (Option<String>, String) {
    let today = Utc::now().date_naive();
    let d = ts.date_naive();
    let offset = (d - today).num_days();
    let badge = match offset {
        0 => Some("Today".to_string()),
        1 => Some("Tomorrow".to_string()),
        -1 => Some("Yesterday".to_string()),
        _ => None,
    };
    let absolute = ts.format("%A, %B %-d").to_string();
    (badge, absolute)
}

/// Relative "time ago" for timeline events. Coarse — minutes, hours,
/// or days. Falls back to an ISO stamp after a week.
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

pub fn iso_year_month_day(ts: DateTime<Utc>) -> String {
    format!("{:04}-{:02}-{:02}", ts.year(), ts.month(), ts.day())
}

// ═══════════════════════════════════════════════════════════════
// Shared helpers
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
    let inner = format!(
        r#"<section class="card">
  <div class="empty-state">
    <span class="glyph">{icon}</span>
    <h3>Not allowed</h3>
    <p>{message}</p>
    <a class="btn btn-primary" href="/ops/appointments">Back to appointments</a>
  </div>
</section>"#,
        icon = ICON_ALERT,
        message = escape_html(message),
    );
    let hero = page_hero("", "Forbidden", "You don't have access to this action.", "");
    let body = render_shell("Forbidden", actor, Nav::Other, &hero, &inner, "");
    let mut resp = html(body);
    *resp.status_mut() = StatusCode::FORBIDDEN;
    resp
}

pub fn not_found_page(actor: &Actor<'_>, what: &str) -> Response {
    let inner = format!(
        r#"<section class="card">
  <div class="empty-state">
    <span class="glyph">{icon}</span>
    <h3>Not found</h3>
    <p>{message}</p>
    <a class="btn" href="/ops/appointments">Back to appointments</a>
  </div>
</section>"#,
        icon = ICON_SEARCH,
        message = escape_html(what),
    );
    let hero = page_hero("", "Not found", "The resource you requested could not be located.", "");
    let body = render_shell("Not found", actor, Nav::Other, &hero, &inner, "");
    let mut resp = html(body);
    *resp.status_mut() = StatusCode::NOT_FOUND;
    resp
}
