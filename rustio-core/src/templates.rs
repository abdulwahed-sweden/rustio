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
    ///
    /// Phase 12/c-fix — when a disk root is supplied, the constructor
    /// scans it once for overrides of embedded templates. Each match
    /// is logged at INFO; an override that looks structurally
    /// incomplete (no `{% extends %}`, no `{% block %}`, no `<html>`
    /// tag) is logged at WARN so a one-line stub of `admin/base.html`
    /// stops being a silent failure. Non-fatal: the override is still
    /// served — the scan exists only to make the failure mode visible.
    pub fn new(project_templates_dir: Option<PathBuf>) -> Result<Arc<Self>> {
        let disk_root = project_templates_dir;
        if let Some(root) = disk_root.as_deref() {
            for v in validate_overrides(root) {
                match v {
                    OverrideValidation::Loaded { name, bytes } => {
                        log::info!(
                            "templates: project override loaded for `{name}` ({bytes} bytes)"
                        );
                    }
                    OverrideValidation::Suspicious { name, bytes } => {
                        log::warn!(
                            "templates: project override for `{name}` looks incomplete \
                             ({bytes} bytes, no `{{% extends %}}`, no `{{% block %}}`, no \
                             `<html>` tag) — the admin UI may render incorrectly. Either \
                             copy the framework default in full or remove the override."
                        );
                    }
                    OverrideValidation::Unreadable { name, error } => {
                        log::warn!(
                            "templates: project override `{name}` exists but cannot be read: {error}"
                        );
                    }
                    OverrideValidation::OrphanAdminFile { path } => {
                        log::warn!(
                            "templates: `{path}` is in the admin namespace but does not \
                             override any embedded template (typo? framework default \
                             will be served unchanged). Project-specific admin pages \
                             belong outside `templates/admin/`."
                        );
                    }
                }
            }
        }
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

/// Phase 12/c-fix — outcome of inspecting one project override file at
/// startup. Per-file, not per-render: the cost is paid once when the
/// `Templates` arc is built, not on every request.
///
/// Pure data — `Templates::new` translates each variant to a log line.
/// Returned as a `Vec` so unit tests can assert on the structural
/// classification without scraping log output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum OverrideValidation {
    /// File loaded and contains at least one of `{% extends %}`,
    /// `{% block %}`, or `<html`. Heuristic for "this looks like a
    /// real template body".
    Loaded { name: &'static str, bytes: usize },
    /// File loaded but contains none of the structural markers.
    /// Most likely a stub or a placeholder; the framework default is
    /// silently being replaced by something that will not render the
    /// admin UI correctly.
    Suspicious { name: &'static str, bytes: usize },
    /// File exists on disk but `read_to_string` failed
    /// (permissions, encoding, IO error).
    Unreadable { name: &'static str, error: String },
    /// 1.8.1 — file in `templates/admin/` whose name does NOT match any
    /// embedded template, i.e. it overrides nothing. Most likely a typo
    /// (`baes.html` instead of `base.html`) — the developer thinks they
    /// have an override but the framework just serves the embedded
    /// default. This variant catches the inverted version of the
    /// silent-failure bug `Suspicious` was added for.
    OrphanAdminFile { path: String },
}

/// Phase 12/c-fix — walk `EMBEDDED_TEMPLATES`, classify any project
/// override of one of those names, return the per-file results.
///
/// Files in `disk_root` that do NOT shadow an embedded name are
/// ignored: those are project-only templates (e.g. `home.html`) and
/// have no framework default to compare against. Only shadowing files
/// can cause the silent-replacement failure mode this scan exists for.
pub(crate) fn validate_overrides(disk_root: &std::path::Path) -> Vec<OverrideValidation> {
    let mut results = Vec::new();
    for (name, _embedded) in EMBEDDED_TEMPLATES {
        let path = disk_root.join(name);
        if !path.is_file() {
            continue;
        }
        match std::fs::read_to_string(&path) {
            Ok(body) => {
                let bytes = body.len();
                let has_structure = body.contains("{% extends")
                    || body.contains("{% block")
                    || body.contains("<html");
                if has_structure {
                    results.push(OverrideValidation::Loaded { name, bytes });
                } else {
                    results.push(OverrideValidation::Suspicious { name, bytes });
                }
            }
            Err(e) => {
                results.push(OverrideValidation::Unreadable {
                    name,
                    error: e.to_string(),
                });
            }
        }
    }

    // 1.8.1 — orphan-admin-file scan. The framework reserves
    // `templates/admin/*.html` for overrides of embedded admin
    // templates. A file in that namespace whose name doesn't match
    // any embedded template overrides nothing — usually a typo
    // (`baes.html` for `base.html`) or a misunderstanding (project
    // admin UI should live under the project's namespace, not under
    // `templates/admin/`). Either way the developer's intent and the
    // runtime's behaviour disagree silently; this scan logs a WARN
    // so the disagreement becomes observable.
    let admin_dir = disk_root.join("admin");
    if admin_dir.is_dir() {
        let known: std::collections::HashSet<&'static str> = EMBEDDED_TEMPLATES
            .iter()
            .filter_map(|(n, _)| n.strip_prefix("admin/"))
            .collect();
        if let Ok(entries) = std::fs::read_dir(&admin_dir) {
            // Sort for deterministic ordering — the loop visits files in
            // arbitrary FS order otherwise, which makes log lines and
            // tests non-deterministic.
            let mut files: Vec<_> = entries
                .filter_map(|e| e.ok())
                .filter(|e| {
                    e.path()
                        .extension()
                        .and_then(|s| s.to_str())
                        .map(|s| s.eq_ignore_ascii_case("html"))
                        .unwrap_or(false)
                })
                .collect();
            files.sort_by_key(|e| e.file_name());
            for entry in files {
                let file_name = entry.file_name();
                let Some(stem_html) = file_name.to_str() else {
                    continue;
                };
                if known.contains(stem_html) {
                    continue;
                }
                results.push(OverrideValidation::OrphanAdminFile {
                    path: format!("admin/{stem_html}"),
                });
            }
        }
    }

    results
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

    // ----- Phase 12/c-fix — validate_overrides ---------------------------

    /// Phase 12/c-fix — a faithful copy of an embedded admin template
    /// (here, just "contains `{% extends %}`") classifies as Loaded.
    /// The structural marker is enough; the content doesn't have to
    /// match the upstream template byte-for-byte.
    #[test]
    fn validate_overrides_classifies_real_template_as_loaded() {
        let dir = tempdir();
        let admin_dir = dir.join("admin");
        std::fs::create_dir_all(&admin_dir).unwrap();
        std::fs::write(
            admin_dir.join("login.html"),
            "{% extends \"admin/base.html\" %}\n{% block content %}hi{% endblock %}",
        )
        .unwrap();
        let v = validate_overrides(&dir);
        assert_eq!(v.len(), 1);
        match &v[0] {
            OverrideValidation::Loaded { name, bytes } => {
                assert_eq!(*name, "admin/login.html");
                assert!(*bytes > 0);
            }
            other => panic!("expected Loaded, got: {other:?}"),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Phase 12/c-fix — the bug we shipped pre-fix: a one-line stub of
    /// admin/base.html silently destroys the admin UI. Validation must
    /// flag it as Suspicious so the operator sees a WARN in the log.
    #[test]
    fn validate_overrides_flags_stub_admin_base_as_suspicious() {
        let dir = tempdir();
        let admin_dir = dir.join("admin");
        std::fs::create_dir_all(&admin_dir).unwrap();
        std::fs::write(admin_dir.join("base.html"), "<h1>TEST</h1>").unwrap();
        let v = validate_overrides(&dir);
        assert_eq!(v.len(), 1);
        match &v[0] {
            OverrideValidation::Suspicious { name, bytes } => {
                assert_eq!(*name, "admin/base.html");
                assert_eq!(*bytes, 13);
            }
            other => panic!("expected Suspicious, got: {other:?}"),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Phase 12/c-fix — a project-only template like `home.html` is NOT
    /// in EMBEDDED_TEMPLATES, so it doesn't shadow a framework default
    /// and must be silently ignored by the validator. Otherwise every
    /// scaffolded project would emit warnings about its own home page.
    #[test]
    fn validate_overrides_ignores_project_only_templates() {
        let dir = tempdir();
        std::fs::write(dir.join("home.html"), "<h1>welcome</h1>").unwrap();
        let v = validate_overrides(&dir);
        assert!(
            v.is_empty(),
            "home.html shadows nothing, must not appear in validation: {v:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Phase 12/c-fix — empty disk root (the common case for projects
    /// that ship no overrides) returns an empty Vec without touching
    /// the filesystem beyond `is_file` checks.
    #[test]
    fn validate_overrides_empty_dir_returns_empty_vec() {
        let dir = tempdir();
        let v = validate_overrides(&dir);
        assert!(v.is_empty(), "no files ⇒ no validations: {v:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Phase 12/c-fix — multiple overrides classify independently.
    /// Order is the order of EMBEDDED_TEMPLATES (deterministic); test
    /// asserts on the multi-set rather than the sequence.
    #[test]
    fn validate_overrides_handles_mixed_loaded_and_suspicious() {
        let dir = tempdir();
        let admin_dir = dir.join("admin");
        std::fs::create_dir_all(&admin_dir).unwrap();
        // A real-looking override of login.html.
        std::fs::write(
            admin_dir.join("login.html"),
            "{% extends \"admin/base.html\" %}\n{% block content %}hi{% endblock %}",
        )
        .unwrap();
        // And a stub of base.html.
        std::fs::write(admin_dir.join("base.html"), "<h1>TEST</h1>").unwrap();
        let v = validate_overrides(&dir);
        assert_eq!(v.len(), 2, "both files must be classified: {v:?}");
        let suspicious_count = v
            .iter()
            .filter(|x| matches!(x, OverrideValidation::Suspicious { .. }))
            .count();
        let loaded_count = v
            .iter()
            .filter(|x| matches!(x, OverrideValidation::Loaded { .. }))
            .count();
        assert_eq!(suspicious_count, 1, "exactly one Suspicious: {v:?}");
        assert_eq!(loaded_count, 1, "exactly one Loaded: {v:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 1.8.1 — a file in `templates/admin/` whose name doesn't match
    /// any embedded template (e.g. typo `baes.html` for `base.html`)
    /// produces no override but the developer thinks it does. The
    /// orphan-admin-file variant catches this inverted-typo case.
    #[test]
    fn validate_overrides_flags_orphan_admin_file() {
        let dir = tempdir();
        let admin_dir = dir.join("admin");
        std::fs::create_dir_all(&admin_dir).unwrap();
        // Typo for "base.html". Looks like an override, isn't.
        std::fs::write(
            admin_dir.join("baes.html"),
            "{% extends \"admin/base.html\" %}\n{% block content %}hi{% endblock %}",
        )
        .unwrap();
        let v = validate_overrides(&dir);
        assert_eq!(v.len(), 1, "exactly one orphan: {v:?}");
        match &v[0] {
            OverrideValidation::OrphanAdminFile { path } => {
                assert_eq!(path, "admin/baes.html");
            }
            other => panic!("expected OrphanAdminFile, got: {other:?}"),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 1.8.1 — a real override and an orphan in the same admin/ dir
    /// classify independently. Real override gets Loaded, typo gets
    /// OrphanAdminFile. Order of results is sorted by file name for
    /// determinism, so we can index into the Vec.
    #[test]
    fn validate_overrides_handles_real_and_orphan_in_same_admin_dir() {
        let dir = tempdir();
        let admin_dir = dir.join("admin");
        std::fs::create_dir_all(&admin_dir).unwrap();
        std::fs::write(
            admin_dir.join("base.html"),
            "{% block content %}real override{% endblock %}",
        )
        .unwrap();
        std::fs::write(admin_dir.join("typo.html"), "<h1>oops</h1>").unwrap();
        let v = validate_overrides(&dir);
        let loaded_count = v
            .iter()
            .filter(|x| matches!(x, OverrideValidation::Loaded { name, .. } if *name == "admin/base.html"))
            .count();
        let orphan_count = v
            .iter()
            .filter(|x| matches!(x, OverrideValidation::OrphanAdminFile { path } if path == "admin/typo.html"))
            .count();
        assert_eq!(loaded_count, 1, "real override must be Loaded: {v:?}");
        assert_eq!(orphan_count, 1, "typo must be OrphanAdminFile: {v:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 1.8.1 — non-html files in `templates/admin/` are skipped (so a
    /// `.gitkeep` or `.DS_Store` doesn't produce an orphan warning).
    #[test]
    fn validate_overrides_orphan_scan_skips_non_html() {
        let dir = tempdir();
        let admin_dir = dir.join("admin");
        std::fs::create_dir_all(&admin_dir).unwrap();
        std::fs::write(admin_dir.join(".gitkeep"), "").unwrap();
        std::fs::write(admin_dir.join("notes.txt"), "draft").unwrap();
        let v = validate_overrides(&dir);
        assert!(
            v.is_empty(),
            "non-html files in admin/ must not produce orphans: {v:?}"
        );
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

    /// Phase 10/b — base context fixture for `admin/user_view.html`.
    /// Matches the `UserViewCtx` shape in `admin/builtin.rs`. Tests
    /// override individual fields via `ctx[...] = …`.
    fn view_ctx_base() -> serde_json::Value {
        serde_json::json!({
            "csrf_token": "test-csrf",
            "user": {
                "id": 42,
                "email": "alice@example.com",
                "full_name": "Alice",
                "full_name_value": null,
                "role": "Staff",
                "is_admin": false,
                "is_developer": false,
                "is_active": true,
                "is_demo": false,
                "demo_label": null,
                "locale": null,
                "timezone": null,
                "created_at_iso": "2026-04-25 12:00 UTC",
                "last_seen_relative": "just now",
                "last_login_iso": "2026-04-25 11:50 UTC",
                "groups": [],
            },
            "users": [],
            "total": 1,
            "activity_count": 0,
            "permission_count": 0,
            "session_count": 0,
            "tab": "overview",
            "recent_events": [],
            "activity_page": 1,
            "activity_total_pages": 1,
            "permissions": [],
            "sessions": [],
            "project_fields": [],
            "can_edit": true,
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

    /// Phase 10/b — Overview tab renders the splitview shell + show-grid
    /// profile + recent-activity timeline. Groups appear as `<code>` chips
    /// in the show-grid Groups row.
    #[test]
    fn user_view_overview_renders_with_groups() {
        let t = Templates::new(None).unwrap();
        let mut ctx = view_ctx_base();
        ctx["user"]["groups"] = serde_json::json!(["Auditors", "Content Editors"]);
        let body = t.render("admin/user_view.html", &ctx).unwrap();

        // Splitview shell + Overview branch markers.
        assert!(body.contains("class=\"splitview\""), "must render the splitview shell");
        assert!(body.contains("class=\"show-grid\""), "Overview must render the show-grid");
        assert!(body.contains("class=\"stat-strip\""), "Overview must render the stat-strip");

        // Groups row content — chips appear inside the show-grid.
        assert!(body.contains("<code>Auditors</code>"));
        assert!(body.contains("<code>Content Editors</code>"));
    }

    /// Phase 10/b — empty groups render the muted "No groups" inline
    /// in the Groups row of the show-grid.
    #[test]
    fn user_view_overview_without_groups_shows_empty_marker() {
        let t = Templates::new(None).unwrap();
        let body = t.render("admin/user_view.html", &view_ctx_base()).unwrap();
        assert!(
            body.contains("No groups"),
            "empty-groups copy must appear in the show-grid Groups row"
        );
    }

    /// Phase 10/b — Activity tab renders the timeline component and a
    /// pager when there's more than one page. Inline-Delete contract
    /// from the pre-10/b template is intentionally gone; the destructive
    /// path lives behind `/admin/users/:id/delete` exclusively.
    #[test]
    fn user_view_activity_tab_renders_pager() {
        let t = Templates::new(None).unwrap();
        let mut ctx = view_ctx_base();
        ctx["tab"] = serde_json::json!("activity");
        ctx["activity_count"] = serde_json::json!(120);
        ctx["activity_page"] = serde_json::json!(2);
        ctx["activity_total_pages"] = serde_json::json!(3);
        ctx["recent_events"] = serde_json::json!([
            { "id": 1, "kind": "info",    "message": "Updated <strong>posts</strong> #5",
              "timestamp_relative": "1h ago", "actor": "user:42" },
            { "id": 2, "kind": "success", "message": "Created <strong>posts</strong> #5",
              "timestamp_relative": "2h ago", "actor": "user:42" },
        ]);
        let body = t.render("admin/user_view.html", &ctx).unwrap();

        assert!(body.contains("class=\"timeline\""), "Activity must render the timeline component");
        assert!(body.contains("tl-info"), "event kind must drive the dot color class");
        assert!(body.contains("class=\"pager\""), "Activity must render the pager when total_pages > 1");
        assert!(body.contains("?tab=activity&page=1"), "pager must link Prev to page-1");
        assert!(body.contains("?tab=activity&page=3"), "pager must link Next to page+1");
        // Tab links must NOT carry &page=… (per Phase 10/b spec — tab clicks reset page).
        assert!(!body.contains("?tab=overview&page="), "Overview tab link must strip page param");
        assert!(!body.contains("?tab=permissions&page="), "Permissions tab link must strip page param");
        assert!(!body.contains("?tab=sessions&page="), "Sessions tab link must strip page param");
    }

    /// Phase 10/b — Permissions tab renders direct + inherited grants
    /// with a source chip on each tile.
    #[test]
    fn user_view_permissions_tab_renders_with_sources() {
        let t = Templates::new(None).unwrap();
        let mut ctx = view_ctx_base();
        ctx["tab"] = serde_json::json!("permissions");
        ctx["permissions"] = serde_json::json!([
            { "name": "posts.add_post",    "source": "direct" },
            { "name": "posts.change_post", "source": "via Editors" },
        ]);
        let body = t.render("admin/user_view.html", &ctx).unwrap();

        assert!(body.contains("class=\"perm-grid\""), "Permissions must render perm-grid");
        assert!(body.contains("posts.add_post"));
        assert!(body.contains("posts.change_post"));
        assert!(body.contains("direct"), "direct grant must show the 'direct' source chip");
        assert!(body.contains("via Editors"), "inherited grant must show the source group");
    }

    /// Phase 10/b — Sessions tab renders the table; absent IP / UA
    /// columns render as the framework's "—" cell-empty marker. The
    /// session token is truncated to its first 7 chars.
    #[test]
    fn user_view_sessions_tab_truncates_token_and_handles_nulls() {
        let t = Templates::new(None).unwrap();
        let mut ctx = view_ctx_base();
        ctx["tab"] = serde_json::json!("sessions");
        ctx["sessions"] = serde_json::json!([
            {
                "token_short": "abc1234",
                "created_at_iso": "2026-05-01 10:00 UTC",
                "last_seen_relative": "5m ago",
                "ip": null,
                "user_agent": null,
            },
        ]);
        let body = t.render("admin/user_view.html", &ctx).unwrap();

        assert!(body.contains("<table class=\"table\""), "Sessions must render the table");
        assert!(body.contains("<code>abc1234</code>"), "token must render truncated, never full-length");
        // Null IP / UA fall back to the framework's empty-cell marker.
        assert!(body.contains("rio-cell-empty"), "absent IP / UA must render as the empty marker");
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

    /// Phase 10/c — when the project registers a `user_profile_extension`,
    /// the closure's `Vec<UserProfileSection>` lands in `project_fields`
    /// and the default `{% block project_user_fields %}` renders each
    /// section as a labeled show-grid. Empty `project_fields` must
    /// produce no extra markup — no orphan headings, no empty divs.
    #[test]
    fn user_view_overview_renders_project_fields_section() {
        let t = Templates::new(None).unwrap();
        let mut ctx = view_ctx_base();
        ctx["project_fields"] = serde_json::json!([
            {
                "label": "Halal certification",
                "rows": [
                    { "label": "Certified by", "value": "ICCV Halal Authority" },
                    { "label": "License #",    "value": "HC-2025-0042" },
                ],
            },
        ]);
        let body = t.render("admin/user_view.html", &ctx).unwrap();

        assert!(body.contains("Halal certification"), "section label must render");
        assert!(body.contains("Certified by"));
        assert!(body.contains("ICCV Halal Authority"));
        assert!(body.contains("License #"));
        assert!(body.contains("HC-2025-0042"));
    }

    /// Phase 10/c — empty project_fields must produce no extra section
    /// markup. Zero-config projects (no extension registered) get a
    /// clean Overview tab with no orphan headings.
    #[test]
    fn user_view_overview_omits_extension_when_project_fields_empty() {
        let t = Templates::new(None).unwrap();
        let body = t.render("admin/user_view.html", &view_ctx_base()).unwrap();
        // Fixture has project_fields=[] — no project label should appear.
        // Spot-check a few labels that appear only if any section rendered.
        assert!(
            !body.contains("Halal certification"),
            "no extension means no project section heading"
        );
    }

    /// Phase 10/b — demo flag drives a yellow badge in the identity
    /// strip of the detail-head; the badge must NOT render for a real
    /// user, and MUST include the demo_label when one is set.
    #[test]
    fn user_view_demo_badge_renders_only_for_demo_users() {
        let t = Templates::new(None).unwrap();
        // Real (non-demo) user: no DEMO badge.
        let body = t.render("admin/user_view.html", &view_ctx_base()).unwrap();
        assert!(
            !body.contains(">DEMO"),
            "DEMO badge must NOT render for a real user"
        );

        // Demo case: badge renders with the label.
        let mut demo_ctx = view_ctx_base();
        demo_ctx["user"]["is_demo"] = serde_json::Value::Bool(true);
        demo_ctx["user"]["demo_label"] = serde_json::Value::String("staff @ rustio.local".into());
        let demo_body = t.render("admin/user_view.html", &demo_ctx).unwrap();
        assert!(
            demo_body.contains(">DEMO"),
            "DEMO badge must render for a demo user"
        );
        assert!(
            demo_body.contains("staff @ rustio.local"),
            "demo label must appear in the badge"
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
