//! Navigation-level regression cover for the admin's CRUD form routes.
//!
//! The existing `admin_form_csrf.rs` asks for `/admin/notes/new` by name. That
//! proves the route exists; it cannot prove the product links to it. A form
//! route and a rendered `href` can drift apart — the admin has carried two
//! form-route contracts at once (`/admin/<model>/new` from the templated
//! engine, `/admin/<model>/create` from `mount_model`) — and every
//! hand-addressed test keeps passing while every visible button 404s.
//!
//! So these tests never type a form URL. They fetch the list page, pull the
//! `href` out of the rendered "Add" button and the first row's "Edit" link,
//! and follow exactly those. If a template starts emitting a URL the router
//! does not serve, this is the test that fails.
//!
//! Registration deliberately matches what `rustio init` generates and what
//! `examples/bookflow` runs — `Admin::new().model::<T>().register(router,
//! &db)` — so the route composition under test is the one users get, not a
//! hand-built router that skips it.
//!
//! Socket-level client, mirroring `admin_form_csrf.rs`, so the test stays
//! within the crate's existing dependency set.

use std::net::SocketAddr;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use rustio_core::admin::Admin;
use rustio_core::auth::{self, authenticate, ROLE_ADMIN, ROLE_USER};
use rustio_core::defaults::with_defaults;
use rustio_core::{Db, Error, Model, Router, Row, RustioAdmin, Server, Value};

#[derive(Debug, RustioAdmin)]
struct Note {
    id: i64,
    title: String,
    priority: i32,
}

impl Model for Note {
    const TABLE: &'static str = "notes";
    const COLUMNS: &'static [&'static str] = &["id", "title", "priority"];
    const INSERT_COLUMNS: &'static [&'static str] = &["title", "priority"];
    fn id(&self) -> i64 {
        self.id
    }
    fn from_row(row: Row<'_>) -> Result<Self, Error> {
        Ok(Self {
            id: row.get_i64("id")?,
            title: row.get_string("title")?,
            priority: row.get_i64("priority")? as i32,
        })
    }
    fn insert_values(&self) -> Vec<Value> {
        vec![self.title.clone().into(), (self.priority as i64).into()]
    }
}

/// A server registered the way a generated project registers: one `Admin`,
/// `.model::<T>()` per model, one `.register(...)`. Seeded with a row so the
/// list page has an Edit link to extract.
async fn spawn_server() -> (SocketAddr, Db) {
    let db = Db::memory().await.expect("db");
    auth::ensure_core_tables(&db).await.expect("core tables");
    auth::user::create(&db, "admin@example.com", "hunter2", ROLE_ADMIN)
        .await
        .expect("seed admin");
    auth::user::create(&db, "viewer@example.com", "hunter2", ROLE_USER)
        .await
        .expect("seed viewer");
    db.execute(
        "CREATE TABLE notes (id INTEGER PRIMARY KEY AUTOINCREMENT, \
         title TEXT NOT NULL, priority INTEGER NOT NULL)",
    )
    .await
    .expect("notes table");
    db.execute("INSERT INTO notes (title, priority) VALUES ('Seeded', 1)")
        .await
        .expect("seed row");

    let router = with_defaults(Router::new()).wrap(authenticate(db.clone()));
    let router = Admin::new().model::<Note>().register(router, &db);

    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    tokio::spawn(async move {
        let _ = Server::serve_router_on(listener, router).await;
    });
    tokio::time::sleep(Duration::from_millis(20)).await;
    (addr, db)
}

async fn send(addr: SocketAddr, request: &str) -> String {
    let mut stream = TcpStream::connect(addr).await.expect("connect");
    let _ = stream.write_all(request.as_bytes()).await;
    let mut buf = Vec::new();
    let _ = stream.read_to_end(&mut buf).await;
    String::from_utf8_lossy(&buf).into_owned()
}

fn status_of(resp: &str) -> u16 {
    resp.lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| panic!("no status in:\n{resp}"))
}

fn location_of(resp: &str) -> Option<String> {
    let headers_end = resp.find("\r\n\r\n").unwrap_or(resp.len());
    resp[..headers_end].lines().find_map(|l| {
        let l = l.trim();
        l.strip_prefix("location: ")
            .or_else(|| l.strip_prefix("Location: "))
            .map(str::to_string)
    })
}

fn extract_cookie(resp: &str, name: &str) -> Option<String> {
    let headers_end = resp.find("\r\n\r\n").unwrap_or(resp.len());
    for line in resp[..headers_end].lines() {
        let line = line.trim();
        let Some(value) = line
            .strip_prefix("set-cookie: ")
            .or_else(|| line.strip_prefix("Set-Cookie: "))
        else {
            continue;
        };
        if let Some((k, v)) = value.split(';').next().and_then(|p| p.split_once('=')) {
            if k == name {
                return Some(v.to_string());
            }
        }
    }
    None
}

fn extract_csrf(html: &str) -> Option<String> {
    let mut found = None;
    for input in html.split("<input") {
        if !input.contains(r#"name="_csrf""#) {
            continue;
        }
        if let Some(start) = input.find("value=\"") {
            let rest = &input[start + "value=\"".len()..];
            if let Some(end) = rest.find('"') {
                found = Some(rest[..end].to_string());
            }
        }
    }
    found
}

fn get(path: &str, cookie: &str) -> String {
    format!(
        "GET {path} HTTP/1.1\r\nHost: t\r\nConnection: close\r\nCookie: rustio_session={cookie}\r\n\r\n"
    )
}

fn post(path: &str, body: &str, cookie: &str) -> String {
    format!(
        "POST {path} HTTP/1.1\r\nHost: t\r\nConnection: close\r\n\
         Cookie: rustio_session={cookie}\r\n\
         Content-Type: application/x-www-form-urlencoded\r\nContent-Length: {len}\r\n\r\n{body}",
        len = body.len(),
    )
}

async fn login_as(addr: SocketAddr, email: &str) -> String {
    let body = format!("email={email}&password=hunter2");
    let resp = send(
        addr,
        &format!(
            "POST /admin/login HTTP/1.1\r\nHost: t\r\nConnection: close\r\n\
             Content-Type: application/x-www-form-urlencoded\r\nContent-Length: {len}\r\n\r\n{body}",
            len = body.len(),
        ),
    )
    .await;
    assert_eq!(status_of(&resp), 303, "login should redirect:\n{resp}");
    extract_cookie(&resp, "rustio_session").expect("session cookie")
}

/// The `href` of the list page's primary "Add" action, as rendered.
fn add_href(list_html: &str) -> String {
    list_html
        .split("<a ")
        .find(|a| a.contains("button-primary") && a.contains("href=\"/admin/"))
        .and_then(|a| a.split_once("href=\"").map(|(_, r)| r))
        .and_then(|r| r.split_once('"').map(|(h, _)| h.to_string()))
        .unwrap_or_else(|| panic!("no Add link on the list page:\n{list_html}"))
}

/// The `href` of the first row's "Edit" action, as rendered.
fn edit_href(list_html: &str) -> String {
    list_html
        .split("<a ")
        .filter_map(|a| {
            let (_, rest) = a.split_once("href=\"")?;
            let (h, _) = rest.split_once('"')?;
            h.ends_with("/edit").then(|| h.to_string())
        })
        .next()
        .unwrap_or_else(|| panic!("no Edit link on the list page:\n{list_html}"))
}

/// The regression this file exists for: the links the admin actually renders
/// must resolve to real forms. Nothing here types a form URL.
#[tokio::test]
async fn rendered_add_and_edit_links_resolve_to_forms() {
    let (addr, _db) = spawn_server().await;
    let cookie = login_as(addr, "admin@example.com").await;

    let list = send(addr, &get("/admin/notes", &cookie)).await;
    assert_eq!(status_of(&list), 200, "list page should render:\n{list}");

    let add = add_href(&list);
    let edit = edit_href(&list);

    let add_page = send(addr, &get(&add, &cookie)).await;
    assert_eq!(
        status_of(&add_page),
        200,
        "the rendered Add link {add} must resolve to a form, not {}:\n{add_page}",
        status_of(&add_page)
    );
    assert!(
        add_page.contains(r#"name="title""#) && add_page.contains(r#"name="priority""#),
        "the Add form renders the model's fields:\n{add_page}"
    );
    assert!(
        add_page.contains(r#"name="_csrf""#),
        "the Add form carries a CSRF token:\n{add_page}"
    );

    let edit_page = send(addr, &get(&edit, &cookie)).await;
    assert_eq!(
        status_of(&edit_page),
        200,
        "the rendered Edit link {edit} must resolve to a form:\n{edit_page}"
    );
    assert!(
        edit_page.contains("Seeded"),
        "the Edit form is populated from the row:\n{edit_page}"
    );
    assert!(
        edit_page.contains(r#"name="_csrf""#),
        "the Edit form carries a CSRF token:\n{edit_page}"
    );
}

/// Both form-route contracts stay served. `/new` is canonical — it is what the
/// templates render and what `rendered_add_and_edit_links_resolve_to_forms`
/// follows — and `/create` is the compatibility alias kept for projects
/// generated before the templated engine landed. Neither may quietly lapse:
/// dropping `/create` breaks an older project's bookmarks and its own
/// generated links, and dropping `/new` breaks every page this admin renders.
#[tokio::test]
async fn both_form_route_contracts_are_served() {
    let (addr, _db) = spawn_server().await;
    let cookie = login_as(addr, "admin@example.com").await;

    let canonical = send(addr, &get("/admin/notes/new", &cookie)).await;
    assert_eq!(
        status_of(&canonical),
        200,
        "/admin/notes/new is the canonical create route:\n{canonical}"
    );

    let alias = send(addr, &get("/admin/notes/create", &cookie)).await;
    assert_eq!(
        status_of(&alias),
        200,
        "/admin/notes/create is kept as a compatibility alias:\n{alias}"
    );
}

/// The failure modes around the form routes stay distinct: an unknown model is
/// 404, an unknown record is 404, and a real model a user may not create is
/// 403 — never a 404 that hides the permission decision.
#[tokio::test]
async fn unknown_model_record_and_denied_permission_keep_their_statuses() {
    let (addr, _db) = spawn_server().await;
    let cookie = login_as(addr, "admin@example.com").await;

    for path in ["/admin/widgets/new", "/admin/widgets/1/edit"] {
        let resp = send(addr, &get(path, &cookie)).await;
        assert_eq!(
            status_of(&resp),
            404,
            "unknown model {path} is 404:\n{resp}"
        );
    }

    // A row that is not there, and an id that could never be a row.
    for path in ["/admin/notes/999999/edit", "/admin/notes/not-an-id/edit"] {
        let missing = send(addr, &get(path, &cookie)).await;
        assert_eq!(
            status_of(&missing),
            404,
            "unknown record {path} is 404, not a blank form:\n{missing}"
        );
    }

    // The POST side answers the same way: an UPDATE against a missing row
    // touches nothing and returns Ok, so the handler has to check first
    // rather than redirect as though the save succeeded.
    let form_page = send(addr, &get("/admin/notes/1/edit", &cookie)).await;
    let csrf = extract_csrf(&form_page).expect("csrf on a real edit form");
    let ghost = send(
        addr,
        &post(
            "/admin/notes/999999/edit",
            &format!("title=Ghost&priority=1&_csrf={csrf}"),
            &cookie,
        ),
    )
    .await;
    assert_eq!(
        status_of(&ghost),
        404,
        "saving a record that does not exist is 404, not a 303:\n{ghost}"
    );

    // A viewer has no create permission; the gate answers 403 by URL, not just
    // by hiding the button.
    let viewer = login_as(addr, "viewer@example.com").await;
    let denied = send(addr, &get("/admin/notes/new", &viewer)).await;
    assert_eq!(
        status_of(&denied),
        403,
        "a user without create permission gets 403, not 404:\n{denied}"
    );
}

/// TASK 4 — the POST half of the contract, driven from the rendered forms:
/// create through the Add form, follow the PRG redirect, prove the row exists,
/// then edit it through its own Edit form and prove the value changed.
#[tokio::test]
async fn create_and_edit_round_trip_through_the_rendered_forms() {
    let (addr, db) = spawn_server().await;
    let cookie = login_as(addr, "admin@example.com").await;

    // --- create -------------------------------------------------------
    let list = send(addr, &get("/admin/notes", &cookie)).await;
    let add = add_href(&list);
    let add_page = send(addr, &get(&add, &cookie)).await;
    let csrf = extract_csrf(&add_page).expect("csrf on the Add form");

    let created = send(
        addr,
        &post(
            &add,
            &format!("title=Fresh&priority=7&_csrf={csrf}"),
            &cookie,
        ),
    )
    .await;
    assert_eq!(
        status_of(&created),
        303,
        "a valid create redirects (PRG):\n{created}"
    );
    let back = location_of(&created).expect("PRG redirect carries a Location");
    assert!(
        back.starts_with("/admin/notes"),
        "the create redirect returns to the list, got {back}"
    );

    let rows = Note::all(&db).await.expect("query notes");
    let fresh = rows
        .iter()
        .find(|n| n.title == "Fresh")
        .expect("the created row exists");
    assert_eq!(fresh.priority, 7, "the submitted value was stored");

    // --- edit ---------------------------------------------------------
    // Take the Edit link the list renders for the row we just created.
    let list = send(addr, &get("/admin/notes", &cookie)).await;
    let edit = list
        .split("<a ")
        .filter_map(|a| {
            let (_, rest) = a.split_once("href=\"")?;
            let (h, _) = rest.split_once('"')?;
            h.ends_with(&format!("/{}/edit", fresh.id))
                .then(|| h.to_string())
        })
        .next()
        .unwrap_or_else(|| panic!("no Edit link for the new row:\n{list}"));

    let edit_page = send(addr, &get(&edit, &cookie)).await;
    assert_eq!(
        status_of(&edit_page),
        200,
        "edit form renders:\n{edit_page}"
    );
    let csrf = extract_csrf(&edit_page).expect("csrf on the Edit form");

    let saved = send(
        addr,
        &post(
            &edit,
            &format!("title=Renamed&priority=9&_csrf={csrf}"),
            &cookie,
        ),
    )
    .await;
    assert_eq!(status_of(&saved), 303, "a valid edit redirects:\n{saved}");
    assert!(
        location_of(&saved)
            .unwrap_or_default()
            .starts_with("/admin/notes"),
        "the edit redirect returns to the list"
    );

    let after = Note::find(&db, fresh.id)
        .await
        .expect("query")
        .expect("the row still exists");
    assert_eq!(after.title, "Renamed", "the edit was persisted");
    assert_eq!(after.priority, 9, "every submitted field was persisted");
}
