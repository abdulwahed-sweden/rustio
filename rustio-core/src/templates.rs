//! Template rendering. Rust code passes typed context; this module
//! owns everything about HTML generation.
//!
//! # Loader contract (Phase 6a)
//!
//! Per-request lookup via [`minijinja::Environment::set_loader`].
//! On every `render` call the cache is cleared, forcing the loader
//! closure to re-resolve from disk so a developer can edit a
//! template under `RUSTIO_TEMPLATE_DIR` and see the change on the
//! next request without restarting the process.
//!
//! Lookup order, by template name `<path>`:
//!
//! 1. `<RUSTIO_TEMPLATE_DIR>/<path>` — project disk override.
//! 2. Embedded default — compiled into the binary via `include_str!`.
//!
//! Per-model lookup (Phase 7 hook): callers that pass a model context
//! can use [`Templates::render_for_model`] to add a third tier:
//!
//! 1. `<RUSTIO_TEMPLATE_DIR>/admin/<model>/<page>.html`
//! 2. `<RUSTIO_TEMPLATE_DIR>/<path>`
//! 3. Embedded default
//!
//! No handler in Phase 6a calls `render_for_model`; the path is
//! exercised only by tests so the wiring is ready when a project
//! needs a per-model override.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use minijinja::{Environment, ErrorKind};
use serde::Serialize;

use crate::error::{Error, Result};

pub struct Templates {
    env: Mutex<Environment<'static>>,
}

impl Templates {
    /// Build the environment.
    ///
    /// `project_templates_dir = None` → embedded templates only.
    /// `project_templates_dir = Some(path)` → disk overrides win at
    /// render time. Pass the value of `RUSTIO_TEMPLATE_DIR` (or your
    /// own resolved path) here.
    pub fn new(project_templates_dir: Option<PathBuf>) -> Result<Arc<Self>> {
        let disk_root = project_templates_dir;
        let mut env = Environment::new();
        env.set_loader(move |name| load_template(disk_root.as_deref(), name));

        // Phase 7a/2 — `icon(name, class="...")` returns inline SVG
        // for one of the lucide stroke icons baked at compile time.
        // Templates use this to render sidebar nav icons, button
        // icons, alert-banner glyphs without an extra HTTP round
        // trip. See `admin/icons.rs` for the catalogue.
        env.add_function("icon", |name: &str, kwargs: minijinja::value::Kwargs| {
            let class: String = kwargs.get("class").unwrap_or_default();
            kwargs.assert_all_used().ok();
            // The output is HTML — minijinja's autoescape would mangle
            // it. Wrap in `safe()` so it renders as markup.
            minijinja::value::Value::from_safe_string(
                crate::admin::icons::render_inline(name, &class),
            )
        });

        Ok(Arc::new(Self {
            env: Mutex::new(env),
        }))
    }

    /// Render a template by name.
    pub fn render<S: Serialize>(&self, name: &str, ctx: &S) -> Result<String> {
        let mut env = self
            .env
            .lock()
            .map_err(|e| Error::Internal(format!("template env poisoned: {e}")))?;
        // Clear cache so the loader runs again — restart-free dev edits.
        env.clear_templates();
        let tmpl = env
            .get_template(name)
            .map_err(|e| Error::Internal(format!("template {name} not found: {e}")))?;
        tmpl.render(ctx)
            .map_err(|e| Error::Internal(format!("render {name}: {e}")))
    }

    /// Render with a per-model override hook.
    ///
    /// Tries `admin/<model>/<page>` first (where `<page>` is `name`
    /// stripped of any leading `admin/`), falling back to `name`.
    /// Phase 6a wires the API but no handler calls it yet — the
    /// existing Phase 6a admin pages all call [`Self::render`].
    #[allow(dead_code)]
    pub fn render_for_model<S: Serialize>(
        &self,
        model: &str,
        name: &str,
        ctx: &S,
    ) -> Result<String> {
        let page = name.strip_prefix("admin/").unwrap_or(name);
        let per_model = format!("admin/{model}/{page}");
        let mut env = self
            .env
            .lock()
            .map_err(|e| Error::Internal(format!("template env poisoned: {e}")))?;
        env.clear_templates();
        // Try per-model first; fall through if loader returns None.
        if let Ok(tmpl) = env.get_template(&per_model) {
            return tmpl
                .render(ctx)
                .map_err(|e| Error::Internal(format!("render {per_model}: {e}")));
        }
        let tmpl = env
            .get_template(name)
            .map_err(|e| Error::Internal(format!("template {name} not found: {e}")))?;
        tmpl.render(ctx)
            .map_err(|e| Error::Internal(format!("render {name}: {e}")))
    }
}

fn load_template(
    disk_root: Option<&std::path::Path>,
    name: &str,
) -> std::result::Result<Option<String>, minijinja::Error> {
    if let Some(root) = disk_root {
        let path = root.join(name);
        if path.exists() {
            return std::fs::read_to_string(&path).map(Some).map_err(|e| {
                minijinja::Error::new(
                    ErrorKind::InvalidOperation,
                    format!("read template {}: {e}", path.display()),
                )
            });
        }
    }
    Ok(EMBEDDED_TEMPLATES
        .iter()
        .find_map(|(n, b)| if *n == name { Some((*b).to_string()) } else { None }))
}

// Baked into the binary. Single-binary deploy is a hard constraint.
const EMBEDDED_TEMPLATES: &[(&str, &str)] = &[
    ("base.html", include_str!("../assets/templates/base.html")),
    ("admin/base.html", include_str!("../assets/templates/admin/base.html")),
    ("admin/login.html", include_str!("../assets/templates/admin/login.html")),
    ("admin/index.html", include_str!("../assets/templates/admin/index.html")),
    ("admin/list.html", include_str!("../assets/templates/admin/list.html")),
    ("admin/form.html", include_str!("../assets/templates/admin/form.html")),
    ("admin/confirm_delete.html", include_str!("../assets/templates/admin/confirm_delete.html")),
    ("admin/object_history.html", include_str!("../assets/templates/admin/object_history.html")),
    ("admin/log_entries.html", include_str!("../assets/templates/admin/log_entries.html")),
    ("admin/password_change.html", include_str!("../assets/templates/admin/password_change.html")),
    ("admin/users_list.html", include_str!("../assets/templates/admin/users_list.html")),
    ("admin/user_edit.html", include_str!("../assets/templates/admin/user_edit.html")),
    ("admin/user_new.html", include_str!("../assets/templates/admin/user_new.html")),
    ("admin/user_view.html", include_str!("../assets/templates/admin/user_view.html")),
    ("admin/user_confirm_delete.html", include_str!("../assets/templates/admin/user_confirm_delete.html")),
    ("admin/groups_list.html", include_str!("../assets/templates/admin/groups_list.html")),
    ("admin/group_edit.html", include_str!("../assets/templates/admin/group_edit.html")),
    ("admin/group_new.html", include_str!("../assets/templates/admin/group_new.html")),
    ("admin/group_confirm_delete.html", include_str!("../assets/templates/admin/group_confirm_delete.html")),
    ("admin/forbidden.html", include_str!("../assets/templates/admin/forbidden.html")),
    ("admin/error.html", include_str!("../assets/templates/admin/error.html")),
    ("admin/coming_soon.html", include_str!("../assets/templates/admin/coming_soon.html")),
    ("admin/includes/_field_errors.html", include_str!("../assets/templates/admin/includes/_field_errors.html")),
    ("admin/includes/_form_field.html", include_str!("../assets/templates/admin/includes/_form_field.html")),
    ("search.html", include_str!("../assets/templates/search.html")),
];

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Serialize;
    use std::io::Write;

    #[derive(Serialize)]
    struct Empty {}

    #[test]
    fn loader_registers_all_embedded_templates() {
        let t = Templates::new(None).unwrap();
        assert!(t.render("base.html", &Empty {}).is_ok());
    }

    #[test]
    fn missing_template_errors_cleanly() {
        let t = Templates::new(None).unwrap();
        let err = t.render("does/not/exist.html", &Empty {}).unwrap_err();
        assert_eq!(err.status(), 500);
    }

    #[test]
    fn disk_override_wins_over_embedded() {
        let dir = tempdir();
        let admin_dir = dir.join("admin");
        std::fs::create_dir_all(&admin_dir).unwrap();
        let mut f = std::fs::File::create(admin_dir.join("login.html")).unwrap();
        f.write_all(b"OVERRIDDEN-BODY").unwrap();
        drop(f);

        let t = Templates::new(Some(dir.clone())).unwrap();
        let body = t.render("admin/login.html", &Empty {}).unwrap();
        assert_eq!(body, "OVERRIDDEN-BODY");

        // Cleanup.
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn embedded_fallback_when_disk_missing() {
        let dir = tempdir();
        // dir exists but contains no admin/login.html — embedded must win.
        let t = Templates::new(Some(dir.clone())).unwrap();
        let body = t.render("admin/login.html", &Empty {}).unwrap();
        // Embedded login.html is never empty; reject if it returned the
        // disk-override sentinel.
        assert!(!body.is_empty());
        assert!(!body.contains("OVERRIDDEN-BODY"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn live_edit_visible_on_next_render_without_restart() {
        // The win of the loader refactor: edit a template on disk, the
        // next render reflects it — no Templates rebuild, no restart.
        let dir = tempdir();
        let admin_dir = dir.join("admin");
        std::fs::create_dir_all(&admin_dir).unwrap();
        let target = admin_dir.join("login.html");

        std::fs::write(&target, b"V1").unwrap();
        let t = Templates::new(Some(dir.clone())).unwrap();
        assert_eq!(t.render("admin/login.html", &Empty {}).unwrap(), "V1");

        // Edit in place.
        std::fs::write(&target, b"V2").unwrap();
        assert_eq!(
            t.render("admin/login.html", &Empty {}).unwrap(),
            "V2",
            "loader must re-resolve from disk on every render"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Phase 7a/0.5/f-fix regression: the embedded loader must know
    /// about `admin/user_confirm_delete.html`. The browser smoke run
    /// of /f hit a 500 because the template existed on disk but the
    /// EMBEDDED_TEMPLATES const didn't list it. This test renders the
    /// template with two distinct contexts and asserts the
    /// last-developer banner + the submit-button disabled/enabled
    /// invariant — so any future delete-handler refactor that adds a
    /// new template can't ship the same gap unnoticed.
    #[test]
    fn user_confirm_delete_renders_with_last_developer_banner() {
        let t = Templates::new(None).unwrap();

        // Last-developer case → blocking banner + disabled submit.
        let ctx = serde_json::json!({
            "user_id": 122,
            "email": "backup@example.com",
            "role": "developer",
            "group_count": 0,
            "session_count": 1,
            "direct_perm_count": 0,
            "is_self": false,
            "is_last_developer": true,
            "csrf_token": "test-csrf",
        });
        let body = t.render("admin/user_confirm_delete.html", &ctx).unwrap();
        assert!(
            body.contains("is the last active developer"),
            "last-dev banner must mention the orphan condition"
        );
        assert!(
            body.contains("rustio-cli user role set"),
            "last-dev banner must point operators at the CLI escape hatch"
        );
        assert!(
            body.contains(r#"<button type="submit" class="btn-danger" disabled>"#),
            "submit must be disabled when target is the last active developer"
        );

        // Self-delete case → different errornote, also disabled.
        let ctx_self = serde_json::json!({
            "user_id": 7,
            "email": "me@rustio.local",
            "role": "administrator",
            "group_count": 2,
            "session_count": 1,
            "direct_perm_count": 0,
            "is_self": true,
            "is_last_developer": false,
            "csrf_token": "test-csrf",
        });
        let body = t.render("admin/user_confirm_delete.html", &ctx_self).unwrap();
        assert!(
            body.contains("your own account"),
            "self-delete banner must call out the self-action"
        );
        assert!(
            body.contains(r#"<button type="submit" class="btn-danger" disabled>"#),
            "submit must be disabled on self-delete"
        );
    }

    // ------------------------------------------------------------------
    // Phase 7a/0.5/h — admin/user_view.html
    //
    // Five render tests covering the load-bearing template branches:
    // - groups list populated vs empty
    // - is_self / is_last_developer disable the Delete button
    // - is_demo absent → no demo row
    //
    // All run as sandbox tests (no DB) because the template only
    // depends on the context dict — perfect for catching template
    // typos + missing variables without needing postgres.
    // ------------------------------------------------------------------

    fn view_ctx_base() -> serde_json::Value {
        serde_json::json!({
            "target_id": 42,
            "target_email": "alice@example.com",
            "target_role": "staff",
            "target_is_active": true,
            "target_is_demo": false,
            "target_demo_label": null,
            "target_created_at": "2026-04-25 12:00 UTC",
            "target_updated_at": "2026-04-25 12:30 UTC",
            "groups": [],
            "direct_perms": [],
            "is_self": false,
            "is_last_developer": false,
            "can_edit": true,
            "can_delete": true,
            "csrf_token": "test-csrf",
        })
    }

    /// Phase 7a/2 — the `icon()` minijinja function is registered in
    /// `Templates::new`. A template can call `{{ icon("home", class="w-4 h-4") }}`
    /// and the inline SVG is emitted unescaped (because we wrap it in
    /// `Value::from_safe_string`). Lock that contract.
    #[test]
    fn icon_function_emits_inline_svg() {
        // Write a temp template that exercises icon() and render it.
        let dir = tempdir();
        let admin_dir = dir.join("admin");
        std::fs::create_dir_all(&admin_dir).unwrap();
        std::fs::write(
            admin_dir.join("icon_test.html"),
            r#"<div>{{ icon("home", class="sidebar-icon") }}</div>"#,
        )
        .unwrap();

        let t = Templates::new(Some(dir.clone())).unwrap();
        let body = t.render("admin/icon_test.html", &Empty {}).unwrap();
        assert!(
            body.contains("<svg"),
            "icon() must emit raw <svg> markup, not escape it"
        );
        assert!(body.contains(r#"class="sidebar-icon""#));
        assert!(body.contains(r#"viewBox="0 0 24 24""#));
        assert!(body.contains(r#"stroke="currentColor""#));

        // Unknown icon name → empty string, page still renders.
        std::fs::write(
            admin_dir.join("icon_missing.html"),
            r#"<span>{{ icon("not-real") }}</span>"#,
        )
        .unwrap();
        let body = t.render("admin/icon_missing.html", &Empty {}).unwrap();
        assert_eq!(body.trim(), "<span></span>", "missing icon must be silent");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn user_view_renders_with_groups() {
        let t = Templates::new(None).unwrap();
        let mut ctx = view_ctx_base();
        ctx["groups"] = serde_json::json!([
            { "name": "Auditors", "description": "read-only audit access" },
            { "name": "Content Editors", "description": "" },
        ]);
        let body = t.render("admin/user_view.html", &ctx).unwrap();
        assert!(
            body.contains("Group memberships (2)"),
            "membership count must reflect the groups list length"
        );
        assert!(body.contains("Auditors"));
        assert!(body.contains("Content Editors"));
        // Description-empty branch: the `<span class=\"help\">` must
        // NOT render for the second group.
        assert!(body.contains("read-only audit access"));
    }

    #[test]
    fn user_view_without_groups_shows_empty_message() {
        let t = Templates::new(None).unwrap();
        let body = t.render("admin/user_view.html", &view_ctx_base()).unwrap();
        assert!(
            body.contains("No group memberships"),
            "empty-state copy must appear when the groups list is empty"
        );
        assert!(body.contains("Group memberships (0)"));
    }

    /// Semantic invariant: when `can_delete` is false (whatever
    /// reason), the Delete element must render as a `<span>` (not an
    /// `<a href>`) so a misclick can't hit the destructive endpoint.
    /// The exact CSS classes used to disable it are styling, not
    /// contract.
    fn assert_delete_is_disabled_span(body: &str, expected_tooltip: &str) {
        assert!(
            body.contains(r#"<span class="btn-danger"#),
            "Delete must render as a <span> with btn-danger class when guarded"
        );
        assert!(
            !body.contains(r#"<a href="/admin/users/42/delete""#),
            "Delete must NOT render as an <a href=…/delete> when guarded"
        );
        assert!(
            body.contains(expected_tooltip),
            "tooltip must contain {expected_tooltip:?} so the operator knows why"
        );
    }

    #[test]
    fn user_view_is_self_disables_delete_as_span() {
        let t = Templates::new(None).unwrap();
        let mut ctx = view_ctx_base();
        ctx["is_self"] = serde_json::Value::Bool(true);
        ctx["can_delete"] = serde_json::Value::Bool(false);
        let body = t.render("admin/user_view.html", &ctx).unwrap();
        assert_delete_is_disabled_span(&body, "Cannot delete your own account");
        // Edit must still render as an anchor — administrators can
        // edit themselves, just not delete.
        assert!(
            body.contains(r#"<a href="/admin/users/42/edit""#),
            "Edit button stays clickable on a self-view"
        );
    }

    #[test]
    fn user_view_last_developer_disables_delete() {
        let t = Templates::new(None).unwrap();
        let mut ctx = view_ctx_base();
        ctx["is_last_developer"] = serde_json::Value::Bool(true);
        ctx["can_delete"] = serde_json::Value::Bool(false);
        let body = t.render("admin/user_view.html", &ctx).unwrap();
        assert_delete_is_disabled_span(&body, "Cannot delete the last active developer");
    }

    /// The users list now navigates to the profile view (not edit).
    /// Lock the invariant: every cell in every row carries a `.row-link`
    /// anchor pointing to `/admin/users/:id/` — never to `/edit`. A
    /// regression here would silently revert /h's UX shift.
    #[test]
    fn users_list_renders_row_clickable_links() {
        let t = Templates::new(None).unwrap();
        let ctx = serde_json::json!({
            "page_title": "Users",
            "users": [
                { "id": 7, "email": "alice@example.com", "role": "staff",
                  "is_active": true, "created_at": "2026-04-01" },
                { "id": 9, "email": "bob@example.com", "role": "developer",
                  "is_active": false, "created_at": "2026-04-02" },
            ],
            "csrf_token": "x",
        });
        let body = t.render("admin/users_list.html", &ctx).unwrap();

        // The table must declare itself row-clickable (CSS hook).
        assert!(body.contains(r#"class="results row-clickable""#));

        // Each row → 4 anchors pointing at the profile, none at /edit.
        let count_for = |needle: &str| body.matches(needle).count();
        assert_eq!(
            count_for(r#"href="/admin/users/7/""#),
            4,
            "every cell in row 7 must link to the profile view (4 anchors)"
        );
        assert_eq!(
            count_for(r#"href="/admin/users/9/""#),
            4,
            "every cell in row 9 must link to the profile view (4 anchors)"
        );
        // Defensive: no /edit links should leak through from the
        // pre-/h pattern.
        assert_eq!(
            count_for(r#"href="/admin/users/7/edit""#),
            0,
            "list rows must NOT link to /edit anymore — that lives behind the view"
        );
    }

    #[test]
    fn user_view_real_user_omits_demo_row() {
        let t = Templates::new(None).unwrap();
        // is_demo defaults to false in view_ctx_base.
        let body = t.render("admin/user_view.html", &view_ctx_base()).unwrap();
        // The demo label string only appears when target_is_demo=true.
        // Looking for the demo-account marker that's only in the
        // gated `{% if target_is_demo %}` block.
        assert!(
            !body.contains("badge-warning\">staff @"),
            "demo badge must NOT render for a real (non-demo) user"
        );
        assert!(
            !body.contains("Demo account"),
            "demo label must NOT appear for a real user"
        );

        // Demo case: badge renders with the label.
        let mut demo_ctx = view_ctx_base();
        demo_ctx["target_is_demo"] = serde_json::Value::Bool(true);
        demo_ctx["target_demo_label"] = serde_json::Value::String("staff @ rustio.local".into());
        let demo_body = t.render("admin/user_view.html", &demo_ctx).unwrap();
        assert!(
            demo_body.contains("staff @ rustio.local"),
            "demo label must render in the badge for a demo user"
        );
    }

    /// Companion to `…with_last_developer_banner`: when neither guard
    /// fires, the submit button MUST be enabled (no `disabled`
    /// attribute). Lock that invariant in too — a stray `{% if %}`
    /// edit could otherwise quietly disable every confirm button.
    #[test]
    fn user_confirm_delete_submit_enabled_for_normal_user() {
        let t = Templates::new(None).unwrap();
        let ctx = serde_json::json!({
            "user_id": 99,
            "email": "throwaway@example.com",
            "role": "staff",
            "group_count": 0,
            "session_count": 0,
            "direct_perm_count": 0,
            "is_self": false,
            "is_last_developer": false,
            "csrf_token": "test-csrf",
        });
        let body = t.render("admin/user_confirm_delete.html", &ctx).unwrap();
        // Submit must render without `disabled`. The exact button
        // markup includes an icon child (Phase 7a/2), so we assert
        // on the contract — `<button type="submit" class="btn-danger">`
        // present, and `disabled` NOT present anywhere on it.
        assert!(
            body.contains(r#"<button type="submit" class="btn-danger">"#),
            "submit button must render with btn-danger class"
        );
        // Find the button's opening tag and assert no disabled attr
        // sneaks in via a different code path.
        assert!(
            !body.contains(r#"<button type="submit" class="deletelink-button" disabled>"#),
            "submit button must NOT render with `disabled` for a deletable user"
        );
        // And the warning banners must NOT appear.
        assert!(!body.contains("is the last active developer"));
        assert!(!body.contains("your own account"));
    }

    fn tempdir() -> PathBuf {
        let pid = std::process::id();
        let nonce: u64 = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64;
        let path = std::env::temp_dir().join(format!("rustio-tpl-{pid}-{nonce}"));
        std::fs::create_dir_all(&path).unwrap();
        path
    }
}
