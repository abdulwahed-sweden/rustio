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

use crate::auth::{self, Identity, Role};
use crate::error::{Error, Result};
use crate::http::{Request, Response};
use crate::orm::Db;
use crate::router::Router;
use crate::templates::Templates;

use super::handlers::{self, AdminCtx};
use super::render;
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
    // Deactivated user: bounce to login. Phase 7a/0.5/e moved the
    // role-floor decision out of `login_guard` and into `role_guard`
    // so the rendered `forbidden.html` page can show the required
    // role. `login_guard` now owns only "session-valid + active".
    if !ident.is_active {
        return Ok(Guard::Redirect(Response::redirect("/admin/login")));
    }
    Ok(Guard::Allow(ident))
}

/// Phase 7a/0.5/b — minimum-role guard.
///
/// Runs `login_guard` first (which already enforces `is_active` +
/// `can_access_panel` via `Identity::can_access_admin`), then checks
/// that the identity's role rank is at least `min`. On failure renders
/// the `admin/forbidden.html` page with a 403 status. The forbidden
/// response is carried via `Guard::Redirect(_)` — the variant name
/// is informational; functionally it carries any non-Allow response
/// the route closure should return as-is.
async fn role_guard(ctx: &AdminCtx, req: &Request, min: Role) -> Result<Guard> {
    match login_guard(ctx, req).await? {
        Guard::Redirect(r) => Ok(Guard::Redirect(r)),
        Guard::Allow(ident) => {
            if ident.role.includes(min) {
                Ok(Guard::Allow(ident))
            } else {
                let body = render::render_forbidden_body(
                    &ctx.admin,
                    &ctx.templates,
                    &ident,
                    handlers::csrf_token(req),
                    None,
                    Some(min.label()),
                )?;
                Ok(Guard::Redirect(
                    Response::html(body).with_status(hyper::StatusCode::FORBIDDEN),
                ))
            }
        }
    }
}

/// Phase 7a/0.5/b — per-model permission guard.
///
/// Floors at `Role::Staff` (the panel-access tier). Administrator and
/// Developer **bypass** the permission lookup via
/// `Role::bypasses_group_checks` (post-sec2 the `is_active` check runs
/// first inside `check_permission`, so an inactive Administrator/Dev
/// is denied). Other tiers consult the M2M permission tables.
async fn perm_guard(ctx: &AdminCtx, req: &Request, perm: &str) -> Result<Guard> {
    match role_guard(ctx, req, Role::Staff).await? {
        Guard::Redirect(r) => Ok(Guard::Redirect(r)),
        Guard::Allow(ident) => {
            if ident.role.bypasses_group_checks() {
                return Ok(Guard::Allow(ident));
            }
            if auth::check_permission(&ctx.db, &ident, perm).await? {
                Ok(Guard::Allow(ident))
            } else {
                let body = render::render_forbidden_body(
                    &ctx.admin,
                    &ctx.templates,
                    &ident,
                    handlers::csrf_token(req),
                    Some(perm.to_string()),
                    None,
                )?;
                Ok(Guard::Redirect(
                    Response::html(body).with_status(hyper::StatusCode::FORBIDDEN),
                ))
            }
        }
    }
}

/// Pure decision logic for `perm_guard`, factored out so it can be
/// unit-tested without a `Db`. The DB-touching halves (login_guard,
/// `check_permission`'s M2M lookup) are exercised by the PG-gated
/// integration tests from sec2/sec3.
///
/// Returns `true` if the identity should be granted access:
/// - Inactive identities are denied (defense-in-depth — login_guard
///   already blocks them at the panel boundary).
/// - Administrator + Developer bypass the per-model check.
/// - Everyone else needs `perm_held == true`.
#[cfg(test)]
fn perm_guard_verdict(ident: &Identity, perm_held: bool) -> bool {
    if !ident.is_active {
        return false;
    }
    if ident.role.bypasses_group_checks() {
        return true;
    }
    perm_held
}

// Phase 7a/0.5/e: `admin_only_guard` deleted. Every former call site
// migrated to `role_guard(Role::Administrator)`. The new path renders
// `admin/forbidden.html` instead of returning a text/plain 403.

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

    // Phase 11.B — render `Err(_)` from /admin/* handlers as styled HTML
    // instead of the framework default `text/plain`. Non-admin paths
    // bubble through unchanged so JSON / curl consumers still get the
    // text body. `Error::Forbidden` (handled by `role_guard` via
    // `admin/forbidden.html`) and login-required redirects (303 → /admin
    // /login) come through as `Ok` responses and bypass this branch.
    let err_admin = ctx.admin.clone();
    let err_templates = ctx.templates.clone();
    let router = router.middleware(move |req, next| {
        let admin = err_admin.clone();
        let templates = err_templates.clone();
        Box::pin(async move {
            // Capture the path *before* `next.run` consumes the request.
            let is_admin_path = req.path().starts_with("/admin");
            let result = next.run(req).await;
            match result {
                Ok(resp) => Ok(resp),
                Err(err) if is_admin_path => Ok(render::render_admin_error_response(
                    &admin,
                    &templates,
                    None,
                    err.status(),
                    err.client_message().to_string(),
                )),
                Err(err) => Err(err),
            }
        })
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

    // Phase 7a/2 — self-hosted Inter (Latin subset, four weights).
    // Registered as separate routes rather than a `:file` wildcard
    // so the binary doesn't risk leaking arbitrary files from the
    // assets dir; only the four explicitly-baked weights are served.
    let router = router.get("/static/fonts/Inter-Regular.woff2", |_req| async move {
        Ok(Response::new(
            hyper::StatusCode::OK,
            bytes::Bytes::from_static(crate::server::embedded_inter_regular()),
        )
        .with_header("content-type", "font/woff2")
        .with_header("cache-control", "public, max-age=31536000, immutable"))
    });
    let router = router.get("/static/fonts/Inter-Medium.woff2", |_req| async move {
        Ok(Response::new(
            hyper::StatusCode::OK,
            bytes::Bytes::from_static(crate::server::embedded_inter_medium()),
        )
        .with_header("content-type", "font/woff2")
        .with_header("cache-control", "public, max-age=31536000, immutable"))
    });
    let router = router.get("/static/fonts/Inter-SemiBold.woff2", |_req| async move {
        Ok(Response::new(
            hyper::StatusCode::OK,
            bytes::Bytes::from_static(crate::server::embedded_inter_semibold()),
        )
        .with_header("content-type", "font/woff2")
        .with_header("cache-control", "public, max-age=31536000, immutable"))
    });
    let router = router.get("/static/fonts/Inter-Bold.woff2", |_req| async move {
        Ok(Response::new(
            hyper::StatusCode::OK,
            bytes::Bytes::from_static(crate::server::embedded_inter_bold()),
        )
        .with_header("content-type", "font/woff2")
        .with_header("cache-control", "public, max-age=31536000, immutable"))
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

    // Dashboard — Staff floor. User-tier sees the forbidden page.
    let c = ctx.clone();
    let router = router.get("/admin", move |req| {
        let c = c.clone();
        async move {
            match role_guard(&c, &req, Role::Staff).await? {
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
            match role_guard(&c, &req, Role::Administrator).await? {
                Guard::Redirect(r) => Ok(r),
                Guard::Allow(ident) => handlers::show_log_entries(&c, ident, &req).await,
            }
        }
    });

    // Phase 6b/5 — self-service password change. Any logged-in user
    // (User-tier and above). User-tier can change their own password
    // even though they can't access the dashboard.
    let c = ctx.clone();
    let router = router.get("/admin/password_change", move |req| {
        let c = c.clone();
        async move {
            match role_guard(&c, &req, Role::User).await? {
                Guard::Redirect(r) => Ok(r),
                Guard::Allow(ident) => handlers::show_password_change(&c, ident, &req).await,
            }
        }
    });
    let c = ctx.clone();
    let router = router.post("/admin/password_change", move |req| {
        let c = c.clone();
        async move {
            match role_guard(&c, &req, Role::User).await? {
                Guard::Redirect(r) => Ok(r),
                Guard::Allow(ident) => handlers::do_password_change(&c, ident, req).await,
            }
        }
    });

    // Phase 7.3 — JSON search endpoint that powers the remote
    // typeahead on FK / M2M `<select>` fields. Staff-guarded (same
    // tier as the dashboard); the admin already exposes per-row
    // metadata through the list pages so this isn't a new
    // information surface, just a faster lookup. Registered with
    // `:model` as the final segment — distinct enough not to
    // collide with the project-level `/admin/:admin_name/...`
    // wildcards registered later in the file.
    let c = ctx.clone();
    let router = router.get("/admin/search/:model", move |req| {
        let c = c.clone();
        async move {
            match role_guard(&c, &req, Role::Staff).await? {
                Guard::Redirect(r) => Ok(r),
                Guard::Allow(ident) => {
                    let model = req.param("model").unwrap_or("").to_string();
                    handlers::show_search(&c, ident, &model, &req).await
                }
            }
        }
    });

    // Phase 7a/0.5/e — Developer-only stub routes. All three share
    // `admin/coming_soon.html`; Phase 8 replaces the bodies with real
    // implementations. Administrator does NOT get access — Developer
    // is a strictly higher tier in the 5-rank ladder.
    let c = ctx.clone();
    let router = router.get("/admin/__schema__", move |req| {
        let c = c.clone();
        async move {
            match role_guard(&c, &req, Role::Developer).await? {
                Guard::Redirect(r) => Ok(r),
                Guard::Allow(ident) => handlers::show_schema_browser(&c, ident, &req).await,
            }
        }
    });
    let c = ctx.clone();
    let router = router.get("/admin/__logs__", move |req| {
        let c = c.clone();
        async move {
            match role_guard(&c, &req, Role::Developer).await? {
                Guard::Redirect(r) => Ok(r),
                Guard::Allow(ident) => handlers::show_execution_logs(&c, ident, &req).await,
            }
        }
    });
    let c = ctx.clone();
    let router = router.get("/admin/__sql_console__", move |req| {
        let c = c.clone();
        async move {
            match role_guard(&c, &req, Role::Developer).await? {
                Guard::Redirect(r) => Ok(r),
                Guard::Allow(ident) => handlers::show_sql_console(&c, ident, &req).await,
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
            match role_guard(&c, &req, Role::Administrator).await? {
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
            match role_guard(&c, &req, Role::Administrator).await? {
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
            match role_guard(&c, &req, Role::Administrator).await? {
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
            match role_guard(&c, &req, Role::Administrator).await? {
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
            match role_guard(&c, &req, Role::Administrator).await? {
                Guard::Redirect(r) => Ok(r),
                Guard::Allow(ident) => {
                    let id = parse_id(req.param("id"))?;
                    super::builtin::do_user_edit(&ac, ident, id, req).await
                }
            }
        }
    });

    // Phase 7a/0.5/f — user delete.
    let c = ctx.clone();
    let ac = auth_ctx.clone();
    let router = router.get("/admin/users/:id/delete", move |req| {
        let c = c.clone();
        let ac = ac.clone();
        async move {
            match role_guard(&c, &req, Role::Administrator).await? {
                Guard::Redirect(r) => Ok(r),
                Guard::Allow(ident) => {
                    let id = parse_id(req.param("id"))?;
                    super::builtin::show_user_delete(&ac, ident, id, handlers::csrf_token(&req)).await
                }
            }
        }
    });

    let c = ctx.clone();
    let ac = auth_ctx.clone();
    let router = router.post("/admin/users/:id/delete", move |req| {
        let c = c.clone();
        let ac = ac.clone();
        async move {
            match role_guard(&c, &req, Role::Administrator).await? {
                Guard::Redirect(r) => Ok(r),
                Guard::Allow(ident) => {
                    let id = parse_id(req.param("id"))?;
                    super::builtin::do_user_delete(&ac, ident, id, req).await
                }
            }
        }
    });

    // Phase 7a/0.5/h — read-only user profile view. MUST be
    // registered AFTER `/admin/users/new` and the `:id/edit` +
    // `:id/delete` routes above: the router matches in insertion
    // order, and `:id` is a wildcard that would happily swallow
    // "new" or extra path segments. Putting this last preserves
    // the more-specific routes' priority.
    let c = ctx.clone();
    let ac = auth_ctx.clone();
    let router = router.get("/admin/users/:id", move |req| {
        let c = c.clone();
        let ac = ac.clone();
        async move {
            match role_guard(&c, &req, Role::Administrator).await? {
                Guard::Redirect(r) => Ok(r),
                Guard::Allow(ident) => {
                    let id = parse_id(req.param("id"))?;
                    // Phase 10/b — `?tab=overview|activity|permissions|sessions`
                    // selects the detail-pane content; `?page=N` paginates the
                    // Activity tab. Tab links strip page; pager links preserve
                    // both. Invalid values fall back silently to defaults.
                    let q = req.query();
                    let tab = q.get("tab").map(|s| s.to_string());
                    let page: i64 = q.get("page").and_then(|s| s.parse().ok()).unwrap_or(1);
                    super::builtin::show_user_view(
                        &ac,
                        ident,
                        id,
                        handlers::csrf_token(&req),
                        tab,
                        page,
                    )
                    .await
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
            match role_guard(&c, &req, Role::Administrator).await? {
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
            match role_guard(&c, &req, Role::Administrator).await? {
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
            match role_guard(&c, &req, Role::Administrator).await? {
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
            match role_guard(&c, &req, Role::Administrator).await? {
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
            match role_guard(&c, &req, Role::Administrator).await? {
                Guard::Redirect(r) => Ok(r),
                Guard::Allow(ident) => {
                    let id = parse_id(req.param("id"))?;
                    super::builtin::do_group_edit(&ac, ident, id, req).await
                }
            }
        }
    });

    // Phase 7a/0.5/sec1 — group delete.
    let c = ctx.clone();
    let ac = auth_ctx.clone();
    let router = router.get("/admin/groups/:id/delete", move |req| {
        let c = c.clone();
        let ac = ac.clone();
        async move {
            match role_guard(&c, &req, Role::Administrator).await? {
                Guard::Redirect(r) => Ok(r),
                Guard::Allow(ident) => {
                    let id = parse_id(req.param("id"))?;
                    super::builtin::show_group_delete(&ac, ident, id, handlers::csrf_token(&req)).await
                }
            }
        }
    });

    let c = ctx.clone();
    let ac = auth_ctx.clone();
    let router = router.post("/admin/groups/:id/delete", move |req| {
        let c = c.clone();
        let ac = ac.clone();
        async move {
            match role_guard(&c, &req, Role::Administrator).await? {
                Guard::Redirect(r) => Ok(r),
                Guard::Allow(ident) => {
                    let id = parse_id(req.param("id"))?;
                    super::builtin::do_group_delete(&ac, ident, id, req).await
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
            match perm_guard(&c, &req, &perm).await? {
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
            match perm_guard(&c, &req, &perm).await? {
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
            match perm_guard(&c, &req, &perm).await? {
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
            match perm_guard(&c, &req, &perm).await? {
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
            match perm_guard(&c, &req, &perm).await? {
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
            match perm_guard(&c, &req, &perm).await? {
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
            match perm_guard(&c, &req, &perm).await? {
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
            match perm_guard(&c, &req, &perm).await? {
                Guard::Redirect(r) => Ok(r),
                Guard::Allow(ident) => {
                    let id = parse_id(req.param("id"))?;
                    handlers::do_delete(&c, ident, &name, id).await
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_identity(role: Role, is_active: bool) -> Identity {
        Identity {
            user_id: 42,
            email: "test@example.com".into(),
            role,
            is_active,
            is_demo: false,
            demo_label: None,
        }
    }

    // ---- role_guard's decision is `Role::includes(min)` -----------------
    // The 25-case matrix lives in `auth::role::tests::includes_matrix_…`;
    // the cases below pin the four most operator-relevant pairings as
    // documentation that the guard reuses that ladder.

    #[test]
    fn role_guard_decision_admin_meets_staff_floor() {
        let id = make_identity(Role::Administrator, true);
        assert!(id.role.includes(Role::Staff));
    }

    #[test]
    fn role_guard_decision_user_does_not_meet_staff() {
        let id = make_identity(Role::User, true);
        assert!(!id.role.includes(Role::Staff));
    }

    #[test]
    fn role_guard_decision_administrator_does_not_meet_developer() {
        let id = make_identity(Role::Administrator, true);
        assert!(!id.role.includes(Role::Developer));
    }

    #[test]
    fn role_guard_decision_developer_meets_everything() {
        let id = make_identity(Role::Developer, true);
        for &min in &[
            Role::User,
            Role::Staff,
            Role::Supervisor,
            Role::Administrator,
            Role::Developer,
        ] {
            assert!(id.role.includes(min), "Developer should meet {min:?}");
        }
    }

    // ---- perm_guard_verdict matrix --------------------------------------

    #[test]
    fn perm_guard_admin_short_circuits_without_perm() {
        let id = make_identity(Role::Administrator, true);
        assert!(perm_guard_verdict(&id, false));
    }

    #[test]
    fn perm_guard_developer_short_circuits_without_perm() {
        let id = make_identity(Role::Developer, true);
        assert!(perm_guard_verdict(&id, false));
    }

    #[test]
    fn perm_guard_staff_with_perm_passes() {
        let id = make_identity(Role::Staff, true);
        assert!(perm_guard_verdict(&id, true));
    }

    #[test]
    fn perm_guard_staff_without_perm_denies() {
        let id = make_identity(Role::Staff, true);
        assert!(!perm_guard_verdict(&id, false));
    }

    #[test]
    fn perm_guard_inactive_admin_denies_even_with_bypass() {
        // Phase 7a/0.5/sec2 invariant — defense-in-depth.
        let id = make_identity(Role::Administrator, false);
        assert!(!perm_guard_verdict(&id, true));
    }

    #[test]
    fn perm_guard_supervisor_without_perm_denies() {
        // Supervisor doesn't bypass; needs the per-model perm.
        let id = make_identity(Role::Supervisor, true);
        assert!(!perm_guard_verdict(&id, false));
    }

    // ---- Phase 7a/0.5/e — wired-guard matrix proofs ---------------------
    //
    // Re-frames the /b primitives as the live guard outcomes per the
    // user's matrix in 7a/0.5/e. Each test pins a specific row.

    #[test]
    fn guards_user_tier_denied_at_dashboard() {
        // Dashboard guard is `role_guard(Staff)`. User-tier fails the
        // floor and renders forbidden.html instead of being redirected.
        let id = make_identity(Role::User, true);
        assert!(
            !id.role.includes(Role::Staff),
            "User-tier must NOT pass the Staff floor on /admin"
        );
    }

    #[test]
    fn guards_staff_can_view_posts_via_auditors_group() {
        // perm_guard("view") on /admin/posts/. Staff with the perm
        // (granted via Auditors group on demo-mode boot) passes.
        let id = make_identity(Role::Staff, true);
        let perm_held = true; // simulates Auditors → posts.view_post
        assert!(
            perm_guard_verdict(&id, perm_held),
            "Staff with view perm must reach the changelist"
        );
    }

    #[test]
    fn guards_staff_cannot_delete_via_auditors_group() {
        // Auditors only has view perms. Staff trying to hit
        // /admin/posts/N/delete fails the perm check.
        let id = make_identity(Role::Staff, true);
        let perm_held = false; // Auditors lacks delete_post
        assert!(
            !perm_guard_verdict(&id, perm_held),
            "Staff without delete perm must hit forbidden"
        );
    }

    #[test]
    fn guards_supervisor_can_change_via_system_operators() {
        // Supervisor in System Operators group has change perms but
        // not delete. Doesn't bypass — the perm IS what carries them.
        let id = make_identity(Role::Supervisor, true);
        // Floor satisfied (Supervisor includes Staff). Per-model perm
        // granted via group.
        assert!(id.role.includes(Role::Staff));
        assert!(perm_guard_verdict(&id, true), "with change perm → allow");
        assert!(
            !perm_guard_verdict(&id, false),
            "without delete perm → deny"
        );
    }

    #[test]
    fn guards_developer_stubs_render_for_developer_only() {
        // role_guard(Developer) on the 3 stub routes. Only Developer
        // identities pass.
        let dev = make_identity(Role::Developer, true);
        assert!(dev.role.includes(Role::Developer), "Developer passes");

        for &lower in &[Role::User, Role::Staff, Role::Supervisor, Role::Administrator] {
            let id = make_identity(lower, true);
            assert!(
                !id.role.includes(Role::Developer),
                "{lower:?} must NOT reach developer stubs"
            );
        }
    }

    #[test]
    fn guards_administrator_blocked_from_developer_stubs() {
        // Specific assertion from the matrix: Administrator is the
        // second-highest tier but does NOT include Developer.
        let admin = make_identity(Role::Administrator, true);
        assert!(
            !admin.role.includes(Role::Developer),
            "Administrator must hit forbidden on /admin/__schema__/ etc."
        );
    }
}
