//! Authentication & authorization.
//!
//! Three pieces:
//! - `users.rs`       — user records, password hashing, login/logout
//! - `sessions.rs`    — DB-backed sessions with expiry cleanup
//! - `permissions.rs` — granular permissions + groups + the
//!   `authorize!` check used throughout the admin
//!
//! A user belongs to zero or more groups. Permissions come from two
//! sources: (a) direct assignments on the user, (b) inherited from
//! the user's groups. The permission string is
//! `"<app>.<action>_<model>"` — e.g. `"posts.change_post"`.

mod permissions;
mod role;
mod sessions;
mod users;

pub use permissions::{
    add_user_to_group, bootstrap_default_groups, check_permission, create_group, grant_to_group,
    grant_to_user, init_permission_tables, lazy_attach_permissions, permissions_for_user,
    register_model_permissions, remove_user_from_group, Permission, PermissionError, Superuser,
};
pub(crate) use permissions::invalidate_user_cache;
pub use role::Role;
pub use sessions::{
    create_session, delete_session, identity_from_session, init_session_tables,
    purge_expired_sessions, session_token_from_cookie, SESSION_COOKIE,
};
pub use users::{
    bootstrap_demo_users, create_user, find_user_by_email, hash_password, init_user_tables, login,
    migrate_user_schema, set_password, update_user_role, verify_password, would_orphan_developers,
    Identity, StoredUser,
};

use crate::error::Result;
use crate::orm::Db;

/// Shared serialization lock for tests that toggle `RUSTIO_DEMO_MODE`
/// (or any other process env var). `tokio::test`s run on a thread
/// pool and `std::env::set_var` mutates process state — without a
/// process-wide lock, parallel tests stomp each other's env state.
/// Using `tokio::sync::Mutex` so it can be held across `.await` (the
/// `await_holding_lock` clippy lint forbids `std::sync::Mutex`).
#[cfg(test)]
pub(crate) static TEST_ENV_LOCK: tokio::sync::Mutex<()> =
    tokio::sync::Mutex::const_new(());

/// Initialise every auth-related table. Safe to call on every boot.
pub async fn init_tables(db: &Db) -> Result<()> {
    init_user_tables(db).await?;
    // Phase 7a/0.5: 5-tier role + demo columns. Idempotent.
    migrate_user_schema(db).await?;
    init_session_tables(db).await?;
    init_permission_tables(db).await?;
    Ok(())
}
