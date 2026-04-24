//! Template rendering. Rust code passes typed context; this module
//! owns everything about HTML generation.
//!
//! Loader order:
//!   1. Embedded defaults, compiled into the binary.
//!   2. Project-local `templates/` directory (user overrides win).
//!
//! A missing template is a 500 — never silent. The embedded defaults
//! cover the whole admin UI, so a clean project works with no disk
//! templates at all.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use minijinja::Environment;
use serde::Serialize;

use crate::error::{Error, Result};

pub struct Templates {
    env: Environment<'static>,
}

impl Templates {
    /// Build the environment. `project_templates_dir` is optional — when
    /// present, user overrides take precedence over the embedded defaults.
    pub fn new(project_templates_dir: Option<PathBuf>) -> Result<Arc<Self>> {
        let mut all: HashMap<String, String> = HashMap::new();
        for (name, body) in EMBEDDED_TEMPLATES {
            all.insert((*name).to_string(), (*body).to_string());
        }

        if let Some(root) = project_templates_dir {
            if root.exists() {
                for file in walkdir(&root) {
                    let relative = file
                        .strip_prefix(&root)
                        .map_err(|e| Error::Internal(format!("strip prefix: {e}")))?
                        .to_string_lossy()
                        .replace('\\', "/");
                    let body = std::fs::read_to_string(&file)?;
                    all.insert(relative, body);
                }
            }
        }

        let mut env = Environment::new();
        for (name, body) in all {
            env.add_template_owned(name.clone(), body)
                .map_err(|e| Error::Internal(format!("template {name}: {e}")))?;
        }

        Ok(Arc::new(Self { env }))
    }

    pub fn render<S: Serialize>(&self, name: &str, ctx: &S) -> Result<String> {
        let tmpl = self
            .env
            .get_template(name)
            .map_err(|e| Error::Internal(format!("template {name} not found: {e}")))?;
        tmpl.render(ctx)
            .map_err(|e| Error::Internal(format!("render {name}: {e}")))
    }
}

fn walkdir(root: &std::path::Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().and_then(|s| s.to_str()) == Some("html") {
                out.push(path);
            }
        }
    }
    out
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
    ("admin/users_list.html", include_str!("../assets/templates/admin/users_list.html")),
    ("admin/user_edit.html", include_str!("../assets/templates/admin/user_edit.html")),
    ("admin/groups_list.html", include_str!("../assets/templates/admin/groups_list.html")),
    ("admin/group_edit.html", include_str!("../assets/templates/admin/group_edit.html")),
    ("search.html", include_str!("../assets/templates/search.html")),
];

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Serialize;

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
}
