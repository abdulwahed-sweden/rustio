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
        base: BaseContext::new(Some(&identity), csrf),
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

    let view = UserEditCtx {
        base: BaseContext::new(Some(&identity), csrf),
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
    };
    let body = ctx.templates.render("admin/user_edit.html", &view)?;
    Ok(Response::html(body))
}

pub(crate) async fn do_user_edit(
    ctx: &AuthAdminCtx,
    _identity: Identity,
    user_id: i64,
    req: Request,
) -> Result<Response> {
    let form = req.form()?;
    let role = Role::parse(form.required("role")?)?;
    let is_active = form.bool_flag("is_active");

    sqlx::query(
        "UPDATE rustio_users SET role = $1, is_active = $2, updated_at = NOW() WHERE id = $3",
    )
    .bind(role.as_str())
    .bind(is_active)
    .bind(user_id)
    .execute(ctx.db.pool())
    .await?;

    // Reset group membership to whatever the form ticked.
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
    sqlx::query("DELETE FROM rustio_user_groups WHERE user_id = $1")
        .bind(user_id)
        .execute(ctx.db.pool())
        .await?;
    for gid in wanted {
        auth::add_user_to_group(&ctx.db, user_id, gid).await?;
    }

    // Optional password reset.
    if let Some(new_password) = form.get("new_password") {
        if !new_password.is_empty() {
            auth::set_password(&ctx.db, user_id, new_password).await?;
        }
    }

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
        base: BaseContext::new(Some(&identity), csrf),
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
        base: BaseContext::new(Some(&identity), csrf),
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
