//! HTTP handlers for the admin. All of them follow the same pattern:
//! check identity → load what you need from the DB → build a typed
//! context → hand it to `Templates::render`.

use std::sync::Arc;

use crate::auth::{self, Identity};
use crate::error::{Error, Result};
use crate::http::{Request, Response};
use crate::orm::Db;
use crate::templates::Templates;

use super::audit;
use super::render;
use super::render::BaseContext;
use super::types::Admin;

pub(crate) struct AdminCtx {
    pub admin: Arc<Admin>,
    pub db: Db,
    pub templates: Arc<Templates>,
}

impl AdminCtx {
    pub fn new(admin: Arc<Admin>, db: Db, templates: Arc<Templates>) -> Self {
        Self { admin, db, templates }
    }
}

// ---- Login / logout -------------------------------------------------------

pub(super) fn csrf_token(req: &Request) -> String {
    req.ctx()
        .get::<crate::middleware::CsrfGuard>()
        .map(|g| g.token.clone())
        .unwrap_or_default()
}

pub(crate) async fn show_login(ctx: &AdminCtx, req: Request) -> Result<Response> {
    let body = ctx.templates.render(
        "admin/login.html",
        &render::LoginCtx {
            base: BaseContext::new(None, csrf_token(&req)),
            error: None,
        },
    )?;
    Ok(Response::html(body))
}

pub(crate) async fn do_login(ctx: &AdminCtx, req: Request) -> Result<Response> {
    let form = req.form()?;
    let email = form.required("email")?;
    let password = form.required("password")?;

    match auth::login(&ctx.db, email, password).await {
        Ok(token) => {
            let cookie = format!(
                "{}={token}; Path=/; HttpOnly; SameSite=Strict; Max-Age=1209600",
                auth::SESSION_COOKIE
            );
            Ok(Response::redirect("/admin").with_header("set-cookie", cookie))
        }
        Err(_) => {
            let body = ctx.templates.render(
                "admin/login.html",
                &render::LoginCtx {
                    base: BaseContext::new(None, csrf_token(&req)),
                    error: Some("Invalid email or password.".into()),
                },
            )?;
            Ok(Response::html(body).with_status(hyper::StatusCode::UNAUTHORIZED))
        }
    }
}

pub(crate) async fn do_logout(ctx: &AdminCtx, req: Request) -> Result<Response> {
    if let Some(cookie) = req.header("cookie") {
        if let Some(token) = auth::session_token_from_cookie(cookie) {
            auth::delete_session(&ctx.db, &token).await?;
        }
    }
    let clear = format!("{}=; Path=/; HttpOnly; SameSite=Strict; Max-Age=0", auth::SESSION_COOKIE);
    Ok(Response::redirect("/admin/login").with_header("set-cookie", clear))
}

// ---- Dashboard -----------------------------------------------------------

pub(crate) async fn dashboard(ctx: &AdminCtx, identity: Identity, req: &Request) -> Result<Response> {
    // The audit table may not exist yet (no Phase 6a wiring point calls
    // ensure_table). Degrade silently to "no recent activity" if the
    // query fails.
    let recent_actions = audit::recent(&ctx.db, 10, None, None)
        .await
        .unwrap_or_default();
    let dash = render::dashboard_ctx(&identity, ctx.admin.entries(), recent_actions, csrf_token(req));
    let body = ctx.templates.render("admin/index.html", &dash)?;
    Ok(Response::html(body))
}

// ---- List page -----------------------------------------------------------

pub(crate) async fn list_model(
    ctx: &AdminCtx,
    identity: Identity,
    admin_name: &str,
    req: &Request,
) -> Result<Response> {
    let entry = ctx
        .admin
        .find(admin_name)
        .ok_or_else(|| Error::NotFound(format!("no admin model: {admin_name}")))?;
    let mut rows = entry.ops.list(&ctx.db).await?;

    // Phase 6a: in-memory search/filter/pagination. Pushdown to AdminOps
    // would mean touching types.rs (out of scope for 6a). Acceptable for
    // small model lists; revisit when a project hits >10k rows.
    let qs = req.query();
    let search = qs.get("q").unwrap_or_default().to_string();
    if !search.is_empty() {
        let needle = search.to_ascii_lowercase();
        rows.retain(|r| {
            r.cells
                .iter()
                .any(|c| c.to_ascii_lowercase().contains(&needle))
        });
    }

    // Build filter groups from the classifier. Selected values come from
    // the request's query string; rows that don't match are filtered out.
    let mut filter_groups: Vec<render::FilterGroupCtx> = Vec::new();
    for f in super::intelligence::infer_filters(entry.fields, None) {
        let current = qs.get(&f.field).map(str::to_string);
        if let Some(val) = &current {
            if !val.is_empty() {
                let col_idx = entry.fields.iter().position(|af| af.name == f.field);
                if let Some(idx) = col_idx {
                    rows.retain(|r| r.cells.get(idx).map(String::as_str) == Some(val.as_str()));
                }
            }
        }
        let options = match f.kind {
            super::intelligence::FilterKind::BoolYesNo => vec![
                render::FilterOptionCtx {
                    value: "true".into(),
                    label: "Yes".into(),
                    selected: current.as_deref() == Some("true"),
                },
                render::FilterOptionCtx {
                    value: "false".into(),
                    label: "No".into(),
                    selected: current.as_deref() == Some("false"),
                },
            ],
            // Phase 6a renders only Bool filters interactively. Other
            // kinds (DateRange, Dropdown, NumericExact, ExactMatch,
            // RelationDropdown) need either input widgets or relation
            // plumbing — Phase 7+.
            _ => Vec::new(),
        };
        if !options.is_empty() {
            filter_groups.push(render::FilterGroupCtx {
                field: f.field,
                label: f.label,
                options,
                current,
            });
        }
    }

    let total_rows = rows.len();
    let per_page = 100usize;
    let page: usize = qs
        .get("p")
        .and_then(|s| s.parse().ok())
        .unwrap_or(1)
        .max(1);
    let start = (page - 1) * per_page;
    let page_rows: Vec<_> = rows.into_iter().skip(start).take(per_page).collect();

    let list = render::list_ctx(
        &identity,
        ctx.admin.entries(),
        entry,
        page_rows,
        search,
        filter_groups,
        page,
        per_page,
        total_rows,
        csrf_token(req),
    );
    let body = ctx.templates.render("admin/list.html", &list)?;
    Ok(Response::html(body))
}

// ---- New / Create --------------------------------------------------------

pub(crate) async fn show_new_form(
    ctx: &AdminCtx,
    identity: Identity,
    admin_name: &str,
    req: &Request,
) -> Result<Response> {
    let entry = ctx
        .admin
        .find(admin_name)
        .ok_or_else(|| Error::NotFound(format!("no admin model: {admin_name}")))?;
    let form = render::form_ctx(&identity, ctx.admin.entries(), entry, "new", None, None, vec![], csrf_token(req));
    let body = ctx.templates.render("admin/form.html", &form)?;
    Ok(Response::html(body))
}

pub(crate) async fn do_create(
    ctx: &AdminCtx,
    identity: Identity,
    admin_name: &str,
    req: Request,
) -> Result<Response> {
    let entry = ctx
        .admin
        .find(admin_name)
        .ok_or_else(|| Error::NotFound(format!("no admin model: {admin_name}")))?;
    let form = req.form()?;
    let intent = submit_intent(&form);
    match entry.ops.create(&ctx.db, &form).await? {
        Ok(id) => {
            if let Some(hook) = &entry.search_hook {
                hook.on_upsert(&ctx.db, id).await;
            }
            Ok(Response::redirect(redirect_after_save(intent, admin_name, id)))
        }
        Err(errors) => {
            let token = csrf_token(&req);
            let ctx_view = render::form_ctx(&identity, ctx.admin.entries(), entry, "new", None, None, errors, token);
            let body = ctx.templates.render("admin/form.html", &ctx_view)?;
            Ok(Response::html(body).with_status(hyper::StatusCode::BAD_REQUEST))
        }
    }
}

/// Which `Save*` button the form submitted with. The change form has
/// three submit buttons (`_save`, `_continue`, `_addanother`) per the
/// Phase 6a design spec; this picks the redirect target after a
/// successful create / update.
#[derive(Debug, Clone, Copy)]
enum SubmitIntent {
    Save,
    Continue,
    AddAnother,
}

fn submit_intent(form: &crate::http::FormData) -> SubmitIntent {
    if form.get("_continue").is_some() {
        SubmitIntent::Continue
    } else if form.get("_addanother").is_some() {
        SubmitIntent::AddAnother
    } else {
        SubmitIntent::Save
    }
}

fn redirect_after_save(intent: SubmitIntent, admin_name: &str, id: i64) -> String {
    match intent {
        SubmitIntent::Save => format!("/admin/{admin_name}"),
        SubmitIntent::Continue => format!("/admin/{admin_name}/{id}/edit"),
        SubmitIntent::AddAnother => format!("/admin/{admin_name}/new"),
    }
}

// ---- Edit / Update -------------------------------------------------------

pub(crate) async fn show_edit_form(
    ctx: &AdminCtx,
    identity: Identity,
    admin_name: &str,
    id: i64,
    req: &Request,
) -> Result<Response> {
    let entry = ctx
        .admin
        .find(admin_name)
        .ok_or_else(|| Error::NotFound(format!("no admin model: {admin_name}")))?;
    let row = entry
        .ops
        .find_row(&ctx.db, id)
        .await?
        .ok_or_else(|| Error::NotFound(format!("{admin_name}/{id}")))?;
    let form = render::form_ctx(
        &identity,
        ctx.admin.entries(),
        entry,
        "edit",
        Some(id),
        Some(&row),
        vec![],
        csrf_token(req),
    );
    let body = ctx.templates.render("admin/form.html", &form)?;
    Ok(Response::html(body))
}

pub(crate) async fn do_update(
    ctx: &AdminCtx,
    identity: Identity,
    admin_name: &str,
    id: i64,
    req: Request,
) -> Result<Response> {
    let entry = ctx
        .admin
        .find(admin_name)
        .ok_or_else(|| Error::NotFound(format!("no admin model: {admin_name}")))?;
    let form = req.form()?;
    let intent = submit_intent(&form);
    match entry.ops.update(&ctx.db, id, &form).await? {
        Ok(()) => {
            if let Some(hook) = &entry.search_hook {
                hook.on_upsert(&ctx.db, id).await;
            }
            Ok(Response::redirect(redirect_after_save(intent, admin_name, id)))
        }
        Err(errors) => {
            let existing = entry.ops.find_row(&ctx.db, id).await?;
            let token = csrf_token(&req);
            let ctx_view = render::form_ctx(
                &identity,
                ctx.admin.entries(),
                entry,
                "edit",
                Some(id),
                existing.as_ref(),
                errors,
                token,
            );
            let body = ctx.templates.render("admin/form.html", &ctx_view)?;
            Ok(Response::html(body).with_status(hyper::StatusCode::BAD_REQUEST))
        }
    }
}

// ---- Delete --------------------------------------------------------------

pub(crate) async fn show_delete_confirm(
    ctx: &AdminCtx,
    identity: Identity,
    admin_name: &str,
    id: i64,
    req: &Request,
) -> Result<Response> {
    let entry = ctx
        .admin
        .find(admin_name)
        .ok_or_else(|| Error::NotFound(format!("no admin model: {admin_name}")))?;
    let label = entry
        .ops
        .object_label(&ctx.db, id)
        .await?
        .ok_or_else(|| Error::NotFound(format!("{admin_name}/{id}")))?;

    // Build a fresh registry from the current admin to identify which
    // models point at this one via a BelongsTo FK. Cheap — runs once
    // per delete-confirm GET, and the schema is small.
    let schema = crate::schema::Schema::from_admin(&ctx.admin);
    let registry = super::relations::RelationRegistry::from_schema(&schema);
    let cascading: Vec<render::CascadeItem> = registry
        .has_many(entry.singular_name)
        .iter()
        .map(|inv| render::CascadeItem {
            source_display_name: inv.source_display_name.clone(),
            source_admin_name: inv.source_admin_name.clone(),
            source_field: inv.source_field.clone(),
        })
        .collect();

    let view = render::confirm_delete_ctx(
        &identity,
        ctx.admin.entries(),
        entry,
        id,
        label,
        cascading,
        csrf_token(req),
    );
    let body = ctx.templates.render("admin/confirm_delete.html", &view)?;
    Ok(Response::html(body))
}

pub(crate) async fn do_delete(
    ctx: &AdminCtx,
    _identity: Identity,
    admin_name: &str,
    id: i64,
) -> Result<Response> {
    let entry = ctx
        .admin
        .find(admin_name)
        .ok_or_else(|| Error::NotFound(format!("no admin model: {admin_name}")))?;
    entry.ops.delete(&ctx.db, id).await?;
    if let Some(hook) = &entry.search_hook {
        hook.on_delete(id);
    }
    Ok(Response::redirect(format!("/admin/{admin_name}")))
}
