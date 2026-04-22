//! Operational console pages — list, new-appointment form, detail.
//!
//! All three render through [`ui::render_shell`] (dark sidebar + content
//! area, matching the reference mock-up the project is targeting).
//! Tailwind utility classes power the layout; no page-level
//! stylesheet survives here.

use chrono::Utc;
use rustio_core::auth::{session, user, CsrfToken, User, SESSION_COOKIE};
use rustio_core::router::Params;
use rustio_core::{html, Db, Error, Model, Request, Response, Router};
use std::collections::HashMap;

use crate::apps::care::models::{Appointment, AppointmentEvent};
use crate::apps::people::models::{Department, Doctor, Patient};
use crate::apps::ui::{
    escape_html, forbidden_page, humanise_status, iso_ymd, not_found_page, pluralise, redirect,
    relative_past, render_shell, short_time, status_dot_color, status_pill_classes, Actor, Nav,
    ShellOpts, ICON_ALERT_SM, ICON_ARROW_LEFT_SM, ICON_CHECK_SM, ICON_CHEVRON_DOWN_SM, ICON_CLOCK,
    ICON_COLUMNS, ICON_FILTER, ICON_INBOX_LG, ICON_PLUS_SM, ICON_SEARCH_LG, ICON_X_SM,
};

// ═══════════════════════════════════════════════════════════════
// Routes
// ═══════════════════════════════════════════════════════════════

pub fn register(router: Router, db: &Db) -> Router {
    let list_db = db.clone();
    let new_db = db.clone();
    let detail_db = db.clone();
    router
        .get("/ops/appointments", move |req, _p| {
            let db = list_db.clone();
            async move { ops_list(&db, req).await }
        })
        .get("/ops/appointments/new", move |req, _p| {
            let db = new_db.clone();
            async move { ops_new(&db, req).await }
        })
        .get("/ops/appointments/:id/edit", move |req, p| {
            let db = detail_db.clone();
            async move { ops_detail(&db, req, &p).await }
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
    let Some(token) = req.cookie(SESSION_COOKIE) else { return Ok(None); };
    let Some(sess) = session::find_valid(db, &token).await? else { return Ok(None); };
    let Some(actor) = user::find_by_id(db, sess.user_id).await? else { return Ok(None); };
    if !actor.is_active { return Ok(None); }
    let csrf = req
        .ctx()
        .get::<CsrfToken>()
        .map(|c| c.0.clone())
        .unwrap_or(sess.csrf_token);
    Ok(Some(Session { user: actor, bearer: token, csrf }))
}

fn role_may(role: &str, action: &str) -> bool {
    if role == "admin" { return true; }
    match (role, action) {
        ("receptionist", "confirm") | ("receptionist", "check-in") | ("receptionist", "cancel") => true,
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
    let doctor_filter = query
        .get("doctor_id")
        .and_then(|v| v.parse::<i64>().ok());
    let dept_filter = query
        .get("department_id")
        .and_then(|v| v.parse::<i64>().ok());
    let after_filter = query.get("after").unwrap_or("").trim().to_string();
    let sort = query.get("sort").unwrap_or("scheduled_asc").to_string();

    let mut appointments = Appointment::all(db).await?;
    let patients = Patient::all(db).await?;
    let doctors = Doctor::all(db).await?;
    let departments = Department::all(db).await?;

    let patient_names: HashMap<i64, String> =
        patients.iter().map(|p| (p.id, p.full_name.clone())).collect();
    let doctor_map: HashMap<i64, (String, String)> = doctors
        .iter()
        .map(|d| (d.id, (d.full_name.clone(), d.specialty.clone())))
        .collect();
    let dept_names: HashMap<i64, String> =
        departments.iter().map(|d| (d.id, d.name.clone())).collect();

    // Stats from the unfiltered set.
    let total_all = appointments.len();
    let active_all = appointments.iter().filter(|a| a.status != "cancelled" && a.status != "completed").count();
    let today_count = appointments
        .iter()
        .filter(|a| a.scheduled_at.date_naive() == Utc::now().date_naive())
        .count();
    let mut by_status: HashMap<String, usize> = HashMap::new();
    for a in &appointments {
        *by_status.entry(a.status.clone()).or_insert(0) += 1;
    }

    // Apply filters.
    if !status_filter.is_empty() {
        appointments.retain(|a| a.status == status_filter);
    }
    if let Some(did) = doctor_filter {
        appointments.retain(|a| a.doctor_id == did);
    }
    if let Some(dept_id) = dept_filter {
        appointments.retain(|a| a.department_id == Some(dept_id));
    }
    if !after_filter.is_empty() {
        if let Ok(d) = chrono::NaiveDate::parse_from_str(&after_filter, "%Y-%m-%d") {
            appointments.retain(|a| a.scheduled_at.date_naive() >= d);
        }
    }
    if !q.is_empty() {
        let needle = q.to_lowercase();
        appointments.retain(|a| {
            let pat = patient_names.get(&a.patient_id).map(|s| s.to_lowercase()).unwrap_or_default();
            let doc = doctor_map.get(&a.doctor_id).map(|(n, _)| n.to_lowercase()).unwrap_or_default();
            pat.contains(&needle)
                || doc.contains(&needle)
                || a.reason.to_lowercase().contains(&needle)
                || a.id.to_string() == needle
        });
    }

    // Sort.
    match sort.as_str() {
        "scheduled_desc" => appointments.sort_by(|a, b| b.scheduled_at.cmp(&a.scheduled_at)),
        "scheduled_asc" => appointments.sort_by_key(|a| a.scheduled_at),
        "id_desc" => appointments.sort_by(|a, b| b.id.cmp(&a.id)),
        "id_asc" => appointments.sort_by_key(|a| a.id),
        _ => appointments.sort_by_key(|a| a.scheduled_at),
    }

    let shown = appointments.len();
    let can_add = can_create(&sess.user);

    // ── Top controls (search + columns + primary action) ────
    let controls = render_controls(&q, can_add);

    // ── Filter grid ─────────────────────────────────────────
    let filter_grid = render_filter_grid(
        &status_filter,
        doctor_filter,
        dept_filter,
        &after_filter,
        &doctors,
        &departments,
    );

    // ── Active chips ────────────────────────────────────────
    let chips = render_chips(&q, &status_filter, doctor_filter, dept_filter, &after_filter, &doctor_map, &dept_names);

    // ── Table rows ─────────────────────────────────────────
    let rows = if shown == 0 {
        render_empty_row(total_all == 0)
    } else {
        appointments
            .iter()
            .map(|a| render_row(a, &patient_names, &doctor_map, &dept_names, &sess.user.role))
            .collect::<String>()
    };

    // ── Sort dropdown (bottom of toolbar) ──────────────────
    let sort_html = render_sort(&sort);

    let content = format!(
        r##"
<div id="banner" class="hidden bg-red-50 border border-red-200 text-red-700 px-4 py-2.5 rounded-lg mb-4 text-sm flex items-center gap-2">
  {alert}<span id="banner-text"></span>
</div>

<!-- Stats row -->
<div class="grid grid-cols-2 md:grid-cols-4 gap-4 mb-6">
  <div class="bg-white rounded-xl shadow-sm border border-slate-200 p-5">
    <p class="text-xs font-semibold text-slate-500 uppercase tracking-wide">Today</p>
    <p class="text-2xl font-bold text-slate-900 mt-1">{today}</p>
    <p class="text-xs text-slate-500 mt-1">{total_label}</p>
  </div>
  <div class="bg-white rounded-xl shadow-sm border border-slate-200 p-5 border-l-4 border-l-cyan-500">
    <p class="text-xs font-semibold text-slate-500 uppercase tracking-wide">Confirmed</p>
    <p class="text-2xl font-bold text-slate-900 mt-1">{conf}</p>
    <p class="text-xs text-slate-500 mt-1">ready to start</p>
  </div>
  <div class="bg-white rounded-xl shadow-sm border border-slate-200 p-5 border-l-4 border-l-amber-500">
    <p class="text-xs font-semibold text-slate-500 uppercase tracking-wide">In progress</p>
    <p class="text-2xl font-bold text-slate-900 mt-1">{prog}</p>
    <p class="text-xs text-slate-500 mt-1">active consultations</p>
  </div>
  <div class="bg-white rounded-xl shadow-sm border border-slate-200 p-5 border-l-4 border-l-emerald-500">
    <p class="text-xs font-semibold text-slate-500 uppercase tracking-wide">Completed</p>
    <p class="text-2xl font-bold text-slate-900 mt-1">{comp}</p>
    <p class="text-xs text-slate-500 mt-1">closed</p>
  </div>
</div>

<!-- Control card (search + columns + primary + filter grid) -->
<div class="bg-white rounded-xl shadow-sm border border-slate-200 mb-6">
  <form method="get" action="/ops/appointments" class="contents">
    {controls}
    {filter_grid}
  </form>
</div>

{chips}

<!-- Table card -->
<div class="bg-white rounded-xl shadow-sm border border-slate-200 overflow-hidden">
  <div class="overflow-x-auto custom-scrollbar">
    <table class="w-full text-sm text-left text-slate-600 whitespace-nowrap">
      <thead class="text-xs text-slate-500 uppercase bg-slate-50 border-b border-slate-200">
        <tr>
          <th class="px-6 py-3 font-semibold w-14">ID</th>
          <th class="px-6 py-3 font-semibold">Patient</th>
          <th class="px-6 py-3 font-semibold">Doctor</th>
          <th class="px-6 py-3 font-semibold">Department</th>
          <th class="px-6 py-3 font-semibold">Scheduled</th>
          <th class="px-6 py-3 font-semibold">Status</th>
          <th class="px-6 py-3 font-semibold text-right">Actions</th>
        </tr>
      </thead>
      <tbody class="divide-y divide-slate-100">
        {rows}
      </tbody>
    </table>
  </div>
  <div class="flex flex-col sm:flex-row items-start sm:items-center justify-between gap-3 p-4 border-t border-slate-200 bg-slate-50/50">
    <div class="text-sm text-slate-500">
      Showing <span class="font-semibold text-slate-700">{shown}</span> of <span class="font-semibold text-slate-700">{total_all}</span> appointments
      &middot; <span class="font-semibold text-slate-700">{active_all}</span> active
    </div>
    {sort_html}
  </div>
</div>
"##,
        alert = ICON_ALERT_SM,
        today = today_count,
        total_label = pluralise(total_all, "appointment"),
        conf = by_status.get("confirmed").copied().unwrap_or(0),
        prog = by_status.get("in_progress").copied().unwrap_or(0),
        comp = by_status.get("completed").copied().unwrap_or(0),
        controls = controls,
        filter_grid = filter_grid,
        chips = chips,
        rows = rows,
        shown = shown,
        total_all = total_all,
        active_all = active_all,
        sort_html = sort_html,
    );

    let actor = Actor {
        user: &sess.user,
        bearer: &sess.bearer,
        csrf: &sess.csrf,
    };
    let opts = ShellOpts {
        header_title: "Appointment Center",
        header_badge: &format!("{} active", active_all),
    };
    Ok(html(render_shell(
        "Appointments",
        &actor,
        Nav::Appointments,
        &opts,
        &content,
        LIST_JS,
    )))
}

fn render_controls(q: &str, can_add: bool) -> String {
    let primary = if can_add {
        format!(
            r#"<a href="/ops/appointments/new" class="inline-flex items-center gap-2 bg-teal-600 text-white px-4 py-2.5 rounded-lg text-sm font-semibold hover:bg-teal-700 transition-colors shadow-sm">{icon}<span>New Appointment</span></a>"#,
            icon = ICON_PLUS_SM,
        )
    } else {
        String::new()
    };

    format!(
        r##"<div class="p-4 border-b border-slate-100 flex flex-col lg:flex-row lg:items-center justify-between gap-3">
  <div class="flex items-stretch gap-2 w-full lg:w-3/5">
    <div class="relative flex-1">
      <div class="absolute inset-y-0 left-0 pl-3.5 flex items-center pointer-events-none text-slate-400">{search_icon}</div>
      <input type="search" name="q" value="{q}"
             placeholder="Search by patient, doctor, reason, or ID…"
             class="w-full pl-11 pr-4 py-2.5 bg-white border border-slate-300 rounded-lg
                    focus:outline-none focus:ring-2 focus:ring-teal-500 focus:border-teal-500
                    text-sm shadow-sm transition-shadow">
    </div>
    <button type="submit" class="inline-flex items-center gap-1.5 bg-slate-900 text-white px-5 py-2.5 rounded-lg text-sm font-medium hover:bg-slate-800 transition-colors shadow-sm">
      {search_icon_sm}<span>Search</span>
    </button>
  </div>

  <div class="flex flex-wrap items-center gap-2">
    <details class="relative">
      <summary class="flex items-center gap-2 bg-white text-slate-700 px-4 py-2.5 rounded-lg text-sm font-medium border border-slate-300 hover:bg-slate-50 transition-colors shadow-sm cursor-pointer">
        <span class="text-slate-500">{cols_icon}</span>
        <span>Columns</span>
        <span class="text-slate-400">{chev}</span>
      </summary>
      <div class="absolute right-0 mt-2 w-52 bg-white border border-slate-200 rounded-lg shadow-xl p-3 z-20">
        <label class="flex items-center gap-2 text-sm text-slate-700 cursor-pointer mb-1.5">
          <input type="checkbox" data-col-toggle="patient-sub" checked class="rounded text-teal-600 focus:ring-teal-500 border-slate-300">
          <span>Patient reason</span>
        </label>
        <label class="flex items-center gap-2 text-sm text-slate-700 cursor-pointer mb-1.5">
          <input type="checkbox" data-col-toggle="doctor-sub" checked class="rounded text-teal-600 focus:ring-teal-500 border-slate-300">
          <span>Doctor specialty</span>
        </label>
        <label class="flex items-center gap-2 text-sm text-slate-700 cursor-pointer mb-1.5">
          <input type="checkbox" data-col-toggle="dept-badge" checked class="rounded text-teal-600 focus:ring-teal-500 border-slate-300">
          <span>Department badge</span>
        </label>
        <label class="flex items-center gap-2 text-sm text-slate-700 cursor-pointer">
          <input type="checkbox" data-col-toggle="date-time" checked class="rounded text-teal-600 focus:ring-teal-500 border-slate-300">
          <span>Time row</span>
        </label>
      </div>
    </details>
    {primary}
  </div>
</div>"##,
        search_icon = ICON_SEARCH_LG,
        search_icon_sm = ICON_SEARCH_LG,
        q = escape_html(q),
        cols_icon = ICON_COLUMNS,
        chev = ICON_CHEVRON_DOWN_SM,
        primary = primary,
    )
}

fn render_filter_grid(
    status: &str,
    doctor_id: Option<i64>,
    dept_id: Option<i64>,
    after: &str,
    doctors: &[Doctor],
    departments: &[Department],
) -> String {
    let status_opts = format!(
        r#"<option value="">All statuses</option>
<option value="scheduled"{s1}>Scheduled</option>
<option value="confirmed"{s2}>Confirmed</option>
<option value="in_progress"{s3}>In progress</option>
<option value="completed"{s4}>Completed</option>
<option value="cancelled"{s5}>Cancelled</option>"#,
        s1 = if status == "scheduled" { " selected" } else { "" },
        s2 = if status == "confirmed" { " selected" } else { "" },
        s3 = if status == "in_progress" { " selected" } else { "" },
        s4 = if status == "completed" { " selected" } else { "" },
        s5 = if status == "cancelled" { " selected" } else { "" },
    );

    let doctor_opts = {
        let mut s = String::from(r#"<option value="">Any doctor</option>"#);
        for d in doctors.iter().filter(|d| d.is_active) {
            let sel = if Some(d.id) == doctor_id { " selected" } else { "" };
            s.push_str(&format!(
                r#"<option value="{id}"{sel}>{name}</option>"#,
                id = d.id,
                sel = sel,
                name = escape_html(&d.full_name),
            ));
        }
        s
    };
    let dept_opts = {
        let mut s = String::from(r#"<option value="">Any department</option>"#);
        for d in departments.iter().filter(|d| d.is_active) {
            let sel = if Some(d.id) == dept_id { " selected" } else { "" };
            s.push_str(&format!(
                r#"<option value="{id}"{sel}>{name}</option>"#,
                id = d.id,
                sel = sel,
                name = escape_html(&d.name),
            ));
        }
        s
    };

    format!(
        r##"<div class="p-4 bg-slate-50/60 rounded-b-xl grid grid-cols-1 sm:grid-cols-2 xl:grid-cols-5 gap-3">
  <div>
    <label class="block text-xs font-semibold text-slate-500 uppercase tracking-wide mb-1.5">Status</label>
    <select name="status" class="w-full border border-slate-300 text-slate-700 text-sm rounded-lg focus:ring-teal-500 focus:border-teal-500 block p-2 bg-white shadow-sm">
      {status_opts}
    </select>
  </div>
  <div>
    <label class="block text-xs font-semibold text-slate-500 uppercase tracking-wide mb-1.5">Doctor</label>
    <select name="doctor_id" class="w-full border border-slate-300 text-slate-700 text-sm rounded-lg focus:ring-teal-500 focus:border-teal-500 block p-2 bg-white shadow-sm">
      {doctor_opts}
    </select>
  </div>
  <div>
    <label class="block text-xs font-semibold text-slate-500 uppercase tracking-wide mb-1.5">Department</label>
    <select name="department_id" class="w-full border border-slate-300 text-slate-700 text-sm rounded-lg focus:ring-teal-500 focus:border-teal-500 block p-2 bg-white shadow-sm">
      {dept_opts}
    </select>
  </div>
  <div>
    <label class="block text-xs font-semibold text-slate-500 uppercase tracking-wide mb-1.5">Scheduled after</label>
    <input type="date" name="after" value="{after}" class="w-full border border-slate-300 text-slate-700 text-sm rounded-lg focus:ring-teal-500 focus:border-teal-500 block p-2 bg-white shadow-sm">
  </div>
  <div class="flex items-end">
    <button type="submit" class="w-full inline-flex items-center justify-center gap-2 bg-teal-600 text-white px-4 py-2 rounded-lg text-sm font-medium hover:bg-teal-700 transition-colors shadow-sm">
      {filter_icon}<span>Apply Filters</span>
    </button>
  </div>
</div>"##,
        status_opts = status_opts,
        doctor_opts = doctor_opts,
        dept_opts = dept_opts,
        after = escape_html(after),
        filter_icon = ICON_FILTER,
    )
}

fn render_chips(
    q: &str,
    status: &str,
    doctor_id: Option<i64>,
    dept_id: Option<i64>,
    after: &str,
    doctor_map: &HashMap<i64, (String, String)>,
    dept_names: &HashMap<i64, String>,
) -> String {
    let mut chips: Vec<String> = Vec::new();
    let chip = |label: &str, val: &str, remove_href: &str| -> String {
        format!(
            r#"<span class="inline-flex items-center gap-1.5 px-3 py-1 rounded-full bg-teal-50 text-teal-700 text-xs font-medium border border-teal-100">
  <span>{label}: <strong>{val}</strong></span>
  <a href="{href}" class="text-teal-500 hover:text-teal-700" aria-label="Remove {label} filter">{xicon}</a>
</span>"#,
            label = escape_html(label),
            val = escape_html(val),
            href = escape_html(remove_href),
            xicon = ICON_X_SM,
        )
    };

    // Base query rebuilder (everything except one key)
    let all = [
        ("q", q.to_string()),
        ("status", status.to_string()),
        ("doctor_id", doctor_id.map(|i| i.to_string()).unwrap_or_default()),
        ("department_id", dept_id.map(|i| i.to_string()).unwrap_or_default()),
        ("after", after.to_string()),
    ];
    let build = |exclude: &str| -> String {
        let parts: Vec<String> = all
            .iter()
            .filter(|(k, v)| *k != exclude && !v.is_empty())
            .map(|(k, v)| format!("{}={}", k, escape_html(v)))
            .collect();
        if parts.is_empty() {
            "/ops/appointments".to_string()
        } else {
            format!("/ops/appointments?{}", parts.join("&"))
        }
    };

    if !q.is_empty() { chips.push(chip("Search", q, &build("q"))); }
    if !status.is_empty() {
        chips.push(chip("Status", &humanise_status(status), &build("status")));
    }
    if let Some(did) = doctor_id {
        let name = doctor_map.get(&did).map(|(n, _)| n.as_str()).unwrap_or("?");
        chips.push(chip("Doctor", name, &build("doctor_id")));
    }
    if let Some(dept_id) = dept_id {
        let name = dept_names.get(&dept_id).map(|s| s.as_str()).unwrap_or("?");
        chips.push(chip("Department", name, &build("department_id")));
    }
    if !after.is_empty() { chips.push(chip("After", after, &build("after"))); }

    if chips.is_empty() { return String::new(); }
    format!(
        r#"<div class="flex flex-wrap items-center gap-2 mb-4">
  <span class="text-xs font-semibold text-slate-500 uppercase tracking-wide mr-1">Active filters:</span>
  {chips}
  <a href="/ops/appointments" class="text-xs text-slate-500 hover:text-slate-700 underline decoration-dotted ml-1">Clear all</a>
</div>"#,
        chips = chips.join(""),
    )
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
    let dept = a.department_id.and_then(|id| dept_names.get(&id).cloned());

    let reason_sub = if a.reason.is_empty() {
        format!(r#"<div class="text-xs text-slate-400 mt-0.5" data-col-sub="patient-sub">No reason given</div>"#)
    } else {
        format!(r#"<div class="text-xs text-slate-400 mt-0.5" data-col-sub="patient-sub">{}</div>"#, escape_html(&a.reason))
    };
    let spec_sub = if doc_specialty.is_empty() {
        String::new()
    } else {
        format!(r#"<div class="text-xs text-slate-400 mt-0.5" data-col-sub="doctor-sub">{}</div>"#, escape_html(&doc_specialty))
    };
    let dept_html = match dept {
        Some(name) => format!(
            r#"<span class="inline-flex items-center px-2.5 py-0.5 rounded-md text-xs font-medium bg-purple-50 text-purple-700 border border-purple-100" data-col-sub="dept-badge">{}</span>"#,
            escape_html(&name),
        ),
        None => r#"<span class="text-slate-400 text-xs">—</span>"#.to_string(),
    };

    let status = a.status.as_str();
    let pill_cls = status_pill_classes(status);
    let dot_cls = status_dot_color(status);

    let actions = render_row_actions(a.id, status, role);
    let manage_link = format!(
        r#"<a href="/ops/appointments/{id}/edit" class="text-teal-700 hover:text-teal-900 font-medium text-xs">Manage</a>"#,
        id = a.id,
    );

    format!(
        r##"<tr class="hover:bg-slate-50 transition-colors">
  <td class="px-6 py-4 font-mono text-xs text-slate-500">#{id}</td>
  <td class="px-6 py-4">
    <div class="font-medium text-slate-800">{patient}</div>
    {reason_sub}
  </td>
  <td class="px-6 py-4">
    <div class="font-medium text-slate-800">{doctor}</div>
    {spec_sub}
  </td>
  <td class="px-6 py-4">{dept_html}</td>
  <td class="px-6 py-4 text-slate-700">
    <div class="font-medium text-slate-700" data-col-sub="date-time">{date}</div>
    <div class="text-xs text-slate-400 mt-0.5" data-col-sub="date-time">{time} UTC</div>
  </td>
  <td class="px-6 py-4">
    <span class="inline-flex items-center gap-1.5 px-2.5 py-1 rounded-full text-xs font-medium border {pill}">
      <span class="w-1.5 h-1.5 rounded-full {dot}"></span> {label}
    </span>
  </td>
  <td class="px-6 py-4 text-right">
    <div class="inline-flex items-center gap-2">
      {actions}
      {manage}
    </div>
  </td>
</tr>"##,
        id = a.id,
        patient = escape_html(&patient),
        reason_sub = reason_sub,
        doctor = escape_html(&doc_name),
        spec_sub = spec_sub,
        dept_html = dept_html,
        date = escape_html(&iso_ymd(a.scheduled_at)),
        time = escape_html(&short_time(a.scheduled_at)),
        pill = pill_cls,
        dot = dot_cls,
        label = escape_html(&humanise_status(status)),
        actions = actions,
        manage = manage_link,
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
        if !role_may(role, action) { continue; }
        let cls = if *danger {
            "px-3 py-1 rounded-md text-xs font-medium text-red-600 bg-white border border-red-200 hover:bg-red-50 transition-colors"
        } else {
            "px-3 py-1 rounded-md text-xs font-medium text-emerald-700 bg-white border border-emerald-200 hover:bg-emerald-50 transition-colors"
        };
        out.push_str(&format!(
            r#"<button class="{cls}" data-action="{a}" data-id="{id}">{label}</button>"#,
            cls = cls, a = action, id = id, label = escape_html(label),
        ));
    }
    out
}

fn render_empty_row(completely_empty: bool) -> String {
    if completely_empty {
        format!(
            r#"<tr><td colspan="7" class="px-6 py-16 text-center">
  <div class="mx-auto w-14 h-14 rounded-full bg-teal-50 text-teal-600 grid place-items-center mb-3">{icon}</div>
  <h3 class="text-base font-semibold text-slate-700 mb-1">No appointments yet</h3>
  <p class="text-sm text-slate-500 mb-4">Create the first appointment to start the clinic workflow.</p>
  <a href="/ops/appointments/new" class="inline-flex items-center gap-2 bg-teal-600 text-white px-4 py-2 rounded-lg text-sm font-semibold hover:bg-teal-700 transition-colors shadow-sm">{plus}<span>New Appointment</span></a>
</td></tr>"#,
            icon = ICON_INBOX_LG,
            plus = ICON_PLUS_SM,
        )
    } else {
        format!(
            r#"<tr><td colspan="7" class="px-6 py-16 text-center">
  <div class="mx-auto w-14 h-14 rounded-full bg-slate-100 text-slate-400 grid place-items-center mb-3">{icon}</div>
  <h3 class="text-base font-semibold text-slate-700 mb-1">No matches</h3>
  <p class="text-sm text-slate-500 mb-4">Try a different search or clear the filters.</p>
  <a href="/ops/appointments" class="inline-flex items-center gap-2 bg-white text-slate-700 border border-slate-300 px-4 py-2 rounded-lg text-sm font-medium hover:bg-slate-50">Clear filters</a>
</td></tr>"#,
            icon = ICON_SEARCH_LG,
        )
    }
}

fn render_sort(sort: &str) -> String {
    format!(
        r##"<form method="get" action="/ops/appointments" class="flex items-center gap-2">
  <label for="sort-field" class="text-xs font-semibold text-slate-500 uppercase tracking-wide">Sort</label>
  <select id="sort-field" name="sort" onchange="this.form.submit()"
          class="border border-slate-300 text-slate-700 text-xs rounded-md focus:ring-teal-500 focus:border-teal-500 block py-1 pl-2 pr-7 bg-white shadow-sm">
    <option value="scheduled_asc"{s1}>Chronological</option>
    <option value="scheduled_desc"{s2}>Newest first</option>
    <option value="id_desc"{s3}>Recently created</option>
    <option value="id_asc"{s4}>Oldest created</option>
  </select>
</form>"##,
        s1 = if sort == "scheduled_asc" { " selected" } else { "" },
        s2 = if sort == "scheduled_desc" { " selected" } else { "" },
        s3 = if sort == "id_desc" { " selected" } else { "" },
        s4 = if sort == "id_asc" { " selected" } else { "" },
    )
}

const LIST_JS: &str = r#"
(function () {
  var tokenEl = document.querySelector('meta[name="api-token"]');
  var token = tokenEl ? tokenEl.getAttribute('content') : '';
  var banner = document.getElementById('banner');
  var bannerText = document.getElementById('banner-text');
  function showError(msg) { bannerText.textContent = msg; banner.classList.remove('hidden'); }
  document.addEventListener('click', function (e) {
    var btn = e.target && e.target.closest ? e.target.closest('[data-action]') : null;
    if (!btn) return;
    var action = btn.getAttribute('data-action');
    var id = btn.getAttribute('data-id');
    if (!action || !id) return;
    btn.disabled = true;
    btn.classList.add('opacity-50', 'cursor-not-allowed');
    fetch('/api/appointments/' + encodeURIComponent(id) + '/' + action, {
      method: 'POST',
      headers: { 'Authorization': 'Bearer ' + token, 'Content-Type': 'application/json' },
      body: '{}'
    }).then(function (r) {
      if (r.ok) { window.location.reload(); return; }
      r.text().then(function (t) {
        showError('Action failed (' + r.status + '): ' + t);
        btn.disabled = false; btn.classList.remove('opacity-50', 'cursor-not-allowed');
      });
    }).catch(function (err) {
      showError('Network error: ' + err);
      btn.disabled = false; btn.classList.remove('opacity-50', 'cursor-not-allowed');
    });
  });

  // Columns toggle — checkbox flips display of `[data-col-sub="..."]`.
  document.addEventListener('change', function (e) {
    var cb = e.target && e.target.closest ? e.target.closest('[data-col-toggle]') : null;
    if (!cb) return;
    var key = cb.getAttribute('data-col-toggle');
    document.querySelectorAll('[data-col-sub="' + key + '"]').forEach(function (el) {
      el.style.display = cb.checked ? '' : 'none';
    });
  });
})();
"#;

// ═══════════════════════════════════════════════════════════════
// /ops/appointments/new — form page
// ═══════════════════════════════════════════════════════════════

async fn ops_new(db: &Db, req: Request) -> Result<Response, Error> {
    let Some(sess) = load_session(db, &req).await? else {
        return Ok(redirect("/admin/login?next=/ops/appointments/new"));
    };
    let actor = Actor { user: &sess.user, bearer: &sess.bearer, csrf: &sess.csrf };
    if !can_create(&sess.user) {
        return Ok(forbidden_page(&actor, "Only receptionists and admins can create appointments."));
    }

    let patients = Patient::all(db).await?;
    let doctors = Doctor::all(db).await?;
    let departments = Department::all(db).await?;

    let min_dt = Utc::now().format("%Y-%m-%dT%H:%M").to_string();
    let max_dt = (Utc::now() + chrono::Duration::days(365 * 2))
        .format("%Y-%m-%dT%H:%M").to_string();

    let content = render_new_content(&patients, &doctors, &departments, &min_dt, &max_dt);
    let opts = ShellOpts {
        header_title: "New Appointment",
        header_badge: "",
    };
    Ok(html(render_shell(
        "New appointment",
        &actor,
        Nav::Appointments,
        &opts,
        &content,
        NEW_JS,
    )))
}

fn opt_list<'a, I>(iter: I) -> String
where I: Iterator<Item = (i64, &'a str)>,
{
    let mut s = String::new();
    for (id, label) in iter {
        s.push_str(&format!(r#"<option value="{id}">{l}</option>"#, id = id, l = escape_html(label)));
    }
    s
}

fn render_new_content(
    patients: &[Patient],
    doctors: &[Doctor],
    departments: &[Department],
    min_dt: &str,
    max_dt: &str,
) -> String {
    let patient_opts = opt_list(
        patients.iter().filter(|p| p.is_active).map(|p| (p.id, p.full_name.as_str())),
    );
    let doctor_opts = opt_list(
        doctors.iter().filter(|d| d.is_active).map(|d| (d.id, d.full_name.as_str())),
    );
    let dept_opts = format!(
        r#"<option value="">— none —</option>{rest}"#,
        rest = opt_list(
            departments.iter().filter(|d| d.is_active).map(|d| (d.id, d.name.as_str()))
        ),
    );

    format!(
        r##"
<!-- Back link -->
<div class="mb-4">
  <a href="/ops/appointments" class="inline-flex items-center gap-2 text-sm text-slate-500 hover:text-slate-800">
    {back}<span>Back to appointments</span>
  </a>
</div>

<div id="banner" class="hidden bg-red-50 border border-red-200 text-red-700 px-4 py-2.5 rounded-lg mb-4 text-sm flex items-center gap-2">
  {alert}<span id="banner-text"></span>
</div>

<div class="grid grid-cols-1 lg:grid-cols-[minmax(0,1fr)_340px] gap-6">
  <!-- Form card -->
  <div class="bg-white rounded-xl shadow-sm border border-slate-200">
    <div class="px-6 py-4 border-b border-slate-100">
      <h3 class="text-base font-semibold text-slate-800">Visit details</h3>
      <p class="text-sm text-slate-500 mt-0.5">Fill in the patient, doctor, schedule, and optional clinical notes.</p>
    </div>
    <form id="new-form" class="p-6 space-y-6" novalidate>

      <!-- Section: Who -->
      <section>
        <p class="text-xs font-bold text-teal-700 uppercase tracking-wider mb-3">Who</p>
        <div class="grid grid-cols-1 md:grid-cols-2 gap-5">
          <div>
            <label for="patient_id" class="block text-sm font-medium text-slate-700 mb-1">Patient <span class="text-red-500">*</span></label>
            <select id="patient_id" name="patient_id" required
                    class="w-full border border-slate-300 text-slate-700 text-sm rounded-lg focus:ring-2 focus:ring-teal-500 focus:border-teal-500 p-2.5 bg-white shadow-sm">
              <option value="">Select a patient…</option>
              {patients}
            </select>
            <div class="text-xs text-red-600 mt-1" id="patient_id-err"></div>
          </div>
          <div>
            <label for="doctor_id" class="block text-sm font-medium text-slate-700 mb-1">Doctor <span class="text-red-500">*</span></label>
            <select id="doctor_id" name="doctor_id" required
                    class="w-full border border-slate-300 text-slate-700 text-sm rounded-lg focus:ring-2 focus:ring-teal-500 focus:border-teal-500 p-2.5 bg-white shadow-sm">
              <option value="">Select a doctor…</option>
              {doctors}
            </select>
            <div class="text-xs text-red-600 mt-1" id="doctor_id-err"></div>
          </div>
          <div>
            <label for="department_id" class="block text-sm font-medium text-slate-700 mb-1">Department <span class="text-slate-400 font-normal text-xs">optional</span></label>
            <select id="department_id" name="department_id"
                    class="w-full border border-slate-300 text-slate-700 text-sm rounded-lg focus:ring-2 focus:ring-teal-500 focus:border-teal-500 p-2.5 bg-white shadow-sm">
              {depts}
            </select>
          </div>
        </div>
      </section>

      <!-- Section: When -->
      <section class="pt-4 border-t border-slate-100">
        <p class="text-xs font-bold text-teal-700 uppercase tracking-wider mb-3">When</p>
        <div class="grid grid-cols-1 md:grid-cols-2 gap-5">
          <div>
            <label for="scheduled_at" class="block text-sm font-medium text-slate-700 mb-1">Scheduled <span class="text-red-500">*</span></label>
            <input id="scheduled_at" name="scheduled_at" type="datetime-local"
                   min="{min_dt}" max="{max_dt}" required
                   class="w-full border border-slate-300 text-slate-700 text-sm rounded-lg focus:ring-2 focus:ring-teal-500 focus:border-teal-500 p-2.5 bg-white shadow-sm">
            <p class="text-xs text-slate-500 mt-1">Stored as UTC. Past times are blocked.</p>
            <div class="text-xs text-red-600 mt-1" id="scheduled_at-err"></div>
          </div>
          <div>
            <label for="duration_preset" class="block text-sm font-medium text-slate-700 mb-1">Duration <span class="text-red-500">*</span></label>
            <div class="flex gap-2">
              <select id="duration_preset" name="duration_preset" required
                      class="flex-1 border border-slate-300 text-slate-700 text-sm rounded-lg focus:ring-2 focus:ring-teal-500 focus:border-teal-500 p-2.5 bg-white shadow-sm">
                <option value="15">15 minutes</option>
                <option value="30" selected>30 minutes</option>
                <option value="45">45 minutes</option>
                <option value="60">1 hour</option>
                <option value="90">1 hour 30 min</option>
                <option value="120">2 hours</option>
                <option value="custom">Custom…</option>
              </select>
              <input id="duration_custom" name="duration_custom" type="number" inputmode="numeric"
                     min="1" max="1440" placeholder="Minutes" hidden
                     class="w-28 border border-slate-300 text-slate-700 text-sm rounded-lg focus:ring-2 focus:ring-teal-500 focus:border-teal-500 p-2.5 bg-white shadow-sm">
            </div>
          </div>
          <div>
            <label for="priority" class="block text-sm font-medium text-slate-700 mb-1">Priority <span class="text-red-500">*</span></label>
            <select id="priority" name="priority" required
                    class="w-full border border-slate-300 text-slate-700 text-sm rounded-lg focus:ring-2 focus:ring-teal-500 focus:border-teal-500 p-2.5 bg-white shadow-sm">
              <option value="1">Low</option>
              <option value="3">Normal</option>
              <option value="5" selected>Standard</option>
              <option value="7">High</option>
              <option value="10">Urgent</option>
            </select>
          </div>
        </div>
      </section>

      <!-- Section: Details -->
      <section class="pt-4 border-t border-slate-100">
        <p class="text-xs font-bold text-teal-700 uppercase tracking-wider mb-3">Details</p>
        <div>
          <label for="reason" class="block text-sm font-medium text-slate-700 mb-1">Reason <span class="text-slate-400 font-normal text-xs">optional</span></label>
          <textarea id="reason" name="reason" rows="2" maxlength="500"
                    placeholder="Brief reason for the visit"
                    class="w-full border border-slate-300 text-slate-700 text-sm rounded-lg focus:ring-2 focus:ring-teal-500 focus:border-teal-500 p-2.5 bg-white shadow-sm resize-y"></textarea>
          <div class="text-xs text-slate-400 text-right mt-1 tabular-nums" id="reason-counter">0 / 500</div>
        </div>
        <div class="mt-4">
          <label for="notes" class="block text-sm font-medium text-slate-700 mb-1">Internal notes <span class="text-slate-400 font-normal text-xs">optional</span></label>
          <textarea id="notes" name="notes" rows="3" maxlength="1000"
                    placeholder="Internal notes (not visible to patient)"
                    class="w-full border border-slate-300 text-slate-700 text-sm rounded-lg focus:ring-2 focus:ring-teal-500 focus:border-teal-500 p-2.5 bg-white shadow-sm resize-y"></textarea>
          <div class="text-xs text-slate-400 text-right mt-1 tabular-nums" id="notes-counter">0 / 1000</div>
        </div>
      </section>
    </form>
  </div>

  <!-- Preview card -->
  <aside class="lg:sticky lg:top-4 self-start bg-white rounded-xl shadow-sm border border-slate-200 p-5">
    <p class="text-xs font-bold text-slate-500 uppercase tracking-wider mb-3">Preview</p>
    <div id="pv-time" class="rounded-lg border border-dashed border-slate-300 bg-slate-50 text-center py-4 px-3 mb-4">
      <div class="text-lg font-semibold text-slate-400">—</div>
      <div class="text-xs text-slate-500 uppercase tracking-wide mt-0.5">pick a date &amp; time</div>
    </div>
    <dl class="space-y-2.5 text-sm">
      <div class="flex items-baseline gap-3">
        <dt class="w-20 text-xs font-semibold text-slate-500 uppercase tracking-wide">Patient</dt>
        <dd id="pv-patient" class="flex-1 text-slate-400 italic">— not selected —</dd>
      </div>
      <div class="flex items-baseline gap-3">
        <dt class="w-20 text-xs font-semibold text-slate-500 uppercase tracking-wide">Doctor</dt>
        <dd id="pv-doctor" class="flex-1 text-slate-400 italic">— not selected —</dd>
      </div>
      <div class="flex items-baseline gap-3">
        <dt class="w-20 text-xs font-semibold text-slate-500 uppercase tracking-wide">Dept</dt>
        <dd id="pv-dept" class="flex-1 text-slate-400">—</dd>
      </div>
      <div class="flex items-baseline gap-3">
        <dt class="w-20 text-xs font-semibold text-slate-500 uppercase tracking-wide">Duration</dt>
        <dd id="pv-duration" class="flex-1 text-slate-700">30 minutes</dd>
      </div>
      <div class="flex items-baseline gap-3">
        <dt class="w-20 text-xs font-semibold text-slate-500 uppercase tracking-wide">Priority</dt>
        <dd id="pv-priority" class="flex-1 text-slate-700">Standard</dd>
      </div>
    </dl>
    <div class="mt-5 pt-5 border-t border-slate-100 space-y-2">
      <button id="submit-btn" form="new-form" type="submit"
              class="w-full inline-flex items-center justify-center gap-2 bg-teal-600 text-white px-4 py-2.5 rounded-lg text-sm font-semibold hover:bg-teal-700 transition-colors shadow-sm">
        {check}<span>Create appointment</span>
      </button>
      <a href="/ops/appointments" class="w-full inline-flex items-center justify-center gap-2 bg-white text-slate-700 border border-slate-300 px-4 py-2.5 rounded-lg text-sm font-medium hover:bg-slate-50 transition-colors">Cancel</a>
      <p class="text-xs text-slate-500 text-center">Submit with <span class="kbd">Ctrl</span>+<span class="kbd">Enter</span></p>
    </div>
  </aside>
</div>
"##,
        back = ICON_ARROW_LEFT_SM,
        alert = ICON_ALERT_SM,
        patients = patient_opts,
        doctors = doctor_opts,
        depts = dept_opts,
        min_dt = escape_html(min_dt),
        max_dt = escape_html(max_dt),
        check = ICON_CHECK_SM,
    )
}

const NEW_JS: &str = r#"
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

  function showBanner(m) { bannerText.textContent = m; banner.classList.remove('hidden'); banner.scrollIntoView({ block: 'nearest' }); }
  function clearBanner() { banner.classList.add('hidden'); bannerText.textContent = ''; }

  function setFieldError(id, msg) {
    var inp = document.getElementById(id);
    var err = document.getElementById(id + '-err');
    if (msg) {
      if (inp) inp.classList.add('border-red-400', 'ring-red-300');
      if (err) err.textContent = msg;
    } else {
      if (inp) inp.classList.remove('border-red-400', 'ring-red-300');
      if (err) err.textContent = '';
    }
  }
  function clearErrors() { ['patient_id','doctor_id','scheduled_at'].forEach(function(i){ setFieldError(i,''); }); }

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

  function wireCounter(id, cid, max) {
    var inp = document.getElementById(id), ctr = document.getElementById(cid);
    function refresh() {
      var n = inp.value.length; ctr.textContent = n + ' / ' + max;
      ctr.classList.toggle('text-red-500', n > max);
      ctr.classList.toggle('font-semibold', n > max);
    }
    inp.addEventListener('input', refresh); refresh();
  }
  wireCounter('reason', 'reason-counter', 500);
  wireCounter('notes',  'notes-counter',  1000);

  function formatDur(m) {
    if (!m || m < 1) return '—';
    if (m < 60) return m + ' minutes';
    var h = Math.floor(m / 60), rem = m % 60;
    return rem === 0 ? h + ' hour' + (h > 1 ? 's' : '') : h + 'h ' + rem + 'm';
  }
  function esc(s) { var d = document.createElement('div'); d.textContent = s; return d.innerHTML; }

  function updatePreview() {
    var p = document.getElementById('patient_id');
    var d = document.getElementById('doctor_id');
    var dept = document.getElementById('department_id');
    var pr = document.getElementById('priority');
    var t = document.getElementById('scheduled_at').value;

    var pvP = document.getElementById('pv-patient');
    var pvD = document.getElementById('pv-doctor');
    var pvDept = document.getElementById('pv-dept');
    var pvDur = document.getElementById('pv-duration');
    var pvPr = document.getElementById('pv-priority');
    var pvTime = document.getElementById('pv-time');

    pvP.innerHTML = p.value
      ? '<span class="text-slate-800 font-medium">' + esc(p.options[p.selectedIndex].textContent) + '</span>'
      : '<span class="italic text-slate-400">— not selected —</span>';
    pvD.innerHTML = d.value
      ? '<span class="text-slate-800 font-medium">' + esc(d.options[d.selectedIndex].textContent) + '</span>'
      : '<span class="italic text-slate-400">— not selected —</span>';
    pvDept.innerHTML = dept.value
      ? esc(dept.options[dept.selectedIndex].textContent)
      : '<span class="text-slate-400">—</span>';
    pvPr.textContent = PRIORITIES[pr.value] || 'Standard';

    var mins = durPreset.value === 'custom' ? parseInt(durCustom.value, 10) : parseInt(durPreset.value, 10);
    pvDur.textContent = formatDur(mins);

    if (t) {
      var parts = t.split('T');
      var date = parts[0], time = (parts[1] || '00:00').slice(0, 5);
      pvTime.className = 'rounded-lg bg-gradient-to-br from-teal-500 to-teal-700 text-white text-center py-4 px-3 mb-4 shadow-sm';
      pvTime.innerHTML =
        '<div class="text-2xl font-bold tabular-nums leading-tight">' + esc(time) + '</div>' +
        '<div class="text-xs uppercase tracking-wider opacity-90 mt-1">' + esc(date) + ' UTC</div>';
    } else {
      pvTime.className = 'rounded-lg border border-dashed border-slate-300 bg-slate-50 text-center py-4 px-3 mb-4';
      pvTime.innerHTML =
        '<div class="text-lg font-semibold text-slate-400">—</div>' +
        '<div class="text-xs text-slate-500 uppercase tracking-wide mt-0.5">pick a date &amp; time</div>';
    }
  }
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
    clearErrors();
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
    submitBtn.classList.add('opacity-70', 'cursor-not-allowed');
    var lbl = submitBtn.querySelector('span');
    if (lbl) lbl.textContent = 'Creating…';
    fetch('/api/appointments', {
      method: 'POST',
      headers: { 'Authorization': 'Bearer ' + token, 'Content-Type': 'application/json' },
      body: JSON.stringify(body)
    }).then(function (r) {
      if (r.ok) { window.location.href = '/ops/appointments'; return; }
      r.text().then(function (t) {
        showBanner('Create failed (' + r.status + '): ' + t);
        submitBtn.disabled = false;
        submitBtn.classList.remove('opacity-70', 'cursor-not-allowed');
        if (lbl) lbl.textContent = 'Create appointment';
      });
    }).catch(function (err) {
      showBanner('Network error: ' + err);
      submitBtn.disabled = false;
      submitBtn.classList.remove('opacity-70', 'cursor-not-allowed');
      if (lbl) lbl.textContent = 'Create appointment';
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
        return Ok(redirect(&format!("/admin/login?next=/ops/appointments/{id}/edit")));
    };
    let actor = Actor { user: &sess.user, bearer: &sess.bearer, csrf: &sess.csrf };
    let Some(id_str) = params.get("id") else {
        return Ok(not_found_page(&actor, "Missing appointment id."));
    };
    let Ok(id) = id_str.parse::<i64>() else {
        return Ok(not_found_page(&actor, "That appointment id isn't a number."));
    };
    let Some(appt) = Appointment::find(db, id).await? else {
        return Ok(not_found_page(&actor, &format!("Appointment #{id} does not exist.")));
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

    let content = render_detail_content(id, &appt, patient.as_ref(), doctor.as_ref(), department.as_ref(), &events, &sess.user.role);
    let opts = ShellOpts {
        header_title: &format!("Appointment #{id}"),
        header_badge: &humanise_status(&appt.status),
    };
    Ok(html(render_shell(
        &format!("Appointment #{id}"),
        &actor,
        Nav::Appointments,
        &opts,
        &content,
        DETAIL_JS,
    )))
}

fn render_detail_content(
    id: i64,
    appt: &Appointment,
    patient: Option<&Patient>,
    doctor: Option<&Doctor>,
    department: Option<&Department>,
    events: &[AppointmentEvent],
    role: &str,
) -> String {
    let p_name = patient.map(|p| p.full_name.as_str()).unwrap_or("—");
    let p_contact = patient
        .map(|p| format!("{} · {}", p.phone, p.email))
        .unwrap_or_else(|| "—".to_string());
    let d_name = doctor.map(|d| d.full_name.as_str()).unwrap_or("—");
    let d_specialty = doctor.map(|d| d.specialty.as_str()).unwrap_or("");
    let dept = department.map(|d| d.name.as_str()).unwrap_or("—");

    let status = appt.status.as_str();
    let pill_cls = status_pill_classes(status);
    let dot_cls = status_dot_color(status);

    let actions = render_detail_hero_actions(id, status, role);

    // Timeline
    let timeline = if events.is_empty() {
        format!(
            r#"<div class="p-6 text-center">
  <div class="mx-auto w-12 h-12 rounded-full bg-slate-100 text-slate-400 grid place-items-center mb-2">{icon}</div>
  <p class="text-sm text-slate-500">No status transitions yet.</p>
  <p class="text-xs text-slate-400 mt-1">The audit trail fills as lifecycle actions are taken.</p>
</div>"#,
            icon = ICON_CLOCK,
        )
    } else {
        let mut items = String::from(r#"<ul class="p-5 space-y-0">"#);
        for e in events {
            let is_cancel = e.to_status == "cancelled";
            items.push_str(&format!(
                r#"<li class="relative pl-7 pb-5 border-l-2 border-slate-200 ml-1.5 last:pb-0 last:border-transparent">
  <span class="timeline-dot{cancel}"></span>
  <p class="text-sm text-slate-700"><strong>{from}</strong> <span class="text-slate-400 mx-1">→</span> <strong>{to}</strong></p>
  <p class="text-xs text-slate-400 mt-0.5">{abs} · {rel}</p>
</li>"#,
                cancel = if is_cancel { " cancelled" } else { "" },
                from = escape_html(&humanise_status(&e.from_status)),
                to = escape_html(&humanise_status(&e.to_status)),
                abs = escape_html(&e.created_at.format("%Y-%m-%d %H:%M").to_string()),
                rel = escape_html(&relative_past(e.created_at)),
            ));
        }
        items.push_str(&format!(
            r#"<li class="relative pl-7 ml-1.5">
  <span class="timeline-dot" style="background:#94a3b8"></span>
  <p class="text-sm text-slate-700"><strong>Created</strong></p>
  <p class="text-xs text-slate-400 mt-0.5">{abs} · {rel}</p>
</li>"#,
            abs = escape_html(&appt.created_at.format("%Y-%m-%d %H:%M").to_string()),
            rel = escape_html(&relative_past(appt.created_at)),
        ));
        items.push_str("</ul>");
        items
    };

    format!(
        r##"
<div class="mb-4 flex items-center justify-between">
  <a href="/ops/appointments" class="inline-flex items-center gap-2 text-sm text-slate-500 hover:text-slate-800">
    {back}<span>Back to appointments</span>
  </a>
  <div class="flex items-center gap-2">{actions}</div>
</div>

<div id="banner" class="hidden bg-red-50 border border-red-200 text-red-700 px-4 py-2.5 rounded-lg mb-4 text-sm flex items-center gap-2">
  {alert}<span id="banner-text"></span>
</div>

<div class="bg-amber-50 border border-amber-200 text-amber-800 px-4 py-2.5 rounded-lg mb-5 text-sm flex items-start gap-2">
  {alert}
  <span><strong>Read-only view.</strong> Patient, doctor, and schedule cannot be edited after booking. Cancel and create a new appointment to reschedule.</span>
</div>

<div class="grid grid-cols-1 lg:grid-cols-[minmax(0,1fr)_320px] gap-6">
  <!-- Information card -->
  <div class="bg-white rounded-xl shadow-sm border border-slate-200">
    <div class="px-5 py-3 border-b border-slate-100 bg-slate-50/60">
      <h3 class="text-xs font-bold text-slate-600 uppercase tracking-wider">Information</h3>
    </div>
    <div class="p-6 grid grid-cols-1 md:grid-cols-2 gap-5">
      <div>
        <p class="text-xs font-semibold text-slate-500 uppercase tracking-wide">Patient</p>
        <p class="text-base font-medium text-slate-800 mt-1">{p_name}</p>
        <p class="text-xs text-slate-500 mt-0.5">{p_contact}</p>
      </div>
      <div>
        <p class="text-xs font-semibold text-slate-500 uppercase tracking-wide">Doctor</p>
        <p class="text-base font-medium text-slate-800 mt-1">{d_name}</p>
        <p class="text-xs text-slate-500 mt-0.5">{d_spec}</p>
      </div>
      <div>
        <p class="text-xs font-semibold text-slate-500 uppercase tracking-wide">Department</p>
        <p class="text-base text-slate-700 mt-1">{dept}</p>
      </div>
      <div>
        <p class="text-xs font-semibold text-slate-500 uppercase tracking-wide">Status</p>
        <p class="mt-1"><span class="inline-flex items-center gap-1.5 px-2.5 py-1 rounded-full text-xs font-medium border {pill}">
          <span class="w-1.5 h-1.5 rounded-full {dot}"></span> {status_label}
        </span></p>
      </div>
      <div>
        <p class="text-xs font-semibold text-slate-500 uppercase tracking-wide">Scheduled</p>
        <p class="text-base text-slate-700 mt-1 tabular-nums">{scheduled}</p>
      </div>
      <div>
        <p class="text-xs font-semibold text-slate-500 uppercase tracking-wide">Duration</p>
        <p class="text-base text-slate-700 mt-1 tabular-nums">{duration} min</p>
      </div>
      <div>
        <p class="text-xs font-semibold text-slate-500 uppercase tracking-wide">Priority</p>
        <p class="text-base text-slate-700 mt-1">{priority}</p>
      </div>
      <div>
        <p class="text-xs font-semibold text-slate-500 uppercase tracking-wide">Active</p>
        <p class="text-base text-slate-700 mt-1">{active}</p>
      </div>
      <div class="md:col-span-2">
        <p class="text-xs font-semibold text-slate-500 uppercase tracking-wide">Reason</p>
        <p class="text-sm text-slate-700 mt-1 whitespace-pre-wrap">{reason}</p>
      </div>
      <div class="md:col-span-2">
        <p class="text-xs font-semibold text-slate-500 uppercase tracking-wide">Notes</p>
        <p class="text-sm text-slate-700 mt-1 whitespace-pre-wrap">{notes}</p>
      </div>
      <div class="md:col-span-2 text-xs text-slate-400 border-t border-slate-100 pt-3 mt-2">
        Created {created}
      </div>
    </div>
  </div>

  <!-- Activity card -->
  <div class="bg-white rounded-xl shadow-sm border border-slate-200">
    <div class="px-5 py-3 border-b border-slate-100 bg-slate-50/60 flex items-center justify-between">
      <h3 class="text-xs font-bold text-slate-600 uppercase tracking-wider">Activity</h3>
      <span class="text-xs font-semibold text-slate-500 bg-white px-2 py-0.5 rounded-full border border-slate-200">{ev_count}</span>
    </div>
    {timeline}
  </div>
</div>
"##,
        back = ICON_ARROW_LEFT_SM,
        actions = actions,
        alert = ICON_ALERT_SM,
        p_name = escape_html(p_name),
        p_contact = escape_html(&p_contact),
        d_name = escape_html(d_name),
        d_spec = escape_html(d_specialty),
        dept = escape_html(dept),
        pill = pill_cls,
        dot = dot_cls,
        status_label = escape_html(&humanise_status(status)),
        scheduled = escape_html(&appt.scheduled_at.format("%Y-%m-%d %H:%M UTC").to_string()),
        duration = appt.duration_minutes,
        priority = match appt.priority {
            1 => "1 · Low",
            3 => "3 · Normal",
            5 => "5 · Standard",
            7 => "7 · High",
            10 => "10 · Urgent",
            _ => "—",
        },
        active = if appt.is_active { "Yes" } else { "No" },
        reason = escape_html(if appt.reason.is_empty() { "—" } else { appt.reason.as_str() }),
        notes = escape_html(if appt.notes.is_empty() { "—" } else { appt.notes.as_str() }),
        created = escape_html(&appt.created_at.format("%Y-%m-%d %H:%M UTC").to_string()),
        ev_count = events.len(),
        timeline = timeline,
    )
}

fn render_detail_hero_actions(id: i64, status: &str, role: &str) -> String {
    let offered: &[(&str, &str, bool)] = match status {
        "scheduled" => &[("confirm", "Confirm", false), ("cancel", "Cancel", true)],
        "confirmed" => &[("check-in", "Check-in", false), ("cancel", "Cancel", true)],
        "in_progress" => &[("complete", "Complete", false), ("cancel", "Cancel", true)],
        _ => &[],
    };
    let mut out = String::new();
    for (action, label, danger) in offered {
        if !role_may(role, action) { continue; }
        let cls = if *danger {
            "inline-flex items-center gap-1.5 bg-white text-red-600 border border-red-200 px-3.5 py-2 rounded-lg text-sm font-medium hover:bg-red-50 transition-colors shadow-sm"
        } else {
            "inline-flex items-center gap-1.5 bg-teal-600 text-white px-3.5 py-2 rounded-lg text-sm font-medium hover:bg-teal-700 transition-colors shadow-sm"
        };
        out.push_str(&format!(
            r#"<button class="{cls}" data-action="{a}" data-id="{id}">{l}</button>"#,
            cls = cls, a = action, id = id, l = escape_html(label),
        ));
    }
    out
}

const DETAIL_JS: &str = r#"
(function () {
  var tokenEl = document.querySelector('meta[name="api-token"]');
  var token = tokenEl ? tokenEl.getAttribute('content') : '';
  var banner = document.getElementById('banner');
  var bannerText = document.getElementById('banner-text');
  function showError(m) { bannerText.textContent = m; banner.classList.remove('hidden'); }
  document.addEventListener('click', function (e) {
    var btn = e.target && e.target.closest ? e.target.closest('[data-action]') : null;
    if (!btn) return;
    var action = btn.getAttribute('data-action');
    var id = btn.getAttribute('data-id');
    if (!action || !id) return;
    btn.disabled = true;
    btn.classList.add('opacity-60','cursor-not-allowed');
    fetch('/api/appointments/' + encodeURIComponent(id) + '/' + action, {
      method: 'POST',
      headers: { 'Authorization': 'Bearer ' + token, 'Content-Type': 'application/json' },
      body: '{}'
    }).then(function (r) {
      if (r.ok) { window.location.reload(); return; }
      r.text().then(function (t) {
        showError('Action failed (' + r.status + '): ' + t);
        btn.disabled = false; btn.classList.remove('opacity-60','cursor-not-allowed');
      });
    }).catch(function (err) {
      showError('Network error: ' + err);
      btn.disabled = false; btn.classList.remove('opacity-60','cursor-not-allowed');
    });
  });
})();
"#;
