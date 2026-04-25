//! Granular permissions with groups.
//!
//! Data model:
//!   rustio_permissions         (id, name, description)
//!   rustio_groups              (id, name, description)
//!   rustio_group_permissions   (group_id, permission_id)
//!   rustio_user_groups         (user_id, group_id)
//!   rustio_user_permissions    (user_id, permission_id)    -- direct grants
//!
//! Permission naming convention: "<app>.<action>_<model>", e.g.
//!   "posts.add_post", "posts.change_post", "posts.delete_post",
//!   "posts.view_post"
//!
//! An Admin-role user automatically has every permission. Staff and
//! User roles are checked against the tables above.
//!
//! Permissions for a user are cached in a `DashMap<user_id, (Vec<String>, expires)>`
//! with a 60-second TTL so hot paths don't hit the DB. A write to the
//! permission tables calls `invalidate_user_cache(user_id)`.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

use dashmap::DashMap;
use once_cell::sync::Lazy;
use sqlx::Row as SqlxRow;

use crate::error::{Error, Result};
use crate::orm::Db;

use super::users::Identity;

#[cfg(test)]
use super::role::Role;

/// Marker type used by the authorize! macro for fast-paths on admins.
pub struct Superuser;

#[derive(Debug, Clone)]
pub struct Permission {
    pub id: i64,
    pub name: String,
    pub description: String,
}

#[derive(Debug, thiserror::Error)]
pub enum PermissionError {
    #[error("permission `{0}` not found")]
    Missing(String),
    #[error("user not found")]
    NoSuchUser,
    #[error("group not found")]
    NoSuchGroup,
}

// --- schema ---------------------------------------------------------------

pub async fn init_permission_tables(db: &Db) -> Result<()> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS rustio_permissions (
            id          BIGSERIAL PRIMARY KEY,
            name        TEXT NOT NULL UNIQUE,
            description TEXT NOT NULL DEFAULT '',
            created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
        )",
    )
    .execute(db.pool())
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS rustio_groups (
            id          BIGSERIAL PRIMARY KEY,
            name        TEXT NOT NULL UNIQUE,
            description TEXT NOT NULL DEFAULT '',
            created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
        )",
    )
    .execute(db.pool())
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS rustio_group_permissions (
            group_id      BIGINT NOT NULL REFERENCES rustio_groups(id)      ON DELETE CASCADE,
            permission_id BIGINT NOT NULL REFERENCES rustio_permissions(id) ON DELETE CASCADE,
            PRIMARY KEY (group_id, permission_id)
        )",
    )
    .execute(db.pool())
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS rustio_user_groups (
            user_id  BIGINT NOT NULL REFERENCES rustio_users(id)  ON DELETE CASCADE,
            group_id BIGINT NOT NULL REFERENCES rustio_groups(id) ON DELETE CASCADE,
            PRIMARY KEY (user_id, group_id)
        )",
    )
    .execute(db.pool())
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS rustio_user_permissions (
            user_id       BIGINT NOT NULL REFERENCES rustio_users(id)       ON DELETE CASCADE,
            permission_id BIGINT NOT NULL REFERENCES rustio_permissions(id) ON DELETE CASCADE,
            PRIMARY KEY (user_id, permission_id)
        )",
    )
    .execute(db.pool())
    .await?;

    Ok(())
}

// --- cache ----------------------------------------------------------------

struct CacheEntry {
    perms: Arc<HashSet<String>>,
    expires: Instant,
}

static PERM_CACHE: Lazy<DashMap<i64, CacheEntry>> = Lazy::new(DashMap::new);

const PERM_CACHE_TTL: Duration = Duration::from_secs(60);

pub(crate) fn invalidate_user_cache(user_id: i64) {
    PERM_CACHE.remove(&user_id);
}

fn invalidate_group_cache(db: &Db, group_id: i64) {
    // Users in this group need their cached permission sets evicted.
    // Fire-and-forget — the TTL will catch anything we miss.
    let db = db.clone();
    tokio::spawn(async move {
        let rows = sqlx::query("SELECT user_id FROM rustio_user_groups WHERE group_id = $1")
            .bind(group_id)
            .fetch_all(db.pool())
            .await
            .unwrap_or_default();
        for r in rows {
            if let Ok(uid) = r.try_get::<i64, _>("user_id") {
                invalidate_user_cache(uid);
            }
        }
    });
}

// --- reads ----------------------------------------------------------------

/// All permission names belonging to the given user — direct + via
/// groups — unioned into one set. Cached for 60s.
pub async fn permissions_for_user(db: &Db, user_id: i64) -> Result<Arc<HashSet<String>>> {
    if let Some(e) = PERM_CACHE.get(&user_id) {
        if e.expires > Instant::now() {
            return Ok(e.perms.clone());
        }
    }

    let rows = sqlx::query(
        "SELECT DISTINCT p.name
           FROM rustio_permissions p
           LEFT JOIN rustio_user_permissions up ON up.permission_id = p.id
           LEFT JOIN rustio_group_permissions gp ON gp.permission_id = p.id
           LEFT JOIN rustio_user_groups ug ON ug.group_id = gp.group_id
          WHERE up.user_id = $1 OR ug.user_id = $1",
    )
    .bind(user_id)
    .fetch_all(db.pool())
    .await?;

    let mut set = HashSet::with_capacity(rows.len());
    for r in rows {
        if let Ok(name) = r.try_get::<String, _>("name") {
            set.insert(name);
        }
    }
    let arc = Arc::new(set);
    PERM_CACHE.insert(
        user_id,
        CacheEntry {
            perms: arc.clone(),
            expires: Instant::now() + PERM_CACHE_TTL,
        },
    );
    Ok(arc)
}

/// Ask "does this identity have permission X?".
///
/// Order of checks (load-bearing — see Phase 7a/0.5/sec2):
/// 1. **`is_active`** — an inactive user is denied even if their role
///    would bypass group checks. Defense-in-depth: `login_guard` already
///    rejects inactive sessions at the panel boundary, but if a future
///    code path calls `check_permission` without the guard, the inactive
///    check here is the second line.
/// 2. **`bypasses_group_checks`** — Administrator and Developer skip the
///    M2M lookup; every other tier consults the tables.
pub async fn check_permission(db: &Db, identity: &Identity, permission: &str) -> Result<bool> {
    if !identity.is_active {
        return Ok(false);
    }
    if identity.role.bypasses_group_checks() {
        return Ok(true);
    }
    let perms = permissions_for_user(db, identity.user_id).await?;
    Ok(perms.contains(permission))
}

// --- writes ---------------------------------------------------------------

async fn permission_id(db: &Db, name: &str) -> Result<i64> {
    // Look up first, then insert if missing — lets the caller use
    // convenient short names without pre-seeding the table.
    if let Some(row) = sqlx::query("SELECT id FROM rustio_permissions WHERE name = $1")
        .bind(name)
        .fetch_optional(db.pool())
        .await?
    {
        return row.try_get("id").map_err(|e| Error::Internal(format!("{e}")));
    }
    let row = sqlx::query(
        "INSERT INTO rustio_permissions (name, description)
         VALUES ($1, $2)
         ON CONFLICT (name) DO UPDATE SET description = rustio_permissions.description
         RETURNING id",
    )
    .bind(name)
    .bind("")
    .fetch_one(db.pool())
    .await?;
    row.try_get("id").map_err(|e| Error::Internal(format!("{e}")))
}

pub async fn grant_to_user(db: &Db, user_id: i64, permission: &str) -> Result<()> {
    let pid = permission_id(db, permission).await?;
    sqlx::query(
        "INSERT INTO rustio_user_permissions (user_id, permission_id)
         VALUES ($1, $2)
         ON CONFLICT DO NOTHING",
    )
    .bind(user_id)
    .bind(pid)
    .execute(db.pool())
    .await?;
    invalidate_user_cache(user_id);
    Ok(())
}

pub async fn grant_to_group(db: &Db, group_id: i64, permission: &str) -> Result<()> {
    let pid = permission_id(db, permission).await?;
    sqlx::query(
        "INSERT INTO rustio_group_permissions (group_id, permission_id)
         VALUES ($1, $2)
         ON CONFLICT DO NOTHING",
    )
    .bind(group_id)
    .bind(pid)
    .execute(db.pool())
    .await?;
    invalidate_group_cache(db, group_id);
    Ok(())
}

pub async fn create_group(db: &Db, name: &str, description: &str) -> Result<i64> {
    let row = sqlx::query(
        "INSERT INTO rustio_groups (name, description)
         VALUES ($1, $2)
         RETURNING id",
    )
    .bind(name)
    .bind(description)
    .fetch_one(db.pool())
    .await?;
    row.try_get("id").map_err(|e| Error::Internal(format!("{e}")))
}

pub async fn add_user_to_group(db: &Db, user_id: i64, group_id: i64) -> Result<()> {
    sqlx::query(
        "INSERT INTO rustio_user_groups (user_id, group_id)
         VALUES ($1, $2)
         ON CONFLICT DO NOTHING",
    )
    .bind(user_id)
    .bind(group_id)
    .execute(db.pool())
    .await?;
    invalidate_user_cache(user_id);
    Ok(())
}

pub async fn remove_user_from_group(db: &Db, user_id: i64, group_id: i64) -> Result<()> {
    sqlx::query("DELETE FROM rustio_user_groups WHERE user_id = $1 AND group_id = $2")
        .bind(user_id)
        .bind(group_id)
        .execute(db.pool())
        .await?;
    invalidate_user_cache(user_id);
    Ok(())
}

/// For an admin model named `posts`, register the canonical four
/// permissions: add_post, change_post, delete_post, view_post. Idempotent.
pub async fn register_model_permissions(
    db: &Db,
    app: &str,
    singular: &str,
) -> Result<()> {
    let actions = ["add", "change", "delete", "view"];
    for action in actions {
        let name = format!("{app}.{action}_{singular}");
        let _ = permission_id(db, &name).await?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Phase 7a/0.5/c — default group bootstrap + lazy permission attachment.
//
// Both functions are gated behind `RUSTIO_DEMO_MODE=1`. Without the flag
// they're no-ops, so production deployments never accidentally seed demo
// data. The flag itself is operator-facing (set in env, not config).
// ---------------------------------------------------------------------------

/// One row in the `DEFAULT_GROUP_SPECS` table: a group name + its
/// permission policy expressed in terms of registered models.
struct GroupSpec {
    name: &'static str,
    description: &'static str,
    model_perms: GroupModelPerms,
}

/// How a group's permissions relate to registered admin models.
enum GroupModelPerms {
    /// Apply these actions to **every** registered (non-core) model.
    /// e.g. `All(&["view"])` for read-only Auditors. Wildcard expansion
    /// happens at attach time so adding a new model auto-extends the
    /// group's access on next startup.
    All(&'static [&'static str]),
    /// Per-model rules. Each tuple is `(model_codename, actions)` where
    /// `model_codename` is the model's `singular_name` lowercased
    /// (`"translator"`, not `"Translator"`). Models that aren't
    /// registered are silently skipped — the spec lists every model
    /// the group could plausibly own; reality boots with whatever
    /// subset is registered.
    Specific(&'static [(&'static str, &'static [&'static str])]),
}

const DEFAULT_GROUP_SPECS: &[GroupSpec] = &[
    GroupSpec {
        name: "Auditors",
        description: "Read-only access to all models",
        model_perms: GroupModelPerms::All(&["view"]),
    },
    GroupSpec {
        name: "Content Editors",
        description: "Create, view, and edit content models",
        model_perms: GroupModelPerms::Specific(&[
            ("project", &["view", "add", "change"]),
            ("language", &["view", "add", "change"]),
        ]),
    },
    GroupSpec {
        name: "HR Managers",
        description: "Manage freelancers and their skills",
        model_perms: GroupModelPerms::Specific(&[
            ("translator", &["view", "add", "change", "delete"]),
            ("skill", &["view", "add", "change", "delete"]),
        ]),
    },
    GroupSpec {
        name: "Finance",
        description: "Manage contracts and billing",
        model_perms: GroupModelPerms::Specific(&[
            ("contract", &["view", "add", "change", "delete"]),
            ("invoice", &["view", "add", "change", "delete"]),
            ("payment", &["view", "add", "change", "delete"]),
        ]),
    },
    GroupSpec {
        name: "Project Coordinators",
        description: "Coordinate projects and assignments",
        model_perms: GroupModelPerms::Specific(&[
            ("project", &["view", "add", "change", "delete"]),
            ("assignment", &["view", "add", "change", "delete"]),
            ("timeentry", &["view", "add", "change", "delete"]),
        ]),
    },
    GroupSpec {
        name: "System Operators",
        description: "View and edit, no destructive operations",
        model_perms: GroupModelPerms::All(&["view", "change"]),
    },
];

fn demo_mode_enabled() -> bool {
    std::env::var("RUSTIO_DEMO_MODE").as_deref() == Ok("1")
}

/// Insert each default group with `ON CONFLICT (name) DO NOTHING`.
/// Idempotent across restarts and against admin-created groups —
/// duplicates by name simply skip without bumping any state.
///
/// Gated by `RUSTIO_DEMO_MODE=1`. Without the flag this is a no-op
/// so production deploys never accidentally seed demo data.
pub async fn bootstrap_default_groups(db: &Db) -> Result<()> {
    if !demo_mode_enabled() {
        return Ok(());
    }
    for spec in DEFAULT_GROUP_SPECS {
        sqlx::query(
            "INSERT INTO rustio_groups (name, description) \
             VALUES ($1, $2) \
             ON CONFLICT (name) DO NOTHING",
        )
        .bind(spec.name)
        .bind(spec.description)
        .execute(db.pool())
        .await?;
    }
    log::info!(
        "RUSTIO_DEMO_MODE: ensured {} default groups exist",
        DEFAULT_GROUP_SPECS.len()
    );
    Ok(())
}

/// For each default group, attach the permissions it can resolve from
/// the currently-registered models. `All(&[…])` becomes one perm per
/// non-core entry; `Specific(&[…])` becomes one perm per matching
/// entry (skipping un-registered models silently).
///
/// Idempotent — `grant_to_group` uses `ON CONFLICT DO NOTHING` and
/// `permission_id` upserts. Adding a new model and restarting picks
/// up the wildcards automatically.
///
/// Gated by `RUSTIO_DEMO_MODE=1`.
pub async fn lazy_attach_permissions(
    db: &Db,
    entries: &[crate::admin::AdminEntry],
) -> Result<()> {
    if !demo_mode_enabled() {
        return Ok(());
    }
    let mut attached = 0usize;
    for spec in DEFAULT_GROUP_SPECS {
        let group_id = match find_group_id_by_name(db, spec.name).await? {
            Some(id) => id,
            None => continue,
        };
        for codename in resolve_perms(&spec.model_perms, entries) {
            grant_to_group(db, group_id, &codename).await?;
            attached += 1;
        }
    }
    log::info!(
        "RUSTIO_DEMO_MODE: lazy-attached {attached} permission rows across {} groups",
        DEFAULT_GROUP_SPECS.len()
    );
    Ok(())
}

async fn find_group_id_by_name(db: &Db, name: &str) -> Result<Option<i64>> {
    let row = sqlx::query("SELECT id FROM rustio_groups WHERE name = $1")
        .bind(name)
        .fetch_optional(db.pool())
        .await?;
    match row {
        Some(r) => Ok(Some(
            r.try_get::<i64, _>("id")
                .map_err(|e| Error::Internal(format!("{e}")))?,
        )),
        None => Ok(None),
    }
}

/// Expand a `GroupModelPerms` against the currently-registered admin
/// entries into the full list of permission codenames to grant.
///
/// Codename format matches `register_model_permissions`:
/// `<entry.admin_name>.<action>_<entry.singular_name.lower>`.
///
/// Core entries (the synthetic User) are skipped — wildcard groups
/// don't get rights over framework infrastructure.
fn resolve_perms(
    spec: &GroupModelPerms,
    entries: &[crate::admin::AdminEntry],
) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    match spec {
        GroupModelPerms::All(actions) => {
            for entry in entries.iter().filter(|e| !e.core) {
                let singular = entry.singular_name.to_ascii_lowercase();
                for action in *actions {
                    out.push(format!("{}.{}_{}", entry.admin_name, action, singular));
                }
            }
        }
        GroupModelPerms::Specific(pairs) => {
            for (codename, actions) in *pairs {
                let entry = entries.iter().find(|e| {
                    !e.core && e.singular_name.eq_ignore_ascii_case(codename)
                });
                if let Some(entry) = entry {
                    let singular = entry.singular_name.to_ascii_lowercase();
                    for action in *actions {
                        out.push(format!("{}.{}_{}", entry.admin_name, action, singular));
                    }
                }
                // Model not registered → skip silently. Wildcard groups
                // expand whatever's there now; specifics skip what's
                // missing.
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn administrator_and_developer_bypass_group_checks() {
        // The two top tiers skip the M2M lookup. Lower tiers don't.
        for &(role, expected) in &[
            (Role::User, false),
            (Role::Staff, false),
            (Role::Supervisor, false),
            (Role::Administrator, true),
            (Role::Developer, true),
        ] {
            let id = Identity {
                user_id: 1,
                email: "a@b.com".into(),
                role,
                is_active: true,
                is_demo: false,
                demo_label: None,
            };
            assert_eq!(
                id.role.bypasses_group_checks(),
                expected,
                "{role:?} should be {expected}"
            );
        }
    }

    #[test]
    fn cache_ttl_is_one_minute() {
        assert_eq!(PERM_CACHE_TTL.as_secs(), 60);
    }

    /// Phase 7a/0.5/sec3: invalidating the perm cache makes a fresh
    /// `permissions_for_user` call read live tables. Without sec3's
    /// fix, `do_user_edit`'s wholesale `DELETE FROM rustio_user_groups`
    /// would leave the user passing every cached perm for up to 60s.
    #[tokio::test]
    #[ignore = "needs `RUSTIO_TEST_DB=1` + a running postgres (URL via RUSTIO_TEST_DATABASE_URL or default)"]
    async fn invalidate_user_cache_drops_stale_perms() {
        use crate::auth::create_user;
        use crate::auth::Role as RoleAlias;

        let url = std::env::var("RUSTIO_TEST_DATABASE_URL")
            .unwrap_or_else(|_| "postgres://postgres:dev@localhost:5432/rustio_dev".into());
        let opts = crate::orm::DbOptions {
            max_connections: 2,
            ..crate::orm::DbOptions::default()
        };
        let db = crate::orm::Db::connect_with(&url, opts).await.unwrap();

        // Seed: one Staff user, one group with `posts.view_post`,
        // user attached to group. Clean per-test by using a unique tag.
        crate::auth::init_user_tables(&db).await.unwrap();
        crate::auth::migrate_user_schema(&db).await.unwrap();
        crate::auth::init_permission_tables(&db).await.unwrap();

        let tag = format!("invtest_{}_{}", std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos());
        let email = format!("{tag}@example.test");
        let user_id = create_user(&db, &email, "secret-pw-123", RoleAlias::Staff).await.unwrap();
        let group_id = create_group(&db, &tag, "tmp").await.unwrap();
        grant_to_group(&db, group_id, "posts.view_post").await.unwrap();
        add_user_to_group(&db, user_id, group_id).await.unwrap();

        let identity = Identity {
            user_id,
            email: email.clone(),
            role: RoleAlias::Staff,
            is_active: true,
            is_demo: false,
            demo_label: None,
        };

        // Sanity: user has the perm via group.
        assert!(
            check_permission(&db, &identity, "posts.view_post").await.unwrap(),
            "user should have view_post via group"
        );

        // Simulate `do_user_edit`'s wholesale DELETE without going
        // through `remove_user_from_group` (which would invalidate the
        // cache for us). This is the exact pattern that bites in
        // production.
        sqlx::query("DELETE FROM rustio_user_groups WHERE user_id = $1")
            .bind(user_id)
            .execute(db.pool())
            .await
            .unwrap();

        // sec3's fix: explicit invalidate after the wholesale delete.
        invalidate_user_cache(user_id);

        assert!(
            !check_permission(&db, &identity, "posts.view_post").await.unwrap(),
            "after wholesale DELETE + invalidate, user must NOT have view_post"
        );

        // Cleanup.
        let _ = sqlx::query("DELETE FROM rustio_users WHERE id = $1")
            .bind(user_id)
            .execute(db.pool())
            .await;
        let _ = sqlx::query("DELETE FROM rustio_groups WHERE id = $1")
            .bind(group_id)
            .execute(db.pool())
            .await;
    }

    /// Phase 7a/0.5/sec2 regression: the order of checks in
    /// `check_permission` must reject inactive users **before** the
    /// `bypasses_group_checks` short-circuit. Otherwise an inactive
    /// Administrator/Developer who somehow holds a session passes
    /// every permission check.
    #[tokio::test]
    #[ignore = "needs `RUSTIO_TEST_DB=1` + a running postgres (URL via RUSTIO_TEST_DATABASE_URL or default)"]
    async fn inactive_administrator_is_denied_before_bypass() {
        let url = std::env::var("RUSTIO_TEST_DATABASE_URL")
            .unwrap_or_else(|_| "postgres://postgres:dev@localhost:5432/rustio_dev".into());
        let opts = crate::orm::DbOptions {
            max_connections: 2,
            ..crate::orm::DbOptions::default()
        };
        let db = crate::orm::Db::connect_with(&url, opts).await.unwrap();

        let id = Identity {
            user_id: -1, // ghost id; the inactive short-circuit fires first, so this never gets queried
            email: "ghost@example.com".into(),
            role: Role::Administrator,
            is_active: false,
            is_demo: false,
            demo_label: None,
        };
        let result = check_permission(&db, &id, "any.permission").await.unwrap();
        assert!(
            !result,
            "inactive Administrator must be denied; bypass must NOT fire before is_active check"
        );

        // Sanity check: same identity with is_active=true would bypass.
        let id_active = Identity {
            is_active: true,
            ..id
        };
        assert!(
            check_permission(&db, &id_active, "any.permission")
                .await
                .unwrap(),
            "active Administrator should bypass and return true"
        );
    }

    // ------------------------------------------------------------------
    // Phase 7a/0.5/c — bootstrap_default_groups + lazy_attach_permissions
    // ------------------------------------------------------------------

    fn pg_url() -> String {
        std::env::var("RUSTIO_TEST_DATABASE_URL")
            .unwrap_or_else(|_| "postgres://postgres:dev@localhost:5432/rustio_dev".into())
    }

    async fn pg_db() -> crate::orm::Db {
        let opts = crate::orm::DbOptions {
            max_connections: 2,
            ..crate::orm::DbOptions::default()
        };
        crate::orm::Db::connect_with(&pg_url(), opts).await.unwrap()
    }

    /// `tokio::test`s run on a thread pool, and `std::env::set_var`
    /// mutates **process** state. Tests that toggle `RUSTIO_DEMO_MODE`
    /// must serialize on this lock to avoid stomping each other's env
    /// state mid-flight. The lock is held across the whole test body
    /// — including `.await` points — so it must be `tokio::sync::Mutex`
    /// (clippy's `await_holding_lock` rejects `std::sync::Mutex` here).
    static ENV_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    /// Reset every default group on the test DB so each test starts
    /// from a clean slate. Cascades through user/perm M2M.
    async fn reset_default_groups(db: &crate::orm::Db) {
        for spec in DEFAULT_GROUP_SPECS {
            let _ = sqlx::query("DELETE FROM rustio_groups WHERE name = $1")
                .bind(spec.name)
                .execute(db.pool())
                .await;
        }
    }

    #[tokio::test]
    #[ignore = "needs `RUSTIO_TEST_DB=1` + a running postgres (URL via RUSTIO_TEST_DATABASE_URL or default)"]
    async fn bootstrap_creates_six_default_groups() {
        let _env = ENV_LOCK.lock().await;
        let db = pg_db().await;
        crate::auth::init_permission_tables(&db).await.unwrap();
        reset_default_groups(&db).await;

        std::env::set_var("RUSTIO_DEMO_MODE", "1");
        bootstrap_default_groups(&db).await.unwrap();
        std::env::remove_var("RUSTIO_DEMO_MODE");

        for spec in DEFAULT_GROUP_SPECS {
            let id = find_group_id_by_name(&db, spec.name).await.unwrap();
            assert!(
                id.is_some(),
                "default group {:?} should exist after bootstrap",
                spec.name
            );
        }

        reset_default_groups(&db).await;
    }

    #[tokio::test]
    #[ignore = "needs `RUSTIO_TEST_DB=1` + a running postgres (URL via RUSTIO_TEST_DATABASE_URL or default)"]
    async fn bootstrap_idempotent_across_restarts() {
        let _env = ENV_LOCK.lock().await;
        let db = pg_db().await;
        crate::auth::init_permission_tables(&db).await.unwrap();
        reset_default_groups(&db).await;

        std::env::set_var("RUSTIO_DEMO_MODE", "1");
        bootstrap_default_groups(&db).await.unwrap();

        let count_after_first: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM rustio_groups WHERE name = ANY($1)")
                .bind(
                    DEFAULT_GROUP_SPECS
                        .iter()
                        .map(|s| s.name.to_string())
                        .collect::<Vec<_>>(),
                )
                .fetch_one(db.pool())
                .await
                .unwrap();
        assert_eq!(count_after_first, DEFAULT_GROUP_SPECS.len() as i64);

        bootstrap_default_groups(&db).await.unwrap();
        let count_after_second: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM rustio_groups WHERE name = ANY($1)")
                .bind(
                    DEFAULT_GROUP_SPECS
                        .iter()
                        .map(|s| s.name.to_string())
                        .collect::<Vec<_>>(),
                )
                .fetch_one(db.pool())
                .await
                .unwrap();
        assert_eq!(
            count_after_first, count_after_second,
            "second bootstrap must not duplicate rows"
        );

        std::env::remove_var("RUSTIO_DEMO_MODE");
        reset_default_groups(&db).await;
    }

    #[tokio::test]
    #[ignore = "needs `RUSTIO_TEST_DB=1` + a running postgres (URL via RUSTIO_TEST_DATABASE_URL or default)"]
    async fn bootstrap_skips_when_demo_mode_unset() {
        let _env = ENV_LOCK.lock().await;
        let db = pg_db().await;
        crate::auth::init_permission_tables(&db).await.unwrap();
        reset_default_groups(&db).await;

        // Without the flag, bootstrap should be a no-op even though
        // every prerequisite exists.
        std::env::remove_var("RUSTIO_DEMO_MODE");
        bootstrap_default_groups(&db).await.unwrap();

        for spec in DEFAULT_GROUP_SPECS {
            let id = find_group_id_by_name(&db, spec.name).await.unwrap();
            assert!(
                id.is_none(),
                "default group {:?} must NOT exist when demo mode is off",
                spec.name
            );
        }
    }

    /// Build a minimal `Admin` with one project entry whose
    /// `singular_name` is `"Post"` — matches what `examples/blog`
    /// registers, so we can exercise wildcard expansion realistically.
    fn admin_with_post_entry() -> crate::admin::Admin {
        // Phase 5a's `AdminEntry::for_testing` exposes the right
        // shape without needing a real `AdminModel` impl.
        const POST_FIELDS: &[crate::admin::AdminField] = &[];
        let post_entry = crate::admin::AdminEntry::for_testing(
            "posts", "Posts", "Post", "posts", POST_FIELDS, false,
        );
        let mut admin = crate::admin::Admin::new();
        admin.entries.push(post_entry);
        admin
    }

    #[tokio::test]
    #[ignore = "needs `RUSTIO_TEST_DB=1` + a running postgres (URL via RUSTIO_TEST_DATABASE_URL or default)"]
    async fn lazy_attach_resolves_wildcard_to_all_entries() {
        let _env = ENV_LOCK.lock().await;
        let db = pg_db().await;
        crate::auth::init_permission_tables(&db).await.unwrap();
        reset_default_groups(&db).await;

        std::env::set_var("RUSTIO_DEMO_MODE", "1");
        bootstrap_default_groups(&db).await.unwrap();
        let admin = admin_with_post_entry();
        register_model_permissions(&db, "posts", "post").await.unwrap();
        lazy_attach_permissions(&db, admin.entries()).await.unwrap();

        // Auditors uses `All(&["view"])`. Against an admin with one
        // non-core entry (`Post`), it should resolve to `posts.view_post`.
        let auditor_id = find_group_id_by_name(&db, "Auditors").await.unwrap().unwrap();
        let perms: Vec<String> = sqlx::query_scalar(
            "SELECT p.name FROM rustio_permissions p \
             JOIN rustio_group_permissions gp ON gp.permission_id = p.id \
             WHERE gp.group_id = $1",
        )
        .bind(auditor_id)
        .fetch_all(db.pool())
        .await
        .unwrap();

        assert!(
            perms.iter().any(|p| p == "posts.view_post"),
            "Auditors should have posts.view_post, got: {perms:?}"
        );

        std::env::remove_var("RUSTIO_DEMO_MODE");
        reset_default_groups(&db).await;
    }

    #[tokio::test]
    #[ignore = "needs `RUSTIO_TEST_DB=1` + a running postgres (URL via RUSTIO_TEST_DATABASE_URL or default)"]
    async fn lazy_attach_skips_unregistered_models() {
        let _env = ENV_LOCK.lock().await;
        let db = pg_db().await;
        crate::auth::init_permission_tables(&db).await.unwrap();
        reset_default_groups(&db).await;

        std::env::set_var("RUSTIO_DEMO_MODE", "1");
        bootstrap_default_groups(&db).await.unwrap();
        // Admin with ONLY `Post` registered — Finance, HR Managers,
        // Project Coordinators target models that aren't registered.
        let admin = admin_with_post_entry();
        lazy_attach_permissions(&db, admin.entries()).await.unwrap();

        let finance_id = find_group_id_by_name(&db, "Finance")
            .await
            .unwrap()
            .unwrap();
        let perms_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM rustio_group_permissions WHERE group_id = $1",
        )
        .bind(finance_id)
        .fetch_one(db.pool())
        .await
        .unwrap();
        assert_eq!(
            perms_count, 0,
            "Finance has no perms when contract/invoice/payment aren't registered"
        );

        std::env::remove_var("RUSTIO_DEMO_MODE");
        reset_default_groups(&db).await;
    }

    #[tokio::test]
    #[ignore = "needs `RUSTIO_TEST_DB=1` + a running postgres (URL via RUSTIO_TEST_DATABASE_URL or default)"]
    async fn lazy_attach_idempotent() {
        let _env = ENV_LOCK.lock().await;
        let db = pg_db().await;
        crate::auth::init_permission_tables(&db).await.unwrap();
        reset_default_groups(&db).await;

        std::env::set_var("RUSTIO_DEMO_MODE", "1");
        bootstrap_default_groups(&db).await.unwrap();
        let admin = admin_with_post_entry();
        register_model_permissions(&db, "posts", "post").await.unwrap();
        lazy_attach_permissions(&db, admin.entries()).await.unwrap();

        let auditor_id = find_group_id_by_name(&db, "Auditors")
            .await
            .unwrap()
            .unwrap();
        let count_after_first: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM rustio_group_permissions WHERE group_id = $1",
        )
        .bind(auditor_id)
        .fetch_one(db.pool())
        .await
        .unwrap();

        // Second pass — must not duplicate.
        lazy_attach_permissions(&db, admin.entries()).await.unwrap();
        let count_after_second: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM rustio_group_permissions WHERE group_id = $1",
        )
        .bind(auditor_id)
        .fetch_one(db.pool())
        .await
        .unwrap();

        assert_eq!(
            count_after_first, count_after_second,
            "lazy_attach must be idempotent — second call should change nothing"
        );

        std::env::remove_var("RUSTIO_DEMO_MODE");
        reset_default_groups(&db).await;
    }

    /// Sanity: `demo_mode_enabled` reflects the env var. Sandbox-only;
    /// no DB. Uses `#[tokio::test]` because `ENV_LOCK` is async-aware —
    /// the same lock keeps env mutations from racing with the PG-gated
    /// tests under `cargo test -- --ignored`.
    #[tokio::test]
    async fn demo_mode_enabled_reflects_env_var() {
        let _env = ENV_LOCK.lock().await;
        let prior = std::env::var("RUSTIO_DEMO_MODE").ok();

        std::env::remove_var("RUSTIO_DEMO_MODE");
        assert!(!demo_mode_enabled(), "no env → disabled");

        std::env::set_var("RUSTIO_DEMO_MODE", "1");
        assert!(demo_mode_enabled(), "RUSTIO_DEMO_MODE=1 → enabled");

        std::env::set_var("RUSTIO_DEMO_MODE", "0");
        assert!(!demo_mode_enabled(), "RUSTIO_DEMO_MODE=0 → disabled");

        // Restore.
        match prior {
            Some(v) => std::env::set_var("RUSTIO_DEMO_MODE", v),
            None => std::env::remove_var("RUSTIO_DEMO_MODE"),
        }
    }
}
