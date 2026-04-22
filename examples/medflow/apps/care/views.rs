//! Operational console pages.
//!
//!   GET /ops/appointments             — dashboard with stats + grouped list
//!   GET /ops/appointments/new         — smart form with live preview
//!   GET /ops/appointments/:id/edit    — detail page with lifecycle + timeline

use chrono::Utc;
use rustio_core::auth::{session, user, CsrfToken, User, SESSION_COOKIE};
use rustio_core::router::Params;
use rustio_core::{html, Db, Error, Model, Request, Response, Router};
use std::collections::HashMap;

use crate::apps::care::models::{Appointment, AppointmentEvent};
use crate::apps::people::models::{Department, Doctor, Patient};
use crate::apps::ui::{
    avatar_color, day_heading, escape_html, forbidden_page, humanise_status, initials,
    not_found_page, page_hero, pluralise, redirect, relative_past, render_shell, time_chip,
    Actor, Nav, ICON_ALERT, ICON_ARROW_LEFT, ICON_CLOCK, ICON_INBOX, ICON_PLUS, ICON_SEARCH,
};

// ═══════════════════════════════════════════════════════════════
// Routes
// ═══════════════════════════════════════════════════════════════

pub fn register(router: Router, db: &Db) -> Router {
    let list_db = db.clone();
    let new_db = db.clone();
    let edit_db = db.clone();
    router
        .get("/ops/appointments", move |req, _params| {
            let db = list_db.clone();
            async move { ops_list(&db, req).await }
        })
        .get("/ops/appointments/new", move |req, _params| {
            let db = new_db.clone();
            async move { ops_new(&db, req).await }
        })
        .get("/ops/appointments/:id/edit", move |req, params| {
            let db = edit_db.clone();
            async move { ops_detail(&db, req, &params).await }
        })
}

// ═══════════════════════════════════════════════════════════════
// Session loader (cookie-based)
// ═══════════════════════════════════════════════════════════════

struct Session {
    user: User,
    bearer: String,
    csrf: String,
}

async fn load_session(db: &Db, req: &Request) -> Result<Option<Session>, Error> {
    let Some(token) = req.cookie(SESSION_COOKIE) else {
        return Ok(None);
    };
    let Some(sess) = session::find_valid(db, &token).await? else {
        return Ok(None);
    };
    let Some(actor) = user::find_by_id(db, sess.user_id).await? else {
        return Ok(None);
    };
    if !actor.is_active {
        return Ok(None);
    }
    let csrf = req
        .ctx()
        .get::<CsrfToken>()
        .map(|c| c.0.clone())
        .unwrap_or(sess.csrf_token);
    Ok(Some(Session {
        user: actor,
        bearer: token,
        csrf,
    }))
}

fn role_may(role: &str, action: &str) -> bool {
    if role == "admin" {
        return true;
    }
    match (role, action) {
        ("receptionist", "confirm")
        | ("receptionist", "check-in")
        | ("receptionist", "cancel") => true,
        ("doctor", "start") | ("doctor", "complete") | ("doctor", "cancel") => true,
        _ => false,
    }
}
fn can_create(actor: &User) -> bool {
    matches!(actor.role.as_str(), "admin" | "receptionist")
}

// ═══════════════════════════════════════════════════════════════
// /ops/appointments — list dashboard
// ═══════════════════════════════════════════════════════════════

async fn ops_list(db: &Db, req: Request) -> Result<Response, Error> {
    let Some(sess) = load_session(db, &req).await? else {
        return Ok(redirect("/admin/login?next=/ops/appointments"));
    };

    let query = req.query();
    let q = query.get("q").unwrap_or("").trim().to_string();
    let status_filter = query.get("status").unwrap_or("").trim().to_string();
    let sort = query.get("sort").unwrap_or("scheduled_asc").to_string();

    let mut appointments = Appointment::all(db).await?;
    let patients = Patient::all(db).await?;
    let doctors = Doctor::all(db).await?;
    let departments = Department::all(db).await?;

    // Lookups
    let patient_names: HashMap<i64, String> =
        patients.into_iter().map(|p| (p.id, p.full_name)).collect();
    let doctor_map: HashMap<i64, (String, String)> = doctors
        .into_iter()
        .map(|d| (d.id, (d.full_name, d.specialty)))
        .collect();
    let dept_names: HashMap<i64, String> =
        departments.into_iter().map(|d| (d.id, d.name)).collect();

    // Stats are computed from the unfiltered set so they reflect the
    // world, not the current view.
    let total_all = appointments.len();
    let today_count = appointments
        .iter()
        .filter(|a| {
            let t = a.scheduled_at.date_naive();
            t == Utc::now().date_naive()
        })
        .count();
    let mut by_status: HashMap<String, usize> = HashMap::new();
    for a in &appointments {
        *by_status.entry(a.status.clone()).or_insert(0) += 1;
    }

    // Filter
    if !status_filter.is_empty() {
        appointments.retain(|a| a.status == status_filter);
    }
    if !q.is_empty() {
        let needle = q.to_lowercase();
        appointments.retain(|a| {
            let pat = patient_names
                .get(&a.patient_id)
                .map(|s| s.to_lowercase())
                .unwrap_or_default();
            let doc = doctor_map
                .get(&a.doctor_id)
                .map(|(n, _)| n.to_lowercase())
                .unwrap_or_default();
            pat.contains(&needle)
                || doc.contains(&needle)
                || a.reason.to_lowercase().contains(&needle)
                || a.id.to_string() == needle
        });
    }

    // Sort
    match sort.as_str() {
        "scheduled_desc" => appointments.sort_by(|a, b| b.scheduled_at.cmp(&a.scheduled_at)),
        "scheduled_asc" => appointments.sort_by_key(|a| a.scheduled_at),
        "id_desc" => appointments.sort_by(|a, b| b.id.cmp(&a.id)),
        "id_asc" => appointments.sort_by_key(|a| a.id),
        _ => appointments.sort_by_key(|a| a.scheduled_at),
    }

    let shown = appointments.len();
    let can_add = can_create(&sess.user);

    // ── Hero ────────────────────────────────────────────────
    let actions = if can_add {
        format!(
            r#"<a class="btn btn-primary btn-lg" href="/ops/appointments/new">{icon}<span>New appointment</span></a>"#,
            icon = ICON_PLUS,
        )
    } else {
        String::new()
    };
    let hero = page_hero(
        "",
        "Appointments",
        "Schedule and coordinate patient visits.",
        &actions,
    );

    // ── Stats strip ─────────────────────────────────────────
    let stats = render_stats(today_count, &by_status, total_all);

    // ── Toolbar (tabs + search + sort) ─────────────────────
    let toolbar = render_toolbar(&q, &status_filter, &sort, &by_status, total_all);

    // ── Rows (grouped by day) ─────────────────────────────
    let rows = if shown == 0 {
        render_empty_row(total_all == 0)
    } else {
        render_grouped_rows(
            &appointments,
            &patient_names,
            &doctor_map,
            &dept_names,
            &sess.user.role,
        )
    };

    // ── Active filter chips ────────────────────────────────
    let chips = render_chips(&q, &status_filter);

    let inner = format!(
        r#"{stats}
{chips}
<div id="banner" class="banner" role="alert" hidden>{alert}<span id="banner-text"></span></div>
<section class="card">
  {toolbar}
  <div style="overflow-x:auto">
    <table class="rows">
      <tbody>
        {rows}
      </tbody>
    </table>
  </div>
</section>"#,
        stats = stats,
        chips = chips,
        alert = ICON_ALERT,
        toolbar = toolbar,
        rows = rows,
    );

    let actor = Actor {
        user: &sess.user,
        bearer: &sess.bearer,
        csrf: &sess.csrf,
    };
    Ok(html(render_shell(
        "Appointments",
        &actor,
        Nav::Appointments,
        &hero,
        &inner,
        LIST_JS,
    )))
}

fn render_stats(
    today: usize,
    by_status: &HashMap<String, usize>,
    total: usize,
) -> String {
    let get = |k: &str| by_status.get(k).copied().unwrap_or(0);
    format!(
        r#"<div class="stats">
  <div class="stat">
    <div class="label">Today</div>
    <div class="value">{today}</div>
    <div class="sub">{total_label} total</div>
  </div>
  <div class="stat stat-ok">
    <div class="label">Confirmed</div>
    <div class="value">{conf}</div>
    <div class="sub">ready to start</div>
  </div>
  <div class="stat stat-warn">
    <div class="label">In progress</div>
    <div class="value">{prog}</div>
    <div class="sub">active consultations</div>
  </div>
  <div class="stat stat-mute">
    <div class="label">Completed</div>
    <div class="value">{comp}</div>
    <div class="sub">closed today &amp; before</div>
  </div>
</div>"#,
        today = today,
        total_label = pluralise(total, "appointment"),
        conf = get("confirmed"),
        prog = get("in_progress"),
        comp = get("completed"),
    )
}

fn render_toolbar(
    q: &str,
    status: &str,
    sort: &str,
    by_status: &HashMap<String, usize>,
    total: usize,
) -> String {
    let tab = |label: &str, value: &str, count: usize| -> String {
        let is_active = (value.is_empty() && status.is_empty()) || value == status;
        let base_href = "/ops/appointments".to_string();
        let mut params: Vec<String> = Vec::new();
        if !q.is_empty() {
            params.push(format!("q={}", escape_html(q)));
        }
        if !value.is_empty() {
            params.push(format!("status={}", value));
        }
        if sort != "scheduled_asc" {
            params.push(format!("sort={}", sort));
        }
        let href = if params.is_empty() {
            base_href
        } else {
            format!("{}?{}", base_href, params.join("&"))
        };
        format!(
            r#"<a class="{cls}" href="{href}">{label}<span class="badge">{count}</span></a>"#,
            cls = if is_active { "active" } else { "" },
            href = escape_html(&href),
            label = escape_html(label),
            count = count,
        )
    };

    let tabs = format!(
        r#"<div class="tabs" role="tablist" aria-label="Filter by status">
  {all}{sched}{conf}{prog}{comp}{canc}
</div>"#,
        all = tab("All", "", total),
        sched = tab("Scheduled", "scheduled", by_status.get("scheduled").copied().unwrap_or(0)),
        conf = tab("Confirmed", "confirmed", by_status.get("confirmed").copied().unwrap_or(0)),
        prog = tab("In progress", "in_progress", by_status.get("in_progress").copied().unwrap_or(0)),
        comp = tab("Completed", "completed", by_status.get("completed").copied().unwrap_or(0)),
        canc = tab("Cancelled", "cancelled", by_status.get("cancelled").copied().unwrap_or(0)),
    );

    format!(
        r##"<form class="toolbar" method="get" action="/ops/appointments" role="search">
{tabs}
<div class="spacer"></div>
<div class="search">
  {search_icon}
  <input type="search" name="q" value="{q}" placeholder="Search patient, doctor, reason…">
</div>
<input type="hidden" name="status" value="{status}">
<select name="sort" aria-label="Sort" onchange="this.form.submit()">
  <option value="scheduled_asc"{so_a}>Chronological</option>
  <option value="scheduled_desc"{so_d}>Newest first</option>
  <option value="id_desc"{si_d}>Recently created</option>
  <option value="id_asc"{si_a}>Oldest created</option>
</select>
<button class="btn" type="submit">Apply</button>
</form>"##,
        tabs = tabs,
        search_icon = ICON_SEARCH,
        q = escape_html(q),
        status = escape_html(status),
        so_a = if sort == "scheduled_asc"  { " selected" } else { "" },
        so_d = if sort == "scheduled_desc" { " selected" } else { "" },
        si_d = if sort == "id_desc"        { " selected" } else { "" },
        si_a = if sort == "id_asc"         { " selected" } else { "" },
    )
}

fn render_chips(q: &str, status: &str) -> String {
    let mut parts: Vec<String> = Vec::new();
    if !q.is_empty() {
        parts.push(format!(
            r#"<span class="filter-chip">Search: &ldquo;{v}&rdquo; <a href="?{href}" aria-label="Clear search">×</a></span>"#,
            v = escape_html(q),
            href = if status.is_empty() { String::new() } else { format!("status={}", escape_html(status)) },
        ));
    }
    if !status.is_empty() {
        parts.push(format!(
            r#"<span class="filter-chip">Status: {v} <a href="?{href}" aria-label="Clear status">×</a></span>"#,
            v = escape_html(&humanise_status(status)),
            href = if q.is_empty() { String::new() } else { format!("q={}", escape_html(q)) },
        ));
    }
    if parts.is_empty() {
        return String::new();
    }
    format!(
        r#"<div class="filter-chips" style="margin-bottom: var(--s-4);">
  {chips}
  <a class="filter-chip" href="/ops/appointments" style="background:transparent;color:var(--text-mute)">Clear all</a>
</div>"#,
        chips = parts.join(""),
    )
}

fn render_grouped_rows(
    appts: &[Appointment],
    patient_names: &HashMap<i64, String>,
    doctor_map: &HashMap<i64, (String, String)>,
    dept_names: &HashMap<i64, String>,
    role: &str,
) -> String {
    let mut out = String::new();
    let mut current_day: Option<chrono::NaiveDate> = None;

    for a in appts {
        let day = a.scheduled_at.date_naive();
        if Some(day) != current_day {
            let (badge, absolute) = day_heading(a.scheduled_at);
            let badge_html = match badge {
                Some(b) => format!(r#"<span class="pill-today">{}</span>"#, escape_html(&b)),
                None => String::new(),
            };
            out.push_str(&format!(
                r#"<tr class="day-row"><td colspan="4">{badge}{abs}</td></tr>"#,
                badge = badge_html,
                abs = escape_html(&absolute),
            ));
            current_day = Some(day);
        }
        out.push_str(&render_row(a, patient_names, doctor_map, dept_names, role));
    }
    out
}

fn render_row(
    a: &Appointment,
    patient_names: &HashMap<i64, String>,
    doctor_map: &HashMap<i64, (String, String)>,
    dept_names: &HashMap<i64, String>,
    role: &str,
) -> String {
    let patient = patient_names
        .get(&a.patient_id)
        .cloned()
        .unwrap_or_else(|| format!("#{}", a.patient_id));
    let (doc_name, doc_specialty) = doctor_map
        .get(&a.doctor_id)
        .cloned()
        .unwrap_or_else(|| (format!("#{}", a.doctor_id), String::new()));
    let dept_badge = a
        .department_id
        .and_then(|id| dept_names.get(&id).cloned())
        .map(|n| {
            format!(
                r#"<span class="dept-badge">{}</span>"#,
                escape_html(&n)
            )
        })
        .unwrap_or_default();

    let (hh, md) = time_chip(a.scheduled_at);
    let p_initials = initials(&patient);
    let d_initials = initials(&doc_name);
    let p_color = avatar_color(&patient);
    let d_color = avatar_color(&doc_name);

    let status = a.status.as_str();
    let actions = render_row_actions(a.id, status, role);

    format!(
        r##"<tr class="appt">
  <td class="time">
    <div class="time-chip">
      <span class="hh">{hh}</span>
      <span class="md">{md}</span>
    </div>
  </td>
  <td>
    <div class="person">
      <div class="avatar" style="background:{p_col}">{p_in}</div>
      <div class="meta">
        <span class="name"><a href="/ops/appointments/{id}/edit">{patient}</a></span>
        <small>{reason}</small>
      </div>
    </div>
  </td>
  <td>
    <div class="person">
      <div class="avatar" style="background:{d_col}">{d_in}</div>
      <div class="meta">
        <span class="name">{doctor}</span>
        <small>{specialty}{sep}{dept}</small>
      </div>
    </div>
  </td>
  <td class="actions-cell">
    <div class="action-group">
      <span class="pill pill-{status_raw}">{status_label}</span>
      {actions}
    </div>
  </td>
</tr>"##,
        hh = escape_html(&hh),
        md = escape_html(&md),
        p_col = p_color,
        p_in = escape_html(&p_initials),
        patient = escape_html(&patient),
        reason = escape_html(
            &(if a.reason.is_empty() {
                "No reason given".to_string()
            } else {
                a.reason.clone()
            })
        ),
        id = a.id,
        d_col = d_color,
        d_in = escape_html(&d_initials),
        doctor = escape_html(&doc_name),
        specialty = escape_html(&doc_specialty),
        sep = if !doc_specialty.is_empty() && !dept_badge.is_empty() { " · " } else { "" },
        dept = dept_badge,
        status_raw = escape_html(status),
        status_label = escape_html(&humanise_status(status)),
        actions = actions,
    )
}

fn render_row_actions(id: i64, status: &str, role: &str) -> String {
    let offered: &[(&str, &str, bool)] = match status {
        "scheduled" => &[("confirm", "Confirm", false), ("cancel", "Cancel", true)],
        "confirmed" => &[("check-in", "Check-in", false), ("cancel", "Cancel", true)],
        "in_progress" => &[("complete", "Complete", false), ("cancel", "Cancel", true)],
        _ => &[],
    };
    let mut out = String::new();
    for (action, label, danger) in offered {
        if !role_may(role, action) {
            continue;
        }
        let cls = if *danger {
            "btn btn-sm btn-danger"
        } else {
            "btn btn-sm"
        };
        out.push_str(&format!(
            r#"<button class="{cls}" data-action="{a}" data-id="{id}">{label}</button>"#,
            cls = cls,
            a = action,
            id = id,
            label = escape_html(label),
        ));
    }
    out
}

fn render_empty_row(completely_empty: bool) -> String {
    if completely_empty {
        format!(
            r#"<tr><td colspan="4" class="empty-state">
  <span class="glyph">{icon}</span>
  <h3>No appointments yet</h3>
  <p>Create the first appointment to start the clinic workflow.</p>
  <a class="btn btn-primary" href="/ops/appointments/new">{plus}<span>New appointment</span></a>
</td></tr>"#,
            icon = ICON_INBOX,
            plus = ICON_PLUS,
        )
    } else {
        format!(
            r#"<tr><td colspan="4" class="empty-state">
  <span class="glyph">{icon}</span>
  <h3>No matches</h3>
  <p>Try a different search or clear the filters.</p>
  <a class="btn" href="/ops/appointments">Clear filters</a>
</td></tr>"#,
            icon = ICON_SEARCH,
        )
    }
}

const LIST_JS: &str = r#"
(function () {
  var tokenEl = document.querySelector('meta[name="api-token"]');
  var token = tokenEl ? tokenEl.getAttribute('content') : '';
  var banner = document.getElementById('banner');
  var bannerText = document.getElementById('banner-text');
  function showError(msg) { bannerText.textContent = msg; banner.hidden = false; }
  document.addEventListener('click', function (e) {
    var btn = e.target && e.target.closest ? e.target.closest('[data-action]') : null;
    if (!btn) return;
    var action = btn.getAttribute('data-action');
    var id = btn.getAttribute('data-id');
    if (!action || !id) return;
    btn.disabled = true;
    fetch('/api/appointments/' + encodeURIComponent(id) + '/' + action, {
      method: 'POST',
      headers: {
        'Authorization': 'Bearer ' + token,
        'Content-Type': 'application/json'
      },
      body: '{}'
    }).then(function (r) {
      if (r.ok) { window.location.reload(); return; }
      r.text().then(function (t) {
        showError('Action failed (' + r.status + '): ' + t);
        btn.disabled = false;
      });
    }).catch(function (err) {
      showError('Network error: ' + err);
      btn.disabled = false;
    });
  });
})();
"#;

// ═══════════════════════════════════════════════════════════════
// /ops/appointments/new — 2-column form
// ═══════════════════════════════════════════════════════════════

async fn ops_new(db: &Db, req: Request) -> Result<Response, Error> {
    let Some(sess) = load_session(db, &req).await? else {
        return Ok(redirect("/admin/login?next=/ops/appointments/new"));
    };
    let actor = Actor {
        user: &sess.user,
        bearer: &sess.bearer,
        csrf: &sess.csrf,
    };
    if !can_create(&sess.user) {
        return Ok(forbidden_page(
            &actor,
            "Only receptionists and admins can create appointments.",
        ));
    }

    let patients = Patient::all(db).await?;
    let doctors = Doctor::all(db).await?;
    let departments = Department::all(db).await?;

    let min_dt = Utc::now().format("%Y-%m-%dT%H:%M").to_string();
    let max_dt = (Utc::now() + chrono::Duration::days(365 * 2))
        .format("%Y-%m-%dT%H:%M")
        .to_string();

    let breadcrumb = format!(
        r#"<a href="/ops/appointments">Appointments</a> {sep} <span>New</span>"#,
        sep = ICON_CHEVRON_RIGHT_SMALL,
    );
    let back_btn = format!(
        r#"<a class="btn btn-ghost" href="/ops/appointments">{icon}<span>Back</span></a>"#,
        icon = ICON_ARROW_LEFT,
    );
    let hero = page_hero(
        &breadcrumb,
        "New appointment",
        "Enter patient, doctor, date and clinical details.",
        &back_btn,
    );

    let inner = render_new_form(&patients, &doctors, &departments, &min_dt, &max_dt);
    Ok(html(render_shell(
        "New appointment",
        &actor,
        Nav::Appointments,
        &hero,
        &inner,
        NEW_FORM_JS,
    )))
}

const ICON_CHEVRON_RIGHT_SMALL: &str = r#"<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><polyline points="9 18 15 12 9 6"/></svg>"#;

fn render_new_form(
    patients: &[Patient],
    doctors: &[Doctor],
    departments: &[Department],
    min_dt: &str,
    max_dt: &str,
) -> String {
    let patient_opts = options_from(
        patients
            .iter()
            .filter(|p| p.is_active)
            .map(|p| (p.id, p.full_name.as_str())),
    );
    let doctor_opts = options_from(
        doctors
            .iter()
            .filter(|d| d.is_active)
            .map(|d| (d.id, d.full_name.as_str())),
    );
    let dept_opts = format!(
        r#"<option value="">— none —</option>{rest}"#,
        rest = options_from(
            departments
                .iter()
                .filter(|d| d.is_active)
                .map(|d| (d.id, d.name.as_str()))
        ),
    );

    format!(
        r##"<div id="banner" class="banner" role="alert" hidden>{alert}<span id="banner-text"></span></div>
<div class="form-layout">
  <section class="card">
    <form id="new-form" class="form" novalidate>
      <fieldset class="group">
        <legend>Who</legend>

        <label class="field" for="patient_id">Patient <span class="req">*</span></label>
        <div>
          <select class="input" id="patient_id" name="patient_id" required>
            <option value="">Select a patient…</option>
            {patients}
          </select>
          <div class="field-error" id="patient_id-err"></div>
        </div>

        <label class="field" for="doctor_id">Doctor <span class="req">*</span></label>
        <div>
          <select class="input" id="doctor_id" name="doctor_id" required>
            <option value="">Select a doctor…</option>
            {doctors}
          </select>
          <div class="field-error" id="doctor_id-err"></div>
        </div>

        <label class="field" for="department_id">Department <span class="opt">optional</span></label>
        <select class="input" id="department_id" name="department_id">
          {depts}
        </select>
      </fieldset>

      <fieldset class="group">
        <legend>When</legend>

        <label class="field" for="scheduled_at">Scheduled <span class="req">*</span></label>
        <div>
          <input class="input" id="scheduled_at" name="scheduled_at" type="datetime-local"
                 min="{min_dt}" max="{max_dt}" required>
          <div class="field-hint">UTC — past times are blocked.</div>
          <div class="field-error" id="scheduled_at-err"></div>
        </div>

        <label class="field" for="duration_preset">Duration <span class="req">*</span></label>
        <div class="duration-combo">
          <select class="input" id="duration_preset" name="duration_preset" required>
            <option value="15">15 minutes</option>
            <option value="30" selected>30 minutes</option>
            <option value="45">45 minutes</option>
            <option value="60">1 hour</option>
            <option value="90">1 hour 30 min</option>
            <option value="120">2 hours</option>
            <option value="custom">Custom…</option>
          </select>
          <input class="input" id="duration_custom" name="duration_custom"
                 type="number" inputmode="numeric" min="1" max="1440"
                 placeholder="Minutes" hidden>
        </div>

        <label class="field" for="priority">Priority <span class="req">*</span></label>
        <select class="input" id="priority" name="priority" required>
          <option value="1">Low</option>
          <option value="3">Normal</option>
          <option value="5" selected>Standard</option>
          <option value="7">High</option>
          <option value="10">Urgent</option>
        </select>
      </fieldset>

      <fieldset class="group">
        <legend>Details</legend>

        <label class="field" for="reason">Reason <span class="opt">optional</span></label>
        <div>
          <textarea class="input" id="reason" name="reason" rows="2" maxlength="500"
                    placeholder="Brief reason for the visit"></textarea>
          <div class="char-counter" id="reason-counter">0 / 500</div>
        </div>

        <label class="field" for="notes">Notes <span class="opt">optional</span></label>
        <div>
          <textarea class="input" id="notes" name="notes" rows="3" maxlength="1000"
                    placeholder="Internal notes (not visible to patient)"></textarea>
          <div class="char-counter" id="notes-counter">0 / 1000</div>
        </div>
      </fieldset>
    </form>
  </section>

  <aside class="preview-card" aria-live="polite">
    <h3>Preview</h3>
    <div class="big-time empty" id="pv-time">
      <span class="hh">—</span>
      <span class="md">pick a date &amp; time</span>
    </div>
    <div class="row">
      <div class="key">Patient</div>
      <div class="val" id="pv-patient"><span class="soft">— not selected —</span></div>
    </div>
    <div class="row">
      <div class="key">Doctor</div>
      <div class="val" id="pv-doctor"><span class="soft">— not selected —</span></div>
    </div>
    <div class="row">
      <div class="key">Dept</div>
      <div class="val" id="pv-dept"><span class="soft">—</span></div>
    </div>
    <div class="row">
      <div class="key">Duration</div>
      <div class="val" id="pv-duration">30 minutes</div>
    </div>
    <div class="row">
      <div class="key">Priority</div>
      <div class="val" id="pv-priority">Standard</div>
    </div>
    <div class="cta">
      <button id="submit-btn" form="new-form" type="submit" class="btn btn-primary btn-lg" style="justify-content:center">
        {check}<span>Create appointment</span>
      </button>
      <a class="btn btn-ghost" href="/ops/appointments" style="justify-content:center">Cancel</a>
      <div class="shortcut"><kbd>Ctrl</kbd>+<kbd>Enter</kbd> to submit</div>
    </div>
  </aside>
</div>"##,
        alert = ICON_ALERT,
        patients = patient_opts,
        doctors = doctor_opts,
        depts = dept_opts,
        min_dt = escape_html(min_dt),
        max_dt = escape_html(max_dt),
        check = ICON_CHECK_SMALL,
    )
}

const ICON_CHECK_SMALL: &str = r#"<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><polyline points="20 6 9 17 4 12"/></svg>"#;

fn options_from<'a, I>(iter: I) -> String
where
    I: Iterator<Item = (i64, &'a str)>,
{
    let mut out = String::new();
    for (id, label) in iter {
        out.push_str(&format!(
            r#"<option value="{id}">{label}</option>"#,
            id = id,
            label = escape_html(label),
        ));
    }
    out
}

const NEW_FORM_JS: &str = r#"
(function () {
  var tokenEl = document.querySelector('meta[name="api-token"]');
  var token = tokenEl ? tokenEl.getAttribute('content') : '';
  var form = document.getElementById('new-form');
  var banner = document.getElementById('banner');
  var bannerText = document.getElementById('banner-text');
  var submitBtn = document.getElementById('submit-btn');

  var durPreset = document.getElementById('duration_preset');
  var durCustom = document.getElementById('duration_custom');

  var PRIORITIES = { '1': 'Low', '3': 'Normal', '5': 'Standard', '7': 'High', '10': 'Urgent' };

  function showBanner(msg) { bannerText.textContent = msg; banner.hidden = false; banner.scrollIntoView({ block: 'nearest' }); }
  function clearBanner() { banner.hidden = true; bannerText.textContent = ''; }

  function setFieldError(id, msg) {
    var input = document.getElementById(id);
    var err = document.getElementById(id + '-err');
    if (msg) { if (input) input.classList.add('invalid'); if (err) err.textContent = msg; }
    else     { if (input) input.classList.remove('invalid'); if (err) err.textContent = ''; }
  }
  function clearAllErrors() { ['patient_id','doctor_id','scheduled_at'].forEach(function(i){ setFieldError(i,''); }); }

  function syncCustom() {
    if (durPreset.value === 'custom') {
      durCustom.hidden = false; durCustom.required = true;
      setTimeout(function(){ durCustom.focus(); }, 0);
    } else { durCustom.hidden = true; durCustom.required = false; }
    updatePreview();
  }
  durPreset.addEventListener('change', syncCustom);
  durCustom.addEventListener('input', updatePreview);
  syncCustom();

  function wireCounter(id, ctrId, max) {
    var inp = document.getElementById(id), ctr = document.getElementById(ctrId);
    function refresh() { var n = inp.value.length; ctr.textContent = n + ' / ' + max; ctr.classList.toggle('over', n > max); }
    inp.addEventListener('input', refresh); refresh();
  }
  wireCounter('reason', 'reason-counter', 500);
  wireCounter('notes',  'notes-counter',  1000);

  function updatePreview() {
    var pSel = document.getElementById('patient_id');
    var dSel = document.getElementById('doctor_id');
    var deptSel = document.getElementById('department_id');
    var prSel = document.getElementById('priority');
    var t = document.getElementById('scheduled_at').value;

    var pvPatient = document.getElementById('pv-patient');
    var pvDoctor = document.getElementById('pv-doctor');
    var pvDept   = document.getElementById('pv-dept');
    var pvDur    = document.getElementById('pv-duration');
    var pvPr     = document.getElementById('pv-priority');
    var pvTime   = document.getElementById('pv-time');

    pvPatient.innerHTML = pSel.value
      ? escapeHtml(pSel.options[pSel.selectedIndex].textContent)
      : '<span class="soft">— not selected —</span>';
    pvDoctor.innerHTML = dSel.value
      ? escapeHtml(dSel.options[dSel.selectedIndex].textContent)
      : '<span class="soft">— not selected —</span>';
    pvDept.innerHTML = deptSel.value
      ? escapeHtml(deptSel.options[deptSel.selectedIndex].textContent)
      : '<span class="soft">—</span>';
    pvPr.textContent = PRIORITIES[prSel.value] || 'Standard';

    var mins;
    if (durPreset.value === 'custom') { mins = parseInt(durCustom.value, 10); }
    else { mins = parseInt(durPreset.value, 10); }
    pvDur.textContent = formatDuration(mins);

    if (t) {
      var parts = t.split('T');
      var date = parts[0]; var time = parts[1] || '00:00';
      pvTime.classList.remove('empty');
      pvTime.innerHTML = '<span class="hh">' + escapeHtml(time.slice(0, 5)) + '</span>' +
                         '<span class="md">' + escapeHtml(date) + ' UTC</span>';
    } else {
      pvTime.classList.add('empty');
      pvTime.innerHTML = '<span class="hh">—</span><span class="md">pick a date &amp; time</span>';
    }
  }
  function formatDuration(m) {
    if (!m || m < 1) return '—';
    if (m < 60) return m + ' minutes';
    var h = Math.floor(m / 60), rem = m % 60;
    return rem === 0 ? h + ' hour' + (h > 1 ? 's' : '') : h + 'h ' + rem + 'm';
  }
  function escapeHtml(s) { var d = document.createElement('div'); d.textContent = s; return d.innerHTML; }
  ['patient_id','doctor_id','department_id','scheduled_at','priority'].forEach(function(id){
    var el = document.getElementById(id);
    el.addEventListener('change', updatePreview);
    el.addEventListener('input',  updatePreview);
  });
  updatePreview();

  function toUtcRfc3339(local) {
    if (!local) return null;
    var withSeconds = local.length === 16 ? local + ':00' : local;
    return withSeconds + 'Z';
  }

  function validate() {
    clearAllErrors();
    var ok = true;
    var fd = new FormData(form);
    if (!fd.get('patient_id')) { setFieldError('patient_id', 'Pick a patient.'); ok = false; }
    if (!fd.get('doctor_id'))  { setFieldError('doctor_id',  'Pick a doctor.');  ok = false; }
    if (!fd.get('scheduled_at')) { setFieldError('scheduled_at', 'Pick a date and time.'); ok = false; }
    if (durPreset.value === 'custom') {
      var n = parseInt(durCustom.value, 10);
      if (!n || n < 1) { showBanner('Duration must be a positive whole number of minutes.'); ok = false; }
    }
    return ok;
  }

  function submit() {
    clearBanner();
    if (!validate()) return;
    var durValue = durPreset.value === 'custom'
      ? parseInt(durCustom.value, 10)
      : parseInt(durPreset.value, 10);
    var fd = new FormData(form);
    var body = {
      patient_id: parseInt(fd.get('patient_id'), 10),
      doctor_id: parseInt(fd.get('doctor_id'), 10),
      scheduled_at: toUtcRfc3339(fd.get('scheduled_at')),
      duration_minutes: durValue,
      priority: parseInt(fd.get('priority'), 10) || 5,
      reason: fd.get('reason') || '',
      notes: fd.get('notes') || ''
    };
    var deptRaw = fd.get('department_id');
    if (deptRaw) body.department_id = parseInt(deptRaw, 10);

    submitBtn.disabled = true;
    submitBtn.lastChild.textContent = 'Creating…';
    fetch('/api/appointments', {
      method: 'POST',
      headers: { 'Authorization': 'Bearer ' + token, 'Content-Type': 'application/json' },
      body: JSON.stringify(body)
    }).then(function (r) {
      if (r.ok) { window.location.href = '/ops/appointments'; return; }
      r.text().then(function (t) {
        showBanner('Create failed (' + r.status + '): ' + t);
        submitBtn.disabled = false;
        submitBtn.lastChild.textContent = 'Create appointment';
      });
    }).catch(function (err) {
      showBanner('Network error: ' + err);
      submitBtn.disabled = false;
      submitBtn.lastChild.textContent = 'Create appointment';
    });
  }
  form.addEventListener('submit', function(e){ e.preventDefault(); submit(); });
  form.addEventListener('keydown', function(e){
    if ((e.ctrlKey || e.metaKey) && e.key === 'Enter') { e.preventDefault(); submit(); }
  });
})();
"#;

// ═══════════════════════════════════════════════════════════════
// /ops/appointments/:id/edit — detail page
// ═══════════════════════════════════════════════════════════════

async fn ops_detail(db: &Db, req: Request, params: &Params) -> Result<Response, Error> {
    let Some(sess) = load_session(db, &req).await? else {
        let id = params.get("id").unwrap_or("");
        return Ok(redirect(&format!(
            "/admin/login?next=/ops/appointments/{id}/edit"
        )));
    };
    let actor = Actor {
        user: &sess.user,
        bearer: &sess.bearer,
        csrf: &sess.csrf,
    };
    let Some(id_str) = params.get("id") else {
        return Ok(not_found_page(&actor, "Missing appointment id."));
    };
    let Ok(id) = id_str.parse::<i64>() else {
        return Ok(not_found_page(&actor, "That appointment id isn't a number."));
    };
    let Some(appt) = Appointment::find(db, id).await? else {
        return Ok(not_found_page(
            &actor,
            &format!("Appointment #{id} does not exist."),
        ));
    };

    let patient = Patient::find(db, appt.patient_id).await?;
    let doctor = Doctor::find(db, appt.doctor_id).await?;
    let department = match appt.department_id {
        Some(d) => Department::find(db, d).await?,
        None => None,
    };

    let events: Vec<AppointmentEvent> = {
        let mut all = AppointmentEvent::all(db).await?;
        all.retain(|e| e.appointment_id == id);
        all.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        all
    };

    let breadcrumb = format!(
        r#"<a href="/ops/appointments">Appointments</a> {sep} <span>#{id}</span>"#,
        sep = ICON_CHEVRON_RIGHT_SMALL,
        id = id,
    );
    let hero_actions = render_detail_actions(id, &appt.status, &sess.user.role);
    let hero_title = format!("Appointment #{id}");
    let subtitle = format!(
        "Status: {status}  ·  {scheduled}",
        status = humanise_status(&appt.status),
        scheduled = appt.scheduled_at.format("%Y-%m-%d %H:%M UTC"),
    );
    let hero = page_hero(&breadcrumb, &hero_title, &subtitle, &hero_actions);

    let info_card = render_detail_info(&appt, patient.as_ref(), doctor.as_ref(), department.as_ref());
    let timeline_card = render_detail_timeline(&appt, &events);

    let inner = format!(
        r#"<div id="banner" class="banner" role="alert" hidden>{alert}<span id="banner-text"></span></div>
<section class="banner banner-warn" style="margin-bottom: var(--s-5);">
  {info_icon}<span><strong>Read-only view.</strong> Patient, doctor, and schedule cannot be changed after booking. To reschedule, cancel and create a new appointment.</span>
</section>
<div class="detail-grid">
  <section class="card">
    <div class="section-head"><h3>Information</h3></div>
    {info}
  </section>
  <section class="card">
    <div class="section-head">
      <h3>Activity</h3>
      <span class="count-pill">{ev_count}</span>
    </div>
    {timeline}
  </section>
</div>"#,
        alert = ICON_ALERT,
        info_icon = ICON_ALERT,
        info = info_card,
        timeline = timeline_card,
        ev_count = events.len(),
    );

    Ok(html(render_shell(
        &format!("Appointment #{id}"),
        &actor,
        Nav::Appointments,
        &hero,
        &inner,
        DETAIL_JS,
    )))
}

fn render_detail_actions(id: i64, status: &str, role: &str) -> String {
    let offered: &[(&str, &str, bool)] = match status {
        "scheduled" => &[("confirm", "Confirm", false), ("cancel", "Cancel", true)],
        "confirmed" => &[("check-in", "Check-in", false), ("cancel", "Cancel", true)],
        "in_progress" => &[("complete", "Complete", false), ("cancel", "Cancel", true)],
        _ => &[],
    };
    let mut out = String::new();
    for (action, label, danger) in offered {
        if !role_may(role, action) {
            continue;
        }
        let cls = if *danger { "btn btn-danger" } else { "btn btn-primary" };
        out.push_str(&format!(
            r#"<button class="{cls}" data-action="{a}" data-id="{id}">{label}</button>"#,
            cls = cls,
            a = action,
            id = id,
            label = escape_html(label),
        ));
    }
    if out.is_empty() {
        out.push_str(&format!(
            r#"<span class="pill pill-{s}">{label}</span>"#,
            s = escape_html(status),
            label = escape_html(&humanise_status(status)),
        ));
    }
    out
}

fn render_detail_info(
    appt: &Appointment,
    patient: Option<&Patient>,
    doctor: Option<&Doctor>,
    department: Option<&Department>,
) -> String {
    let p_name = patient.map(|p| p.full_name.as_str()).unwrap_or("—");
    let p_contact = patient.map(|p| format!("{} · {}", p.phone, p.email)).unwrap_or_else(|| "—".to_string());
    let d_name = doctor.map(|d| d.full_name.as_str()).unwrap_or("—");
    let d_specialty = doctor.map(|d| d.specialty.as_str()).unwrap_or("");
    let dept = department.map(|d| d.name.as_str()).unwrap_or("—");

    format!(
        r#"<div class="info-grid">
  <div class="item"><span class="k">Patient</span><span class="v">{p_name}</span><small>{p_contact}</small></div>
  <div class="item"><span class="k">Doctor</span><span class="v">{d_name}</span><small>{d_spec}</small></div>
  <div class="item"><span class="k">Department</span><span class="v">{dept}</span></div>
  <div class="item"><span class="k">Status</span><span class="v"><span class="pill pill-{status_raw}">{status_label}</span></span></div>
  <div class="item"><span class="k">Scheduled</span><span class="v">{scheduled}</span></div>
  <div class="item"><span class="k">Duration</span><span class="v">{duration} min</span></div>
  <div class="item"><span class="k">Priority</span><span class="v">{priority}</span></div>
  <div class="item"><span class="k">Active</span><span class="v">{active}</span></div>
  <div class="item wide"><span class="k">Reason</span><span class="v">{reason}</span></div>
  <div class="item wide"><span class="k">Notes</span><span class="v">{notes}</span></div>
  <div class="item wide"><span class="k">Created</span><small>{created}</small></div>
</div>"#,
        p_name = escape_html(p_name),
        p_contact = escape_html(&p_contact),
        d_name = escape_html(d_name),
        d_spec = escape_html(d_specialty),
        dept = escape_html(dept),
        status_raw = escape_html(&appt.status),
        status_label = escape_html(&humanise_status(&appt.status)),
        scheduled = escape_html(&appt.scheduled_at.format("%Y-%m-%d %H:%M UTC").to_string()),
        duration = appt.duration_minutes,
        priority = appt.priority,
        active = if appt.is_active { "yes" } else { "no" },
        reason = escape_html(if appt.reason.is_empty() { "—" } else { appt.reason.as_str() }),
        notes = escape_html(if appt.notes.is_empty() { "—" } else { appt.notes.as_str() }),
        created = escape_html(&appt.created_at.format("%Y-%m-%d %H:%M UTC").to_string()),
    )
}

fn render_detail_timeline(appt: &Appointment, events: &[AppointmentEvent]) -> String {
    if events.is_empty() {
        return format!(
            r#"<div class="empty-state">
  <span class="glyph">{icon}</span>
  <h3>No status transitions yet</h3>
  <p>When this appointment's status changes, the audit trail will appear here.</p>
</div>"#,
            icon = ICON_CLOCK,
        );
    }
    let mut out = String::from(r#"<div class="timeline">"#);
    // Synthetic "Created" event at the tail.
    let created_event = format!(
        r#"<div class="timeline-event">
  <div class="what">Appointment created</div>
  <div class="when">{abs} · {rel}</div>
</div>"#,
        abs = escape_html(&appt.created_at.format("%Y-%m-%d %H:%M").to_string()),
        rel = escape_html(&relative_past(appt.created_at)),
    );
    for e in events {
        let cls = if e.to_status == "cancelled" { "cancelled" } else { "" };
        out.push_str(&format!(
            r#"<div class="timeline-event {cls}">
  <div class="what"><strong>{from}</strong><span class="arrow">→</span><strong>{to}</strong></div>
  <div class="when">{abs} · {rel}</div>
</div>"#,
            cls = cls,
            from = escape_html(&humanise_status(&e.from_status)),
            to = escape_html(&humanise_status(&e.to_status)),
            abs = escape_html(&e.created_at.format("%Y-%m-%d %H:%M").to_string()),
            rel = escape_html(&relative_past(e.created_at)),
        ));
    }
    out.push_str(&created_event);
    out.push_str("</div>");
    out
}

const DETAIL_JS: &str = r#"
(function () {
  var tokenEl = document.querySelector('meta[name="api-token"]');
  var token = tokenEl ? tokenEl.getAttribute('content') : '';
  var banner = document.getElementById('banner');
  var bannerText = document.getElementById('banner-text');
  function showError(msg) { bannerText.textContent = msg; banner.hidden = false; }
  document.addEventListener('click', function (e) {
    var btn = e.target && e.target.closest ? e.target.closest('[data-action]') : null;
    if (!btn) return;
    var action = btn.getAttribute('data-action');
    var id = btn.getAttribute('data-id');
    if (!action || !id) return;
    btn.disabled = true;
    fetch('/api/appointments/' + encodeURIComponent(id) + '/' + action, {
      method: 'POST',
      headers: { 'Authorization': 'Bearer ' + token, 'Content-Type': 'application/json' },
      body: '{}'
    }).then(function (r) {
      if (r.ok) { window.location.reload(); return; }
      r.text().then(function (t) { showError('Action failed (' + r.status + '): ' + t); btn.disabled = false; });
    }).catch(function (err) { showError('Network error: ' + err); btn.disabled = false; });
  });
})();
"#;
