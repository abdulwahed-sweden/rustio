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
    ("admin/error.html", include_str!("../assets/templates/admin/error.html")),
    ("admin/object_history.html", include_str!("../assets/templates/admin/object_history.html")),
    ("admin/log_entries.html", include_str!("../assets/templates/admin/log_entries.html")),
    ("admin/users_list.html", include_str!("../assets/templates/admin/users_list.html")),
    ("admin/user_edit.html", include_str!("../assets/templates/admin/user_edit.html")),
    ("admin/groups_list.html", include_str!("../assets/templates/admin/groups_list.html")),
    ("admin/group_edit.html", include_str!("../assets/templates/admin/group_edit.html")),
    ("admin/includes/_field_errors.html", include_str!("../assets/templates/admin/includes/_field_errors.html")),
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
