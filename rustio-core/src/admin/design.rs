//! Admin design customisation.
//!
//! RustIO's admin is **framework-owned** — projects don't edit
//! templates or stylesheets. What they *can* change is the thin layer
//! of visual identity: logo initial, project display name, primary /
//! accent colours, density. Those values live in `rustio.design.json`
//! at the project root and are loaded once per process via
//! [`Design::global`].
//!
//! The config is **visual only**. It cannot alter page structure,
//! routing, form semantics, or any admin behaviour. Any value outside
//! the accepted range (e.g. a colour string containing `;` or `}`) is
//! silently replaced with the safe default at render time so a bad
//! config can't break the admin.

use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

/// Visual-only admin design config.
///
/// Defaults produce a calm, slate-and-indigo look that works out of
/// the box. Projects override fields individually; unspecified fields
/// fall back to the default.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Design {
    /// Name shown in the sidebar and page titles, e.g. "Workflowdesk".
    pub project_name: String,
    /// Single character rendered in the square logo mark. Projects
    /// using longer glyphs should pick a wide character, e.g. "◈".
    pub logo_initial: String,
    /// CSS colour used for the primary action button and the sidebar
    /// logo-mark background. Accept hex (`#0f172a`), rgb(), hsl(), or
    /// named colours. Values containing `;`, `{`, `}`, `<`, or `\`
    /// are rejected and replaced with the default at render time.
    pub primary_color: String,
    /// CSS colour used for focus rings and hyperlinks.
    pub accent_color: String,
    /// Row density for tables and forms.
    pub density: Density,
}

/// Row-density modes for tables, cards, and forms.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Density {
    /// Default — relaxed spacing, 14px cell vertical padding.
    #[default]
    Comfortable,
    /// Tighter spacing. 10px cell vertical padding. Surfaces more
    /// data per screen; sacrifices some readability.
    Compact,
}

impl Default for Design {
    fn default() -> Self {
        Self {
            project_name: "RustIO".to_string(),
            logo_initial: "R".to_string(),
            // Rust-600 from the ink+rust design system — the single
            // brand accent. Overrideable via rustio.design.json.
            primary_color: "#B84318".to_string(),
            accent_color: "#B84318".to_string(),
            density: Density::Comfortable,
        }
    }
}

impl Design {
    /// Load from `rustio.design.json` in the current working
    /// directory, or return defaults if the file is missing /
    /// unreadable / malformed.
    ///
    /// Silently falls back on any error. Logging the parse failure is
    /// a project concern — we never want a bad design config to block
    /// the admin from rendering.
    pub fn load() -> Self {
        let path = std::path::Path::new("rustio.design.json");
        if let Ok(bytes) = std::fs::read(path) {
            if let Ok(parsed) = serde_json::from_slice::<Design>(&bytes) {
                return parsed;
            }
        }
        Self::default()
    }

    /// Process-global instance. Lazily loaded on first access. Not
    /// reloadable at runtime — restart the server to pick up a new
    /// config. This matches the static-asset posture of the rest of
    /// the admin.
    pub fn global() -> &'static Self {
        static INSTANCE: OnceLock<Design> = OnceLock::new();
        INSTANCE.get_or_init(Design::load)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_values_are_reasonable() {
        let d = Design::default();
        assert_eq!(d.project_name, "RustIO");
        assert_eq!(d.logo_initial, "R");
        assert!(d.primary_color.starts_with('#'));
        assert!(matches!(d.density, Density::Comfortable));
    }

    #[test]
    fn density_serializes_as_snake_case() {
        let d = Density::Comfortable;
        let s = serde_json::to_string(&d).unwrap();
        assert_eq!(s, "\"comfortable\"");
        let d2 = Density::Compact;
        let s2 = serde_json::to_string(&d2).unwrap();
        assert_eq!(s2, "\"compact\"");
    }

    #[test]
    fn parse_rejects_unknown_fields() {
        let json = r#"{"project_name":"X","surprise":"yes"}"#;
        let parsed = serde_json::from_str::<Design>(json);
        assert!(
            parsed.is_err(),
            "deny_unknown_fields must reject `surprise`"
        );
    }

    #[test]
    fn parse_accepts_partial_config() {
        // Using r##"..."## because the JSON contains `"#` which would
        // otherwise terminate a single-hash raw string.
        let json = r##"{"project_name":"Workflowdesk","primary_color":"#1e40af"}"##;
        let d: Design = serde_json::from_str(json).unwrap();
        assert_eq!(d.project_name, "Workflowdesk");
        assert_eq!(d.primary_color, "#1e40af");
        // Missing fields fall back to defaults.
        assert_eq!(d.logo_initial, "R");
        assert_eq!(d.accent_color, "#B84318");
    }
}
