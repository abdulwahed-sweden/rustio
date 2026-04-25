//! Built-in admin pages for users and groups. These are wired into
//! the admin automatically — users never have to opt in, since every
//! project needs them.
//!
//! The pages are simpler than normal model admin pages (smaller forms,
//! custom actions like "set password"), so they don't use the
//! `AdminModel` trait — they're rendered by bespoke handlers.

use std::sync::Arc;

use serde::Serialize;

use crate::auth::{self, Identity, Role};
use crate::error::{Error, Result};
use crate::http::{Request, Response};
use crate::orm::{Db, Row};
use crate::templates::Templates;

use super::render::{BaseContext, FlashCtx, SidebarEntry};
use super::types::Admin;

pub(crate) struct AuthAdminCtx {
    pub admin: Arc<Admin>,
    pub db: Db,
    pub templates: Arc<Templates>,
}

// ---------- Users list ----------

#[derive(Serialize)]
struct UserRow {
    id: i64,
    email: String,
    role: String,
    is_active: bool,
    created_at: String,
}

#[derive(Serialize)]
struct UsersListCtx {
    #[serde(flatten)]
    base: BaseContext,
    page_title: &'static str,
    entries: Vec<SidebarEntry>,
    users: Vec<UserRow>,
    flash: Option<FlashCtx>,
}

pub(crate) async fn list_users(
    ctx: &AuthAdminCtx,
    identity: Identity,
    csrf: String,
) -> Result<Response> {
    let rows = sqlx::query(
        "SELECT id, email, role, is_active, created_at
           FROM rustio_users
          ORDER BY id ASC",
    )
    .fetch_all(ctx.db.pool())
    .await?;

    let users = rows
        .iter()
        .map(|r| {
            let r = Row::from_pg(r);
            Ok(UserRow {
                id: r.get_i64("id")?,
                email: r.get_string("email")?,
                role: r.get_string("role")?,
                is_active: r.get_bool("is_active")?,
                created_at: r
                    .get_datetime("created_at")?
                    .format("%Y-%m-%d %H:%M")
                    .to_string(),
            })
        })
        .collect::<Result<Vec<_>>>()?;

    let view = UsersListCtx {
        base: BaseContext::new(Some(&identity), csrf, &ctx.admin),
        page_title: "Users",
        entries: ctx.admin.entries().iter().map(SidebarEntry::from).collect(),
        users,
        flash: None,
    };
    let body = ctx.templates.render("admin/users_list.html", &view)?;
    Ok(Response::html(body))
}

// ---------- User edit ----------

#[derive(Serialize)]
struct UserEditCtx {
    #[serde(flatten)]
    base: BaseContext,
    page_title: String,
    entries: Vec<SidebarEntry>,
    user_id: i64,
    email: String,
    role: String,
    is_active: bool,
    all_groups: Vec<GroupRow>,
    user_groups: Vec<i64>,
    errors: Vec<String>,
    flash: Option<FlashCtx>,
    /// Phase 7a/0.5/f — set when this user is the sole active
    /// developer. The template renders a yellow banner so admins know
    /// a role change here will be rejected by `do_user_edit`.
    is_last_developer: bool,
}

#[derive(Serialize)]
struct GroupRow {
    id: i64,
    name: String,
    description: String,
}

async fn load_groups(db: &Db) -> Result<Vec<GroupRow>> {
    let rows =
        sqlx::query("SELECT id, name, description FROM rustio_groups ORDER BY name ASC")
            .fetch_all(db.pool())
            .await?;
    rows.iter()
        .map(|r| {
            let r = Row::from_pg(r);
            Ok(GroupRow {
                id: r.get_i64("id")?,
                name: r.get_string("name")?,
                description: r.get_string("description")?,
            })
        })
        .collect()
}

pub(crate) async fn show_user_edit(
    ctx: &AuthAdminCtx,
    identity: Identity,
    user_id: i64,
    csrf: String,
) -> Result<Response> {
    let row = sqlx::query(
        "SELECT id, email, role, is_active FROM rustio_users WHERE id = $1",
    )
    .bind(user_id)
    .fetch_optional(ctx.db.pool())
    .await?;
    let row = row.ok_or_else(|| Error::NotFound(format!("user #{user_id}")))?;
    let r = Row::from_pg(&row);

    let group_ids: Vec<i64> = sqlx::query_scalar::<_, i64>(
        "SELECT group_id FROM rustio_user_groups WHERE user_id = $1",
    )
    .bind(user_id)
    .fetch_all(ctx.db.pool())
    .await?;

    let is_last_developer =
        auth::would_orphan_developers(&ctx.db, user_id, Some(Role::User)).await?;

    let view = UserEditCtx {
        base: BaseContext::new(Some(&identity), csrf, &ctx.admin),
        page_title: format!("Edit user #{user_id}"),
        entries: ctx.admin.entries().iter().map(SidebarEntry::from).collect(),
        user_id,
        email: r.get_string("email")?,
        role: r.get_string("role")?,
        is_active: r.get_bool("is_active")?,
        all_groups: load_groups(&ctx.db).await?,
        user_groups: group_ids,
        errors: vec![],
        flash: None,
        is_last_developer,
    };
    let body = ctx.templates.render("admin/user_edit.html", &view)?;
    Ok(Response::html(body))
}

pub(crate) async fn do_user_edit(
    ctx: &AuthAdminCtx,
    identity: Identity,
    user_id: i64,
    req: Request,
) -> Result<Response> {
    let form = req.form()?;
    let role = Role::parse(form.required("role")?)?;
    let is_active = form.bool_flag("is_active");

    // Collect the ticked group ids upfront — used either to apply
    // below, or to preserve the user's selection on a re-render with
    // errors.
    let mut wanted: Vec<i64> = Vec::new();
    for (k, v) in form.as_map() {
        if let Some(id_str) = k.strip_prefix("group_") {
            if v == "on" {
                if let Ok(gid) = id_str.parse::<i64>() {
                    wanted.push(gid);
                }
            }
        }
    }
    let new_password = form
        .get("new_password")
        .map(|s| s.to_string())
        .unwrap_or_default();

    // Phase 7a/0.5/f — last-developer guard. Block any change that
    // would leave the system with zero active developers. The helper
    // is role-based; a deactivation (is_active=false) is equivalent
    // to removing this user from the active-developer pool, so we
    // pass a non-Developer sentinel role in that case so the helper
    // catches it.
    let effective_role = if is_active { role } else { Role::User };
    if auth::would_orphan_developers(&ctx.db, user_id, Some(effective_role)).await? {
        let csrf = req
            .ctx()
            .get::<crate::middleware::CsrfGuard>()
            .map(|g| g.token.clone())
            .unwrap_or_default();
        return render_user_edit_with_errors(
            ctx,
            &identity,
            user_id,
            role,
            is_active,
            wanted,
            csrf,
            vec![
                "Cannot demote or deactivate the last active developer. \
                 Use rustio-cli to promote a backup developer first."
                    .into(),
            ],
        )
        .await;
    }

    sqlx::query(
        "UPDATE rustio_users SET role = $1, is_active = $2, updated_at = NOW() WHERE id = $3",
    )
    .bind(role.as_str())
    .bind(is_active)
    .bind(user_id)
    .execute(ctx.db.pool())
    .await?;

    sqlx::query("DELETE FROM rustio_user_groups WHERE user_id = $1")
        .bind(user_id)
        .execute(ctx.db.pool())
        .await?;
    // Phase 7a/0.5/sec3 — the wholesale DELETE bypasses
    // `remove_user_from_group`'s built-in cache invalidation. Without
    // this explicit call, a user demoted to zero groups keeps every
    // permission for up to 60 seconds (PERM_CACHE_TTL). When the
    // checkbox loop below adds at least one group back, that path's
    // own invalidation covers us — but the all-unchecked case lands
    // here.
    auth::invalidate_user_cache(user_id);
    for gid in wanted {
        auth::add_user_to_group(&ctx.db, user_id, gid).await?;
    }

    if !new_password.is_empty() {
        auth::set_password(&ctx.db, user_id, &new_password).await?;
    }

    Ok(Response::redirect("/admin/users"))
}

/// Re-render the user edit form with validation errors displayed
/// inline. Used by `do_user_edit` when the orphan guard rejects a
/// change. Returns 400 Bad Request so callers (and tests) can
/// distinguish a rejected save from a successful redirect.
#[allow(clippy::too_many_arguments)]
async fn render_user_edit_with_errors(
    ctx: &AuthAdminCtx,
    identity: &Identity,
    user_id: i64,
    role: Role,
    is_active: bool,
    user_groups: Vec<i64>,
    csrf: String,
    errors: Vec<String>,
) -> Result<Response> {
    let row = sqlx::query("SELECT email FROM rustio_users WHERE id = $1")
        .bind(user_id)
        .fetch_optional(ctx.db.pool())
        .await?;
    let row = row.ok_or_else(|| Error::NotFound(format!("user #{user_id}")))?;
    let r = Row::from_pg(&row);

    let is_last_developer =
        auth::would_orphan_developers(&ctx.db, user_id, Some(Role::User)).await?;

    let view = UserEditCtx {
        base: BaseContext::new(Some(identity), csrf, &ctx.admin),
        page_title: format!("Edit user #{user_id}"),
        entries: ctx.admin.entries().iter().map(SidebarEntry::from).collect(),
        user_id,
        email: r.get_string("email")?,
        role: role.as_str().into(),
        is_active,
        all_groups: load_groups(&ctx.db).await?,
        user_groups,
        errors,
        flash: None,
        is_last_developer,
    };
    let body = ctx.templates.render("admin/user_edit.html", &view)?;
    Ok(Response::html(body).with_status(hyper::StatusCode::BAD_REQUEST))
}

// ---------- User view (Phase 7a/0.5/h) ----------
//
// Read-only profile page. Sits between the users list (which now
// links each row here, not to /edit) and the destructive surfaces.
// The view is the navigation hub — Back, Edit, Delete buttons all
// live here — so the edit + delete pages don't have to render
// profile metadata; they stay focused on the action they perform.

#[derive(Serialize)]
struct UserViewGroup {
    name: String,
    description: String,
}

#[derive(Serialize)]
struct UserViewCtx {
    #[serde(flatten)]
    base: BaseContext,
    page_title: String,
    entries: Vec<SidebarEntry>,
    target_id: i64,
    target_email: String,
    target_role: String,
    target_is_active: bool,
    target_is_demo: bool,
    target_demo_label: Option<String>,
    target_created_at: String,
    target_updated_at: String,
    groups: Vec<UserViewGroup>,
    /// Direct permission grants (NOT via groups). Empty for the
    /// common case — direct grants are the rare exception, callers
    /// usually attach via groups.
    direct_perms: Vec<String>,
    /// `Identity::user_id == target.id`. Disables the Delete button
    /// (matches `do_user_delete`'s self-delete guard).
    is_self: bool,
    /// `would_orphan_developers(target.id, Some(Role::User))`. Same
    /// flag the delete confirm page uses.
    is_last_developer: bool,
    /// Convenience flag for the template: Administrator sessions
    /// always have edit perm for built-in user pages, but threading
    /// the bool through keeps the template free of role logic.
    can_edit: bool,
    /// `!is_self && !is_last_developer`. The template renders the
    /// Delete button as `<a>` when true, `<span class="disabled">`
    /// otherwise — same blocking surface as the confirm page, just
    /// promoted to the navigation row.
    can_delete: bool,
}

pub(crate) async fn show_user_view(
    ctx: &AuthAdminCtx,
    identity: Identity,
    user_id: i64,
    csrf: String,
) -> Result<Response> {
    let row = sqlx::query(
        "SELECT id, email, role, is_active, is_demo, demo_label, \
                created_at, updated_at \
         FROM rustio_users WHERE id = $1",
    )
    .bind(user_id)
    .fetch_optional(ctx.db.pool())
    .await?;
    let row = row.ok_or_else(|| Error::NotFound(format!("user #{user_id}")))?;
    let r = Row::from_pg(&row);

    // Group memberships for display. Same JOIN shape as
    // `do_user_edit`'s checkbox seed, but ordered for readable output.
    let group_rows = sqlx::query(
        "SELECT g.name, g.description \
         FROM rustio_groups g \
         JOIN rustio_user_groups ug ON ug.group_id = g.id \
         WHERE ug.user_id = $1 \
         ORDER BY g.name ASC",
    )
    .bind(user_id)
    .fetch_all(ctx.db.pool())
    .await?;
    let groups = group_rows
        .iter()
        .map(|r| {
            let r = Row::from_pg(r);
            Ok(UserViewGroup {
                name: r.get_string("name")?,
                description: r.get_string("description")?,
            })
        })
        .collect::<Result<Vec<_>>>()?;

    // Direct permission grants — explicitly NOT joined through
    // groups. These are the rare per-user overrides; the template
    // calls them out so admins can spot drift from group-only policy.
    let direct_perms: Vec<String> = sqlx::query_scalar(
        "SELECT p.name \
         FROM rustio_permissions p \
         JOIN rustio_user_permissions up ON up.permission_id = p.id \
         WHERE up.user_id = $1 \
         ORDER BY p.name ASC",
    )
    .bind(user_id)
    .fetch_all(ctx.db.pool())
    .await?;

    let is_self = identity.user_id == user_id;
    let is_last_developer =
        auth::would_orphan_developers(&ctx.db, user_id, Some(Role::User)).await?;

    let target_email = r.get_string("email")?;
    let view = UserViewCtx {
        base: BaseContext::new(Some(&identity), csrf, &ctx.admin),
        page_title: format!("User: {target_email}"),
        entries: ctx.admin.entries().iter().map(SidebarEntry::from).collect(),
        target_id: user_id,
        target_email,
        target_role: r.get_string("role")?,
        target_is_active: r.get_bool("is_active")?,
        target_is_demo: r.get_bool("is_demo")?,
        target_demo_label: r.get_optional_string("demo_label")?,
        target_created_at: r
            .get_datetime("created_at")?
            .format("%Y-%m-%d %H:%M UTC")
            .to_string(),
        target_updated_at: r
            .get_datetime("updated_at")?
            .format("%Y-%m-%d %H:%M UTC")
            .to_string(),
        groups,
        direct_perms,
        is_self,
        is_last_developer,
        can_edit: true,
        can_delete: !is_self && !is_last_developer,
    };
    let body = ctx.templates.render("admin/user_view.html", &view)?;
    Ok(Response::html(body))
}

// ---------- User delete (Phase 7a/0.5/f) ----------

#[derive(Serialize)]
struct UserDeleteCtx {
    #[serde(flatten)]
    base: BaseContext,
    page_title: String,
    entries: Vec<SidebarEntry>,
    user_id: i64,
    email: String,
    role: String,
    /// Memberships dropped on cascade.
    group_count: i64,
    /// Active sessions terminated on cascade.
    session_count: i64,
    /// Direct permission grants dropped on cascade.
    direct_perm_count: i64,
    /// Set when the target is the currently logged-in user. The
    /// confirm form hides the submit button in this case.
    is_self: bool,
    /// Set when removing this user would leave zero active developers.
    /// Like `is_self`, this disables the submit button.
    is_last_developer: bool,
}

pub(crate) async fn show_user_delete(
    ctx: &AuthAdminCtx,
    identity: Identity,
    user_id: i64,
    csrf: String,
) -> Result<Response> {
    let row = sqlx::query("SELECT id, email, role FROM rustio_users WHERE id = $1")
        .bind(user_id)
        .fetch_optional(ctx.db.pool())
        .await?;
    let row = row.ok_or_else(|| Error::NotFound(format!("user #{user_id}")))?;
    let r = Row::from_pg(&row);

    let group_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM rustio_user_groups WHERE user_id = $1")
            .bind(user_id)
            .fetch_one(ctx.db.pool())
            .await?;
    let session_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM rustio_sessions WHERE user_id = $1 AND expires_at > NOW()",
    )
    .bind(user_id)
    .fetch_one(ctx.db.pool())
    .await?;
    let direct_perm_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM rustio_user_permissions WHERE user_id = $1")
            .bind(user_id)
            .fetch_one(ctx.db.pool())
            .await?;

    let is_self = identity.user_id == user_id;
    // Pretend the user is being demoted to nothing — that's what a
    // delete effectively is. `would_orphan_developers` returns true if
    // the target is the sole active developer.
    let is_last_developer =
        auth::would_orphan_developers(&ctx.db, user_id, Some(Role::User)).await?;

    let email = r.get_string("email")?;
    let view = UserDeleteCtx {
        base: BaseContext::new(Some(&identity), csrf, &ctx.admin),
        page_title: format!("Delete user: {email}"),
        entries: ctx.admin.entries().iter().map(SidebarEntry::from).collect(),
        user_id,
        email,
        role: r.get_string("role")?,
        group_count,
        session_count,
        direct_perm_count,
        is_self,
        is_last_developer,
    };
    let body = ctx.templates.render("admin/user_confirm_delete.html", &view)?;
    Ok(Response::html(body))
}

pub(crate) async fn do_user_delete(
    ctx: &AuthAdminCtx,
    identity: Identity,
    user_id: i64,
    _req: Request,
) -> Result<Response> {
    // Self-delete guard: an admin cannot delete their own logged-in
    // account. Without this, a successful POST would invalidate the
    // current session via cascade and leave the system in an odd state
    // (and the admin would need to re-login just to undo a typo).
    if identity.user_id == user_id {
        return Err(Error::BadRequest(
            "You cannot delete your own account while signed in.".into(),
        ));
    }

    // Last-developer guard. A delete is the strongest form of demotion;
    // reuse the helper by passing a non-Developer role.
    if auth::would_orphan_developers(&ctx.db, user_id, Some(Role::User)).await? {
        return Err(Error::BadRequest(
            "Cannot delete the last active developer. \
             Use rustio-cli to promote a backup developer first."
                .into(),
        ));
    }

    sqlx::query("DELETE FROM rustio_users WHERE id = $1")
        .bind(user_id)
        .execute(ctx.db.pool())
        .await?;

    // Cascade through user_groups, user_permissions, and sessions
    // happens at the FK level. The permission cache is keyed on
    // user_id — drop the entry so a re-created user with the same id
    // (vanishingly unlikely with BIGSERIAL but cheap insurance) starts
    // clean.
    auth::invalidate_user_cache(user_id);

    Ok(Response::redirect("/admin/users"))
}

// ---------- Groups ----------

#[derive(Serialize)]
struct GroupsListCtx {
    #[serde(flatten)]
    base: BaseContext,
    page_title: &'static str,
    entries: Vec<SidebarEntry>,
    groups: Vec<GroupRow>,
    flash: Option<FlashCtx>,
}

pub(crate) async fn list_groups(
    ctx: &AuthAdminCtx,
    identity: Identity,
    csrf: String,
) -> Result<Response> {
    let view = GroupsListCtx {
        base: BaseContext::new(Some(&identity), csrf, &ctx.admin),
        page_title: "Groups",
        entries: ctx.admin.entries().iter().map(SidebarEntry::from).collect(),
        groups: load_groups(&ctx.db).await?,
        flash: None,
    };
    let body = ctx.templates.render("admin/groups_list.html", &view)?;
    Ok(Response::html(body))
}

#[derive(Serialize)]
struct GroupEditCtx {
    #[serde(flatten)]
    base: BaseContext,
    page_title: String,
    entries: Vec<SidebarEntry>,
    group_id: i64,
    name: String,
    description: String,
    all_permissions: Vec<PermRow>,
    group_permissions: Vec<i64>,
    errors: Vec<String>,
    flash: Option<FlashCtx>,
}

#[derive(Serialize)]
struct PermRow {
    id: i64,
    name: String,
}

pub(crate) async fn show_group_edit(
    ctx: &AuthAdminCtx,
    identity: Identity,
    group_id: i64,
    csrf: String,
) -> Result<Response> {
    let row = sqlx::query("SELECT id, name, description FROM rustio_groups WHERE id = $1")
        .bind(group_id)
        .fetch_optional(ctx.db.pool())
        .await?;
    let row = row.ok_or_else(|| Error::NotFound(format!("group #{group_id}")))?;
    let r = Row::from_pg(&row);

    let all: Vec<PermRow> = {
        let rows = sqlx::query("SELECT id, name FROM rustio_permissions ORDER BY name ASC")
            .fetch_all(ctx.db.pool())
            .await?;
        rows.iter()
            .map(|r| {
                let r = Row::from_pg(r);
                Ok(PermRow {
                    id: r.get_i64("id")?,
                    name: r.get_string("name")?,
                })
            })
            .collect::<Result<Vec<_>>>()?
    };

    let current: Vec<i64> = sqlx::query_scalar::<_, i64>(
        "SELECT permission_id FROM rustio_group_permissions WHERE group_id = $1",
    )
    .bind(group_id)
    .fetch_all(ctx.db.pool())
    .await?;

    let view = GroupEditCtx {
        base: BaseContext::new(Some(&identity), csrf, &ctx.admin),
        page_title: format!("Edit group #{group_id}"),
        entries: ctx.admin.entries().iter().map(SidebarEntry::from).collect(),
        group_id,
        name: r.get_string("name")?,
        description: r.get_string("description")?,
        all_permissions: all,
        group_permissions: current,
        errors: vec![],
        flash: None,
    };
    let body = ctx.templates.render("admin/group_edit.html", &view)?;
    Ok(Response::html(body))
}

pub(crate) async fn do_group_edit(
    ctx: &AuthAdminCtx,
    _identity: Identity,
    group_id: i64,
    req: Request,
) -> Result<Response> {
    let form = req.form()?;
    let name = form.required("name")?;
    let description = form.get("description").unwrap_or("");

    sqlx::query(
        "UPDATE rustio_groups SET name = $1, description = $2 WHERE id = $3",
    )
    .bind(name)
    .bind(description)
    .bind(group_id)
    .execute(ctx.db.pool())
    .await?;

    // Rewrite permission assignment.
    sqlx::query("DELETE FROM rustio_group_permissions WHERE group_id = $1")
        .bind(group_id)
        .execute(ctx.db.pool())
        .await?;

    for (k, v) in form.as_map() {
        if let Some(id_str) = k.strip_prefix("perm_") {
            if v == "on" {
                if let Ok(pid) = id_str.parse::<i64>() {
                    sqlx::query(
                        "INSERT INTO rustio_group_permissions (group_id, permission_id)
                         VALUES ($1, $2) ON CONFLICT DO NOTHING",
                    )
                    .bind(group_id)
                    .bind(pid)
                    .execute(ctx.db.pool())
                    .await?;
                }
            }
        }
    }

    Ok(Response::redirect("/admin/groups"))
}

// ---------- Group delete (Phase 7a/0.5/sec1) ----------

#[derive(Serialize)]
struct GroupDeleteCtx {
    #[serde(flatten)]
    base: BaseContext,
    page_title: String,
    entries: Vec<SidebarEntry>,
    group_id: i64,
    name: String,
    description: String,
    /// How many users currently belong to this group. The delete
    /// cascades through `rustio_user_groups` (FK ON DELETE CASCADE)
    /// so the row count drops to zero on save.
    user_count: i64,
    /// How many permissions are currently attached. Cascade through
    /// `rustio_group_permissions`.
    perm_count: i64,
}

pub(crate) async fn show_group_delete(
    ctx: &AuthAdminCtx,
    identity: Identity,
    group_id: i64,
    csrf: String,
) -> Result<Response> {
    let row = sqlx::query("SELECT id, name, description FROM rustio_groups WHERE id = $1")
        .bind(group_id)
        .fetch_optional(ctx.db.pool())
        .await?;
    let row = row.ok_or_else(|| Error::NotFound(format!("group #{group_id}")))?;
    let r = Row::from_pg(&row);

    let user_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM rustio_user_groups WHERE group_id = $1")
            .bind(group_id)
            .fetch_one(ctx.db.pool())
            .await?;
    let perm_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM rustio_group_permissions WHERE group_id = $1")
            .bind(group_id)
            .fetch_one(ctx.db.pool())
            .await?;

    let name = r.get_string("name")?;
    let view = GroupDeleteCtx {
        base: BaseContext::new(Some(&identity), csrf, &ctx.admin),
        page_title: format!("Delete group: {name}"),
        entries: ctx.admin.entries().iter().map(SidebarEntry::from).collect(),
        group_id,
        name,
        description: r.get_string("description")?,
        user_count,
        perm_count,
    };
    let body = ctx.templates.render("admin/group_confirm_delete.html", &view)?;
    Ok(Response::html(body))
}

pub(crate) async fn do_group_delete(
    ctx: &AuthAdminCtx,
    _identity: Identity,
    group_id: i64,
    _req: Request,
) -> Result<Response> {
    // Capture every user that's losing this group BEFORE the cascade
    // wipes the M2M table — we need the ids to invalidate the perm
    // cache once their membership drops.
    let user_ids: Vec<i64> = sqlx::query_scalar(
        "SELECT user_id FROM rustio_user_groups WHERE group_id = $1",
    )
    .bind(group_id)
    .fetch_all(ctx.db.pool())
    .await?;

    // The FKs on rustio_user_groups + rustio_group_permissions are
    // ON DELETE CASCADE, so this single DELETE clears all M2M rows.
    sqlx::query("DELETE FROM rustio_groups WHERE id = $1")
        .bind(group_id)
        .execute(ctx.db.pool())
        .await?;

    // Cache invalidation has to be explicit — the cascade ran in PG,
    // not via our `remove_user_from_group` helper.
    for uid in user_ids {
        crate::auth::invalidate_user_cache(uid);
    }

    Ok(Response::redirect("/admin/groups"))
}

// ---------- New user ----------

#[derive(Serialize)]
struct UserNewCtx {
    #[serde(flatten)]
    base: BaseContext,
    page_title: &'static str,
    entries: Vec<SidebarEntry>,
    email: String,
    /// Selected role string for re-rendering on validation failure.
    /// Defaults to `"staff"` on a fresh form (Phase 7a/0.5/d).
    role: String,
    errors: Vec<String>,
}

pub(crate) async fn show_new_user(
    ctx: &AuthAdminCtx,
    identity: Identity,
    csrf: String,
) -> Result<Response> {
    let view = UserNewCtx {
        base: BaseContext::new(Some(&identity), csrf, &ctx.admin),
        page_title: "Add user",
        entries: ctx.admin.entries().iter().map(SidebarEntry::from).collect(),
        email: String::new(),
        role: "staff".into(),
        errors: Vec::new(),
    };
    let body = ctx.templates.render("admin/user_new.html", &view)?;
    Ok(Response::html(body))
}

/// Same minimum length used by the self-service password change page.
const MIN_NEW_USER_PASSWORD_LEN: usize = 8;

/// Cheap email-shape check — `<x>@<y>.<z>` with non-empty parts.
/// Matches what most folks expect from a "looks like an email"
/// validation; the canonical RFC check happens at deliverability time
/// elsewhere.
fn looks_like_email(s: &str) -> bool {
    let s = s.trim();
    let Some((local, domain)) = s.split_once('@') else {
        return false;
    };
    if local.is_empty() || domain.is_empty() {
        return false;
    }
    let Some((host, tld)) = domain.rsplit_once('.') else {
        return false;
    };
    !host.is_empty() && !tld.is_empty()
}

pub(crate) async fn do_new_user(
    ctx: &AuthAdminCtx,
    identity: Identity,
    req: Request,
) -> Result<Response> {
    let form = req.form()?;
    let email = form.get("email").unwrap_or("").trim().to_string();
    let password = form.get("password").unwrap_or("");
    let role_str = form.get("role").unwrap_or("staff").to_string();

    let mut errors: Vec<String> = Vec::new();

    // Parse role first so we can preserve the user's selection on
    // re-render even if other fields fail.
    let role_parsed = Role::parse(&role_str).ok();
    if role_parsed.is_none() {
        errors.push(format!("Unknown role: \"{role_str}\"."));
    }

    if email.is_empty() {
        errors.push("Email is required.".into());
    } else if !looks_like_email(&email) {
        errors.push("Enter a valid email address.".into());
    } else {
        // Pre-check uniqueness for a clean message — the unique
        // constraint would otherwise surface as a Postgres error.
        let existing = auth::find_user_by_email(&ctx.db, &email).await?;
        if existing.is_some() {
            errors.push(format!("A user with email \"{email}\" already exists."));
        }
    }

    if password.len() < MIN_NEW_USER_PASSWORD_LEN {
        errors.push(format!(
            "This password is too short. It must contain at least {MIN_NEW_USER_PASSWORD_LEN} characters."
        ));
    }

    if errors.is_empty() {
        let role = role_parsed.expect("role parsed when errors empty");
        let new_id = auth::create_user(&ctx.db, &email, password, role).await?;
        return Ok(Response::redirect(format!("/admin/users/{new_id}/edit")));
    }

    // Re-render with errors. Password is intentionally NOT echoed back —
    // the user retypes. Email and role selection ARE preserved.
    let csrf = req
        .ctx()
        .get::<crate::middleware::CsrfGuard>()
        .map(|g| g.token.clone())
        .unwrap_or_default();
    let view = UserNewCtx {
        base: BaseContext::new(Some(&identity), csrf, &ctx.admin),
        page_title: "Add user",
        entries: ctx.admin.entries().iter().map(SidebarEntry::from).collect(),
        email,
        role: role_str,
        errors,
    };
    let body = ctx.templates.render("admin/user_new.html", &view)?;
    Ok(Response::html(body).with_status(hyper::StatusCode::BAD_REQUEST))
}

// ---------- New group ----------

#[derive(Serialize)]
struct GroupNewCtx {
    #[serde(flatten)]
    base: BaseContext,
    page_title: &'static str,
    entries: Vec<SidebarEntry>,
    name: String,
    description: String,
    errors: Vec<String>,
}

pub(crate) async fn show_new_group(
    ctx: &AuthAdminCtx,
    identity: Identity,
    csrf: String,
) -> Result<Response> {
    let view = GroupNewCtx {
        base: BaseContext::new(Some(&identity), csrf, &ctx.admin),
        page_title: "Add group",
        entries: ctx.admin.entries().iter().map(SidebarEntry::from).collect(),
        name: String::new(),
        description: String::new(),
        errors: Vec::new(),
    };
    let body = ctx.templates.render("admin/group_new.html", &view)?;
    Ok(Response::html(body))
}

pub(crate) async fn do_new_group(
    ctx: &AuthAdminCtx,
    identity: Identity,
    req: Request,
) -> Result<Response> {
    let form = req.form()?;
    let name = form.get("name").unwrap_or("").trim().to_string();
    let description = form.get("description").unwrap_or("").to_string();

    let mut errors: Vec<String> = Vec::new();
    if name.is_empty() {
        errors.push("Name is required.".into());
    } else if name.len() > 150 {
        errors.push("Name must be 150 characters or fewer.".into());
    }

    if errors.is_empty() {
        // INSERT — `ON CONFLICT (name) DO NOTHING` would mask the
        // duplicate-name error; let the unique-constraint violation
        // bubble up so the user sees a real error.
        let result = sqlx::query(
            "INSERT INTO rustio_groups (name, description) VALUES ($1, $2) RETURNING id",
        )
        .bind(&name)
        .bind(&description)
        .fetch_one(ctx.db.pool())
        .await;

        match result {
            Ok(row) => {
                let r = Row::from_pg(&row);
                let new_id: i64 = r.get_i64("id")?;
                return Ok(Response::redirect(format!("/admin/groups/{new_id}/edit")));
            }
            Err(sqlx::Error::Database(db_err)) if db_err.constraint().is_some() => {
                errors.push(format!("A group named \"{name}\" already exists."));
            }
            Err(e) => return Err(e.into()),
        }
    }

    let csrf = req
        .ctx()
        .get::<crate::middleware::CsrfGuard>()
        .map(|g| g.token.clone())
        .unwrap_or_default();
    let view = GroupNewCtx {
        base: BaseContext::new(Some(&identity), csrf, &ctx.admin),
        page_title: "Add group",
        entries: ctx.admin.entries().iter().map(SidebarEntry::from).collect(),
        name,
        description,
        errors,
    };
    let body = ctx.templates.render("admin/group_new.html", &view)?;
    Ok(Response::html(body).with_status(hyper::StatusCode::BAD_REQUEST))
}
