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
}
