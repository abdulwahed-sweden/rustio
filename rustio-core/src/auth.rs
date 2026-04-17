//! Identity-in-context authentication.
//!
//! [`authenticate`] is an additive middleware: it attaches an [`Identity`]
//! to the request context when a valid `Authorization: Bearer` token is
//! provided, and does nothing otherwise. Handlers enforce their own
//! requirement with [`require_auth`] / [`require_admin`].
//!
//! The built-in token mapping (`dev-admin` / `dev-user`) is for development
//! only. As a safety guard, `authenticate` refuses to recognize any dev
//! token when the `RUSTIO_ENV` environment variable is set to `"production"`
//! (or `"prod"`). In that mode the middleware is a no-op and admin routes
//! will return 401 — the correct fix is to register your own auth
//! middleware that populates [`Identity`].

use std::sync::atomic::{AtomicBool, Ordering};

use crate::context::Context;
use crate::error::Error;
use crate::http::{Request, Response};
use crate::middleware::Next;

#[derive(Debug, Clone)]
pub struct Identity {
    pub user_id: String,
    pub is_admin: bool,
}

/// One-shot latch so we only print the production warning once per process,
/// no matter how many requests come in.
static PRODUCTION_WARNED: AtomicBool = AtomicBool::new(false);

/// `true` when `RUSTIO_ENV` indicates a production deployment.
///
/// Accepts `production` or `prod` (case-insensitive). Anything else —
/// including unset — is treated as development.
pub fn in_production() -> bool {
    std::env::var("RUSTIO_ENV")
        .map(|v| {
            let v = v.to_ascii_lowercase();
            v == "production" || v == "prod"
        })
        .unwrap_or(false)
}

pub async fn authenticate(mut req: Request, next: Next) -> Result<Response, Error> {
    if in_production() {
        // Emit a single loud warning the first time this runs in
        // production. The user almost certainly meant to register a real
        // auth middleware and forgot.
        if !PRODUCTION_WARNED.swap(true, Ordering::Relaxed) {
            eprintln!(
                "rustio_core::auth: RUSTIO_ENV={} — built-in dev tokens are disabled. \
                 Replace `authenticate` with your own middleware before accepting traffic.",
                std::env::var("RUSTIO_ENV").unwrap_or_default()
            );
        }
        // Skip dev-token handling entirely. An admin route will now
        // return 401 to any caller, rather than accepting `dev-admin`.
        return next.run(req).await;
    }

    // Accept the token from two places in priority order:
    //   1. `Authorization: Bearer <token>` — used by API/curl callers.
    //   2. `rustio_token` cookie — set by the admin login form so browser
    //      users don't have to inject headers manually.
    let token = bearer_token(&req)
        .map(str::to_owned)
        .or_else(|| req.cookie("rustio_token"));

    if let Some(t) = token {
        if let Some(identity) = dev_identity(&t) {
            req.ctx_mut().insert(identity);
        }
    }
    next.run(req).await
}

pub fn bearer_token(req: &Request) -> Option<&str> {
    req.headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
}

pub(crate) fn dev_identity(token: &str) -> Option<Identity> {
    match token {
        "dev-admin" => Some(Identity {
            user_id: String::from("admin"),
            is_admin: true,
        }),
        "dev-user" => Some(Identity {
            user_id: String::from("user"),
            is_admin: false,
        }),
        _ => None,
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn user(is_admin: bool) -> Identity {
        Identity {
            user_id: String::from(if is_admin { "admin" } else { "user" }),
            is_admin,
        }
    }

    #[test]
    fn identity_returns_none_when_absent() {
        let ctx = Context::new();
        assert!(identity(&ctx).is_none());
    }

    #[test]
    fn identity_returns_reference_when_attached() {
        let mut ctx = Context::new();
        ctx.insert(user(false));
        assert_eq!(identity(&ctx).map(|i| i.user_id.as_str()), Some("user"));
    }

    #[test]
    fn require_auth_missing_returns_unauthorized() {
        let ctx = Context::new();
        assert!(matches!(require_auth(&ctx), Err(Error::Unauthorized)));
    }

    #[test]
    fn require_auth_present_returns_identity() {
        let mut ctx = Context::new();
        ctx.insert(user(false));
        let id = require_auth(&ctx).unwrap();
        assert_eq!(id.user_id, "user");
        assert!(!id.is_admin);
    }

    #[test]
    fn require_admin_without_identity_returns_unauthorized() {
        let ctx = Context::new();
        assert!(matches!(require_admin(&ctx), Err(Error::Unauthorized)));
    }

    #[test]
    fn require_admin_with_non_admin_returns_forbidden() {
        let mut ctx = Context::new();
        ctx.insert(user(false));
        assert!(matches!(require_admin(&ctx), Err(Error::Forbidden)));
    }

    #[test]
    fn require_admin_with_admin_returns_identity() {
        let mut ctx = Context::new();
        ctx.insert(user(true));
        let id = require_admin(&ctx).unwrap();
        assert_eq!(id.user_id, "admin");
        assert!(id.is_admin);
    }

    #[test]
    fn dev_identity_rejects_unknown_tokens() {
        assert!(dev_identity("garbage").is_none());
        assert!(dev_identity("").is_none());
    }

    #[test]
    fn dev_identity_maps_known_tokens() {
        let admin = dev_identity("dev-admin").unwrap();
        assert!(admin.is_admin);
        let user = dev_identity("dev-user").unwrap();
        assert!(!user.is_admin);
    }

    #[test]
    fn in_production_detects_known_values() {
        // We don't touch env in tests — inspect the parser via an inline
        // helper that mirrors the real function but takes a value directly.
        fn detect(v: Option<&str>) -> bool {
            v.map(|s| {
                let s = s.to_ascii_lowercase();
                s == "production" || s == "prod"
            })
            .unwrap_or(false)
        }
        assert!(detect(Some("production")));
        assert!(detect(Some("PRODUCTION")));
        assert!(detect(Some("prod")));
        assert!(detect(Some("Prod")));
        assert!(!detect(Some("dev")));
        assert!(!detect(Some("staging")));
        assert!(!detect(Some("")));
        assert!(!detect(None));
    }
}
