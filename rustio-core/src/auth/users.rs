//! User records, password hashing, and the login flow.

use argon2::password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use chrono::Utc;
use sqlx::Row as SqlxRow;

use crate::error::{Error, Result};
use crate::orm::{Db, Row};

use super::sessions::create_session;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Admin,
    Staff,
    User,
}

impl Role {
    pub fn as_str(&self) -> &'static str {
        match self {
            Role::Admin => "admin",
            Role::Staff => "staff",
            Role::User => "user",
        }
    }

    pub fn parse(s: &str) -> Result<Self> {
        match s {
            "admin" => Ok(Role::Admin),
            "staff" => Ok(Role::Staff),
            "user" => Ok(Role::User),
            other => Err(Error::BadRequest(format!("unknown role: {other}"))),
        }
    }

    /// Admin bypasses permission checks. Staff still gets checked.
    pub fn is_superuser(&self) -> bool {
        matches!(self, Role::Admin)
    }
}

/// The identity attached to a request by the auth middleware. Kept
/// cheap to clone because we pass it into handler bodies.
#[derive(Debug, Clone)]
pub struct Identity {
    pub user_id: i64,
    pub email: String,
    pub role: Role,
    pub is_active: bool,
}

impl Identity {
    pub fn is_admin(&self) -> bool {
        self.is_active && matches!(self.role, Role::Admin)
    }

    pub fn can_access_admin(&self) -> bool {
        self.is_active && matches!(self.role, Role::Admin | Role::Staff)
    }
}

pub struct StoredUser {
    pub id: i64,
    pub email: String,
    pub password_hash: String,
    pub role: Role,
    pub is_active: bool,
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
        "SELECT id, email, password_hash, role, is_active
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

    #[test]
    fn role_parsing() {
        assert_eq!(Role::parse("admin").unwrap(), Role::Admin);
        assert_eq!(Role::parse("staff").unwrap(), Role::Staff);
        assert_eq!(Role::parse("user").unwrap(), Role::User);
        assert!(Role::parse("root").is_err());
    }

    #[test]
    fn admin_is_superuser_staff_is_not() {
        assert!(Role::Admin.is_superuser());
        assert!(!Role::Staff.is_superuser());
        assert!(!Role::User.is_superuser());
    }
}
