//! Operational pages for the care app.
//!
//!   * `GET /ops/appointments`     — the working console, with
//!                                    query-string filter bar (`?q=`,
//!                                    `?status=`, `?sort=`).
//!   * `GET /ops/appointments/new` — the smart-widget create form.
//!
//! Both render through [`ui::render_shell`] so the shell, nav,
//! typography, and footer stay in lockstep. No page-level
//! stylesheet survives here.

use chrono::Utc;
use rustio_core::auth::{session, user, CsrfToken, User, SESSION_COOKIE};
use rustio_core::{html, Db, Error, Model, Request, Response, Router};
use std::collections::HashMap;

use crate::apps::care::models::Appointment;
use crate::apps::people::models::{Department, Doctor, Patient};
use crate::apps::ui::{
    escape_html, forbidden_page, format_dt_smart, humanise_status, pluralise, redirect,
    render_shell, Actor, Nav, ICON_ALERT, ICON_INBOX, ICON_PLUS, ICON_SEARCH,
};

// ═══════════════════════════════════════════════════════════════
// Route registration
// ═══════════════════════════════════════════════════════════════

pub fn register(router: Router, db: &Db) -> Router {
    let list_db = db.clone();
    let new_db = db.clone();
    router
        .get("/ops/appointments", move |req, _params| {
            let db = list_db.clone();
            async move { ops_appointments(&db, req).await }
        })
        .get("/ops/appointments/new", move |req, _params| {
            let db = new_db.clone();
            async move { ops_new_appointment(&db, req).await }
        })
}

// ═══════════════════════════════════════════════════════════════
// Auth helper — cookie-based
// ═══════════════════════════════════════════════════════════════

struct Session {
    user: User,
    bearer: String,
    csrf: String,
}

async fn load_actor(db: &Db, req: &Request) -> Result<Option<Session>, Error> {
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
    // `authenticate` middleware put a CsrfToken in the context; we
    // prefer that. Fallback to the session row's csrf_token so this
    // also works if called outside the middleware.
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

// ═══════════════════════════════════════════════════════════════
// /ops/appointments — list console
// ═══════════════════════════════════════════════════════════════

async fn ops_appointments(db: &Db, req: Request) -> Result<Response, Error> {
    let Some(sess) = load_actor(db, &req).await? else {
        return Ok(redirect("/admin/login?next=/ops/appointments"));
    };

    // ─── Filter state from query string ────────────────────
    let query = req.query();
    let q = query.get("q").unwrap_or("").trim().to_string();
    let status_filter = query.get("status").unwrap_or("").trim().to_string();
    let sort = query.get("sort").unwrap_or("scheduled_desc").to_string();

    // ─── Data ──────────────────────────────────────────────
    let mut appointments = Appointment::all(db).await?;
    let patients = Patient::all(db).await?;
    let doctors = Doctor::all(db).await?;

    let patient_names: HashMap<i64, String> =
        patients.into_iter().map(|p| (p.id, p.full_name)).collect();
    let doctor_names: HashMap<i64, String> =
        doctors.into_iter().map(|d| (d.id, d.full_name)).collect();

    let total = appointments.len();

    // ─── Filter ────────────────────────────────────────────
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
            let doc = doctor_names
                .get(&a.doctor_id)
                .map(|s| s.to_lowercase())
                .unwrap_or_default();
            pat.contains(&needle)
                || doc.contains(&needle)
                || a.reason.to_lowercase().contains(&needle)
                || a.id.to_string() == needle
        });
    }

    // ─── Sort ──────────────────────────────────────────────
    match sort.as_str() {
        "scheduled_asc" => appointments.sort_by_key(|a| a.scheduled_at),
        "scheduled_desc" => {
            appointments.sort_by(|a, b| b.scheduled_at.cmp(&a.scheduled_at))
        }
        "id_asc" => appointments.sort_by_key(|a| a.id),
        "id_desc" => appointments.sort_by(|a, b| b.id.cmp(&a.id)),
        _ => appointments.sort_by(|a, b| b.scheduled_at.cmp(&a.scheduled_at)),
    }

    let filtered = appointments.len();
    let can_create = can_create(&sess.user);
    let rows: String = appointments
        .iter()
        .map(|a| render_row(a, &patient_names, &doctor_names, &sess.user.role))
        .collect();

    let actor = Actor {
        user: &sess.user,
        bearer: &sess.bearer,
        csrf: &sess.csrf,
    };
    let inner = render_list_body(
        total, filtered, &rows, can_create, &q, &status_filter, &sort,
    );
    let body = render_shell("Appointments", &actor, Nav::Appointments, &inner, LIST_JS);
    Ok(html(body))
}

fn render_list_body(
    total: usize,
    shown: usize,
    rows_html: &str,
    can_create: bool,
    q: &str,
    status_filter: &str,
    sort: &str,
) -> String {
    let count_label = if total == shown {
        pluralise(total, "record")
    } else {
        format!("{shown} of {total}")
    };

    let new_btn = if can_create {
        format!(
            r#"<a class="btn btn-primary" href="/ops/appointments/new">{icon}<span>New appointment</span></a>"#,
            icon = ICON_PLUS,
        )
    } else {
        String::new()
    };

    let active_chips = render_active_chips(q, status_filter);

    let table_body = if shown == 0 {
        render_empty_state(total == 0)
    } else {
        rows_html.to_string()
    };

    format!(
        r##"<div class="page-head">
  <h1>Appointments</h1>
  <span class="count">{count}</span>
  <div class="spacer"></div>
  {new_btn}
</div>
<div id="banner" role="alert" class="banner" hidden></div>
<section class="card">
  <form class="filter-bar" method="get" action="/ops/appointments" role="search">
    <div class="search">
      {search_icon}
      <input type="search" name="q" value="{q_val}" placeholder="Search patient, doctor, or reason…">
    </div>
    <select name="status" aria-label="Filter by status">
      <option value=""{sel_all}>All statuses</option>
      <option value="scheduled"{sel_sched}>Scheduled</option>
      <option value="confirmed"{sel_conf}>Confirmed</option>
      <option value="in_progress"{sel_prog}>In progress</option>
      <option value="completed"{sel_comp}>Completed</option>
      <option value="cancelled"{sel_canc}>Cancelled</option>
    </select>
    <select name="sort" aria-label="Sort">
      <option value="scheduled_desc"{so_sd}>Newest first</option>
      <option value="scheduled_asc"{so_sa}>Oldest first</option>
      <option value="id_desc"{so_id}>Created (new → old)</option>
      <option value="id_asc"{so_ia}>Created (old → new)</option>
    </select>
    <button class="btn" type="submit">Apply</button>
    {chips}
  </form>
  <div style="overflow-x:auto">
  <table class="grid">
    <thead>
      <tr>
        <th style="width:54px">ID</th>
        <th>Patient</th>
        <th>Doctor</th>
        <th>Scheduled</th>
        <th style="width:140px">Status</th>
        <th style="width:240px">Actions</th>
      </tr>
    </thead>
    <tbody>
      {rows}
    </tbody>
  </table>
  </div>
</section>"##,
        count = escape_html(&count_label),
        new_btn = new_btn,
        search_icon = ICON_SEARCH,
        q_val = escape_html(q),
        sel_all = if status_filter.is_empty() { " selected" } else { "" },
        sel_sched = if status_filter == "scheduled" { " selected" } else { "" },
        sel_conf = if status_filter == "confirmed" { " selected" } else { "" },
        sel_prog = if status_filter == "in_progress" { " selected" } else { "" },
        sel_comp = if status_filter == "completed" { " selected" } else { "" },
        sel_canc = if status_filter == "cancelled" { " selected" } else { "" },
        so_sd = if sort == "scheduled_desc" { " selected" } else { "" },
        so_sa = if sort == "scheduled_asc"  { " selected" } else { "" },
        so_id = if sort == "id_desc"        { " selected" } else { "" },
        so_ia = if sort == "id_asc"         { " selected" } else { "" },
        chips = active_chips,
        rows = table_body,
    )
}

fn render_active_chips(q: &str, status: &str) -> String {
    let mut parts: Vec<String> = Vec::new();
    if !q.is_empty() {
        parts.push(format!(
            r#"<span class="filter-chip">Search: &quot;{v}&quot; <a href="?{href}" aria-label="Clear search">×</a></span>"#,
            v = escape_html(q),
            href = if status.is_empty() {
                String::new()
            } else {
                format!("status={}", escape_html(status))
            },
        ));
    }
    if !status.is_empty() {
        parts.push(format!(
            r#"<span class="filter-chip">Status: {v} <a href="?{href}" aria-label="Clear status">×</a></span>"#,
            v = escape_html(&humanise_status(status)),
            href = if q.is_empty() {
                String::new()
            } else {
                format!("q={}", escape_html(q))
            },
        ));
    }
    if parts.is_empty() {
        return String::new();
    }
    format!(
        r#"<div class="filter-chips">{chips}<a class="filter-chip" href="/ops/appointments" style="background:transparent;color:var(--text-mute)">Clear all</a></div>"#,
        chips = parts.join(""),
    )
}

fn render_empty_state(completely_empty: bool) -> String {
    if completely_empty {
        format!(
            r#"<tr><td class="empty" colspan="6">
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
            r#"<tr><td class="empty" colspan="6">
  <span class="glyph">{icon}</span>
  <h3>No matches</h3>
  <p>Try a different search or clear the filters.</p>
  <a class="btn" href="/ops/appointments">Clear filters</a>
</td></tr>"#,
            icon = ICON_SEARCH,
        )
    }
}

fn render_row(
    appt: &Appointment,
    patient_names: &HashMap<i64, String>,
    doctor_names: &HashMap<i64, String>,
    role: &str,
) -> String {
    let patient = patient_names
        .get(&appt.patient_id)
        .cloned()
        .unwrap_or_else(|| format!("#{}", appt.patient_id));
    let doctor = doctor_names
        .get(&appt.doctor_id)
        .cloned()
        .unwrap_or_else(|| format!("#{}", appt.doctor_id));
    let status = appt.status.as_str();
    let actions = render_actions(appt.id, status, role);

    let (main_line, relative) = format_dt_smart(appt.scheduled_at);
    let today_flag = matches!(relative.as_deref(), Some("Today"));
    let relative_html = match relative {
        Some(r) if today_flag => format!(r#"<span class="today-flag">{}</span>"#, escape_html(&r)),
        Some(r) => format!(r#"<span class="relative">{}</span>"#, escape_html(&r)),
        None => String::new(),
    };

    format!(
        r#"<tr>
  <td class="id-col">#{id}</td>
  <td>{patient}</td>
  <td>{doctor}</td>
  <td class="date-col">{relative_html}{main_line}</td>
  <td><span class="pill pill-{status_raw}">{status_label}</span></td>
  <td>{actions}</td>
</tr>
"#,
        id = appt.id,
        patient = escape_html(&patient),
        doctor = escape_html(&doctor),
        main_line = escape_html(&main_line),
        relative_html = relative_html,
        status_raw = escape_html(status),
        status_label = escape_html(&humanise_status(status)),
        actions = actions,
    )
}

fn render_actions(id: i64, status: &str, role: &str) -> String {
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
        let classes = if *danger {
            "btn btn-sm btn-danger"
        } else {
            "btn btn-sm"
        };
        out.push_str(&format!(
            r#"<button class="{classes}" data-action="{action}" data-id="{id}">{label}</button> "#,
        ));
    }
    if out.is_empty() {
        out.push_str(r#"<span style="color:var(--text-mute)">—</span>"#);
    }
    out
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

const LIST_JS: &str = r#"
(function () {
  var tokenEl = document.querySelector('meta[name="api-token"]');
  var token = tokenEl ? tokenEl.getAttribute('content') : '';
  var banner = document.getElementById('banner');
  function showError(msg) { banner.textContent = msg; banner.hidden = false; }
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
// /ops/appointments/new — smart create form
// ═══════════════════════════════════════════════════════════════

async fn ops_new_appointment(db: &Db, req: Request) -> Result<Response, Error> {
    let Some(sess) = load_actor(db, &req).await? else {
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
    let inner = render_new_form(&patients, &doctors, &departments, &min_dt, &max_dt);
    let body = render_shell(
        "New appointment",
        &actor,
        Nav::Appointments,
        &inner,
        NEW_FORM_JS,
    );
    Ok(html(body))
}

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
        r##"<div class="page-head">
  <h1>New appointment</h1>
  <div class="spacer"></div>
  <a class="btn btn-ghost" href="/ops/appointments">← Back to list</a>
</div>
<div id="banner" class="banner" role="alert" hidden>{alert_icon}<span id="banner-text"></span></div>
<section class="card">
  <form id="new-form" class="form" novalidate>
    <fieldset class="group">
      <legend>Who</legend>

      <label class="field" for="patient_id">Patient <span class="req">*</span></label>
      <div>
        <select class="input" id="patient_id" name="patient_id" required aria-describedby="patient_id-err">
          <option value="">Select a patient…</option>
          {patients}
        </select>
        <div class="field-error" id="patient_id-err"></div>
      </div>

      <label class="field" for="doctor_id">Doctor <span class="req">*</span></label>
      <div>
        <select class="input" id="doctor_id" name="doctor_id" required aria-describedby="doctor_id-err">
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

      <label class="field" for="scheduled_at">Scheduled at <span class="req">*</span></label>
      <div>
        <input class="input" id="scheduled_at" name="scheduled_at" type="datetime-local"
               min="{min_dt}" max="{max_dt}" required aria-describedby="scheduled_at-err">
        <div class="field-hint">UTC; past times are blocked.</div>
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

      <div class="summary-box" id="summary" hidden></div>
    </fieldset>

    <div class="form-footer">
      <button id="submit-btn" class="btn btn-primary" type="submit">Create appointment</button>
      <a class="btn btn-ghost" href="/ops/appointments">Cancel</a>
      <span class="shortcut">Submit with <kbd>Ctrl</kbd>+<kbd>Enter</kbd> or <kbd>⌘</kbd>+<kbd>Enter</kbd></span>
    </div>
  </form>
</section>"##,
        alert_icon = ICON_ALERT,
        patients = patient_opts,
        doctors = doctor_opts,
        depts = dept_opts,
        min_dt = escape_html(min_dt),
        max_dt = escape_html(max_dt),
    )
}

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
  var summary = document.getElementById('summary');

  function showBanner(msg) {
    bannerText.textContent = msg;
    banner.hidden = false;
    banner.scrollIntoView({ block: 'nearest' });
  }
  function clearBanner() { banner.hidden = true; bannerText.textContent = ''; }

  function setFieldError(id, msg) {
    var input = document.getElementById(id);
    var err = document.getElementById(id + '-err');
    if (msg) {
      if (input) input.classList.add('invalid');
      if (err) err.textContent = msg;
    } else {
      if (input) input.classList.remove('invalid');
      if (err) err.textContent = '';
    }
  }
  function clearAllErrors() {
    ['patient_id', 'doctor_id', 'scheduled_at'].forEach(function (id) { setFieldError(id, ''); });
  }

  // ── Custom-duration reveal ─────────────────────────────
  function syncCustom() {
    if (durPreset.value === 'custom') {
      durCustom.hidden = false;
      durCustom.required = true;
      setTimeout(function () { durCustom.focus(); }, 0);
    } else {
      durCustom.hidden = true;
      durCustom.required = false;
    }
  }
  durPreset.addEventListener('change', syncCustom);
  syncCustom();

  // ── Live character counters ────────────────────────────
  function wireCounter(inputId, counterId, max) {
    var inp = document.getElementById(inputId);
    var ctr = document.getElementById(counterId);
    function refresh() {
      var n = inp.value.length;
      ctr.textContent = n + ' / ' + max;
      ctr.classList.toggle('over', n > max);
    }
    inp.addEventListener('input', refresh);
    refresh();
  }
  wireCounter('reason', 'reason-counter', 500);
  wireCounter('notes', 'notes-counter', 1000);

  // ── Summary preview ────────────────────────────────────
  function refreshSummary() {
    var pSel = document.getElementById('patient_id');
    var dSel = document.getElementById('doctor_id');
    var tVal = document.getElementById('scheduled_at').value;
    if (!pSel.value || !dSel.value || !tVal) {
      summary.hidden = true;
      return;
    }
    var p = pSel.options[pSel.selectedIndex].textContent;
    var d = dSel.options[dSel.selectedIndex].textContent;
    summary.innerHTML = 'Booking <strong>' + escapeHtml(p) + '</strong> with <strong>' +
      escapeHtml(d) + '</strong> on <strong>' + escapeHtml(tVal.replace('T', ' ')) + ' UTC</strong>.';
    summary.hidden = false;
  }
  function escapeHtml(s) { var div = document.createElement('div'); div.textContent = s; return div.innerHTML; }
  ['patient_id', 'doctor_id', 'scheduled_at'].forEach(function (id) {
    document.getElementById(id).addEventListener('change', refreshSummary);
    document.getElementById(id).addEventListener('input',  refreshSummary);
  });

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
      if (!n || n < 1) {
        setFieldError('scheduled_at', '');
        showBanner('Duration must be a positive whole number of minutes.');
        ok = false;
      }
    }
    return ok;
  }

  function submit() {
    clearBanner();
    if (!validate()) return;

    var durValue;
    if (durPreset.value === 'custom') {
      durValue = parseInt(durCustom.value, 10);
    } else {
      durValue = parseInt(durPreset.value, 10);
    }

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
    submitBtn.textContent = 'Creating…';
    fetch('/api/appointments', {
      method: 'POST',
      headers: {
        'Authorization': 'Bearer ' + token,
        'Content-Type': 'application/json'
      },
      body: JSON.stringify(body)
    }).then(function (r) {
      if (r.ok) { window.location.href = '/ops/appointments'; return; }
      r.text().then(function (t) {
        showBanner('Create failed (' + r.status + '): ' + t);
        submitBtn.disabled = false;
        submitBtn.textContent = 'Create appointment';
      });
    }).catch(function (err) {
      showBanner('Network error: ' + err);
      submitBtn.disabled = false;
      submitBtn.textContent = 'Create appointment';
    });
  }

  form.addEventListener('submit', function (e) { e.preventDefault(); submit(); });

  // Ctrl / ⌘ + Enter anywhere on the form → submit
  form.addEventListener('keydown', function (e) {
    if ((e.ctrlKey || e.metaKey) && e.key === 'Enter') {
      e.preventDefault();
      submit();
    }
  });
})();
"#;
