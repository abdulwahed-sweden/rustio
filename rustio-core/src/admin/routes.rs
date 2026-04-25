//! Admin route registration with permission checks.
//!
//! Every admin URL is gated by a specific permission:
//!   GET  /admin/:model            → posts.view_post
//!   GET  /admin/:model/new        → posts.add_post
//!   POST /admin/:model/new        → posts.add_post
//!   GET  /admin/:model/:id/edit   → posts.change_post
//!   POST /admin/:model/:id/edit   → posts.change_post
//!   GET  /admin/:model/:id/delete → posts.delete_post
//!   POST /admin/:model/:id/delete → posts.delete_post
//!
//! Admin-role users bypass every check (see `Role::is_superuser`).
//! Staff-role users need the specific permission granted either
//! directly or via a group.

use std::sync::Arc;

use crate::auth::{self, Identity};
use crate::error::{Error, Result};
use crate::http::{Request, Response};
use crate::orm::Db;
use crate::router::Router;
use crate::templates::Templates;

use super::handlers::{self, AdminCtx};
use super::types::Admin;

/// Either an identity + a permission check passed, a redirect to
/// /admin/login, or a hard error (403).
enum Guard {
    Allow(Identity),
    Redirect(Response),
}

async fn login_guard(ctx: &AdminCtx, req: &Request) -> Result<Guard> {
    let cookie = match req.header("cookie") {
        Some(c) => c,
        None => return Ok(Guard::Redirect(Response::redirect("/admin/login"))),
    };
    let token = match auth::session_token_from_cookie(cookie) {
        Some(t) => t,
        None => return Ok(Guard::Redirect(Response::redirect("/admin/login"))),
    };
    let ident = match auth::identity_from_session(&ctx.db, &token).await? {
        Some(i) => i,
        None => return Ok(Guard::Redirect(Response::redirect("/admin/login"))),
    };
    if !ident.can_access_admin() {
        return Err(Error::Forbidden("admin access required".into()));
    }
    Ok(Guard::Allow(ident))
}

/// Like `login_guard` but also checks a specific permission. Admins
/// skip the check.
async fn permission_guard(ctx: &AdminCtx, req: &Request, permission: &str) -> Result<Guard> {
    match login_guard(ctx, req).await? {
        Guard::Redirect(r) => Ok(Guard::Redirect(r)),
        Guard::Allow(ident) => {
            if !auth::check_permission(&ctx.db, &ident, permission).await? {
                return Err(Error::Forbidden(format!(
                    "missing permission: {permission}"
                )));
            }
            Ok(Guard::Allow(ident))
        }
    }
}

/// Only admins may pass. Used for user/group management.
async fn admin_only_guard(ctx: &AdminCtx, req: &Request) -> Result<Guard> {
    match login_guard(ctx, req).await? {
        Guard::Redirect(r) => Ok(Guard::Redirect(r)),
        Guard::Allow(ident) => {
            if !ident.is_admin() {
                return Err(Error::Forbidden("admin role required".into()));
            }
            Ok(Guard::Allow(ident))
        }
    }
}

fn parse_id(raw: Option<&str>) -> Result<i64> {
    raw.and_then(|s| s.parse().ok())
        .ok_or_else(|| Error::BadRequest("invalid id".into()))
}

fn model_name_from_req(req: &Request) -> Result<String> {
    req.param("admin_name")
        .map(|s| s.to_string())
        .ok_or_else(|| Error::BadRequest("missing model".into()))
}

fn perm_for(ctx: &AdminCtx, admin_name: &str, action: &str) -> Result<String> {
    let entry = ctx
        .admin
        .find(admin_name)
        .ok_or_else(|| Error::NotFound(format!("no admin model: {admin_name}")))?;
    let singular = entry.singular_name.to_ascii_lowercase();
    Ok(format!("{admin_name}.{action}_{singular}"))
}

pub fn register_admin_routes(
    router: Router,
    admin: Admin,
    db: Db,
    templates: Arc<Templates>,
) -> Router {
    let ctx = Arc::new(AdminCtx::new(Arc::new(admin), db.clone(), templates.clone()));

    // Wire the builtin pages (users/groups) using their own ctx type.
    let auth_ctx = Arc::new(super::builtin::AuthAdminCtx {
        admin: ctx.admin.clone(),
        db,
        templates,
    });

    // Embedded stylesheet used by the admin templates. Routed here so
    // every app that calls `register_admin_routes` gets a styled UI
    // without having to plumb its own static handler.
    let router = router.get("/static/rustio.css", |_req| async move {
        Ok(Response::new(
            hyper::StatusCode::OK,
            bytes::Bytes::from_static(crate::server::embedded_rustio_css().as_bytes()),
        )
        .with_header("content-type", "text/css; charset=utf-8")
        .with_header("cache-control", "public, max-age=3600"))
    });

    // Phase 6a admin stylesheet — Django classic layout, RustIO brand.
    // Served alongside /static/rustio.css; the new admin pages link to
    // this one, the legacy templates keep using rustio.css.
    let router = router.get("/static/admin.css", |_req| async move {
        Ok(Response::new(
            hyper::StatusCode::OK,
            bytes::Bytes::from_static(crate::server::embedded_admin_css().as_bytes()),
        )
        .with_header("content-type", "text/css; charset=utf-8")
        .with_header("cache-control", "public, max-age=3600"))
    });

    // Client for the embedded search page.
    let router = router.get("/static/search.js", |_req| async move {
        Ok(Response::new(
            hyper::StatusCode::OK,
            bytes::Bytes::from_static(crate::server::embedded_search_js().as_bytes()),
        )
        .with_header("content-type", "application/javascript; charset=utf-8")
        .with_header("cache-control", "public, max-age=3600"))
    });

    // Public: login/logout.
    let c = ctx.clone();
    let router = router.get("/admin/login", move |req| {
        let c = c.clone();
        async move { handlers::show_login(&c, req).await }
    });

    let c = ctx.clone();
    let router = router.post("/admin/login", move |req| {
        let c = c.clone();
        async move { handlers::do_login(&c, req).await }
    });

    let c = ctx.clone();
    let router = router.post("/admin/logout", move |req| {
        let c = c.clone();
        async move { handlers::do_logout(&c, req).await }
    });

    // Dashboard — any logged-in staff/admin.
    let c = ctx.clone();
    let router = router.get("/admin", move |req| {
        let c = c.clone();
        async move {
            match login_guard(&c, &req).await? {
                Guard::Redirect(r) => Ok(r),
                Guard::Allow(ident) => handlers::dashboard(&c, ident, &req).await,
            }
        }
    });

    // Phase 6b/2 — global history log (admin-only; high-signal page).
    let c = ctx.clone();
    let router = router.get("/admin/history", move |req| {
        let c = c.clone();
        async move {
            match admin_only_guard(&c, &req).await? {
                Guard::Redirect(r) => Ok(r),
                Guard::Allow(ident) => handlers::show_log_entries(&c, ident, &req).await,
            }
        }
    });

    // Phase 6b/5 — self-service password change. Any logged-in user
    // (including non-admin staff) can change their own.
    let c = ctx.clone();
    let router = router.get("/admin/password_change", move |req| {
        let c = c.clone();
        async move {
            match login_guard(&c, &req).await? {
                Guard::Redirect(r) => Ok(r),
                Guard::Allow(ident) => handlers::show_password_change(&c, ident, &req).await,
            }
        }
    });
    let c = ctx.clone();
    let router = router.post("/admin/password_change", move |req| {
        let c = c.clone();
        async move {
            match login_guard(&c, &req).await? {
                Guard::Redirect(r) => Ok(r),
                Guard::Allow(ident) => handlers::do_password_change(&c, ident, req).await,
            }
        }
    });

    // --- Built-in users admin (admin-only) ---
    let c = ctx.clone();
    let ac = auth_ctx.clone();
    let router = router.get("/admin/users", move |req| {
        let c = c.clone();
        let ac = ac.clone();
        async move {
            match admin_only_guard(&c, &req).await? {
                Guard::Redirect(r) => Ok(r),
                Guard::Allow(ident) => super::builtin::list_users(&ac, ident, handlers::csrf_token(&req)).await,
            }
        }
    });

    let c = ctx.clone();
    let ac = auth_ctx.clone();
    let router = router.get("/admin/users/new", move |req| {
        let c = c.clone();
        let ac = ac.clone();
        async move {
            match admin_only_guard(&c, &req).await? {
                Guard::Redirect(r) => Ok(r),
                Guard::Allow(ident) => super::builtin::show_new_user(&ac, ident, handlers::csrf_token(&req)).await,
            }
        }
    });

    let c = ctx.clone();
    let ac = auth_ctx.clone();
    let router = router.post("/admin/users/new", move |req| {
        let c = c.clone();
        let ac = ac.clone();
        async move {
            match admin_only_guard(&c, &req).await? {
                Guard::Redirect(r) => Ok(r),
                Guard::Allow(ident) => super::builtin::do_new_user(&ac, ident, req).await,
            }
        }
    });

    let c = ctx.clone();
    let ac = auth_ctx.clone();
    let router = router.get("/admin/users/:id/edit", move |req| {
        let c = c.clone();
        let ac = ac.clone();
        async move {
            match admin_only_guard(&c, &req).await? {
                Guard::Redirect(r) => Ok(r),
                Guard::Allow(ident) => {
                    let id = parse_id(req.param("id"))?;
                    super::builtin::show_user_edit(&ac, ident, id, handlers::csrf_token(&req)).await
                }
            }
        }
    });

    let c = ctx.clone();
    let ac = auth_ctx.clone();
    let router = router.post("/admin/users/:id/edit", move |req| {
        let c = c.clone();
        let ac = ac.clone();
        async move {
            match admin_only_guard(&c, &req).await? {
                Guard::Redirect(r) => Ok(r),
                Guard::Allow(ident) => {
                    let id = parse_id(req.param("id"))?;
                    super::builtin::do_user_edit(&ac, ident, id, req).await
                }
            }
        }
    });

    // --- Built-in groups admin (admin-only) ---
    let c = ctx.clone();
    let ac = auth_ctx.clone();
    let router = router.get("/admin/groups", move |req| {
        let c = c.clone();
        let ac = ac.clone();
        async move {
            match admin_only_guard(&c, &req).await? {
                Guard::Redirect(r) => Ok(r),
                Guard::Allow(ident) => super::builtin::list_groups(&ac, ident, handlers::csrf_token(&req)).await,
            }
        }
    });

    let c = ctx.clone();
    let ac = auth_ctx.clone();
    let router = router.get("/admin/groups/new", move |req| {
        let c = c.clone();
        let ac = ac.clone();
        async move {
            match admin_only_guard(&c, &req).await? {
                Guard::Redirect(r) => Ok(r),
                Guard::Allow(ident) => super::builtin::show_new_group(&ac, ident, handlers::csrf_token(&req)).await,
            }
        }
    });

    let c = ctx.clone();
    let ac = auth_ctx.clone();
    let router = router.post("/admin/groups/new", move |req| {
        let c = c.clone();
        let ac = ac.clone();
        async move {
            match admin_only_guard(&c, &req).await? {
                Guard::Redirect(r) => Ok(r),
                Guard::Allow(ident) => super::builtin::do_new_group(&ac, ident, req).await,
            }
        }
    });

    let c = ctx.clone();
    let ac = auth_ctx.clone();
    let router = router.get("/admin/groups/:id/edit", move |req| {
        let c = c.clone();
        let ac = ac.clone();
        async move {
            match admin_only_guard(&c, &req).await? {
                Guard::Redirect(r) => Ok(r),
                Guard::Allow(ident) => {
                    let id = parse_id(req.param("id"))?;
                    super::builtin::show_group_edit(&ac, ident, id, handlers::csrf_token(&req)).await
                }
            }
        }
    });

    let c = ctx.clone();
    let ac = auth_ctx.clone();
    let router = router.post("/admin/groups/:id/edit", move |req| {
        let c = c.clone();
        let ac = ac.clone();
        async move {
            match admin_only_guard(&c, &req).await? {
                Guard::Redirect(r) => Ok(r),
                Guard::Allow(ident) => {
                    let id = parse_id(req.param("id"))?;
                    super::builtin::do_group_edit(&ac, ident, id, req).await
                }
            }
        }
    });

    // Per-model list — needs `view` permission.
    let c = ctx.clone();
    let router = router.get("/admin/:admin_name", move |req| {
        let c = c.clone();
        async move {
            let name = model_name_from_req(&req)?;
            let perm = perm_for(&c, &name, "view")?;
            match permission_guard(&c, &req, &perm).await? {
                Guard::Redirect(r) => Ok(r),
                Guard::Allow(ident) => handlers::list_model(&c, ident, &name, &req).await,
            }
        }
    });

    // Create.
    let c = ctx.clone();
    let router = router.get("/admin/:admin_name/new", move |req| {
        let c = c.clone();
        async move {
            let name = model_name_from_req(&req)?;
            let perm = perm_for(&c, &name, "add")?;
            match permission_guard(&c, &req, &perm).await? {
                Guard::Redirect(r) => Ok(r),
                Guard::Allow(ident) => handlers::show_new_form(&c, ident, &name, &req).await,
            }
        }
    });
    let c = ctx.clone();
    let router = router.post("/admin/:admin_name/new", move |req| {
        let c = c.clone();
        async move {
            let name = model_name_from_req(&req)?;
            let perm = perm_for(&c, &name, "add")?;
            match permission_guard(&c, &req, &perm).await? {
                Guard::Redirect(r) => Ok(r),
                Guard::Allow(ident) => handlers::do_create(&c, ident, &name, req).await,
            }
        }
    });

    // Edit.
    let c = ctx.clone();
    let router = router.get("/admin/:admin_name/:id/edit", move |req| {
        let c = c.clone();
        async move {
            let name = model_name_from_req(&req)?;
            let perm = perm_for(&c, &name, "change")?;
            match permission_guard(&c, &req, &perm).await? {
                Guard::Redirect(r) => Ok(r),
                Guard::Allow(ident) => {
                    let id = parse_id(req.param("id"))?;
                    handlers::show_edit_form(&c, ident, &name, id, &req).await
                }
            }
        }
    });
    let c = ctx.clone();
    let router = router.post("/admin/:admin_name/:id/edit", move |req| {
        let c = c.clone();
        async move {
            let name = model_name_from_req(&req)?;
            let perm = perm_for(&c, &name, "change")?;
            match permission_guard(&c, &req, &perm).await? {
                Guard::Redirect(r) => Ok(r),
                Guard::Allow(ident) => {
                    let id = parse_id(req.param("id"))?;
                    handlers::do_update(&c, ident, &name, id, req).await
                }
            }
        }
    });

    // Phase 6b/2 — per-object history. Read-only; same `view` permission
    // as the changelist (if you can list, you can read the audit trail).
    let c = ctx.clone();
    let router = router.get("/admin/:admin_name/:id/history", move |req| {
        let c = c.clone();
        async move {
            let name = model_name_from_req(&req)?;
            let perm = perm_for(&c, &name, "view")?;
            match permission_guard(&c, &req, &perm).await? {
                Guard::Redirect(r) => Ok(r),
                Guard::Allow(ident) => {
                    let id = parse_id(req.param("id"))?;
                    handlers::show_object_history(&c, ident, &name, id, &req).await
                }
            }
        }
    });

    // Delete.
    let c = ctx.clone();
    let router = router.get("/admin/:admin_name/:id/delete", move |req| {
        let c = c.clone();
        async move {
            let name = model_name_from_req(&req)?;
            let perm = perm_for(&c, &name, "delete")?;
            match permission_guard(&c, &req, &perm).await? {
                Guard::Redirect(r) => Ok(r),
                Guard::Allow(ident) => {
                    let id = parse_id(req.param("id"))?;
                    handlers::show_delete_confirm(&c, ident, &name, id, &req).await
                }
            }
        }
    });
    let c = ctx.clone();
    router.post("/admin/:admin_name/:id/delete", move |req| {
        let c = c.clone();
        async move {
            let name = model_name_from_req(&req)?;
            let perm = perm_for(&c, &name, "delete")?;
            match permission_guard(&c, &req, &perm).await? {
                Guard::Redirect(r) => Ok(r),
                Guard::Allow(ident) => {
                    let id = parse_id(req.param("id"))?;
                    handlers::do_delete(&c, ident, &name, id).await
                }
            }
        }
    })
}
