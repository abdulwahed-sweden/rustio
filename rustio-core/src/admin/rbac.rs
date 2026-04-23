//! Role-based access control for the admin UI (0.10+).
//!
//! Four built-in roles map to a per-model permission matrix over four
//! actions. The matrix is hardcoded at this stage; DB-backed per-(role,
//! model) overrides are a future extension.
//!
//! This module is self-contained at stage 3 — no middleware or handler
//! consumes it yet. Stage 4 wires it into the admin request path and the
//! template context.
//!
//! ## Source of role data
//!
//! Roles are stored as strings in the existing `rustio_users.role`
//! column. No schema migration is required. [`Role::from_role_string`]
//! resolves the column value to a typed `Role`; unknown / empty values
//! resolve to `None`, meaning *no admin access at all* — the caller is
//! expected to 403 in that case.
//!
//! ## Backward compatibility with the `role` column
//!
//! Pre-0.10 only `"admin"` and `"user"` were recognised.
//!
//! - `"admin"` → [`Role::SuperAdmin`]. The legacy admin tier had
//!   unrestricted power; resolving it to SuperAdmin preserves that.
//!   Projects that want the restricted-admin tier introduced in 0.10.0
//!   should store `"restricted_admin"`.
//! - `"user"`, empty, unknown → `None`. Pre-0.10 these users had no
//!   admin access; they continue to have none.
//!
//! ## Permission matrix (defaults)
//!
//! "System table" = any table whose name begins with `rustio_` (the
//! framework's own `rustio_users`, `rustio_sessions`,
//! `rustio_admin_actions`, etc.).
//!
//! | role            | system tables     | app tables              |
//! |-----------------|-------------------|-------------------------|
//! | SuperAdmin      | view/create/edit/delete | view/create/edit/delete |
//! | Admin           | view              | view/create/edit/delete |
//! | Editor          | view              | view/create/edit        |
//! | Viewer          | view              | view                    |

use std::fmt;

/// Framework-owned roles. `#[non_exhaustive]` because we may introduce
/// additional built-in tiers (e.g. `Auditor`) in a minor release;
/// downstream code must not rely on exhaustive matching.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Role {
    /// Unrestricted access. The legacy `"admin"` role value resolves to
    /// this tier for back-compat.
    SuperAdmin,
    /// Full read/write on application models; view-only on framework
    /// system tables (`rustio_users`, sessions, audit log).
    Admin,
    /// View / create / edit on application models; no delete anywhere;
    /// view on system tables.
    Editor,
    /// View-only on every accessible model.
    Viewer,
}

/// The four actions a permission gates. Mapped one-to-one to the
/// fields of [`PermissionSet`]; kept as a typed enum so handlers can
/// `rbac::require(ctx, "posts", Permission::Delete)?` without stringly
/// typing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Permission {
    View,
    Create,
    Edit,
    Delete,
}

/// Boolean flags for the four actions on a single model. Passed into
/// the template context so the UI can hide or disable controls the
/// user can't use. Also consulted by handlers before they act.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize)]
pub struct PermissionSet {
    pub view: bool,
    pub create: bool,
    pub edit: bool,
    pub delete: bool,
}

impl PermissionSet {
    pub const NONE: Self = Self {
        view: false,
        create: false,
        edit: false,
        delete: false,
    };
    pub const VIEW_ONLY: Self = Self {
        view: true,
        create: false,
        edit: false,
        delete: false,
    };
    pub const NO_DELETE: Self = Self {
        view: true,
        create: true,
        edit: true,
        delete: false,
    };
    pub const ALL: Self = Self {
        view: true,
        create: true,
        edit: true,
        delete: true,
    };

    pub fn allows(&self, action: Permission) -> bool {
        match action {
            Permission::View => self.view,
            Permission::Create => self.create,
            Permission::Edit => self.edit,
            Permission::Delete => self.delete,
        }
    }
}

impl Role {
    /// Parse the stored `rustio_users.role` column value. Returns
    /// `None` for values that grant no admin access (empty, `"user"`,
    /// or an unknown string). Case-insensitive; accepts a few
    /// reasonable aliases (`super_admin`, `super-admin`).
    pub fn from_role_string(s: &str) -> Option<Role> {
        let normalised = s.trim().to_ascii_lowercase();
        match normalised.as_str() {
            "superadmin" | "super_admin" | "super-admin" => Some(Role::SuperAdmin),
            // Legacy: pre-0.10 had a single "admin" tier with full
            // power. Resolve to SuperAdmin so no project is silently
            // downgraded by the upgrade.
            "admin" => Some(Role::SuperAdmin),
            // New in 0.10: restricted-admin tier. Use this string when
            // you want the `Role::Admin` matrix rather than the
            // legacy-full-power behaviour.
            "restricted_admin" | "restricted-admin" => Some(Role::Admin),
            "editor" => Some(Role::Editor),
            "viewer" => Some(Role::Viewer),
            _ => None,
        }
    }

    /// Canonical wire string for the role, matching the values
    /// [`Self::from_role_string`] produces. Note this is **not**
    /// lossless with the legacy mapping — `Role::SuperAdmin` serialises
    /// to `"superadmin"`, while the legacy value `"admin"` also parses
    /// back as `Role::SuperAdmin`.
    pub fn as_str(self) -> &'static str {
        match self {
            Role::SuperAdmin => "superadmin",
            Role::Admin => "restricted_admin",
            Role::Editor => "editor",
            Role::Viewer => "viewer",
        }
    }

    /// Human-readable label for the role, suitable for rendering in the
    /// admin header. Unlike [`Self::as_str`], this is purely cosmetic.
    pub fn display_name(self) -> &'static str {
        match self {
            Role::SuperAdmin => "Super Admin",
            Role::Admin => "Admin",
            Role::Editor => "Editor",
            Role::Viewer => "Viewer",
        }
    }

    /// Resolve the permission matrix for this role on a specific model
    /// table. `model_table` is the raw SQL table name as it appears in
    /// the schema (e.g. `"posts"`, `"rustio_users"`). Unknown models
    /// get the same defaults as any app table — the caller is
    /// responsible for 404-ing models the schema doesn't know about.
    pub fn permissions_for(self, model_table: &str) -> PermissionSet {
        let system = is_system_table(model_table);
        match (self, system) {
            (Role::SuperAdmin, _) => PermissionSet::ALL,
            (Role::Admin, true) => PermissionSet::VIEW_ONLY,
            (Role::Admin, false) => PermissionSet::ALL,
            (Role::Editor, true) => PermissionSet::VIEW_ONLY,
            (Role::Editor, false) => PermissionSet::NO_DELETE,
            (Role::Viewer, _) => PermissionSet::VIEW_ONLY,
        }
    }

    /// Shorthand for `permissions_for(table).allows(action)`.
    pub fn can(self, model_table: &str, action: Permission) -> bool {
        self.permissions_for(model_table).allows(action)
    }
}

impl fmt::Display for Role {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Framework tables use the `rustio_` prefix. Project tables never
/// should — this is a contract the migration generator enforces. We
/// treat the prefix as authoritative here; projects that smuggle
/// `rustio_`-prefixed tables into their schema get the restrictive
/// matrix, which is the safe default.
pub fn is_system_table(table: &str) -> bool {
    table.starts_with("rustio_")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_admin_resolves_to_super_admin() {
        assert_eq!(Role::from_role_string("admin"), Some(Role::SuperAdmin));
        assert_eq!(Role::from_role_string("ADMIN"), Some(Role::SuperAdmin));
    }

    #[test]
    fn legacy_user_and_unknown_resolve_to_none() {
        assert_eq!(Role::from_role_string("user"), None);
        assert_eq!(Role::from_role_string(""), None);
        assert_eq!(Role::from_role_string("   "), None);
        assert_eq!(Role::from_role_string("nobody"), None);
    }

    #[test]
    fn canonical_strings_round_trip() {
        for role in [Role::SuperAdmin, Role::Admin, Role::Editor, Role::Viewer] {
            assert_eq!(
                Role::from_role_string(role.as_str()),
                Some(role),
                "round-trip failed for {role:?}"
            );
        }
    }

    #[test]
    fn super_admin_can_do_everything_everywhere() {
        let r = Role::SuperAdmin;
        assert_eq!(r.permissions_for("posts"), PermissionSet::ALL);
        assert_eq!(r.permissions_for("rustio_users"), PermissionSet::ALL);
        assert_eq!(r.permissions_for("rustio_sessions"), PermissionSet::ALL);
    }

    #[test]
    fn admin_cannot_write_system_tables() {
        let r = Role::Admin;
        assert_eq!(r.permissions_for("posts"), PermissionSet::ALL);
        assert_eq!(r.permissions_for("rustio_users"), PermissionSet::VIEW_ONLY);
        assert!(!r.can("rustio_users", Permission::Delete));
        assert!(!r.can("rustio_users", Permission::Edit));
        assert!(r.can("rustio_users", Permission::View));
    }

    #[test]
    fn editor_cannot_delete_anywhere() {
        let r = Role::Editor;
        assert!(!r.can("posts", Permission::Delete));
        assert!(r.can("posts", Permission::Edit));
        assert!(r.can("posts", Permission::Create));
        assert!(!r.can("rustio_users", Permission::Edit));
    }

    #[test]
    fn viewer_only_views() {
        let r = Role::Viewer;
        assert_eq!(r.permissions_for("posts"), PermissionSet::VIEW_ONLY);
        assert_eq!(r.permissions_for("rustio_users"), PermissionSet::VIEW_ONLY);
    }

    #[test]
    fn is_system_table_matches_prefix() {
        assert!(is_system_table("rustio_users"));
        assert!(is_system_table("rustio_admin_actions"));
        assert!(!is_system_table("posts"));
        assert!(!is_system_table("my_rustio_table"));
    }

    #[test]
    fn display_name_is_humanised() {
        assert_eq!(Role::SuperAdmin.display_name(), "Super Admin");
        assert_eq!(Role::Admin.display_name(), "Admin");
    }

    #[test]
    fn permission_set_allows_matches_field() {
        let ps = PermissionSet::NO_DELETE;
        assert!(ps.allows(Permission::View));
        assert!(ps.allows(Permission::Create));
        assert!(ps.allows(Permission::Edit));
        assert!(!ps.allows(Permission::Delete));
    }
}
