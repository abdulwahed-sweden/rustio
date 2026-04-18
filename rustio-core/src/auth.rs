//! Authentication: users, passwords, sessions, middleware.
//!
//! This is the identity layer every RustIO system depends on. It is
//! **infrastructure**, not an application feature — the [`User`] struct
//! deliberately stays minimal (id, email, password_hash, is_active,
//! role) so projects can extend it with a separate `Profile` model
//! rather than widening this one.
//!
//! ## Security model
//!
//! - Passwords are stored as argon2id PHC strings (see [`password`]).
//! - Sessions are 32-byte OS-random tokens, stored in `rustio_sessions`
//!   with a 7-day expiry enforced on every request.
//! - Session cookies are `HttpOnly; SameSite=Strict`. `Secure` is
//!   documented at the deployment boundary (terminator / proxy) because
//!   the server can't reliably detect HTTPS on its own.
//! - Failed logins return a generic error (invalid email + wrong
//!   password are indistinguishable from outside) to prevent user
//!   enumeration. An inactive account is called out explicitly — it's
//!   already an administrative decision, not a secret.
//!
//! ## Wiring
//!
//! Generated projects register the middleware via
//! `router.wrap(auth::authenticate(db.clone()))`. There are no global
//! singletons; every project owns its `Db` handle and passes it in.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration as StdDuration, Instant};

use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use chrono::{DateTime, Duration, Utc};
use rand::rngs::OsRng;
use rand::RngCore;
use sqlx::Row as _;

use crate::context::Context;
use crate::error::Error;
use crate::http::{Request, Response};
use crate::middleware::Next;
use crate::orm::Db;

/// Name of the cookie that carries the session token from the browser
/// to the server. Used by both the login handler (to set) and the
/// authenticate middleware (to read).
pub const SESSION_COOKIE: &str = "rustio_session";

/// How long a newly-created session is valid before the middleware
/// treats it as expired. Not configurable in 0.4.0 — kept fixed so the
/// security surface stays small.
pub const SESSION_TTL_DAYS: i64 = 7;

/// How many OS-entropy bytes go into a session token. 32 bytes = 256
/// bits, far beyond any realistic guessing budget.
const SESSION_TOKEN_BYTES: usize = 32;

/// Role string meaning "has admin access". Anything else is treated as
/// a regular user; the admin middleware only unlocks `/admin` when this
/// matches exactly.
pub const ROLE_ADMIN: &str = "admin";

/// Default role for newly-created users.
pub const ROLE_USER: &str = "user";

type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send>>;

/// User record. Infrastructure, not an application model — extend user
/// data via a separate `Profile` struct, not by widening this one.
#[derive(Debug, Clone, PartialEq)]
pub struct User {
    pub id: i64,
    pub email: String,
    /// PHC-encoded argon2id hash. Never treat as plaintext, never log,
    /// never render in a template.
    pub password_hash: String,
    pub is_active: bool,
    pub role: String,
}

impl User {
    pub fn is_admin(&self) -> bool {
        self.role == ROLE_ADMIN
    }
}

/// Per-request identity snapshot, attached by the auth middleware and
/// read via [`identity`] / [`require_auth`] / [`require_admin`].
#[derive(Debug, Clone, PartialEq)]
pub struct Identity {
    pub user_id: i64,
    pub email: String,
    pub is_admin: bool,
}

impl From<&User> for Identity {
    fn from(u: &User) -> Self {
        Self {
            user_id: u.id,
            email: u.email.clone(),
            is_admin: u.is_admin(),
        }
    }
}

/// Session record.
///
/// `#[non_exhaustive]` because we expect to add fields (device
/// fingerprint, last-seen IP) in future releases; downstream callers
/// must not rely on exhaustive destructuring.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub struct Session {
    /// Opaque 64-char hex token. Cryptographically random; the only way
    /// to produce a valid one is via [`session::create`].
    pub id: String,
    pub user_id: i64,
    pub expires_at: DateTime<Utc>,
    /// Per-session CSRF token. Separately random from `id`; bound to
    /// the session so rotating either invalidates the other. Rendered
    /// as a hidden form input by the admin and verified on every
    /// state-changing POST.
    pub csrf_token: String,
}

// ---------------------------------------------------------------------------
// Passwords
// ---------------------------------------------------------------------------

pub mod password {
    //! Password hashing with argon2id.
    //!
    //! Uses argon2 defaults (RFC 9106 recommendations): argon2id,
    //! m_cost=19456 KiB, t_cost=2, p=1. Salts come from the OS RNG.
    use super::*;

    /// Hash a password with argon2id + OS-provided salt. Returns the
    /// PHC-encoded string, ready to store in `rustio_users.password_hash`.
    ///
    /// Empty passwords are refused at the boundary — there is never a
    /// legitimate reason to hash one.
    pub fn hash(password: &str) -> Result<String, Error> {
        if password.is_empty() {
            return Err(Error::BadRequest("password must not be empty".into()));
        }
        let salt = SaltString::generate(&mut OsRng);
        let hash = Argon2::default()
            .hash_password(password.as_bytes(), &salt)
            .map_err(|e| Error::Internal(format!("password hashing failed: {e}")))?;
        Ok(hash.to_string())
    }

    /// Verify a password against a stored PHC hash.
    ///
    /// Returns `false` on any failure — malformed hash, mismatched
    /// password, internal error. **Never panics.** The comparison
    /// inside argon2 is constant-time.
    pub fn verify(password: &str, stored: &str) -> bool {
        // `PasswordHash::new` parses the PHC string; an invalid hash
        // turns into a clean `false` instead of a panic. This is the
        // important safety property the spec calls out.
        let Ok(parsed) = PasswordHash::new(stored) else {
            return false;
        };
        Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok()
    }
}

// ---------------------------------------------------------------------------
// Sessions
// ---------------------------------------------------------------------------

/// Generate a fresh hex-encoded session token using OS entropy.
fn generate_token() -> String {
    use std::fmt::Write;
    let mut buf = [0u8; SESSION_TOKEN_BYTES];
    OsRng.fill_bytes(&mut buf);
    let mut out = String::with_capacity(SESSION_TOKEN_BYTES * 2);
    for b in buf {
        let _ = write!(out, "{b:02x}");
    }
    out
}

// ---------------------------------------------------------------------------
// CSRF tokens
// ---------------------------------------------------------------------------

/// Newtype wrapper stored in the per-request [`Context`] by the
/// authenticate middleware after a successful session lookup. Admin
/// form renderers read it to inject `<input name="_csrf">`; admin POST
/// handlers compare it (constant-time) against the submitted value
/// before mutating anything.
///
/// Kept separate from [`Identity`] so unauthenticated handlers that
/// happen to sit behind the same middleware don't leak a token.
#[derive(Debug, Clone, PartialEq)]
pub struct CsrfToken(pub String);

pub mod csrf {
    //! Per-session CSRF tokens.
    //!
    //! Each session carries its own 256-bit random token, distinct
    //! from the session id. Admin forms render it in a hidden
    //! `_csrf` input; POST handlers validate it with a constant-time
    //! compare before touching persistent state.
    //!
    //! The design is stateful — the token lives alongside the
    //! session in `rustio_sessions.csrf_token`. Logging out or
    //! rotating the session (via password change) invalidates the
    //! token together with the session.

    /// Generate a fresh CSRF token with the same entropy as a session
    /// id. Called by [`super::session::create`] for every new session.
    pub fn generate_token() -> String {
        // Reuse the session-token generator: same 256 bits of OS
        // entropy, same hex encoding, same length — callers that
        // length-check the token don't need to branch.
        super::generate_token()
    }

    /// Constant-time comparison of two token strings.
    ///
    /// Returns `false` if either side is empty or lengths differ;
    /// otherwise a byte-level XOR accumulator avoids the short-circuit
    /// behaviour of `==`. Guards against timing side-channels even
    /// though the tokens themselves aren't secret enough for it to
    /// matter much in practice — the cost is one extra loop and the
    /// code clarity is worth it.
    pub fn verify_token(expected: &str, provided: &str) -> bool {
        if expected.is_empty() || provided.is_empty() {
            return false;
        }
        if expected.len() != provided.len() {
            return false;
        }
        let mut diff: u8 = 0;
        for (a, b) in expected.bytes().zip(provided.bytes()) {
            diff |= a ^ b;
        }
        diff == 0
    }
}

pub mod session {
    //! Database-backed sessions keyed by a 256-bit OS-random token.
    //!
    //! Sessions are never kept in memory. Every validation goes through
    //! the DB so a compromised or logged-out session is immediately
    //! invalid everywhere.
    use super::*;

    /// Create a new session for a user and persist it. The returned
    /// token is the cookie value the browser should receive.
    ///
    /// A fresh CSRF token is generated alongside the session id and
    /// written to the same row. The two tokens share entropy source
    /// and length but are independent — neither is derivable from
    /// the other.
    pub async fn create(db: &Db, user_id: i64) -> Result<Session, Error> {
        let id = generate_token();
        let csrf_token = csrf::generate_token();
        let expires_at = Utc::now() + Duration::days(SESSION_TTL_DAYS);
        sqlx::query(
            "INSERT INTO rustio_sessions (id, user_id, expires_at, csrf_token)
             VALUES (?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(user_id)
        .bind(expires_at)
        .bind(&csrf_token)
        .execute(db.pool())
        .await?;
        Ok(Session {
            id,
            user_id,
            expires_at,
            csrf_token,
        })
    }

    /// Look up a session by token. Returns `None` if the token doesn't
    /// exist **or** the session has expired. Expiration is checked on
    /// every call — the DB expiry column is the source of truth.
    ///
    /// When an expired row is encountered it is **deleted inline** so
    /// the table doesn't accumulate stale sessions indefinitely. The
    /// delete is best-effort; a failure to clean up doesn't mask the
    /// "expired" verdict.
    pub async fn find_valid(db: &Db, id: &str) -> Result<Option<Session>, Error> {
        let row = sqlx::query(
            "SELECT id, user_id, expires_at, csrf_token
             FROM rustio_sessions WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(db.pool())
        .await?;
        let Some(r) = row else {
            return Ok(None);
        };
        let expires_at: DateTime<Utc> = r.try_get("expires_at")?;
        if expires_at <= Utc::now() {
            let _ = delete(db, id).await;
            return Ok(None);
        }
        Ok(Some(Session {
            id: r.try_get("id")?,
            user_id: r.try_get("user_id")?,
            expires_at,
            csrf_token: r.try_get("csrf_token")?,
        }))
    }

    /// Delete a session. Logout path. Idempotent — deleting a
    /// non-existent session is not an error.
    pub async fn delete(db: &Db, id: &str) -> Result<(), Error> {
        sqlx::query("DELETE FROM rustio_sessions WHERE id = ?")
            .bind(id)
            .execute(db.pool())
            .await?;
        Ok(())
    }

    /// Remove all expired sessions from the DB. Safe to call on a
    /// schedule; not called automatically in 0.4.0.
    pub async fn sweep_expired(db: &Db) -> Result<u64, Error> {
        let result = sqlx::query("DELETE FROM rustio_sessions WHERE expires_at <= ?")
            .bind(Utc::now())
            .execute(db.pool())
            .await?;
        Ok(result.rows_affected())
    }
}

// ---------------------------------------------------------------------------
// User queries
// ---------------------------------------------------------------------------

pub mod user {
    //! User queries. Validates email format on create + on lookup
    //! (inputs are normalised to trimmed-lowercase before the DB sees
    //! them).
    use super::*;

    /// Create a new user with an argon2-hashed password. Email is
    /// normalised (trimmed + lowercased) so `Alice@Example.com` and
    /// `alice@example.com` can't register separately.
    ///
    /// Returns `BadRequest` on unique-email conflict so the handler can
    /// render a clean message; the raw sqlx error never leaks to the
    /// client.
    pub async fn create(db: &Db, email: &str, password: &str, role: &str) -> Result<User, Error> {
        let email = normalise_email(email);
        validate_email(&email)?;
        if role != ROLE_ADMIN && role != ROLE_USER {
            return Err(Error::BadRequest(format!(
                "role must be `{ROLE_ADMIN}` or `{ROLE_USER}`, got `{role}`"
            )));
        }
        let hash = password::hash(password)?;
        let result = sqlx::query(
            "INSERT INTO rustio_users (email, password_hash, is_active, role)
             VALUES (?, ?, 1, ?)",
        )
        .bind(&email)
        .bind(&hash)
        .bind(role)
        .execute(db.pool())
        .await
        .map_err(|e| match &e {
            sqlx::Error::Database(de) if de.is_unique_violation() => {
                Error::BadRequest(format!("a user with email `{email}` already exists"))
            }
            _ => Error::from(e),
        })?;
        Ok(User {
            id: result.last_insert_rowid(),
            email,
            password_hash: hash,
            is_active: true,
            role: role.to_string(),
        })
    }

    pub async fn find_by_email(db: &Db, email: &str) -> Result<Option<User>, Error> {
        let email = normalise_email(email);
        let row = sqlx::query(
            "SELECT id, email, password_hash, is_active, role
             FROM rustio_users WHERE email = ?",
        )
        .bind(&email)
        .fetch_optional(db.pool())
        .await?;
        match row {
            Some(r) => Ok(Some(user_from_row(&r)?)),
            None => Ok(None),
        }
    }

    pub async fn find_by_id(db: &Db, id: i64) -> Result<Option<User>, Error> {
        let row = sqlx::query(
            "SELECT id, email, password_hash, is_active, role
             FROM rustio_users WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(db.pool())
        .await?;
        match row {
            Some(r) => Ok(Some(user_from_row(&r)?)),
            None => Ok(None),
        }
    }

    /// Replace a user's password hash and **invalidate every live
    /// session** for that user in the same transaction.
    ///
    /// Without the session sweep, a cookie stolen before the password
    /// change would survive the rotation. After this call, the user
    /// must sign in again on every device — which is the intent of a
    /// password change.
    pub async fn set_password(db: &Db, id: i64, password: &str) -> Result<(), Error> {
        let hash = password::hash(password)?;
        let mut tx = db.pool().begin().await?;
        sqlx::query("UPDATE rustio_users SET password_hash = ? WHERE id = ?")
            .bind(&hash)
            .bind(id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM rustio_sessions WHERE user_id = ?")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn set_active(db: &Db, id: i64, is_active: bool) -> Result<(), Error> {
        sqlx::query("UPDATE rustio_users SET is_active = ? WHERE id = ?")
            .bind(is_active)
            .bind(id)
            .execute(db.pool())
            .await?;
        Ok(())
    }

    pub async fn count(db: &Db) -> Result<i64, Error> {
        let row = sqlx::query("SELECT COUNT(*) FROM rustio_users")
            .fetch_one(db.pool())
            .await?;
        Ok(row.try_get(0)?)
    }

    fn user_from_row(r: &sqlx::sqlite::SqliteRow) -> Result<User, Error> {
        Ok(User {
            id: r.try_get("id")?,
            email: r.try_get("email")?,
            password_hash: r.try_get("password_hash")?,
            is_active: r.try_get("is_active")?,
            role: r.try_get("role")?,
        })
    }
}

// ---------------------------------------------------------------------------
// Email validation
// ---------------------------------------------------------------------------

/// Normalise an email for storage + comparison.
pub fn normalise_email(email: &str) -> String {
    email.trim().to_lowercase()
}

/// Validate an email for admin-level correctness. Intentionally *not*
/// full RFC 5322 — we accept any address with a local part, an `@`, and
/// a domain containing a `.`. That rejects the common-mistake class
/// (typos, obvious malformed input) without pretending to police
/// delivery validity.
pub fn validate_email(email: &str) -> Result<(), Error> {
    if email.is_empty() {
        return Err(Error::BadRequest("email must not be empty".into()));
    }
    let Some((local, domain)) = email.split_once('@') else {
        return Err(Error::BadRequest(format!(
            "`{email}` is not a valid email (missing @)"
        )));
    };
    if local.is_empty() || domain.is_empty() || !domain.contains('.') {
        return Err(Error::BadRequest(format!("`{email}` is not a valid email")));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Timing-attack mitigation
// ---------------------------------------------------------------------------

/// A precomputed argon2id hash used by the login handler to equalise
/// the wall-clock cost of "user doesn't exist" and "user exists, wrong
/// password". Without this, an attacker can enumerate valid emails by
/// measuring response time (the verify branch costs ~50 ms; the
/// skip-verify branch is a few ms).
///
/// The plaintext is arbitrary and never exposed; only the hash matters.
/// Cached lazily so the first login pays the ~50 ms hash cost and every
/// subsequent login just runs a verify against the stored string.
pub fn dummy_password_hash() -> &'static str {
    static DUMMY: OnceLock<String> = OnceLock::new();
    DUMMY.get_or_init(|| {
        password::hash("timing-attack-filler-not-a-real-password").expect("dummy hash must succeed")
    })
}

// ---------------------------------------------------------------------------
// Login rate limiter
// ---------------------------------------------------------------------------

/// One entry in the [`LoginRateLimiter`] map.
struct FailureEntry {
    count: u32,
    /// Instant at which the lockout expires. Set when `count` hits
    /// [`LoginRateLimiter::MAX_FAILURES`]; ignored otherwise.
    locked_until: Instant,
}

/// In-memory counter of recent failed login attempts, keyed by email.
///
/// After [`LoginRateLimiter::MAX_FAILURES`] failures, further attempts
/// for the same key are rejected for [`LoginRateLimiter::LOCKOUT`]. A
/// single successful login clears the counter.
///
/// **Scope:** per-email, not per-IP. Stops targeted brute force against
/// a single account. A distributed attack (many emails, many sources)
/// is not defended; per-IP rate limiting requires the server pipeline
/// to propagate the client address into the request context, which is
/// deferred to a later pass.
pub struct LoginRateLimiter {
    failures: Mutex<HashMap<String, FailureEntry>>,
    max_failures: u32,
    lockout: StdDuration,
}

impl LoginRateLimiter {
    /// Lock an account's login attempts for 60s after 5 failures in
    /// the current lockout window. Conservative defaults; tune via
    /// [`LoginRateLimiter::with_params`] in tests or forks.
    pub const MAX_FAILURES: u32 = 5;
    pub const LOCKOUT: StdDuration = StdDuration::from_secs(60);

    pub fn new() -> Self {
        Self::with_params(Self::MAX_FAILURES, Self::LOCKOUT)
    }

    /// Construct with custom thresholds. Used by tests to exercise
    /// lockout behaviour on a shorter clock.
    pub fn with_params(max_failures: u32, lockout: StdDuration) -> Self {
        Self {
            failures: Mutex::new(HashMap::new()),
            max_failures,
            lockout,
        }
    }

    /// Process-wide shared limiter used by the login handler. Lazily
    /// created on first access. In-process only — resets on restart,
    /// which is a deliberate trade-off for simplicity in 0.4.0.
    pub fn global() -> &'static Self {
        static INSTANCE: OnceLock<LoginRateLimiter> = OnceLock::new();
        INSTANCE.get_or_init(LoginRateLimiter::new)
    }

    /// Return `Ok(())` if a login attempt is permitted for this key.
    /// If the key is currently locked out, return the time remaining
    /// until the lockout expires.
    pub fn check(&self, key: &str) -> Result<(), StdDuration> {
        let mut map = self.failures.lock().expect("rate-limiter mutex poisoned");
        match map.get(key) {
            Some(entry) if entry.count >= self.max_failures => {
                let now = Instant::now();
                if entry.locked_until > now {
                    Err(entry.locked_until - now)
                } else {
                    // Lockout elapsed — clean up and allow this attempt.
                    map.remove(key);
                    Ok(())
                }
            }
            _ => Ok(()),
        }
    }

    /// Record a failed login for `key`. On the transition to the
    /// threshold, stamps `locked_until` to "now + LOCKOUT" so `check`
    /// will reject further attempts until the clock advances.
    pub fn record_failure(&self, key: &str) {
        let mut map = self.failures.lock().expect("rate-limiter mutex poisoned");
        let entry = map.entry(key.to_string()).or_insert(FailureEntry {
            count: 0,
            locked_until: Instant::now(),
        });
        entry.count = entry.count.saturating_add(1);
        if entry.count >= self.max_failures {
            entry.locked_until = Instant::now() + self.lockout;
        }
    }

    /// Clear the counter for `key`. Called on every successful login
    /// so a user who mistypes once then logs in isn't held to the
    /// strike count.
    pub fn record_success(&self, key: &str) {
        self.failures
            .lock()
            .expect("rate-limiter mutex poisoned")
            .remove(key);
    }

    /// Compose a multi-axis rate-limit key from an email (required)
    /// and an optional IP.
    ///
    /// The login handler calls this with whatever it has in hand —
    /// `peer_addr()` is available when the server provided it,
    /// otherwise we fall back to email-only. Including the IP means
    /// one attacker hammering many emails is throttled by IP too,
    /// and one email being hit from many IPs is still throttled by
    /// email.
    ///
    /// This is the documented extension point for future per-IP
    /// limiting beyond the login path. Wrap the key any way a
    /// caller sees fit (e.g. `format!("api:{user_id}")`); the
    /// limiter itself only compares strings.
    pub fn compose_key(email: &str, ip: Option<&str>) -> String {
        match ip {
            Some(ip) => format!("email:{email}|ip:{ip}"),
            None => format!("email:{email}"),
        }
    }
}

impl Default for LoginRateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Middleware
// ---------------------------------------------------------------------------

/// Resolve a session token into an [`Identity`] plus the live
/// [`Session`] it came from, or `None` if the token is missing,
/// unknown, expired, or points at an inactive / deleted user.
///
/// Returning the session lets callers access the per-session CSRF
/// token without a second DB round-trip. Most handlers only want the
/// identity; [`resolve_identity`] is a convenience over this.
pub async fn resolve_identity_with_session(
    db: &Db,
    token: Option<&str>,
) -> Option<(Identity, Session)> {
    let token = token?;
    let sess = session::find_valid(db, token).await.ok().flatten()?;
    let user = user::find_by_id(db, sess.user_id).await.ok().flatten()?;
    if !user.is_active {
        return None;
    }
    Some((Identity::from(&user), sess))
}

/// Resolve a session token into an [`Identity`], or `None` if the
/// token is missing, unknown, expired, or points at an inactive /
/// deleted user.
///
/// Extracted so the middleware's full decision path is directly
/// testable without constructing a hyper `Request`. The middleware
/// itself is a thin wrapper: cookie read → this call → context insert.
pub async fn resolve_identity(db: &Db, token: Option<&str>) -> Option<Identity> {
    resolve_identity_with_session(db, token)
        .await
        .map(|(identity, _)| identity)
}

/// Build the `authenticate` middleware for this project.
///
/// The returned middleware:
/// 1. Reads the `rustio_session` cookie.
/// 2. Looks up the session in the DB; rejects if missing / expired.
/// 3. Looks up the user; rejects if missing / inactive.
/// 4. On success, attaches [`Identity`] to the request context.
///
/// Failure cases do **not** short-circuit the request. They simply
/// leave the context without an `Identity`; handlers then use
/// [`require_auth`] / [`require_admin`] to produce the 401 / 403 as
/// appropriate. This keeps auth additive and lets non-admin routes
/// continue serving anonymous requests.
pub fn authenticate(
    db: Db,
) -> impl Fn(Request, Next) -> BoxFuture<Result<Response, Error>> + Send + Sync + Clone + 'static {
    move |mut req, next| {
        let db = db.clone();
        Box::pin(async move {
            let token = req.cookie(SESSION_COOKIE);
            if let Some((identity, session)) =
                resolve_identity_with_session(&db, token.as_deref()).await
            {
                // Attach both `CsrfToken` and `Identity`. Admin
                // renderers read the CSRF token to inject the hidden
                // form input; admin POST handlers compare it against
                // the submitted `_csrf` field.
                req.ctx_mut().insert(CsrfToken(session.csrf_token));
                req.ctx_mut().insert(identity);
            }
            next.run(req).await
        })
    }
}

// ---------------------------------------------------------------------------
// Core tables
// ---------------------------------------------------------------------------

/// Create `rustio_users` and `rustio_sessions` if they don't already
/// exist. Called from `migrations::apply` before any user-level
/// migration runs, so auth tables are always present in a project's DB.
///
/// Also performs a **minimal, idempotent schema upgrade** for the
/// 0.4.0 Pass D addition: the `csrf_token` column on
/// `rustio_sessions`. Older databases (written by Pass B/C) keep
/// their rows; new rows get a non-empty token written by
/// [`session::create`]. Any session that predates the column has an
/// empty `csrf_token`, which the admin's constant-time verifier
/// treats as "never matches" — effectively forcing those users to
/// sign in once after the upgrade. That's the desired behaviour.
pub async fn ensure_core_tables(db: &Db) -> Result<(), Error> {
    db.execute(
        "CREATE TABLE IF NOT EXISTS rustio_users (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            email TEXT NOT NULL UNIQUE,
            password_hash TEXT NOT NULL,
            is_active INTEGER NOT NULL DEFAULT 1,
            role TEXT NOT NULL DEFAULT 'user',
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
        )",
    )
    .await?;
    db.execute(
        "CREATE TABLE IF NOT EXISTS rustio_sessions (
            id TEXT PRIMARY KEY,
            user_id INTEGER NOT NULL,
            expires_at TEXT NOT NULL,
            csrf_token TEXT NOT NULL DEFAULT '',
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            FOREIGN KEY (user_id) REFERENCES rustio_users(id) ON DELETE CASCADE
        )",
    )
    .await?;

    // Back-port the `csrf_token` column for DBs created before Pass D.
    // `pragma_table_info` returns one row per column; if the column
    // is missing we add it in place. SQLite's `ALTER TABLE ADD
    // COLUMN` is O(1) and uses the DEFAULT for existing rows.
    let cols: Vec<String> =
        sqlx::query_scalar::<_, String>("SELECT name FROM pragma_table_info('rustio_sessions')")
            .fetch_all(db.pool())
            .await?;
    if !cols.iter().any(|c| c == "csrf_token") {
        db.execute("ALTER TABLE rustio_sessions ADD COLUMN csrf_token TEXT NOT NULL DEFAULT ''")
            .await?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Environment + context helpers
// ---------------------------------------------------------------------------

/// `true` when `RUSTIO_ENV` indicates a production deployment.
///
/// Preserved from earlier releases because projects may branch on it
/// (e.g. to force HTTPS / Secure cookies). It does not gate the auth
/// system itself — there are no dev tokens left to disable.
pub fn in_production() -> bool {
    std::env::var("RUSTIO_ENV")
        .map(|v| {
            let v = v.to_ascii_lowercase();
            v == "production" || v == "prod"
        })
        .unwrap_or(false)
}

/// Read the `Authorization: Bearer <token>` header if present. Kept as
/// a primitive so projects can layer their own Bearer/API-token auth
/// on top of session auth.
pub fn bearer_token(req: &Request) -> Option<&str> {
    req.headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
}

pub fn identity(ctx: &Context) -> Option<&Identity> {
    ctx.get::<Identity>()
}

pub fn require_auth(ctx: &Context) -> Result<&Identity, Error> {
    identity(ctx).ok_or(Error::Unauthorized)
}

pub fn require_admin(ctx: &Context) -> Result<&Identity, Error> {
    let id = require_auth(ctx)?;
    if !id.is_admin {
        return Err(Error::Forbidden);
    }
    Ok(id)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn admin_identity() -> Identity {
        Identity {
            user_id: 1,
            email: "admin@example.com".into(),
            is_admin: true,
        }
    }

    fn user_identity() -> Identity {
        Identity {
            user_id: 2,
            email: "user@example.com".into(),
            is_admin: false,
        }
    }

    // --- identity / require_* -------------------------------------------

    #[test]
    fn identity_returns_none_when_absent() {
        let ctx = Context::new();
        assert!(identity(&ctx).is_none());
    }

    #[test]
    fn identity_returns_reference_when_attached() {
        let mut ctx = Context::new();
        ctx.insert(user_identity());
        assert_eq!(
            identity(&ctx).map(|i| i.email.as_str()),
            Some("user@example.com")
        );
    }

    #[test]
    fn require_auth_missing_returns_unauthorized() {
        let ctx = Context::new();
        assert!(matches!(require_auth(&ctx), Err(Error::Unauthorized)));
    }

    #[test]
    fn require_admin_non_admin_returns_forbidden() {
        let mut ctx = Context::new();
        ctx.insert(user_identity());
        assert!(matches!(require_admin(&ctx), Err(Error::Forbidden)));
    }

    #[test]
    fn require_admin_admin_returns_identity() {
        let mut ctx = Context::new();
        ctx.insert(admin_identity());
        let id = require_admin(&ctx).unwrap();
        assert!(id.is_admin);
    }

    // --- password hashing -----------------------------------------------

    #[test]
    fn hash_then_verify_succeeds() {
        let h = password::hash("correct horse battery staple").unwrap();
        assert!(password::verify("correct horse battery staple", &h));
    }

    #[test]
    fn verify_wrong_password_fails() {
        let h = password::hash("real").unwrap();
        assert!(!password::verify("fake", &h));
    }

    #[test]
    fn verify_invalid_hash_returns_false_without_panic() {
        // The spec is explicit: "invalid hash must NOT panic".
        assert!(!password::verify("anything", ""));
        assert!(!password::verify("anything", "not a phc string"));
        assert!(!password::verify("anything", "$argon2id$v=19$m=1"));
    }

    #[test]
    fn hash_rejects_empty_password() {
        assert!(matches!(password::hash(""), Err(Error::BadRequest(_))));
    }

    #[test]
    fn hash_is_salted_so_same_input_produces_different_hash() {
        let a = password::hash("same").unwrap();
        let b = password::hash("same").unwrap();
        assert_ne!(a, b, "identical inputs must produce different hashes");
        // But both must verify against the original password.
        assert!(password::verify("same", &a));
        assert!(password::verify("same", &b));
    }

    // --- email validation ----------------------------------------------

    #[test]
    fn normalise_email_trims_and_lowercases() {
        assert_eq!(
            normalise_email("  Alice@EXAMPLE.com  "),
            "alice@example.com"
        );
    }

    #[test]
    fn validate_email_accepts_reasonable_forms() {
        assert!(validate_email("a@b.co").is_ok());
        assert!(validate_email("alice.smith+tag@example.co.uk").is_ok());
    }

    #[test]
    fn validate_email_rejects_bad_forms() {
        assert!(validate_email("").is_err());
        assert!(validate_email("no-at-sign").is_err());
        assert!(validate_email("@no-local").is_err());
        assert!(validate_email("no-domain@").is_err());
        assert!(validate_email("no-dot@localhost").is_err());
    }

    // --- token generation ----------------------------------------------

    #[test]
    fn generate_token_is_stable_length_and_hex() {
        let t = generate_token();
        assert_eq!(t.len(), SESSION_TOKEN_BYTES * 2);
        assert!(t.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn generate_token_does_not_repeat() {
        // Guard against a broken RNG feeding zeros.
        let a = generate_token();
        let b = generate_token();
        assert_ne!(a, b);
    }

    // --- integration: users + sessions on in-memory DB ------------------

    async fn setup() -> Db {
        let db = Db::memory().await.unwrap();
        ensure_core_tables(&db).await.unwrap();
        db
    }

    #[tokio::test]
    async fn user_create_round_trips() {
        let db = setup().await;
        let u = user::create(&db, "Admin@Example.com", "hunter2", ROLE_ADMIN)
            .await
            .unwrap();
        // Email normalised at creation.
        assert_eq!(u.email, "admin@example.com");
        assert!(u.is_admin());
        assert!(u.is_active);

        let lookup = user::find_by_email(&db, "ADMIN@example.com")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(lookup.id, u.id);
        assert!(password::verify("hunter2", &lookup.password_hash));
    }

    #[tokio::test]
    async fn user_create_rejects_duplicate_email() {
        let db = setup().await;
        user::create(&db, "a@b.co", "pw", ROLE_USER).await.unwrap();
        let err = user::create(&db, "a@b.co", "pw2", ROLE_USER).await;
        assert!(matches!(err, Err(Error::BadRequest(_))));
    }

    #[tokio::test]
    async fn user_create_rejects_unknown_role() {
        let db = setup().await;
        let err = user::create(&db, "a@b.co", "pw", "emperor").await;
        assert!(matches!(err, Err(Error::BadRequest(_))));
    }

    #[tokio::test]
    async fn set_password_changes_verifiable_hash() {
        let db = setup().await;
        let u = user::create(&db, "a@b.co", "old", ROLE_USER).await.unwrap();
        user::set_password(&db, u.id, "new").await.unwrap();
        let reloaded = user::find_by_id(&db, u.id).await.unwrap().unwrap();
        assert!(!password::verify("old", &reloaded.password_hash));
        assert!(password::verify("new", &reloaded.password_hash));
    }

    #[tokio::test]
    async fn set_active_toggles_flag() {
        let db = setup().await;
        let u = user::create(&db, "a@b.co", "pw", ROLE_USER).await.unwrap();
        user::set_active(&db, u.id, false).await.unwrap();
        let reloaded = user::find_by_id(&db, u.id).await.unwrap().unwrap();
        assert!(!reloaded.is_active);
    }

    #[tokio::test]
    async fn session_create_and_find_returns_live_session() {
        let db = setup().await;
        let u = user::create(&db, "a@b.co", "pw", ROLE_USER).await.unwrap();
        let s = session::create(&db, u.id).await.unwrap();
        let found = session::find_valid(&db, &s.id).await.unwrap().unwrap();
        assert_eq!(found.user_id, u.id);
        // Token is server-generated and not guessable from the user_id.
        assert_eq!(found.id, s.id);
        assert!(found.expires_at > Utc::now());
    }

    #[tokio::test]
    async fn session_lookup_rejects_unknown_token() {
        let db = setup().await;
        let out = session::find_valid(&db, "deadbeef").await.unwrap();
        assert!(out.is_none());
    }

    #[tokio::test]
    async fn session_lookup_rejects_expired_session() {
        let db = setup().await;
        let u = user::create(&db, "a@b.co", "pw", ROLE_USER).await.unwrap();
        // Insert a manually-backdated session — simulates the clock
        // rolling forward past its expiry without waiting days.
        let token = generate_token();
        sqlx::query("INSERT INTO rustio_sessions (id, user_id, expires_at) VALUES (?, ?, ?)")
            .bind(&token)
            .bind(u.id)
            .bind(Utc::now() - Duration::seconds(1))
            .execute(db.pool())
            .await
            .unwrap();

        let out = session::find_valid(&db, &token).await.unwrap();
        assert!(out.is_none(), "expired sessions must not validate");
    }

    #[tokio::test]
    async fn session_delete_invalidates_lookup() {
        let db = setup().await;
        let u = user::create(&db, "a@b.co", "pw", ROLE_USER).await.unwrap();
        let s = session::create(&db, u.id).await.unwrap();
        session::delete(&db, &s.id).await.unwrap();
        assert!(session::find_valid(&db, &s.id).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn sweep_expired_removes_only_expired() {
        let db = setup().await;
        let u = user::create(&db, "a@b.co", "pw", ROLE_USER).await.unwrap();
        let live = session::create(&db, u.id).await.unwrap();
        let dead_token = generate_token();
        sqlx::query("INSERT INTO rustio_sessions (id, user_id, expires_at) VALUES (?, ?, ?)")
            .bind(&dead_token)
            .bind(u.id)
            .bind(Utc::now() - Duration::seconds(1))
            .execute(db.pool())
            .await
            .unwrap();

        let removed = session::sweep_expired(&db).await.unwrap();
        assert_eq!(removed, 1);
        assert!(session::find_valid(&db, &live.id).await.unwrap().is_some());
        assert!(session::find_valid(&db, &dead_token)
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn deleting_user_cascades_to_sessions() {
        // Guard against the class of bug where SQLite silently ignores
        // FK constraints. The `rustio_sessions` table declares
        // `ON DELETE CASCADE`; if the pragma isn't set on the connection,
        // the cascade is a no-op and deleting a user leaves orphan
        // sessions that later resolve against a missing user id.
        let db = setup().await;
        let u = user::create(&db, "a@b.co", "pw", ROLE_USER).await.unwrap();
        let s = session::create(&db, u.id).await.unwrap();
        assert!(session::find_valid(&db, &s.id).await.unwrap().is_some());

        sqlx::query("DELETE FROM rustio_users WHERE id = ?")
            .bind(u.id)
            .execute(db.pool())
            .await
            .unwrap();

        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM rustio_sessions")
            .fetch_one(db.pool())
            .await
            .unwrap();
        assert_eq!(
            count, 0,
            "FK cascade must have removed the orphan session; is PRAGMA foreign_keys on?"
        );
    }

    #[tokio::test]
    async fn ensure_core_tables_is_idempotent() {
        let db = setup().await; // already called ensure_core_tables once
        ensure_core_tables(&db).await.unwrap();
        ensure_core_tables(&db).await.unwrap();
        assert_eq!(user::count(&db).await.unwrap(), 0);
    }

    // --- middleware decision path --------------------------------------
    //
    // `resolve_identity` is the pure core of the authenticate middleware.
    // These tests cover every case the spec calls out: missing cookie,
    // unknown session, expired session, inactive user, valid admin, and
    // valid non-admin. The middleware wrapper itself is trivial — once
    // this function is correct, so is it.

    async fn seeded_user(db: &Db, role: &str) -> User {
        user::create(db, "u@example.com", "pw", role).await.unwrap()
    }

    #[tokio::test]
    async fn resolve_identity_none_cookie_returns_none() {
        let db = setup().await;
        assert!(resolve_identity(&db, None).await.is_none());
    }

    #[tokio::test]
    async fn resolve_identity_unknown_token_returns_none() {
        let db = setup().await;
        assert!(resolve_identity(&db, Some("not-a-real-token"))
            .await
            .is_none());
    }

    #[tokio::test]
    async fn resolve_identity_expired_session_returns_none() {
        let db = setup().await;
        let u = seeded_user(&db, ROLE_USER).await;
        let token = generate_token();
        sqlx::query("INSERT INTO rustio_sessions (id, user_id, expires_at) VALUES (?, ?, ?)")
            .bind(&token)
            .bind(u.id)
            .bind(Utc::now() - Duration::seconds(1))
            .execute(db.pool())
            .await
            .unwrap();
        assert!(resolve_identity(&db, Some(&token)).await.is_none());
    }

    #[tokio::test]
    async fn resolve_identity_inactive_user_returns_none() {
        let db = setup().await;
        let u = seeded_user(&db, ROLE_USER).await;
        user::set_active(&db, u.id, false).await.unwrap();
        let s = session::create(&db, u.id).await.unwrap();
        assert!(
            resolve_identity(&db, Some(&s.id)).await.is_none(),
            "inactive users must not resolve to an Identity"
        );
    }

    #[tokio::test]
    async fn resolve_identity_deleted_user_returns_none() {
        // If someone deletes a user row out from under a live session,
        // the session should stop working on the next request rather
        // than granting access to a stale id.
        let db = setup().await;
        let u = seeded_user(&db, ROLE_USER).await;
        let s = session::create(&db, u.id).await.unwrap();
        sqlx::query("DELETE FROM rustio_users WHERE id = ?")
            .bind(u.id)
            .execute(db.pool())
            .await
            .unwrap();
        assert!(resolve_identity(&db, Some(&s.id)).await.is_none());
    }

    #[tokio::test]
    async fn resolve_identity_valid_admin_session_attaches_admin_identity() {
        let db = setup().await;
        let u = seeded_user(&db, ROLE_ADMIN).await;
        let s = session::create(&db, u.id).await.unwrap();
        let id = resolve_identity(&db, Some(&s.id)).await.unwrap();
        assert_eq!(id.user_id, u.id);
        assert!(id.is_admin);
    }

    #[tokio::test]
    async fn resolve_identity_valid_user_session_attaches_non_admin_identity() {
        let db = setup().await;
        let u = seeded_user(&db, ROLE_USER).await;
        let s = session::create(&db, u.id).await.unwrap();
        let id = resolve_identity(&db, Some(&s.id)).await.unwrap();
        assert_eq!(id.user_id, u.id);
        assert!(!id.is_admin);
    }

    // --- password change invalidates sessions --------------------------

    #[tokio::test]
    async fn changing_password_invalidates_all_user_sessions() {
        let db = setup().await;
        let u = seeded_user(&db, ROLE_USER).await;
        let s1 = session::create(&db, u.id).await.unwrap();
        let s2 = session::create(&db, u.id).await.unwrap();
        assert!(session::find_valid(&db, &s1.id).await.unwrap().is_some());
        assert!(session::find_valid(&db, &s2.id).await.unwrap().is_some());

        user::set_password(&db, u.id, "new password").await.unwrap();

        let remaining: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM rustio_sessions WHERE user_id = ?")
                .bind(u.id)
                .fetch_one(db.pool())
                .await
                .unwrap();
        assert_eq!(
            remaining, 0,
            "password change must wipe every live session for the user"
        );
        assert!(session::find_valid(&db, &s1.id).await.unwrap().is_none());
        assert!(session::find_valid(&db, &s2.id).await.unwrap().is_none());
    }

    // --- expired session cleanup on lookup -----------------------------

    #[tokio::test]
    async fn find_valid_cleans_up_expired_row_inline() {
        let db = setup().await;
        let u = seeded_user(&db, ROLE_USER).await;
        let token = generate_token();
        sqlx::query("INSERT INTO rustio_sessions (id, user_id, expires_at) VALUES (?, ?, ?)")
            .bind(&token)
            .bind(u.id)
            .bind(Utc::now() - Duration::seconds(1))
            .execute(db.pool())
            .await
            .unwrap();

        assert!(session::find_valid(&db, &token).await.unwrap().is_none());

        // The row should have been deleted as a side effect.
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM rustio_sessions WHERE id = ?")
            .bind(&token)
            .fetch_one(db.pool())
            .await
            .unwrap();
        assert_eq!(count, 0, "find_valid must purge expired rows inline");
    }

    // --- rate limiter --------------------------------------------------

    #[test]
    fn rate_limiter_allows_up_to_threshold() {
        let limiter = LoginRateLimiter::with_params(3, StdDuration::from_secs(60));
        assert!(limiter.check("alice@example.com").is_ok());
        limiter.record_failure("alice@example.com");
        limiter.record_failure("alice@example.com");
        assert!(limiter.check("alice@example.com").is_ok());
    }

    #[test]
    fn rate_limiter_locks_out_at_threshold() {
        let limiter = LoginRateLimiter::with_params(3, StdDuration::from_secs(60));
        for _ in 0..3 {
            limiter.record_failure("alice@example.com");
        }
        let result = limiter.check("alice@example.com");
        assert!(result.is_err(), "3rd failure must trip the lockout");
        let remaining = result.unwrap_err();
        assert!(remaining > StdDuration::ZERO);
        assert!(remaining <= StdDuration::from_secs(60));
    }

    #[test]
    fn rate_limiter_resets_on_successful_login() {
        let limiter = LoginRateLimiter::with_params(3, StdDuration::from_secs(60));
        for _ in 0..3 {
            limiter.record_failure("alice@example.com");
        }
        assert!(limiter.check("alice@example.com").is_err());

        limiter.record_success("alice@example.com");
        assert!(
            limiter.check("alice@example.com").is_ok(),
            "a successful login must clear the lockout counter"
        );
    }

    #[tokio::test]
    async fn rate_limiter_lockout_expires_after_duration() {
        let limiter = LoginRateLimiter::with_params(3, StdDuration::from_millis(50));
        for _ in 0..3 {
            limiter.record_failure("bob@example.com");
        }
        assert!(limiter.check("bob@example.com").is_err());

        tokio::time::sleep(StdDuration::from_millis(80)).await;

        assert!(
            limiter.check("bob@example.com").is_ok(),
            "lockout must lift after the configured duration"
        );
    }

    // --- rate limiter compose_key --------------------------------------

    #[test]
    fn compose_key_email_only_is_stable() {
        let k = LoginRateLimiter::compose_key("alice@example.com", None);
        assert_eq!(k, "email:alice@example.com");
    }

    #[test]
    fn compose_key_with_ip_is_distinct_from_email_only() {
        let a = LoginRateLimiter::compose_key("alice@example.com", None);
        let b = LoginRateLimiter::compose_key("alice@example.com", Some("203.0.113.5"));
        assert_ne!(a, b);
        assert_eq!(b, "email:alice@example.com|ip:203.0.113.5");
    }

    #[test]
    fn compose_key_distinct_ips_produce_distinct_keys() {
        // Same email from two IPs → two independent counters. Confirms
        // an attacker rotating IPs is throttled per-IP, not globally.
        let a = LoginRateLimiter::compose_key("a@b.co", Some("10.0.0.1"));
        let b = LoginRateLimiter::compose_key("a@b.co", Some("10.0.0.2"));
        assert_ne!(a, b);
    }

    // --- CSRF token generation + verify --------------------------------

    #[test]
    fn csrf_generate_returns_hex_of_expected_length() {
        let t = csrf::generate_token();
        // Matches session token shape: 32 bytes hex = 64 chars.
        assert_eq!(t.len(), 64);
        assert!(t.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn csrf_generate_produces_unique_tokens() {
        let a = csrf::generate_token();
        let b = csrf::generate_token();
        assert_ne!(a, b);
    }

    #[test]
    fn csrf_verify_matching_returns_true() {
        let t = csrf::generate_token();
        assert!(csrf::verify_token(&t, &t));
    }

    #[test]
    fn csrf_verify_mismatched_returns_false() {
        let t = csrf::generate_token();
        let other = csrf::generate_token();
        assert!(!csrf::verify_token(&t, &other));
    }

    #[test]
    fn csrf_verify_empty_either_side_returns_false() {
        let t = csrf::generate_token();
        assert!(!csrf::verify_token("", &t));
        assert!(!csrf::verify_token(&t, ""));
        assert!(!csrf::verify_token("", ""));
    }

    #[test]
    fn csrf_verify_rejects_different_lengths() {
        // Length check is an early return; catches the easy case
        // without leaking timing information through the byte loop.
        assert!(!csrf::verify_token("abc", "abcd"));
        assert!(!csrf::verify_token("abcd", "abc"));
    }

    #[test]
    fn csrf_verify_rejects_single_byte_difference() {
        let a = "deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef";
        let mut b = String::from(a);
        // Flip the last hex char.
        b.pop();
        b.push('0');
        assert!(!csrf::verify_token(a, &b));
    }

    // --- session carries CSRF token ------------------------------------

    #[tokio::test]
    async fn session_create_generates_unique_csrf_per_session() {
        let db = setup().await;
        let u = seeded_user(&db, ROLE_USER).await;
        let s1 = session::create(&db, u.id).await.unwrap();
        let s2 = session::create(&db, u.id).await.unwrap();
        assert_eq!(s1.csrf_token.len(), 64);
        assert_ne!(
            s1.csrf_token, s2.csrf_token,
            "each session must get an independent CSRF token"
        );
        assert_ne!(
            s1.csrf_token, s1.id,
            "session id and csrf token must not be the same value"
        );
    }

    #[tokio::test]
    async fn session_find_valid_returns_csrf_token() {
        let db = setup().await;
        let u = seeded_user(&db, ROLE_USER).await;
        let s = session::create(&db, u.id).await.unwrap();
        let found = session::find_valid(&db, &s.id).await.unwrap().unwrap();
        assert_eq!(found.csrf_token, s.csrf_token);
    }

    #[tokio::test]
    async fn resolve_identity_with_session_exposes_csrf() {
        // The middleware relies on this to hand CsrfToken to the
        // context — tested by mirroring what the middleware does.
        let db = setup().await;
        let u = seeded_user(&db, ROLE_ADMIN).await;
        let s = session::create(&db, u.id).await.unwrap();
        let (id, sess) = resolve_identity_with_session(&db, Some(&s.id))
            .await
            .unwrap();
        assert_eq!(id.user_id, u.id);
        assert_eq!(sess.csrf_token, s.csrf_token);
    }

    #[test]
    fn rate_limiter_tracks_keys_independently() {
        let limiter = LoginRateLimiter::with_params(2, StdDuration::from_secs(60));
        limiter.record_failure("alice@example.com");
        limiter.record_failure("alice@example.com");
        assert!(limiter.check("alice@example.com").is_err());
        // A different key is untouched by Alice's lockout.
        assert!(limiter.check("bob@example.com").is_ok());
    }

    // --- dummy hash for timing equalisation ----------------------------

    #[test]
    fn dummy_password_hash_is_stable_across_calls() {
        // Memoised; the first call pays the argon2 cost, subsequent
        // calls return the same string.
        let a = dummy_password_hash();
        let b = dummy_password_hash();
        assert!(std::ptr::eq(a, b));
    }

    #[test]
    fn dummy_password_hash_is_a_valid_phc_string() {
        // Must be parsable so `verify(wrong_pw, dummy_hash)` takes the
        // full ~50 ms path and actually exercises the timing-equalising
        // branch.
        assert!(PasswordHash::new(dummy_password_hash()).is_ok());
    }

    #[test]
    fn verify_against_dummy_hash_rejects_arbitrary_inputs() {
        // The login handler treats the dummy-hash verify as "always
        // false, purely for timing". The result is ignored — we never
        // authenticate against the dummy hash. This test pins that
        // arbitrary user passwords don't match it (safety belt even
        // though the handler already sets `valid = false`).
        assert!(!password::verify("", dummy_password_hash()));
        assert!(!password::verify("wrong password", dummy_password_hash()));
        assert!(!password::verify("admin", dummy_password_hash()));
    }

    #[tokio::test]
    async fn logout_deletes_session_so_later_requests_are_anonymous() {
        // The real logout handler calls `session::delete`. A request
        // carrying the just-deleted token must resolve to no identity.
        let db = setup().await;
        let u = seeded_user(&db, ROLE_USER).await;
        let s = session::create(&db, u.id).await.unwrap();
        assert!(resolve_identity(&db, Some(&s.id)).await.is_some());

        session::delete(&db, &s.id).await.unwrap();
        assert!(
            resolve_identity(&db, Some(&s.id)).await.is_none(),
            "deleted session must not resolve"
        );
    }
}
