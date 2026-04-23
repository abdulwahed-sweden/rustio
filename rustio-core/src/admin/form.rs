//! Admin-new form engine.
//!
//! Turn a [`FormConfig`] of [`FieldConfig`]s into drawer-based form
//! HTML. Every input type routes through a dedicated renderer that
//! uses only classnames present in the approved `components.css` —
//! no new classes, no new styles, no JS, no filesystem, no DB.
//!
//! The **ForeignKey rule** is load-bearing: FK fields *always* render
//! as a `<select>` populated from [`FieldConfig::options`]. Raw numeric
//! row ids are never presented to the admin user; the caller resolves
//! `(value, label)` pairs upstream and passes human-readable labels in.
//!
//! UI-level validation lives here too: [`validate_form`] walks the
//! fields, applies a small set of basic rules (required / email /
//! number), and writes any failure into `FieldConfig::error`. The
//! renderers add `class="invalid"` to the input and append a
//! `.field-error` block below — both classes are referenced but **not
//! styled here**; styling is assumed to live in the bundled CSS.

use std::collections::HashMap;

use crate::admin::ui::html_escape;

// ---------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldType {
    Text,
    Email,
    Number,
    DateTime,
    Boolean,
    Select,
    ForeignKey,
    TextArea,
}

#[derive(Debug, Clone)]
pub struct FieldConfig {
    pub name: String,
    pub label: String,
    pub field_type: FieldType,

    pub required: bool,
    pub readonly: bool,

    pub placeholder: Option<String>,
    pub help: Option<String>,

    pub value: Option<String>,

    /// `(value, label)` pairs used by [`FieldType::Select`] and
    /// [`FieldType::ForeignKey`]. The label is what the user sees;
    /// the value is what the form submits.
    pub options: Vec<(String, String)>,

    /// UI-level validation error attached to this field. `Some(msg)`
    /// causes the renderer to add `class="invalid"` to the input and
    /// append a `<div class="field-error">{msg}</div>` block below.
    /// Populated by [`validate_form`] or by callers directly.
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct FormConfig {
    pub title: String,
    pub subtitle: String,
    pub fields: Vec<FieldConfig>,

    /// `true` once values have been bound from a real submission. The
    /// renderer uses this to swap the inline error summary for a
    /// `.form-success` banner when validation passes, so the user
    /// sees an explicit confirmation rather than a silent re-render.
    /// Set automatically by [`bind_form`].
    pub submitted: bool,

    /// `true` if a submission was bound + validated cleanly but the
    /// downstream persistence write returned an error. The renderer
    /// shows a `.form-error-summary` "save failed" banner instead of
    /// the success banner. Independent of `submitted` so callers can
    /// flip just this one field.
    pub save_failed: bool,

    /// Extra `<input type="hidden">` fields to emit inside the form,
    /// in order. Used to round-trip the editing primary key (and any
    /// other state the route handler wants preserved across a POST)
    /// without requiring the field to appear in `fields`.
    pub hidden_fields: Vec<(String, String)>,
}

// ---------------------------------------------------------------
// Name-based inference
// ---------------------------------------------------------------

/// Infer a likely [`FieldType`] from a field name.
///
/// Callers that don't know a field's type up front (e.g. auto-built
/// forms from a schema with only column names) can seed
/// `FieldConfig::field_type` with this helper's output. Rules mirror
/// Django-style admin conventions:
///
/// - ends with `_id`   → [`FieldType::ForeignKey`]
/// - starts with `is_` or `has_` → [`FieldType::Boolean`]
/// - contains `email`  → [`FieldType::Email`]
/// - contains `amount` or `price` → [`FieldType::Number`]
/// - otherwise         → [`FieldType::Text`]
pub fn infer_field_type(name: &str) -> FieldType {
    let lower = name.to_ascii_lowercase();
    if lower.ends_with("_id") {
        return FieldType::ForeignKey;
    }
    if lower.starts_with("is_") || lower.starts_with("has_") {
        return FieldType::Boolean;
    }
    if lower.contains("email") {
        return FieldType::Email;
    }
    if lower.contains("amount") || lower.contains("price") {
        return FieldType::Number;
    }
    FieldType::Text
}

/// Decide which control actually renders. `ForeignKey` stays
/// `ForeignKey` (rendered as a select internally). `Boolean` stays
/// `Boolean` (switch) even if the caller passes options — true/false
/// isn't a dropdown. Every other type with a non-empty `options`
/// promotes to `Select`.
fn effective_type(f: &FieldConfig) -> FieldType {
    match f.field_type {
        FieldType::ForeignKey | FieldType::Boolean => f.field_type,
        other if !f.options.is_empty() && other != FieldType::Select => FieldType::Select,
        other => other,
    }
}

// ---------------------------------------------------------------
// Validation
// ---------------------------------------------------------------

/// Walk the form's fields and apply the basic validation rules:
///
/// - **Required:** empty value on a `required` field → `"This field
///   is required"`.
/// - **Email:** non-empty `Email` value missing `@` →
///   `"Invalid email address"`.
/// - **Number:** non-empty `Number` value that doesn't parse as
///   `f64` → `"Must be a valid number"`.
///
/// The first failing rule per field wins; the function never panics
/// or unwraps. Empty optional fields skip the type-specific checks.
pub fn validate_form(form: &mut FormConfig) {
    for field in form.fields.iter_mut() {
        let value = field.value.as_deref().unwrap_or("").trim();
        if field.required && value.is_empty() {
            field.error = Some("This field is required".into());
            continue;
        }
        if value.is_empty() {
            continue;
        }
        match field.field_type {
            FieldType::Email if !value.contains('@') => {
                field.error = Some("Invalid email address".into());
            }
            FieldType::Number if value.parse::<f64>().is_err() => {
                field.error = Some("Must be a valid number".into());
            }
            _ => {}
        }
    }
}

/// Render the `.field-error` block for a single message. Empty
/// `msg` → empty string, so callers don't need to gate the call.
pub fn render_error(msg: &str) -> String {
    if msg.is_empty() {
        return String::new();
    }
    format!(r#"<div class="field-error">{}</div>"#, html_escape(msg))
}

/// Pull submitted values out of a parsed urlencoded body and write
/// them into the form's fields.
///
/// - Non-Boolean: the field's `value` is set to the matching `params`
///   entry if present, otherwise left as-is.
/// - Boolean: HTML form submission omits unchecked checkboxes /
///   switches entirely, so the rule is *presence-of-key*, not value:
///   `value = Some(params.contains_key(name).to_string())`.
///
/// Pre-existing per-field `error`s are cleared so subsequent calls
/// to [`validate_form`] start from a clean slate. The form's
/// `submitted` flag flips to `true` so [`render_form`] can swap the
/// error summary for the `.form-success` banner when validation
/// passes.
///
/// Never panics; missing keys silently leave non-boolean fields
/// untouched.
pub fn bind_form(form: &mut FormConfig, params: &HashMap<String, String>) {
    form.submitted = true;
    // A fresh bind clears any previous save failure — the caller
    // will re-set it from the new persistence outcome.
    form.save_failed = false;
    for field in form.fields.iter_mut() {
        if field.field_type == FieldType::Boolean {
            // Boolean now ships a real `<input>` pair: a hidden
            // baseline (`value="false"`) plus a real checkbox
            // (`value="true"`). When checked, both submit and
            // last-write-wins parsing yields `"true"`. When
            // unchecked, only the hidden submits → `"false"`. So
            // we just read the value through and fall back to
            // `"false"` for the (now rare) case where neither input
            // was present in the body at all.
            field.value = params
                .get(&field.name)
                .cloned()
                .or(Some("false".to_string()));
        } else if let Some(v) = params.get(&field.name) {
            field.value = Some(v.clone());
        }
        field.error = None;
    }
}

// ---------------------------------------------------------------
// Field renderer
// ---------------------------------------------------------------

pub fn render_field(f: &FieldConfig) -> String {
    render_field_inner(f, false)
}

/// Internal render path — same as [`render_field`] but with an
/// `autofocus` flag the form-level renderer can flip on for the first
/// invalid field. Public signature of [`render_field`] is preserved.
fn render_field_inner(f: &FieldConfig, autofocus: bool) -> String {
    match effective_type(f) {
        // Boolean is a switch (a label, not an input element). The
        // browser cannot autofocus a label, so we ignore the flag for
        // booleans rather than emit invalid HTML.
        FieldType::Boolean => render_boolean_field(f),
        FieldType::Text => wrap_field(f, &render_input(f, "text", false, autofocus)),
        FieldType::Email => wrap_field(f, &render_input(f, "email", false, autofocus)),
        FieldType::Number => wrap_field(f, &render_input(f, "number", true, autofocus)),
        FieldType::DateTime => wrap_field(f, &render_input(f, "datetime-local", false, autofocus)),
        FieldType::TextArea => wrap_field(f, &render_textarea(f, autofocus)),
        FieldType::Select | FieldType::ForeignKey => wrap_field(f, &render_select(f, autofocus)),
    }
}

fn wrap_field(f: &FieldConfig, input_html: &str) -> String {
    let req = if f.required {
        r#"<span class="field-required">*</span>"#
    } else {
        ""
    };
    let help = match &f.help {
        Some(h) if !h.is_empty() => {
            format!(r#"<div class="field-help">{}</div>"#, html_escape(h))
        }
        _ => String::new(),
    };
    let error = match &f.error {
        Some(e) if !e.is_empty() => render_error(e),
        _ => String::new(),
    };
    format!(
        r#"<div class="field">
  <label class="field-label" for="{id}">{label}{req}</label>
  {input}
  {help}
  {error}
</div>"#,
        id = html_escape(&f.name),
        label = html_escape(&f.label),
        req = req,
        input = input_html,
        help = help,
        error = error,
    )
}

fn render_input(f: &FieldConfig, input_type: &str, mono: bool, autofocus: bool) -> String {
    let class_attr = match (mono, f.error.is_some()) {
        (true, true) => r#" class="mono invalid""#.to_string(),
        (true, false) => r#" class="mono""#.to_string(),
        (false, true) => r#" class="invalid""#.to_string(),
        (false, false) => String::new(),
    };
    let value_attr = match &f.value {
        Some(v) if !v.is_empty() => format!(r#" value="{}""#, html_escape(v)),
        _ => String::new(),
    };
    let placeholder_attr = match &f.placeholder {
        Some(p) if !p.is_empty() => format!(r#" placeholder="{}""#, html_escape(p)),
        _ => String::new(),
    };
    let required_attr = if f.required { " required" } else { "" };
    let readonly_attr = if f.readonly { " readonly" } else { "" };
    let autofocus_attr = if autofocus { " autofocus" } else { "" };
    format!(
        r#"<input type="{ty}" id="{id}" name="{name}"{cls}{val}{ph}{req}{ro}{af}>"#,
        ty = input_type,
        id = html_escape(&f.name),
        name = html_escape(&f.name),
        cls = class_attr,
        val = value_attr,
        ph = placeholder_attr,
        req = required_attr,
        ro = readonly_attr,
        af = autofocus_attr,
    )
}

fn render_textarea(f: &FieldConfig, autofocus: bool) -> String {
    let value = f.value.as_deref().unwrap_or("");
    let class_attr = if f.error.is_some() {
        r#" class="mono invalid""#
    } else {
        r#" class="mono""#
    };
    let placeholder_attr = match &f.placeholder {
        Some(p) if !p.is_empty() => format!(r#" placeholder="{}""#, html_escape(p)),
        _ => String::new(),
    };
    let required_attr = if f.required { " required" } else { "" };
    let readonly_attr = if f.readonly { " readonly" } else { "" };
    let autofocus_attr = if autofocus { " autofocus" } else { "" };
    format!(
        r#"<textarea id="{id}" name="{name}"{cls}{ph}{req}{ro}{af}>{value}</textarea>"#,
        id = html_escape(&f.name),
        name = html_escape(&f.name),
        cls = class_attr,
        ph = placeholder_attr,
        req = required_attr,
        ro = readonly_attr,
        af = autofocus_attr,
        value = html_escape(value),
    )
}

fn render_select(f: &FieldConfig, autofocus: bool) -> String {
    let selected_value = f.value.as_deref().unwrap_or("");
    let class_attr = if f.error.is_some() {
        r#" class="invalid""#
    } else {
        ""
    };
    let required_attr = if f.required { " required" } else { "" };
    // <select> has no `readonly` attribute; `disabled` is the closest
    // native equivalent. Disabled selects don't submit, which matches
    // the intent of "user can't change this value here".
    let disabled_attr = if f.readonly { " disabled" } else { "" };
    let autofocus_attr = if autofocus { " autofocus" } else { "" };
    let mut s = format!(
        r#"<select id="{id}" name="{name}"{cls}{req}{dis}{af}>"#,
        id = html_escape(&f.name),
        name = html_escape(&f.name),
        cls = class_attr,
        req = required_attr,
        dis = disabled_attr,
        af = autofocus_attr,
    );
    for (value, label) in &f.options {
        let selected = if value == selected_value {
            " selected"
        } else {
            ""
        };
        s.push_str(&format!(
            r#"<option value="{}"{}>{}</option>"#,
            html_escape(value),
            selected,
            html_escape(label),
        ));
    }
    s.push_str("</select>");
    s
}

/// Boolean follows the approved components.html pattern: the switch
/// *is* the field and carries its own visible label via `.switch-label`
/// — no redundant `.field-label` above it. `.field-help` and the
/// validation `.field-error` block still render below when supplied.
///
/// Two real form controls are injected inside the existing `.switch`
/// label so the field actually submits data:
///
/// 1. A hidden `<input type="hidden" value="false">` always sends a
///    `false` baseline. This guarantees the field name appears in
///    every POST body even when the switch is off.
/// 2. A real `<input type="checkbox" value="true" hidden>` overrides
///    the baseline when checked — its `value` is sent *after* the
///    hidden's, and `FormData::parse` uses last-write-wins, so the
///    final bound value is `"true"` when checked, `"false"` when not.
///
/// The `hidden` HTML attribute keeps the checkbox out of the visual
/// flow — no CSS changes needed, no class names added.
fn render_boolean_field(f: &FieldConfig) -> String {
    let on = matches!(f.value.as_deref(), Some("1" | "true" | "on" | "yes"));
    let on_cls = if on { " on" } else { "" };
    let checked_attr = if on { " checked" } else { "" };
    let help = match &f.help {
        Some(h) if !h.is_empty() => {
            format!(r#"<div class="field-help">{}</div>"#, html_escape(h))
        }
        _ => String::new(),
    };
    let error = match &f.error {
        Some(e) if !e.is_empty() => render_error(e),
        _ => String::new(),
    };
    format!(
        r#"<div class="field">
  <label class="switch{cls}">
    <input type="hidden" name="{name}" value="false">
    <input type="checkbox" name="{name}" value="true" hidden{checked}>
    <span class="switch-track"></span>
    <span class="switch-label">{label}</span>
  </label>
  {help}
  {error}
</div>"#,
        cls = on_cls,
        name = html_escape(&f.name),
        checked = checked_attr,
        label = html_escape(&f.label),
        help = help,
        error = error,
    )
}

// ---------------------------------------------------------------
// Form renderer
// ---------------------------------------------------------------

pub fn render_form(form: &FormConfig) -> String {
    // First-invalid lookup powers two UX behaviours at once: the
    // `autofocus` attribute on that field's input, and the gating of
    // the submit button + the inline error summary.
    let first_invalid = form.fields.iter().position(|f| f.error.is_some());
    let has_errors = first_invalid.is_some();

    let summary = render_form_banner(form, has_errors);

    let mut body = String::new();
    body.push_str(&summary);
    for (i, field) in form.fields.iter().enumerate() {
        let autofocus = Some(i) == first_invalid;
        body.push_str(&render_field_inner(field, autofocus));
    }

    // Hidden state pieces (e.g. the editing `id`) live inside the
    // form so they round-trip on submit. Caller-controlled order;
    // values are HTML-escaped.
    let mut hidden = String::new();
    for (k, v) in &form.hidden_fields {
        hidden.push_str(&format!(
            r#"<input type="hidden" name="{}" value="{}">"#,
            html_escape(k),
            html_escape(v),
        ));
    }

    let save_disabled_attr = if has_errors { " disabled" } else { "" };

    // The drawer is wrapped in a single `<form data-admin-form>`:
    //   - Save / Cancel both belong to one POST submission flow.
    //   - admin.js scopes Esc-to-close + Cmd+Enter-to-submit to
    //     `[data-admin-form]`, so unrelated UI is unaffected.
    //   - Backdrop is a sibling element so a click outside the drawer
    //     closes it via the same `href="?"` cancel target.
    // Header × and footer Cancel both navigate to `?` — empty query
    // string drops `?id=` and the browser re-renders the list view
    // with the drawer hidden. Search / filter / sort state is lost on
    // cancel; a follow-up could thread the current state through.
    format!(
        r#"<a class="drawer-backdrop open" href="?" aria-label="Close drawer"></a>
<form data-admin-form class="drawer open" action="" method="post">
  {hidden}
  <header class="drawer-header">
    <div class="drawer-title-group">
      <h2 class="drawer-title">{title}</h2>
      <div class="drawer-subtitle">{subtitle}</div>
    </div>
    <a class="drawer-close" href="?" aria-label="Close">×</a>
  </header>
  <div class="drawer-body">
    {body}
  </div>
  <footer class="drawer-footer">
    <a class="btn btn-ghost" href="?">Cancel</a>
    <button type="submit" class="btn btn-primary"{save_disabled}>Save changes</button>
  </footer>
</form>"#,
        title = html_escape(&form.title),
        subtitle = html_escape(&form.subtitle),
        body = body,
        hidden = hidden,
        save_disabled = save_disabled_attr,
    )
}

/// Render the banner that appears at the top of the form's body —
/// validation errors take priority, then save failure, then the
/// "Saved successfully" success banner. `Pristine` (GET, never
/// submitted) renders nothing.
fn render_form_banner(form: &FormConfig, has_errors: bool) -> String {
    if has_errors {
        return String::from(
            r#"<div class="form-banner form-banner-error" role="alert">Please fix the errors below.</div>"#,
        );
    }
    if form.save_failed {
        return String::from(
            r#"<div class="form-banner form-banner-error" role="alert">Could not save the record.</div>"#,
        );
    }
    if form.submitted {
        return String::from(
            r#"<div class="form-banner form-banner-success" role="status">Changes saved.</div>"#,
        );
    }
    String::new()
}
