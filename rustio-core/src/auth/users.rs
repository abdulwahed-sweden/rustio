//! User records, password hashing, and the login flow.

use argon2::password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use chrono::{DateTime, Utc};
use sqlx::Row as SqlxRow;

use crate::error::{Error, Result};
use crate::orm::{Db, Row};

use super::role::Role;
use super::sessions::create_session;

/// The identity attached to a request by the auth middleware. Kept
/// cheap to clone because we pass it into handler bodies.
#[derive(Debug, Clone)]
pub struct Identity {
    pub user_id: i64,
    pub email: String,
    pub role: Role,
    pub is_active: bool,
    /// Phase 7a/0.5: whether this user was seeded by the demo
    /// bootstrap (`RUSTIO_DEMO_MODE=1`). Drives the red banner.
    pub is_demo: bool,
    pub demo_label: Option<String>,
}

impl Identity {
    /// Administrator-or-higher (Administrator, Developer). Phase 6a/6b
    /// callers used this to gate the user/group management pages.
    pub fn is_admin(&self) -> bool {
        self.is_active && self.role.includes(Role::Administrator)
    }

    /// Anyone allowed into the admin panel (Staff and above).
    pub fn can_access_admin(&self) -> bool {
        self.is_active && self.role.can_access_panel()
    }
}

pub struct StoredUser {
    pub id: i64,
    pub email: String,
    pub password_hash: String,
    pub role: Role,
    pub is_active: bool,
    pub is_demo: bool,
    pub demo_label: Option<String>,
}

/// Read-only view of a user, used by the built-in admin profile page
/// and passed into project-registered profile extensions. Excludes
/// `password_hash` deliberately — extensions must never see credential
/// material. Construct via [`load_user_profile`].
#[derive(Debug, Clone)]
pub struct UserProfile {
    pub id: i64,
    pub email: String,
    pub role: Role,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub full_name: Option<String>,
    pub locale: Option<String>,
    pub timezone: Option<String>,
    pub is_demo: bool,
    pub demo_label: Option<String>,
}

pub async fn init_user_tables(db: &Db) -> Result<()> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS rustio_users (
            id            BIGSERIAL PRIMARY KEY,
            email         TEXT NOT NULL UNIQUE,
            password_hash TEXT NOT NULL,
            role          TEXT NOT NULL DEFAULT 'user',
            is_active     BOOLEAN NOT NULL DEFAULT TRUE,
            created_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            updated_at    TIMESTAMPTZ NOT NULL DEFAULT NOW()
        )",
    )
    .execute(db.pool())
    .await?;

    sqlx::query("CREATE INDEX IF NOT EXISTS rustio_users_email_idx ON rustio_users (email)")
        .execute(db.pool())
        .await?;

    Ok(())
}

/// Idempotent schema upgrade for the 5-tier role hierarchy + demo flag.
///
/// Phase 7a/0.5/a — runs after `init_user_tables` on every boot. Safe to
/// call repeatedly; safe to run on a fresh DB and on a Phase 6b DB.
///
/// Order is load-bearing:
/// 1. Rename existing `'admin'` rows to `'administrator'` BEFORE the CHECK
///    constraint exists, otherwise the constraint would reject the row.
/// 2. Add the two demo columns idempotently.
/// 3. Add the CHECK constraint conditionally (PG has no `IF NOT EXISTS`
///    for CHECK constraints, so we guard via `pg_constraint`).
/// 4. Add the indexes (`CREATE INDEX IF NOT EXISTS` is native).
pub async fn migrate_user_schema(db: &Db) -> Result<()> {
    // 1. Rename 'admin' → 'administrator' on existing rows.
    sqlx::query("UPDATE rustio_users SET role = 'administrator' WHERE role = 'admin'")
        .execute(db.pool())
        .await?;

    // 2. Add demo columns.
    sqlx::query(
        "ALTER TABLE rustio_users \
         ADD COLUMN IF NOT EXISTS is_demo BOOLEAN NOT NULL DEFAULT FALSE",
    )
    .execute(db.pool())
    .await?;
    sqlx::query("ALTER TABLE rustio_users ADD COLUMN IF NOT EXISTS demo_label TEXT")
        .execute(db.pool())
        .await?;

    // 3. CHECK constraint — guarded by pg_constraint lookup. The DO block
    //    runs as one statement; sqlx happily executes PL/pgSQL strings.
    sqlx::query(
        "DO $$
         BEGIN
            IF NOT EXISTS (
                SELECT 1 FROM pg_constraint WHERE conname = 'rustio_users_role_check'
            ) THEN
                ALTER TABLE rustio_users
                ADD CONSTRAINT rustio_users_role_check
                CHECK (role IN ('user','staff','supervisor','administrator','developer'));
            END IF;
         END $$",
    )
    .execute(db.pool())
    .await?;

    // 4. Indexes.
    sqlx::query("CREATE INDEX IF NOT EXISTS rustio_users_role_idx ON rustio_users(role)")
        .execute(db.pool())
        .await?;
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS rustio_users_is_demo_idx \
         ON rustio_users(is_demo) WHERE is_demo = TRUE",
    )
    .execute(db.pool())
    .await?;

    // 5. Phase 10/a — profile-display columns. All nullable, no defaults,
    //    no backfill. Read by `load_user_profile` and the built-in user
    //    show page; never required by the auth path itself.
    sqlx::query("ALTER TABLE rustio_users ADD COLUMN IF NOT EXISTS full_name TEXT")
        .execute(db.pool())
        .await?;
    sqlx::query("ALTER TABLE rustio_users ADD COLUMN IF NOT EXISTS locale TEXT")
        .execute(db.pool())
        .await?;
    sqlx::query("ALTER TABLE rustio_users ADD COLUMN IF NOT EXISTS timezone TEXT")
        .execute(db.pool())
        .await?;

    Ok(())
}

pub fn hash_password(plain: &str) -> Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(plain.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| Error::Internal(format!("password hashing: {e}")))
}

pub fn verify_password(plain: &str, stored_hash: &str) -> bool {
    match PasswordHash::new(stored_hash) {
        Ok(parsed) => Argon2::default()
            .verify_password(plain.as_bytes(), &parsed)
            .is_ok(),
        Err(_) => false,
    }
}

pub async fn create_user(db: &Db, email: &str, password: &str, role: Role) -> Result<i64> {
    let hash = hash_password(password)?;
    let row = sqlx::query(
        "INSERT INTO rustio_users (email, password_hash, role)
         VALUES ($1, $2, $3)
         RETURNING id",
    )
    .bind(email)
    .bind(&hash)
    .bind(role.as_str())
    .fetch_one(db.pool())
    .await
    .map_err(|e| {
        // Phase 7a/0.5/sec4: keep Postgres internals out of the
        // client response. The full error stays in the operator's
        // log; the user sees a clean, generic message — except the
        // unique-email collision, which is worth surfacing because
        // it's actionable.
        log::warn!("create_user failed for {email}: {e}");
        let detail = e.to_string();
        if detail.contains("rustio_users_email_key") {
            Error::BadRequest("An account with this email already exists.".into())
        } else {
            Error::BadRequest("Could not create user. Please check your input.".into())
        }
    })?;
    let id: i64 = row
        .try_get("id")
        .map_err(|e| Error::Internal(format!("returning id: {e}")))?;
    Ok(id)
}

/// Phase 7a/0.5/d — INSERT-with-conflict-skip variant of `create_user`
/// for the demo bootstrap flow. Sets `is_demo = TRUE` and writes an
/// optional human-readable `demo_label`. Returns `Some(id)` on insert,
/// `None` if the email is already taken (a real user holds it). The
/// public `create_user` API is intentionally untouched.
async fn create_demo_user(
    db: &Db,
    email: &str,
    password: &str,
    role: Role,
    demo_label: Option<&str>,
) -> Result<Option<i64>> {
    let hash = hash_password(password)?;
    let row = sqlx::query(
        "INSERT INTO rustio_users (email, password_hash, role, is_demo, demo_label)
         VALUES ($1, $2, $3, TRUE, $4)
         ON CONFLICT (email) DO NOTHING
         RETURNING id",
    )
    .bind(email)
    .bind(&hash)
    .bind(role.as_str())
    .bind(demo_label)
    .fetch_optional(db.pool())
    .await?;
    match row {
        Some(r) => {
            let id: i64 = r
                .try_get("id")
                .map_err(|e| Error::Internal(format!("returning id: {e}")))?;
            Ok(Some(id))
        }
        None => Ok(None),
    }
}

/// Phase 7a/0.5/d — gated by `RUSTIO_DEMO_MODE=1`. Inserts the five
/// demo users keyed off `branding.domain` (e.g. `staff@rustio.local`)
/// and attaches each to the matching default groups (which must
/// already exist; call `bootstrap_default_groups` + `lazy_attach_*`
/// first). Idempotent via the demo-count gate: re-running on a DB
/// that already has demo users is a no-op. Real users coexist —
/// the gate counts only `is_demo = TRUE` rows.
pub async fn bootstrap_demo_users(
    db: &Db,
    branding: &crate::admin::SiteBranding,
) -> Result<()> {
    if std::env::var("RUSTIO_DEMO_MODE").as_deref() != Ok("1") {
        return Ok(());
    }
    let demo_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM rustio_users WHERE is_demo = TRUE")
        .fetch_one(db.pool())
        .await?;
    if demo_count > 0 {
        return Ok(());
    }

    type DemoSpec = (&'static str, Role, &'static [&'static str]);
    let demo_specs: [DemoSpec; 5] = [
        ("user", Role::User, &[]),
        ("staff", Role::Staff, &["Auditors"]),
        ("supervisor", Role::Supervisor, &["System Operators"]),
        (
            "administrator",
            Role::Administrator,
            &[
                "Auditors",
                "Content Editors",
                "HR Managers",
                "Finance",
                "Project Coordinators",
                "System Operators",
            ],
        ),
        (
            "developer",
            Role::Developer,
            &[
                "Auditors",
                "Content Editors",
                "HR Managers",
                "Finance",
                "Project Coordinators",
                "System Operators",
            ],
        ),
    ];

    let mut created = 0usize;
    for (slug, role, group_names) in demo_specs {
        let email = format!("{slug}@{}", branding.domain);
        let label = format!("Demo {}", role.label());
        match create_demo_user(db, &email, slug, role, Some(&label)).await? {
            Some(user_id) => {
                created += 1;
                for group_name in group_names {
                    if let Some(group_id) =
                        crate::auth::permissions::find_group_id_by_name(db, group_name).await?
                    {
                        crate::auth::add_user_to_group(db, user_id, group_id).await?;
                    }
                }
            }
            None => {
                log::warn!("RUSTIO_DEMO_MODE: skipping demo user {email} — email already taken");
            }
        }
    }
    log::info!("RUSTIO_DEMO_MODE: created {created} demo users (passwords match role slugs)");
    Ok(())
}

pub async fn find_user_by_email(db: &Db, email: &str) -> Result<Option<StoredUser>> {
    let row = sqlx::query(
        "SELECT id, email, password_hash, role, is_active, is_demo, demo_label
           FROM rustio_users
          WHERE email = $1",
    )
    .bind(email)
    .fetch_optional(db.pool())
    .await?;
    match row {
        Some(r) => {
            let r = Row::from_pg(&r);
            Ok(Some(StoredUser {
                id: r.get_i64("id")?,
                email: r.get_string("email")?,
                password_hash: r.get_string("password_hash")?,
                role: Role::parse(&r.get_string("role")?)?,
                is_active: r.get_bool("is_active")?,
                is_demo: r.get_bool("is_demo")?,
                demo_label: r.get_optional_string("demo_label")?,
            }))
        }
        None => Ok(None),
    }
}

/// Load a user by id for display purposes. Returns `Ok(None)` for a
/// missing id (callers map to 404). Returns `Err` only on a real DB
/// failure or a corrupted role string.
///
/// Phase 10/a — companion to [`UserProfile`]. Reads the columns added
/// by `migrate_user_schema` (full_name, locale, timezone) plus the
/// existing demo flags. Never reads `password_hash`.
pub async fn load_user_profile(db: &Db, user_id: i64) -> Result<Option<UserProfile>> {
    let row = sqlx::query(
        "SELECT id, email, role, is_active, created_at,
                full_name, locale, timezone, is_demo, demo_label
           FROM rustio_users
          WHERE id = $1",
    )
    .bind(user_id)
    .fetch_optional(db.pool())
    .await?;
    match row {
        Some(r) => {
            let r = Row::from_pg(&r);
            Ok(Some(UserProfile {
                id: r.get_i64("id")?,
                email: r.get_string("email")?,
                role: Role::parse(&r.get_string("role")?)?,
                is_active: r.get_bool("is_active")?,
                created_at: r.get_datetime("created_at")?,
                full_name: r.get_optional_string("full_name")?,
                locale: r.get_optional_string("locale")?,
                timezone: r.get_optional_string("timezone")?,
                is_demo: r.get_bool("is_demo")?,
                demo_label: r.get_optional_string("demo_label")?,
            }))
        }
        None => Ok(None),
    }
}

pub async fn set_password(db: &Db, user_id: i64, new_password: &str) -> Result<()> {
    let hash = hash_password(new_password)?;
    sqlx::query(
        "UPDATE rustio_users SET password_hash = $1, updated_at = $2 WHERE id = $3",
    )
    .bind(&hash)
    .bind(Utc::now())
    .bind(user_id)
    .execute(db.pool())
    .await?;
    Ok(())
}

pub async fn update_user_role(db: &Db, user_id: i64, role: Role) -> Result<()> {
    sqlx::query(
        "UPDATE rustio_users SET role = $1, updated_at = $2 WHERE id = $3",
    )
    .bind(role.as_str())
    .bind(Utc::now())
    .bind(user_id)
    .execute(db.pool())
    .await?;
    Ok(())
}

/// Phase 7a/0.5/f — would the proposed change leave the system with
/// zero active Developers?
///
/// `new_role`:
/// - `None` → user is being deleted entirely.
/// - `Some(role)` → user's role is being changed to `role`.
///
/// Returns `true` only when:
/// - exactly one active Developer exists, AND
/// - the target user IS that Developer, AND
/// - the action would remove their Developer status (deletion or
///   demotion to anything other than Developer).
///
/// Used as a server-side guard in `do_user_edit` and `do_user_delete`,
/// and as a CLI warning before destructive role changes.
pub async fn would_orphan_developers(
    db: &Db,
    user_id: i64,
    new_role: Option<Role>,
) -> Result<bool> {
    // Cheap-out: if the change KEEPS the user as a Developer, no orphan risk.
    if matches!(new_role, Some(Role::Developer)) {
        return Ok(false);
    }

    let active_dev_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM rustio_users \
         WHERE role = 'developer' AND is_active = TRUE",
    )
    .fetch_one(db.pool())
    .await?;

    // No-developers → not orphaning anyone (a fresh DB pre-bootstrap
    // is allowed, by design).
    if active_dev_count == 0 {
        return Ok(false);
    }
    // 2+ developers → demoting/deleting one always leaves ≥1 left.
    if active_dev_count > 1 {
        return Ok(false);
    }

    // Exactly one. Is it `user_id`?
    let target_role: Option<String> = sqlx::query_scalar(
        "SELECT role FROM rustio_users WHERE id = $1 AND is_active = TRUE",
    )
    .bind(user_id)
    .fetch_optional(db.pool())
    .await?;
    Ok(target_role.as_deref() == Some("developer"))
}

/// Verify credentials and create a session. Returns the session token
/// to set in the cookie. A deliberately vague error on failure — we
/// don't want to leak whether the email was valid.
pub async fn login(db: &Db, email: &str, password: &str) -> Result<String> {
    let user = find_user_by_email(db, email)
        .await?
        .ok_or_else(|| Error::Unauthorized("invalid email or password".into()))?;
    if !user.is_active {
        return Err(Error::Forbidden("account disabled".into()));
    }
    if !verify_password(password, &user.password_hash) {
        return Err(Error::Unauthorized("invalid email or password".into()));
    }
    create_session(db, user.id).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_profile_derives_debug_and_clone() {
        // Phase 10/a — UserProfile must be Debug + Clone so handlers
        // and template-context builders can format it and pass it by
        // value into the project extension closure without ceremony.
        fn assert_traits<T: std::fmt::Debug + Clone>() {}
        assert_traits::<UserProfile>();
    }

    #[test]
    fn password_round_trip() {
        let h = hash_password("secret").unwrap();
        assert!(verify_password("secret", &h));
        assert!(!verify_password("wrong", &h));
    }

    // `Role` parsing + ladder semantics moved to `auth/role.rs`
    // (25-case `includes` matrix + parse round-trip).

    /// Phase 7a/0.5/sec4 regression: duplicate-email creation must
    /// surface a clean, actionable message — never the raw Postgres
    /// constraint name, never an SQLSTATE code.
    #[tokio::test]
    #[ignore = "needs `RUSTIO_TEST_DB=1` + a running postgres (URL via RUSTIO_TEST_DATABASE_URL or default)"]
    async fn duplicate_email_is_clean_error_message() {
        let url = std::env::var("RUSTIO_TEST_DATABASE_URL")
            .unwrap_or_else(|_| "postgres://postgres:dev@localhost:5432/rustio_dev".into());
        let opts = crate::orm::DbOptions {
            max_connections: 2,
            ..crate::orm::DbOptions::default()
        };
        let db = crate::orm::Db::connect_with(&url, opts).await.unwrap();
        crate::auth::init_user_tables(&db).await.unwrap();
        crate::auth::migrate_user_schema(&db).await.unwrap();

        let tag = format!(
            "dup_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let email = format!("{tag}@example.test");

        // First insert succeeds.
        let first_id = create_user(&db, &email, "secret-pw-123", Role::User)
            .await
            .unwrap();

        // Second insert fails — assert the response message is clean
        // and contains no Postgres-internal detail.
        let err = create_user(&db, &email, "secret-pw-123", Role::User)
            .await
            .unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("already exists"),
            "expected actionable duplicate-email message, got: {msg}"
        );
        for leaked in [
            "rustio_users_email_key",
            "duplicate key value",
            "constraint",
            "SQLSTATE",
            "23505",
            "Postgres",
            "pg::",
        ] {
            assert!(
                !msg.contains(leaked),
                "client message must NOT contain {leaked:?}, got: {msg}"
            );
        }

        // Cleanup.
        let _ = sqlx::query("DELETE FROM rustio_users WHERE id = $1")
            .bind(first_id)
            .execute(db.pool())
            .await;
    }

    // ------------------------------------------------------------------
    // Phase 7a/0.5/d — bootstrap_demo_users
    // ------------------------------------------------------------------

    use crate::auth::TEST_ENV_LOCK as ENV_LOCK;

    async fn pg_db() -> crate::orm::Db {
        let url = std::env::var("RUSTIO_TEST_DATABASE_URL")
            .unwrap_or_else(|_| "postgres://postgres:dev@localhost:5432/rustio_dev".into());
        let opts = crate::orm::DbOptions {
            max_connections: 2,
            ..crate::orm::DbOptions::default()
        };
        crate::orm::Db::connect_with(&url, opts).await.unwrap()
    }

    /// Wipe every demo user + every default group on the test DB so
    /// each test starts from a clean slate. Cascades through the M2M.
    async fn reset_demo_state(db: &crate::orm::Db) {
        let _ = sqlx::query("DELETE FROM rustio_users WHERE is_demo = TRUE")
            .execute(db.pool())
            .await;
        // Match the 6 default group names from permissions.rs.
        for name in [
            "Auditors",
            "Content Editors",
            "HR Managers",
            "Finance",
            "Project Coordinators",
            "System Operators",
        ] {
            let _ = sqlx::query("DELETE FROM rustio_groups WHERE name = $1")
                .bind(name)
                .execute(db.pool())
                .await;
        }
    }

    fn test_branding() -> crate::admin::SiteBranding {
        crate::admin::SiteBranding {
            domain: "rustio.local".into(),
            ..crate::admin::SiteBranding::default()
        }
    }

    #[tokio::test]
    #[ignore = "needs `RUSTIO_TEST_DB=1` + a running postgres (URL via RUSTIO_TEST_DATABASE_URL or default)"]
    async fn bootstrap_creates_five_demo_users() {
        let _env = ENV_LOCK.lock().await;
        let db = pg_db().await;
        crate::auth::init_user_tables(&db).await.unwrap();
        crate::auth::migrate_user_schema(&db).await.unwrap();
        crate::auth::init_permission_tables(&db).await.unwrap();
        reset_demo_state(&db).await;

        std::env::set_var("RUSTIO_DEMO_MODE", "1");
        crate::auth::bootstrap_default_groups(&db).await.unwrap();
        bootstrap_demo_users(&db, &test_branding()).await.unwrap();

        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM rustio_users WHERE is_demo = TRUE \
             AND email LIKE '%@rustio.local'",
        )
        .fetch_one(db.pool())
        .await
        .unwrap();
        assert_eq!(count, 5, "expected 5 demo users, got {count}");

        std::env::remove_var("RUSTIO_DEMO_MODE");
        reset_demo_state(&db).await;
    }

    #[tokio::test]
    #[ignore = "needs `RUSTIO_TEST_DB=1` + a running postgres (URL via RUSTIO_TEST_DATABASE_URL or default)"]
    async fn bootstrap_skips_when_demo_users_already_exist() {
        let _env = ENV_LOCK.lock().await;
        let db = pg_db().await;
        crate::auth::init_user_tables(&db).await.unwrap();
        crate::auth::migrate_user_schema(&db).await.unwrap();
        crate::auth::init_permission_tables(&db).await.unwrap();
        reset_demo_state(&db).await;

        std::env::set_var("RUSTIO_DEMO_MODE", "1");
        crate::auth::bootstrap_default_groups(&db).await.unwrap();
        bootstrap_demo_users(&db, &test_branding()).await.unwrap();
        let first: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM rustio_users WHERE is_demo = TRUE",
        )
        .fetch_one(db.pool())
        .await
        .unwrap();
        assert_eq!(first, 5);

        // Re-run — gate must short-circuit and add nothing.
        bootstrap_demo_users(&db, &test_branding()).await.unwrap();
        let second: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM rustio_users WHERE is_demo = TRUE",
        )
        .fetch_one(db.pool())
        .await
        .unwrap();
        assert_eq!(first, second, "second bootstrap must NOT add rows");

        std::env::remove_var("RUSTIO_DEMO_MODE");
        reset_demo_state(&db).await;
    }

    #[tokio::test]
    #[ignore = "needs `RUSTIO_TEST_DB=1` + a running postgres (URL via RUSTIO_TEST_DATABASE_URL or default)"]
    async fn bootstrap_assigns_groups_correctly() {
        let _env = ENV_LOCK.lock().await;
        let db = pg_db().await;
        crate::auth::init_user_tables(&db).await.unwrap();
        crate::auth::migrate_user_schema(&db).await.unwrap();
        crate::auth::init_permission_tables(&db).await.unwrap();
        reset_demo_state(&db).await;

        std::env::set_var("RUSTIO_DEMO_MODE", "1");
        crate::auth::bootstrap_default_groups(&db).await.unwrap();
        bootstrap_demo_users(&db, &test_branding()).await.unwrap();

        // staff → 1 group (Auditors)
        let staff_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM rustio_user_groups ug \
             JOIN rustio_users u ON u.id = ug.user_id \
             WHERE u.email = $1",
        )
        .bind("staff@rustio.local")
        .fetch_one(db.pool())
        .await
        .unwrap();
        assert_eq!(staff_count, 1, "staff should belong to 1 group");

        // administrator → 6 groups (every default)
        let admin_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM rustio_user_groups ug \
             JOIN rustio_users u ON u.id = ug.user_id \
             WHERE u.email = $1",
        )
        .bind("administrator@rustio.local")
        .fetch_one(db.pool())
        .await
        .unwrap();
        assert_eq!(admin_count, 6, "administrator should belong to all 6");

        // user → 0 groups
        let user_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM rustio_user_groups ug \
             JOIN rustio_users u ON u.id = ug.user_id \
             WHERE u.email = $1",
        )
        .bind("user@rustio.local")
        .fetch_one(db.pool())
        .await
        .unwrap();
        assert_eq!(user_count, 0, "user has no group memberships");

        std::env::remove_var("RUSTIO_DEMO_MODE");
        reset_demo_state(&db).await;
    }

    #[tokio::test]
    #[ignore = "needs `RUSTIO_TEST_DB=1` + a running postgres (URL via RUSTIO_TEST_DATABASE_URL or default)"]
    async fn demo_user_emails_use_branding_domain() {
        let _env = ENV_LOCK.lock().await;
        let db = pg_db().await;
        crate::auth::init_user_tables(&db).await.unwrap();
        crate::auth::migrate_user_schema(&db).await.unwrap();
        crate::auth::init_permission_tables(&db).await.unwrap();
        reset_demo_state(&db).await;

        std::env::set_var("RUSTIO_DEMO_MODE", "1");
        crate::auth::bootstrap_default_groups(&db).await.unwrap();

        // Use a non-default domain to prove branding flows through.
        let branding = crate::admin::SiteBranding {
            domain: "tolkhuset.test".into(),
            ..crate::admin::SiteBranding::default()
        };
        bootstrap_demo_users(&db, &branding).await.unwrap();

        let emails: Vec<String> = sqlx::query_scalar(
            "SELECT email FROM rustio_users WHERE is_demo = TRUE ORDER BY email",
        )
        .fetch_all(db.pool())
        .await
        .unwrap();
        assert_eq!(emails.len(), 5);
        for e in &emails {
            assert!(
                e.ends_with("@tolkhuset.test"),
                "demo email should use branding domain, got: {e}"
            );
        }

        std::env::remove_var("RUSTIO_DEMO_MODE");
        reset_demo_state(&db).await;
    }

    #[tokio::test]
    #[ignore = "needs `RUSTIO_TEST_DB=1` + a running postgres (URL via RUSTIO_TEST_DATABASE_URL or default)"]
    async fn real_user_unaffected_by_demo_bootstrap() {
        let _env = ENV_LOCK.lock().await;
        let db = pg_db().await;
        crate::auth::init_user_tables(&db).await.unwrap();
        crate::auth::migrate_user_schema(&db).await.unwrap();
        crate::auth::init_permission_tables(&db).await.unwrap();
        reset_demo_state(&db).await;

        // Seed a real user. Must NOT be flagged is_demo afterward.
        let real_email = format!(
            "real_{}_{}@example.test",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let real_id = create_user(&db, &real_email, "secret-pw-123", Role::User)
            .await
            .unwrap();

        std::env::set_var("RUSTIO_DEMO_MODE", "1");
        crate::auth::bootstrap_default_groups(&db).await.unwrap();
        bootstrap_demo_users(&db, &test_branding()).await.unwrap();

        // The real user's row is unchanged.
        let row = find_user_by_email(&db, &real_email).await.unwrap().unwrap();
        assert!(!row.is_demo, "real user must NOT be flagged is_demo");
        assert_eq!(row.demo_label, None, "real user must NOT have a demo_label");
        assert_eq!(row.role, Role::User, "real user's role must be unchanged");

        // Demo users coexist with the real user.
        let demo_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM rustio_users WHERE is_demo = TRUE",
        )
        .fetch_one(db.pool())
        .await
        .unwrap();
        assert_eq!(demo_count, 5);

        std::env::remove_var("RUSTIO_DEMO_MODE");
        reset_demo_state(&db).await;
        // Cleanup the real user.
        let _ = sqlx::query("DELETE FROM rustio_users WHERE id = $1")
            .bind(real_id)
            .execute(db.pool())
            .await;
    }

    // ------------------------------------------------------------------
    // Phase 7a/0.5/f — would_orphan_developers
    //
    // The helper is the single source of truth for "is this change
    // about to leave the system without a developer?". The UI guard
    // (`do_user_edit`, `do_user_delete`) and the CLI confirmation
    // (`user role set`) all delegate to it, so the contract MUST hold:
    // - sole active dev demoted/deleted → true
    // - two active devs, one demoted    → false
    // - inactive devs don't count toward the active pool
    // - non-dev targets never trigger
    // - "zero devs" pre-bootstrap is allowed
    // ------------------------------------------------------------------

    /// Insert a unique-emailed user for orphan-guard tests. Returns
    /// the new id; caller cleans up via `delete_user(...)` to keep the
    /// DB tidy (these rows are NOT flagged `is_demo` so
    /// `reset_demo_state` won't catch them).
    async fn make_user(db: &crate::orm::Db, role: Role, is_active: bool) -> i64 {
        let email = format!(
            "orphan_{}_{}_{}@example.test",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            // tie-break in case two calls land on the same nanosecond.
            rand::random::<u32>(),
        );
        let id = create_user(db, &email, "secret-pw-123", role).await.unwrap();
        if !is_active {
            sqlx::query("UPDATE rustio_users SET is_active = FALSE WHERE id = $1")
                .bind(id)
                .execute(db.pool())
                .await
                .unwrap();
        }
        id
    }

    async fn delete_user(db: &crate::orm::Db, id: i64) {
        let _ = sqlx::query("DELETE FROM rustio_users WHERE id = $1")
            .bind(id)
            .execute(db.pool())
            .await;
    }

    /// Snapshot of pre-existing developer ids so we can restore the DB
    /// to its starting state. Tests run against a shared dev DB and
    /// must not leak rows or flip seeded users' active flags.
    async fn snapshot_active_devs(db: &crate::orm::Db) -> Vec<i64> {
        sqlx::query_scalar(
            "SELECT id FROM rustio_users \
             WHERE role = 'developer' AND is_active = TRUE",
        )
        .fetch_all(db.pool())
        .await
        .unwrap()
    }

    /// Move every active developer NOT in `keep` to `is_active = FALSE`
    /// for the duration of a test. We restore them in
    /// `restore_active_devs`. We deactivate (rather than delete) so
    /// FK references (sessions, group memberships) survive.
    async fn isolate_developers(db: &crate::orm::Db, keep: &[i64]) -> Vec<i64> {
        let snapshot = snapshot_active_devs(db).await;
        for id in &snapshot {
            if !keep.contains(id) {
                sqlx::query("UPDATE rustio_users SET is_active = FALSE WHERE id = $1")
                    .bind(id)
                    .execute(db.pool())
                    .await
                    .unwrap();
            }
        }
        snapshot
    }

    async fn restore_active_devs(db: &crate::orm::Db, ids: &[i64]) {
        for id in ids {
            let _ = sqlx::query("UPDATE rustio_users SET is_active = TRUE WHERE id = $1")
                .bind(id)
                .execute(db.pool())
                .await;
        }
    }

    #[tokio::test]
    #[ignore = "needs `RUSTIO_TEST_DB=1` + a running postgres (URL via RUSTIO_TEST_DATABASE_URL or default)"]
    async fn orphan_when_sole_active_dev_demoted_to_user() {
        let _env = ENV_LOCK.lock().await;
        let db = pg_db().await;
        crate::auth::init_user_tables(&db).await.unwrap();
        crate::auth::migrate_user_schema(&db).await.unwrap();

        let dev = make_user(&db, Role::Developer, true).await;
        let restore = isolate_developers(&db, &[dev]).await;

        let orphan = would_orphan_developers(&db, dev, Some(Role::User))
            .await
            .unwrap();
        assert!(orphan, "demoting the sole active developer must orphan");

        restore_active_devs(&db, &restore).await;
        delete_user(&db, dev).await;
    }

    #[tokio::test]
    #[ignore = "needs `RUSTIO_TEST_DB=1` + a running postgres (URL via RUSTIO_TEST_DATABASE_URL or default)"]
    async fn no_orphan_when_sole_dev_kept_as_dev() {
        let _env = ENV_LOCK.lock().await;
        let db = pg_db().await;
        crate::auth::init_user_tables(&db).await.unwrap();
        crate::auth::migrate_user_schema(&db).await.unwrap();

        let dev = make_user(&db, Role::Developer, true).await;
        let restore = isolate_developers(&db, &[dev]).await;

        // Identity update — the cheap-out path returns false without
        // even querying the DB.
        let orphan = would_orphan_developers(&db, dev, Some(Role::Developer))
            .await
            .unwrap();
        assert!(!orphan, "Developer → Developer is a no-op, never orphans");

        restore_active_devs(&db, &restore).await;
        delete_user(&db, dev).await;
    }

    #[tokio::test]
    #[ignore = "needs `RUSTIO_TEST_DB=1` + a running postgres (URL via RUSTIO_TEST_DATABASE_URL or default)"]
    async fn no_orphan_when_two_active_devs() {
        let _env = ENV_LOCK.lock().await;
        let db = pg_db().await;
        crate::auth::init_user_tables(&db).await.unwrap();
        crate::auth::migrate_user_schema(&db).await.unwrap();

        let dev_a = make_user(&db, Role::Developer, true).await;
        let dev_b = make_user(&db, Role::Developer, true).await;
        let restore = isolate_developers(&db, &[dev_a, dev_b]).await;

        // Demoting either still leaves the other.
        let orphan_a = would_orphan_developers(&db, dev_a, Some(Role::User))
            .await
            .unwrap();
        let orphan_b = would_orphan_developers(&db, dev_b, Some(Role::Administrator))
            .await
            .unwrap();
        assert!(!orphan_a, "two devs → demoting A leaves B");
        assert!(!orphan_b, "two devs → demoting B leaves A");

        restore_active_devs(&db, &restore).await;
        delete_user(&db, dev_a).await;
        delete_user(&db, dev_b).await;
    }

    #[tokio::test]
    #[ignore = "needs `RUSTIO_TEST_DB=1` + a running postgres (URL via RUSTIO_TEST_DATABASE_URL or default)"]
    async fn inactive_devs_do_not_count() {
        let _env = ENV_LOCK.lock().await;
        let db = pg_db().await;
        crate::auth::init_user_tables(&db).await.unwrap();
        crate::auth::migrate_user_schema(&db).await.unwrap();

        let active_dev = make_user(&db, Role::Developer, true).await;
        let inactive_dev = make_user(&db, Role::Developer, false).await;
        let restore = isolate_developers(&db, &[active_dev]).await;

        // Even though there's an "inactive developer" in the table,
        // they don't count toward the active pool — demoting the only
        // active one still orphans the system.
        let orphan = would_orphan_developers(&db, active_dev, Some(Role::User))
            .await
            .unwrap();
        assert!(
            orphan,
            "inactive developers must not satisfy the active-dev requirement"
        );

        restore_active_devs(&db, &restore).await;
        delete_user(&db, active_dev).await;
        delete_user(&db, inactive_dev).await;
    }

    #[tokio::test]
    #[ignore = "needs `RUSTIO_TEST_DB=1` + a running postgres (URL via RUSTIO_TEST_DATABASE_URL or default)"]
    async fn non_developer_target_never_orphans() {
        let _env = ENV_LOCK.lock().await;
        let db = pg_db().await;
        crate::auth::init_user_tables(&db).await.unwrap();
        crate::auth::migrate_user_schema(&db).await.unwrap();

        let dev = make_user(&db, Role::Developer, true).await;
        let staff = make_user(&db, Role::Staff, true).await;
        let restore = isolate_developers(&db, &[dev]).await;

        // Demoting / deactivating a non-developer never orphans the
        // developer pool, regardless of how many devs exist.
        let orphan = would_orphan_developers(&db, staff, Some(Role::User))
            .await
            .unwrap();
        assert!(!orphan, "demoting a non-developer can't orphan developers");

        restore_active_devs(&db, &restore).await;
        delete_user(&db, dev).await;
        delete_user(&db, staff).await;
    }

    #[tokio::test]
    #[ignore = "needs `RUSTIO_TEST_DB=1` + a running postgres (URL via RUSTIO_TEST_DATABASE_URL or default)"]
    async fn zero_developers_is_not_an_orphan_state() {
        let _env = ENV_LOCK.lock().await;
        let db = pg_db().await;
        crate::auth::init_user_tables(&db).await.unwrap();
        crate::auth::migrate_user_schema(&db).await.unwrap();

        // Park every active dev as inactive — pre-bootstrap fresh-DB
        // simulation — and demote a random non-dev. Must NOT orphan.
        let restore = isolate_developers(&db, &[]).await;
        let staff = make_user(&db, Role::Staff, true).await;

        let orphan = would_orphan_developers(&db, staff, Some(Role::User))
            .await
            .unwrap();
        assert!(
            !orphan,
            "a zero-developer DB is allowed; the guard only kicks in once at least one dev exists"
        );

        restore_active_devs(&db, &restore).await;
        delete_user(&db, staff).await;
    }

    // ------------------------------------------------------------------
    // Phase 10/a — UserProfile + load_user_profile
    // ------------------------------------------------------------------

    fn unique_email(tag: &str) -> String {
        let pid = std::process::id();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        format!("{tag}_{pid}_{nanos}@example.test")
    }

    /// E.1 (PG) — running `init_tables` twice is a no-op the second time;
    /// the new profile columns and session columns must be present after.
    #[tokio::test]
    #[ignore = "needs `RUSTIO_TEST_DB=1` + a running postgres (URL via RUSTIO_TEST_DATABASE_URL or default)"]
    async fn migration_is_idempotent_and_columns_present() {
        let db = pg_db().await;
        crate::auth::init_tables(&db).await.unwrap();
        // Second run must not error.
        crate::auth::init_tables(&db).await.unwrap();

        let user_cols: Vec<(String,)> = sqlx::query_as(
            "SELECT column_name::text FROM information_schema.columns
             WHERE table_name = 'rustio_users'
               AND column_name IN ('full_name','locale','timezone')
             ORDER BY column_name",
        )
        .fetch_all(db.pool())
        .await
        .unwrap();
        assert_eq!(user_cols.len(), 3, "expected 3 new user columns, got {user_cols:?}");

        let session_cols: Vec<(String,)> = sqlx::query_as(
            "SELECT column_name::text FROM information_schema.columns
             WHERE table_name = 'rustio_sessions'
               AND column_name IN ('ip','user_agent')
             ORDER BY column_name",
        )
        .fetch_all(db.pool())
        .await
        .unwrap();
        assert_eq!(session_cols.len(), 2, "expected 2 new session columns, got {session_cols:?}");
    }

    /// E.2 (PG) — `load_user_profile` returns a fully-populated `UserProfile`
    /// for an existing user; the new optional columns default to None on a
    /// freshly-created user.
    #[tokio::test]
    #[ignore = "needs `RUSTIO_TEST_DB=1` + a running postgres (URL via RUSTIO_TEST_DATABASE_URL or default)"]
    async fn load_user_profile_happy_path() {
        let db = pg_db().await;
        crate::auth::init_tables(&db).await.unwrap();

        let email = unique_email("profile_happy");
        let id = create_user(&db, &email, "secret-pw-123", Role::Staff)
            .await
            .unwrap();

        let profile = load_user_profile(&db, id).await.unwrap().expect("user exists");
        assert_eq!(profile.id, id);
        assert_eq!(profile.email, email);
        assert_eq!(profile.role, Role::Staff);
        assert!(profile.is_active);
        assert!(profile.full_name.is_none());
        assert!(profile.locale.is_none());
        assert!(profile.timezone.is_none());
        assert!(!profile.is_demo);
        assert!(profile.demo_label.is_none());

        let _ = sqlx::query("DELETE FROM rustio_users WHERE id = $1")
            .bind(id)
            .execute(db.pool())
            .await;
    }

    /// E.3 (PG) — `load_user_profile` for a missing id returns Ok(None),
    /// not Err. Callers map None to 404 themselves.
    #[tokio::test]
    #[ignore = "needs `RUSTIO_TEST_DB=1` + a running postgres (URL via RUSTIO_TEST_DATABASE_URL or default)"]
    async fn load_user_profile_missing_returns_none() {
        let db = pg_db().await;
        crate::auth::init_tables(&db).await.unwrap();
        let result = load_user_profile(&db, 999_999_999).await.unwrap();
        assert!(result.is_none(), "missing id must yield Ok(None)");
    }

    /// E.4 (PG) — existing CRUD path (create → find → set_password) keeps
    /// working after the migration. Smoke; not a re-run of the full auth
    /// suite.
    #[tokio::test]
    #[ignore = "needs `RUSTIO_TEST_DB=1` + a running postgres (URL via RUSTIO_TEST_DATABASE_URL or default)"]
    async fn existing_user_crud_unaffected_by_migration() {
        let db = pg_db().await;
        crate::auth::init_tables(&db).await.unwrap();

        let email = unique_email("crud_smoke");
        let id = create_user(&db, &email, "secret-pw-123", Role::User)
            .await
            .unwrap();

        let found = find_user_by_email(&db, &email).await.unwrap().expect("found");
        assert_eq!(found.id, id);

        set_password(&db, id, "new-secret-456").await.unwrap();
        let after = find_user_by_email(&db, &email).await.unwrap().expect("still there");
        assert!(verify_password("new-secret-456", &after.password_hash));

        let _ = sqlx::query("DELETE FROM rustio_users WHERE id = $1")
            .bind(id)
            .execute(db.pool())
            .await;
    }
}
