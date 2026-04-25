//! User records, password hashing, and the login flow.

use argon2::password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use chrono::Utc;
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
    .map_err(|e| Error::BadRequest(format!("could not create user: {e}")))?;
    let id: i64 = row
        .try_get("id")
        .map_err(|e| Error::Internal(format!("returning id: {e}")))?;
    Ok(id)
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
    fn password_round_trip() {
        let h = hash_password("secret").unwrap();
        assert!(verify_password("secret", &h));
        assert!(!verify_password("wrong", &h));
    }

    // `Role` parsing + ladder semantics moved to `auth/role.rs`
    // (25-case `includes` matrix + parse round-trip).
}
