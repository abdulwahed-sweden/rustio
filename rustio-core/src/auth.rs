//! Identity-in-context authentication.
//!
//! [`authenticate`] is an additive middleware: it attaches an [`Identity`]
//! to the request context when a valid `Authorization: Bearer` token is
//! provided, and does nothing otherwise. Handlers enforce their own
//! requirement with [`require_auth`] / [`require_admin`].
//!
//! The built-in token mapping is for development only — replace it with
//! your own middleware before deploying.

use crate::context::Context;
use crate::error::Error;
use crate::http::{Request, Response};
use crate::middleware::Next;

#[derive(Debug, Clone)]
pub struct Identity {
    pub user_id: String,
    pub is_admin: bool,
}

pub async fn authenticate(mut req: Request, next: Next) -> Result<Response, Error> {
    if let Some(token) = bearer_token(&req) {
        if let Some(identity) = dev_identity(token) {
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

fn dev_identity(token: &str) -> Option<Identity> {
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
}
