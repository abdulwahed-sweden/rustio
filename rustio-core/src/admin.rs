//! Auto-generated CRUD admin backed by [`crate::orm`].
//!
//! Build an [`Admin`] by chaining `.model::<T>()` calls, then mount it with
//! [`Admin::register`]. This attaches list / create / edit / delete routes
//! at `/admin/<admin_name>` for each model and an index page at `/admin`
//! listing every registered model.
//!
//! For a single-model app, [`register`] is a convenience wrapper.

use std::sync::Arc;

use bytes::Bytes;
use http_body_util::{BodyExt, Full};

use crate::auth::require_admin;
use crate::error::Error;
use crate::http::{html, Request, Response};
use crate::orm::{Db, Model};
use crate::router::Router;

// `FormData` lives in `http` and is re-exported here so that the
// `#[derive(RustioAdmin)]`-generated code referencing
// `::rustio_core::admin::FormData` continues to work.
pub use crate::http::FormData;

#[derive(Debug, Clone, Copy)]
pub enum FieldType {
    I32,
    I64,
    String,
    Bool,
}

#[derive(Debug, Clone, Copy)]
pub struct AdminField {
    pub name: &'static str,
    pub ty: FieldType,
    pub editable: bool,
}

pub trait AdminModel: Model {
    const ADMIN_NAME: &'static str;
    const DISPLAY_NAME: &'static str;
    const FIELDS: &'static [AdminField];

    fn field_display(&self, name: &str) -> Option<String>;
    fn from_form(form: &FormData, id: Option<i64>) -> Result<Self, Error>;

    /// Singular form of the display name. Used for labels like "New X" and
    /// "Edit X". Defaults to [`DISPLAY_NAME`]; the `#[derive(RustioAdmin)]`
    /// macro generates a proper singular form.
    fn singular_name() -> &'static str {
        Self::DISPLAY_NAME
    }
}

/// Metadata about one registered admin model.
#[derive(Debug, Clone)]
pub struct AdminEntry {
    pub admin_name: &'static str,
    pub display_name: &'static str,
    pub singular_name: &'static str,
}

type ModelRegistrar = Box<dyn FnOnce(Router, &Db) -> Router + Send + Sync>;

/// Builder that collects admin models and mounts them with a shared
/// `/admin` index page.
///
/// ```no_run
/// use rustio_core::admin::Admin;
/// # use rustio_core::{Db, Router};
/// # fn demo(router: Router, db: &Db) -> Router {
/// # struct Post; struct User;
/// # impl rustio_core::Model for Post {
/// #   const TABLE: &'static str = "posts";
/// #   const COLUMNS: &'static [&'static str] = &[];
/// #   const INSERT_COLUMNS: &'static [&'static str] = &[];
/// #   fn id(&self) -> i64 { 0 }
/// #   fn from_row(_: rustio_core::Row<'_>) -> Result<Self, rustio_core::Error> { unimplemented!() }
/// #   fn insert_values(&self) -> Vec<rustio_core::Value> { vec![] }
/// # }
/// # impl rustio_core::Model for User {
/// #   const TABLE: &'static str = "users";
/// #   const COLUMNS: &'static [&'static str] = &[];
/// #   const INSERT_COLUMNS: &'static [&'static str] = &[];
/// #   fn id(&self) -> i64 { 0 }
/// #   fn from_row(_: rustio_core::Row<'_>) -> Result<Self, rustio_core::Error> { unimplemented!() }
/// #   fn insert_values(&self) -> Vec<rustio_core::Value> { vec![] }
/// # }
/// # impl rustio_core::admin::AdminModel for Post {
/// #   const ADMIN_NAME: &'static str = "posts"; const DISPLAY_NAME: &'static str = "Posts";
/// #   const FIELDS: &'static [rustio_core::admin::AdminField] = &[];
/// #   fn field_display(&self, _: &str) -> Option<String> { None }
/// #   fn from_form(_: &rustio_core::admin::FormData, _: Option<i64>) -> Result<Self, rustio_core::Error> { unimplemented!() }
/// # }
/// # impl rustio_core::admin::AdminModel for User {
/// #   const ADMIN_NAME: &'static str = "users"; const DISPLAY_NAME: &'static str = "Users";
/// #   const FIELDS: &'static [rustio_core::admin::AdminField] = &[];
/// #   fn field_display(&self, _: &str) -> Option<String> { None }
/// #   fn from_form(_: &rustio_core::admin::FormData, _: Option<i64>) -> Result<Self, rustio_core::Error> { unimplemented!() }
/// # }
/// Admin::new()
///     .model::<Post>()
///     .model::<User>()
///     .register(router, db)
/// # }
/// ```
pub struct Admin {
    entries: Vec<AdminEntry>,
    registrars: Vec<ModelRegistrar>,
}

impl Admin {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            registrars: Vec::new(),
        }
    }

    /// Register a model on this admin. Adds its metadata to the index
    /// and queues its CRUD routes for mounting.
    pub fn model<T: AdminModel>(mut self) -> Self {
        self.entries.push(AdminEntry {
            admin_name: T::ADMIN_NAME,
            display_name: T::DISPLAY_NAME,
            singular_name: T::singular_name(),
        });
        self.registrars
            .push(Box::new(|router, db| mount_model::<T>(router, db)));
        self
    }

    /// Number of registered models.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Metadata for inspection.
    pub fn entries(&self) -> &[AdminEntry] {
        &self.entries
    }

    /// Mount the admin onto a router: installs `/admin` (index) and
    /// CRUD routes for every registered model. Admin-only; handlers
    /// return 401/403 via [`require_admin`].
    pub fn register(self, mut router: Router, db: &Db) -> Router {
        let entries = Arc::new(self.entries);
        let index_entries = entries.clone();
        router = router.get("/admin", move |req, _params| {
            let entries = index_entries.clone();
            async move {
                require_admin(req.ctx())?;
                Ok::<Response, Error>(html(admin_layout("Admin", &index_page(&entries))))
            }
        });
        for registrar in self.registrars {
            router = registrar(router, db);
        }
        router
    }
}

impl Default for Admin {
    fn default() -> Self {
        Self::new()
    }
}

/// Convenience: mount CRUD routes and an `/admin` index for a single model.
/// Equivalent to `Admin::new().model::<T>().register(router, db)`.
pub fn register<T>(router: Router, db: &Db) -> Router
where
    T: AdminModel + Model,
{
    Admin::new().model::<T>().register(router, db)
}

fn mount_model<T>(mut router: Router, db: &Db) -> Router
where
    T: AdminModel + Model,
{
    let base = format!("/admin/{}", T::ADMIN_NAME);
    let create_path = format!("{base}/create");
    let edit_path = format!("{base}/:id/edit");
    let delete_path = format!("{base}/:id/delete");

    let list_db = db.clone();
    router = router.get(&base, move |req, _params| {
        let db = list_db.clone();
        async move {
            require_admin(req.ctx())?;
            let items = T::all(&db).await?;
            Ok::<Response, Error>(html(admin_layout(T::DISPLAY_NAME, &list_page::<T>(&items))))
        }
    });

    router = router.get(&create_path, |req, _params| async move {
        require_admin(req.ctx())?;
        Ok::<Response, Error>(html(admin_layout(
            &format!("New {}", T::DISPLAY_NAME),
            &form_page::<T>(None, &format!("/admin/{}/create", T::ADMIN_NAME)),
        )))
    });

    let create_db = db.clone();
    router = router.post(&create_path, move |req, _params| {
        let db = create_db.clone();
        async move {
            require_admin(req.ctx())?;
            let form = read_form(req).await?;
            let item = T::from_form(&form, None)?;
            item.create(&db).await?;
            Ok::<Response, Error>(redirect(&format!("/admin/{}", T::ADMIN_NAME)))
        }
    });

    let edit_db = db.clone();
    router = router.get(&edit_path, move |req, params| {
        let db = edit_db.clone();
        async move {
            require_admin(req.ctx())?;
            let id = parse_id_param(&params)?;
            let item = T::find(&db, id).await?.ok_or(Error::NotFound)?;
            Ok::<Response, Error>(html(admin_layout(
                &format!("Edit {}", T::DISPLAY_NAME),
                &form_page::<T>(
                    Some(&item),
                    &format!("/admin/{}/{}/edit", T::ADMIN_NAME, id),
                ),
            )))
        }
    });

    let update_db = db.clone();
    router = router.post(&edit_path, move |req, params| {
        let db = update_db.clone();
        async move {
            require_admin(req.ctx())?;
            let id = parse_id_param(&params)?;
            let form = read_form(req).await?;
            let item = T::from_form(&form, Some(id))?;
            item.update(&db).await?;
            Ok::<Response, Error>(redirect(&format!("/admin/{}", T::ADMIN_NAME)))
        }
    });

    let delete_db = db.clone();
    router = router.post(&delete_path, move |req, params| {
        let db = delete_db.clone();
        async move {
            require_admin(req.ctx())?;
            let id = parse_id_param(&params)?;
            T::delete(&db, id).await?;
            Ok::<Response, Error>(redirect(&format!("/admin/{}", T::ADMIN_NAME)))
        }
    });

    router
}

fn parse_id_param(params: &crate::router::Params) -> Result<i64, Error> {
    params
        .get("id")
        .and_then(|s| s.parse::<i64>().ok())
        .ok_or_else(|| Error::BadRequest(String::from("invalid id")))
}

async fn read_form(req: Request) -> Result<FormData, Error> {
    let (_, body, _) = req.into_parts();
    let collected = body
        .collect()
        .await
        .map_err(|e| Error::BadRequest(e.to_string()))?
        .to_bytes();
    let body_str = std::str::from_utf8(&collected).map_err(|e| Error::BadRequest(e.to_string()))?;
    Ok(FormData::parse(body_str))
}

fn redirect(to: &str) -> Response {
    hyper::Response::builder()
        .status(303)
        .header("location", to)
        .body(Full::new(Bytes::new()))
        .expect("valid redirect")
}

fn admin_layout(title: &str, content: &str) -> String {
    format!(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title} — RustIO Admin</title>
<style>{css}</style>
</head>
<body>
<header><h1><a href="/admin">RustIO Admin</a></h1></header>
<main>{content}</main>
</body>
</html>"#,
        title = escape_html(title),
        css = ADMIN_CSS,
        content = content,
    )
}

fn index_page(entries: &[AdminEntry]) -> String {
    if entries.is_empty() {
        return String::from(
            r#"<h2>Admin</h2>
<p class="empty">No models are registered. Add one with
<code>Admin::new().model::&lt;YourModel&gt;()</code> or scaffold an app
via <code>rustio new app &lt;name&gt;</code>.</p>"#,
        );
    }
    let rows: String = entries
        .iter()
        .map(|e| {
            format!(
                r#"<li><a href="/admin/{name}"><span class="label">{display}</span><span class="path">/admin/{name}</span></a></li>"#,
                name = escape_html(e.admin_name),
                display = escape_html(e.display_name),
            )
        })
        .collect();
    format!(
        r#"<h2>Admin</h2>
<ul class="admin-index">{rows}</ul>"#
    )
}

fn list_page<T: AdminModel>(items: &[T]) -> String {
    let headers: String = T::FIELDS
        .iter()
        .map(|f| format!("<th>{}</th>", escape_html(f.name)))
        .collect();
    let rows: String = items
        .iter()
        .map(|item| {
            let cells: String = T::FIELDS
                .iter()
                .map(|f| {
                    let v = item.field_display(f.name).unwrap_or_default();
                    format!("<td>{}</td>", escape_html(&v))
                })
                .collect();
            let id = item.id();
            let actions = format!(
                r#"<td class="actions">
<a href="/admin/{name}/{id}/edit">edit</a>
<form method="post" action="/admin/{name}/{id}/delete">
<button type="submit" class="danger">delete</button>
</form>
</td>"#,
                name = T::ADMIN_NAME,
                id = id,
            );
            format!("<tr>{cells}{actions}</tr>")
        })
        .collect();

    format!(
        r#"<div class="toolbar">
<h2>{title}</h2>
<a class="button" href="/admin/{name}/create">New {singular}</a>
</div>
<table>
<thead><tr>{headers}<th>actions</th></tr></thead>
<tbody>{rows}</tbody>
</table>"#,
        title = escape_html(T::DISPLAY_NAME),
        singular = escape_html(T::singular_name()),
        name = T::ADMIN_NAME,
    )
}

fn form_page<T: AdminModel>(item: Option<&T>, action: &str) -> String {
    let fields: String = T::FIELDS
        .iter()
        .filter(|f| f.editable)
        .map(|f| render_field::<T>(f, item))
        .collect();
    let heading = if item.is_some() {
        format!("Edit {}", T::singular_name())
    } else {
        format!("New {}", T::singular_name())
    };
    format!(
        r#"<h2>{heading}</h2>
<form method="post" action="{action}">
{fields}
<div class="form-actions">
<button type="submit">Save</button>
<a class="cancel" href="/admin/{name}">Cancel</a>
</div>
</form>"#,
        heading = escape_html(&heading),
        action = escape_html(action),
        name = T::ADMIN_NAME,
    )
}

fn render_field<T: AdminModel>(f: &AdminField, item: Option<&T>) -> String {
    let current = item
        .and_then(|i| i.field_display(f.name))
        .unwrap_or_default();
    let input = match f.ty {
        FieldType::Bool => format!(
            r#"<input type="checkbox" name="{n}" {checked}>"#,
            n = escape_html(f.name),
            checked = if current == "true" { "checked" } else { "" },
        ),
        FieldType::I32 | FieldType::I64 => format!(
            r#"<input type="number" name="{n}" value="{v}">"#,
            n = escape_html(f.name),
            v = escape_html(&current),
        ),
        FieldType::String => format!(
            r#"<input type="text" name="{n}" value="{v}">"#,
            n = escape_html(f.name),
            v = escape_html(&current),
        ),
    };
    format!(
        r#"<label><span>{label}</span>{input}</label>"#,
        label = escape_html(f.name),
        input = input,
    )
}

fn escape_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

const ADMIN_CSS: &str = r#"
*, *::before, *::after { box-sizing: border-box; }
body { font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
  background: #fafafa; color: #222; margin: 0; }
header { background: #222; color: white; padding: 1rem 2rem; }
header h1 { margin: 0; font-size: 1.1rem; font-weight: 600; letter-spacing: 0.02em; }
header h1 a { color: inherit; text-decoration: none; }
header h1 a:hover { opacity: 0.9; }
ul.admin-index { list-style: none; padding: 0; margin: 0; display: grid; gap: 0.5rem; }
ul.admin-index li { background: white; border-radius: 6px; box-shadow: 0 1px 3px rgba(0,0,0,0.04); }
ul.admin-index li a { display: flex; justify-content: space-between; align-items: center; padding: 0.9rem 1.1rem; text-decoration: none; color: #222; }
ul.admin-index li a:hover { background: #f4f4f5; }
ul.admin-index li .label { font-weight: 600; }
ul.admin-index li .path { color: #888; font-family: ui-monospace, SFMono-Regular, Menlo, monospace; font-size: 0.85rem; }
p.empty { color: #666; }
p.empty code { background: #f0f0f2; padding: 0.1rem 0.35rem; border-radius: 3px; font-size: 0.9em; }
main { padding: 2rem; max-width: 60rem; margin: 0 auto; }
h2 { margin: 0; }
.toolbar { display: flex; align-items: center; justify-content: space-between; margin-bottom: 1.5rem; }
table { border-collapse: collapse; width: 100%; background: white; border-radius: 6px; overflow: hidden;
  box-shadow: 0 1px 3px rgba(0,0,0,0.04); }
th, td { text-align: left; padding: 0.6rem 0.9rem; border-bottom: 1px solid #eee; font-size: 0.95rem; }
th { background: #f4f4f5; font-weight: 600; }
tbody tr:last-child td { border-bottom: none; }
td.actions { display: flex; gap: 0.5rem; align-items: center; }
td.actions form { margin: 0; display: inline; }
a { color: #0366d6; text-decoration: none; }
a:hover { text-decoration: underline; }
label { display: block; margin-bottom: 1rem; }
label span { display: block; font-weight: 500; margin-bottom: 0.25rem; font-size: 0.9rem; }
input[type=text], input[type=number] { padding: 0.5rem 0.75rem; border: 1px solid #d0d0d4;
  border-radius: 4px; width: 24rem; max-width: 100%; font: inherit; }
input[type=checkbox] { transform: scale(1.1); }
button, .button { padding: 0.5rem 1rem; background: #222; color: white; border: none;
  border-radius: 4px; cursor: pointer; font: inherit; text-decoration: none; display: inline-block; }
button:hover, .button:hover { background: #000; text-decoration: none; }
button.danger { background: #b42318; }
button.danger:hover { background: #8a1c12; }
.form-actions { display: flex; gap: 0.5rem; align-items: center; margin-top: 1rem; }
.form-actions .cancel { color: #666; }
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_html_escapes_dangerous_chars() {
        assert_eq!(
            escape_html("<script>alert(\"xss\")</script>"),
            "&lt;script&gt;alert(&quot;xss&quot;)&lt;/script&gt;"
        );
        assert_eq!(escape_html("a & b"), "a &amp; b");
        assert_eq!(escape_html("it's"), "it&#39;s");
    }
}
