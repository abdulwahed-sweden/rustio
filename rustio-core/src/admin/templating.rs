//! Admin UI template engine.
//!
//! Stage 2 scaffolding for the 0.10.0 admin rebuild. The admin is rendered
//! by `minijinja` against templates shipped at
//! `rustio-core/assets/templates/`, bundled into the binary via
//! `include_str!`. User projects may override any template by placing a
//! file of the same relative path under their project's `templates/`
//! directory — the loader chain is filesystem-first, embedded-fallback.
//!
//! Stage 4a onwards consumes [`env()`] — the process-wide
//! `Arc<Environment>` shared by every admin handler.

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use minijinja::{Environment, ErrorKind};

/// Process-wide admin template environment. Built once on first
/// access from [`TemplatingConfig::default()`]; auto-reload is
/// enabled under `debug_assertions`, so template edits are picked up
/// without a restart in development.
pub fn env() -> Arc<Environment<'static>> {
    static CELL: OnceLock<Arc<Environment<'static>>> = OnceLock::new();
    CELL.get_or_init(|| Arc::new(environment(&TemplatingConfig::default())))
        .clone()
}

/// Runtime configuration for the admin template environment.
#[derive(Clone, Debug)]
pub struct TemplatingConfig {
    /// Directory scanned for per-project admin template overrides. When
    /// `None`, only the embedded defaults are served.
    pub overrides_root: Option<PathBuf>,
    /// Re-read templates on every render. Intended for development; keep
    /// off in release to avoid per-request filesystem work.
    pub auto_reload: bool,
}

impl Default for TemplatingConfig {
    fn default() -> Self {
        let overrides_root = std::env::current_dir()
            .ok()
            .map(|cwd| cwd.join("templates"))
            .filter(|p| p.is_dir());
        Self {
            overrides_root,
            auto_reload: cfg!(debug_assertions),
        }
    }
}

/// Construct a fresh `minijinja::Environment` with the framework's
/// embedded templates pre-registered and an optional filesystem override
/// root. Callers typically wrap the result in `Arc` and cache it.
pub fn environment(config: &TemplatingConfig) -> Environment<'static> {
    let mut env = Environment::new();
    env.set_auto_escape_callback(|name| {
        if name.ends_with(".html") || name.ends_with(".htm") {
            minijinja::AutoEscape::Html
        } else {
            minijinja::AutoEscape::None
        }
    });

    let overrides_root: Option<Arc<Path>> = config.overrides_root.clone().map(Into::into);
    let auto_reload = config.auto_reload;

    if auto_reload {
        let overrides_root = overrides_root.clone();
        env.set_loader(move |name| load_template(name, overrides_root.as_deref()));
    } else {
        let resolved = resolve_all(overrides_root.as_deref());
        env.set_loader(move |name| Ok(resolved.lookup(name).map(|s| s.to_string())));
    }
    env
}

fn load_template(
    name: &str,
    overrides_root: Option<&Path>,
) -> Result<Option<String>, minijinja::Error> {
    if let Some(root) = overrides_root {
        match read_override(root, name) {
            Ok(Some(source)) => return Ok(Some(source)),
            Ok(None) => {}
            Err(err) => {
                return Err(minijinja::Error::new(
                    ErrorKind::InvalidOperation,
                    format!("reading override template {name}: {err}"),
                ));
            }
        }
    }
    Ok(embedded(name).map(str::to_string))
}

fn read_override(root: &Path, name: &str) -> std::io::Result<Option<String>> {
    let path = safe_join(root, name);
    if !path.is_file() {
        return Ok(None);
    }
    std::fs::read_to_string(path).map(Some)
}

/// Reject traversal attempts. Template names are framework-controlled, but
/// treating them defensively keeps future callers honest.
fn safe_join(root: &Path, name: &str) -> PathBuf {
    let mut out = root.to_path_buf();
    for segment in name.split('/') {
        if segment.is_empty() || segment == "." || segment == ".." {
            continue;
        }
        out.push(segment);
    }
    out
}

struct ResolvedSet {
    cache: std::collections::HashMap<&'static str, String>,
}

impl ResolvedSet {
    fn lookup(&self, name: &str) -> Option<&str> {
        if let Some(s) = self.cache.get(name) {
            return Some(s.as_str());
        }
        // Fallback to embedded directly — callers outside the known set
        // still work, just without override support after snapshot.
        embedded(name)
    }
}

fn resolve_all(overrides_root: Option<&Path>) -> ResolvedSet {
    let mut cache = std::collections::HashMap::with_capacity(EMBEDDED.len());
    for (name, default_source) in EMBEDDED {
        let source = overrides_root
            .and_then(|root| std::fs::read_to_string(safe_join(root, name)).ok())
            .unwrap_or_else(|| (*default_source).to_string());
        cache.insert(*name, source);
    }
    ResolvedSet { cache }
}

/// Compile-time bundled defaults. Extend this table when you add a new
/// framework-owned admin template. The loader also serves names outside
/// this set via filesystem overrides, so extra user templates keep
/// working.
const EMBEDDED: &[(&str, &str)] = &[
    (
        "base.html",
        include_str!("../../assets/templates/base.html"),
    ),
    (
        "base_admin.html",
        include_str!("../../assets/templates/base_admin.html"),
    ),
    (
        "includes/header.html",
        include_str!("../../assets/templates/includes/header.html"),
    ),
    (
        "includes/sidebar.html",
        include_str!("../../assets/templates/includes/sidebar.html"),
    ),
    (
        "includes/footer.html",
        include_str!("../../assets/templates/includes/footer.html"),
    ),
    (
        "admin/dashboard.html",
        include_str!("../../assets/templates/admin/dashboard.html"),
    ),
    (
        "admin/list.html",
        include_str!("../../assets/templates/admin/list.html"),
    ),
    (
        "admin/form.html",
        include_str!("../../assets/templates/admin/form.html"),
    ),
    (
        "admin/profile.html",
        include_str!("../../assets/templates/admin/profile.html"),
    ),
    (
        "admin/password_change.html",
        include_str!("../../assets/templates/admin/password_change.html"),
    ),
    (
        "admin/password_change_done.html",
        include_str!("../../assets/templates/admin/password_change_done.html"),
    ),
    (
        "admin/actions.html",
        include_str!("../../assets/templates/admin/actions.html"),
    ),
    (
        "admin/suggestion_review.html",
        include_str!("../../assets/templates/admin/suggestion_review.html"),
    ),
    (
        "admin/suggestion_applied.html",
        include_str!("../../assets/templates/admin/suggestion_applied.html"),
    ),
    (
        "auth/login.html",
        include_str!("../../assets/templates/auth/login.html"),
    ),
    (
        "auth/forbidden.html",
        include_str!("../../assets/templates/auth/forbidden.html"),
    ),
    (
        "auth/not_found.html",
        include_str!("../../assets/templates/auth/not_found.html"),
    ),
];

fn embedded(name: &str) -> Option<&'static str> {
    EMBEDDED
        .iter()
        .find_map(|(n, s)| if *n == name { Some(*s) } else { None })
}

/// Framework CSS/JS served under `/admin/static/…`. The tuples are
/// `(path_under_admin_static, content_type, bytes)`. As of 0.11.x the
/// admin no longer ships Bootstrap — the design system lives in
/// `admin.css`, compiled at build time by `build.rs` from the Tailwind
/// v4 source at `assets/static/admin.css`. The compiled bytes land in
/// `OUT_DIR/admin.css` and are inlined here via `include_bytes!`.
pub const BUNDLED_ASSETS: &[(&str, &str, &[u8])] = &[
    (
        "admin.css",
        "text/css; charset=utf-8",
        include_bytes!(concat!(env!("OUT_DIR"), "/admin.css")),
    ),
    (
        "app.js",
        "application/javascript; charset=utf-8",
        include_bytes!("../../assets/static/app.js"),
    ),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_embedded_template_parses() {
        let env = environment(&TemplatingConfig {
            overrides_root: None,
            auto_reload: false,
        });
        for (name, _) in EMBEDDED {
            env.get_template(name)
                .unwrap_or_else(|e| panic!("embedded template {name} failed to parse: {e}"));
        }
    }

    #[test]
    fn dashboard_renders_with_minimum_context() {
        let env = environment(&TemplatingConfig {
            overrides_root: None,
            auto_reload: false,
        });
        let tmpl = env.get_template("admin/dashboard.html").unwrap();
        let out = tmpl
            .render(minijinja::context! {
                design => minijinja::context! { project_name => "Test" },
                current_user => minijinja::Value::from(()),
                sidebar_entries => Vec::<minijinja::Value>::new(),
                dashboard_cards => Vec::<minijinja::Value>::new(),
            })
            .unwrap();
        // The 0.10.x design system titles the dashboard page
        // "Your workspace" inside the page-head, and "Overview ·
        // <project>" in the <title>. Either signals the template
        // resolved + rendered with the design context dict.
        assert!(out.contains("Overview"));
        assert!(out.contains("workspace"));
        assert!(out.contains("Test"));
    }

    #[test]
    fn safe_join_blocks_traversal() {
        let root = PathBuf::from("/tmp/root");
        assert_eq!(safe_join(&root, "../etc/passwd"), root.join("etc/passwd"));
        assert_eq!(safe_join(&root, "./a"), root.join("a"));
    }

    #[test]
    fn bundled_assets_are_non_empty() {
        for (path, _ctype, bytes) in BUNDLED_ASSETS {
            assert!(!bytes.is_empty(), "bundled asset {path} is empty");
        }
    }

    #[test]
    fn env_accessor_is_cached() {
        let a = super::env();
        let b = super::env();
        assert!(
            Arc::ptr_eq(&a, &b),
            "env() should return the same Arc on repeated calls"
        );
        a.get_template("base.html").expect("base.html missing");
    }
}
