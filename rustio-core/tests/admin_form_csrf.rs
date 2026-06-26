//! SF-1 regression: the templated admin **create** (`form.html`) and
//! **delete** (`list.html`) forms must render the `_csrf` token, and their
//! handlers must enforce it.
//!
//! Drives the real HTTP handler path end to end (no `Request` is
//! hand-constructed): a tiny `Note` model is registered, its table created,
//! an admin logs in, and the create/delete actions are exercised with a
//! valid token (succeeds), no token (403), and a wrong token (403).
//!
//! Socket-level client, mirroring `login_flow.rs`, so the test stays within
//! the crate's existing dependency set.

use std::net::SocketAddr;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use rustio_core::admin::Admin;
use rustio_core::auth::{self, authenticate, ROLE_ADMIN, ROLE_USER};
use rustio_core::defaults::with_defaults;
use rustio_core::{Db, Error, Model, Router, Row, RustioAdmin, Server, Value};

// A minimal CRUD model so the dynamic `/admin/notes/...` routes exist.
#[derive(Debug, RustioAdmin)]
struct Note {
    id: i64,
    title: String,
}

impl Model for Note {
    const TABLE: &'static str = "notes";
    const COLUMNS: &'static [&'static str] = &["id", "title"];
    const INSERT_COLUMNS: &'static [&'static str] = &["title"];
    fn id(&self) -> i64 {
        self.id
    }
    fn from_row(row: Row<'_>) -> Result<Self, Error> {
        Ok(Self {
            id: row.get_i64("id")?,
            title: row.get_string("title")?,
        })
    }
    fn insert_values(&self) -> Vec<Value> {
        vec![self.title.clone().into()]
    }
}

/// Spin up a server with the core auth tables, an admin user, and the
/// `Note` model + its table. Returns the address to dial.
async fn spawn_server() -> SocketAddr {
    let db = Db::memory().await.expect("db");
    auth::ensure_core_tables(&db).await.expect("core tables");
    auth::user::create(&db, "admin@example.com", "hunter2", ROLE_ADMIN)
        .await
        .expect("seed admin");
    auth::user::create(&db, "viewer@example.com", "hunter2", ROLE_USER)
        .await
        .expect("seed non-admin viewer");
    db.execute("CREATE TABLE notes (id INTEGER PRIMARY KEY AUTOINCREMENT, title TEXT NOT NULL)")
        .await
        .expect("notes table");

    let router = with_defaults(Router::new()).wrap(authenticate(db.clone()));
    let router = Admin::new().model::<Note>().register(router, &db);

    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    tokio::spawn(async move {
        let _ = Server::serve_router_on(listener, router).await;
    });
    tokio::time::sleep(Duration::from_millis(20)).await;
    addr
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

/// Scrape the **last** `_csrf` value on the page. Pages now carry several
/// (header logout, the action form, …); all are the same session token, so
/// any match works — taking the last keeps us robust if ordering changes.
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

fn get_with_cookie(path: &str, cookie: &str) -> String {
    format!(
        "GET {path} HTTP/1.1\r\nHost: t\r\nConnection: close\r\nCookie: rustio_session={cookie}\r\n\r\n"
    )
}

fn post_with_cookie(path: &str, body: &str, cookie: &str) -> String {
    format!(
        "POST {path} HTTP/1.1\r\nHost: t\r\nConnection: close\r\n\
         Cookie: rustio_session={cookie}\r\n\
         Content-Type: application/x-www-form-urlencoded\r\nContent-Length: {len}\r\n\r\n{body}",
        len = body.len(),
    )
}

/// Log in with the given credentials and return the session cookie.
async fn login_as(addr: SocketAddr, email: &str, password: &str) -> String {
    let body = format!("email={email}&password={password}");
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

/// Log in as the seeded admin.
async fn login(addr: SocketAddr) -> String {
    login_as(addr, "admin@example.com", "hunter2").await
}

#[tokio::test]
async fn create_form_renders_csrf_and_enforces_it() {
    let addr = spawn_server().await;
    let cookie = login(addr).await;

    // The new-record form now renders the CSRF token.
    let form_page = send(addr, &get_with_cookie("/admin/notes/new", &cookie)).await;
    assert_eq!(
        status_of(&form_page),
        200,
        "new form should render:\n{form_page}"
    );
    assert!(
        form_page.contains(r#"name="_csrf""#),
        "create form must render the _csrf token:\n{form_page}"
    );
    let csrf = extract_csrf(&form_page).expect("csrf token on the new-record page");

    // WITHOUT a token → 403 (and nothing created).
    let no_tok = send(
        addr,
        &post_with_cookie("/admin/notes/new", "title=NoToken", &cookie),
    )
    .await;
    assert_eq!(
        status_of(&no_tok),
        403,
        "create without _csrf must be rejected:\n{no_tok}"
    );

    // WRONG token → 403.
    let wrong = send(
        addr,
        &post_with_cookie("/admin/notes/new", "title=WrongTok&_csrf=deadbeef", &cookie),
    )
    .await;
    assert_eq!(
        status_of(&wrong),
        403,
        "create with a wrong _csrf must be rejected"
    );

    // WITH the valid token → reaches the action (303 redirect).
    let ok = send(
        addr,
        &post_with_cookie(
            "/admin/notes/new",
            &format!("title=Created&_csrf={csrf}"),
            &cookie,
        ),
    )
    .await;
    assert_eq!(
        status_of(&ok),
        303,
        "create with a valid _csrf must succeed:\n{ok}"
    );

    // Confirm the row exists (and the rejected ones did not write).
    let list = send(addr, &get_with_cookie("/admin/notes", &cookie)).await;
    assert!(
        list.contains("Created"),
        "the created row must appear:\n{list}"
    );
    assert!(
        !list.contains("NoToken"),
        "the no-token POST must not have written"
    );
    assert!(
        !list.contains("WrongTok"),
        "the wrong-token POST must not have written"
    );
}

#[tokio::test]
async fn delete_form_renders_csrf_and_enforces_it() {
    let addr = spawn_server().await;
    let cookie = login(addr).await;

    // Seed one row through the (now CSRF-correct) create action.
    let form_page = send(addr, &get_with_cookie("/admin/notes/new", &cookie)).await;
    let csrf = extract_csrf(&form_page).expect("csrf");
    let created = send(
        addr,
        &post_with_cookie(
            "/admin/notes/new",
            &format!("title=DeleteMe&_csrf={csrf}"),
            &cookie,
        ),
    )
    .await;
    assert_eq!(status_of(&created), 303);

    // The list page's delete form renders the CSRF token.
    let list = send(addr, &get_with_cookie("/admin/notes", &cookie)).await;
    assert!(
        list.contains("DeleteMe"),
        "seeded row should be listed:\n{list}"
    );
    assert!(
        list.contains(r#"action="/admin/notes/1/delete""#),
        "delete form should target the row:\n{list}"
    );
    assert!(
        list.contains(r#"name="_csrf""#),
        "delete form must render the _csrf token:\n{list}"
    );
    let csrf = extract_csrf(&list).expect("csrf on the list page");

    // Delete WITHOUT a token → 403; row survives.
    let no_tok = send(
        addr,
        &post_with_cookie("/admin/notes/1/delete", "", &cookie),
    )
    .await;
    assert_eq!(
        status_of(&no_tok),
        403,
        "delete without _csrf must be rejected:\n{no_tok}"
    );
    let still = send(addr, &get_with_cookie("/admin/notes", &cookie)).await;
    assert!(
        still.contains("DeleteMe"),
        "row must survive a rejected delete"
    );

    // Delete WITH the valid token → 303; row gone.
    let ok = send(
        addr,
        &post_with_cookie("/admin/notes/1/delete", &format!("_csrf={csrf}"), &cookie),
    )
    .await;
    assert_eq!(
        status_of(&ok),
        303,
        "delete with a valid _csrf must succeed:\n{ok}"
    );
    let gone = send(addr, &get_with_cookie("/admin/notes", &cookie)).await;
    assert!(!gone.contains("DeleteMe"), "row must be deleted:\n{gone}");
}

/// Phase 9a — the composition editor's GET renders the CSRF token, and the
/// save POST is CSRF-protected, edit-gated, and rejects an unparseable role
/// without writing anything.
#[tokio::test]
async fn view_editor_csrf_permission_and_reject() {
    // Any stale view file from a previous run would confuse the no-write
    // assertion; the save here is exercised only on the rejection path,
    // which must NOT create the file.
    let _ = std::fs::remove_file("note.view.json");

    let addr = spawn_server().await;
    let cookie = login(addr).await;

    // 1. GET editor → 200, renders _csrf and a role <select> per field.
    let editor = send(addr, &get_with_cookie("/admin/notes/view", &cookie)).await;
    assert_eq!(status_of(&editor), 200, "editor should render:\n{editor}");
    assert!(
        editor.contains(r#"name="_csrf""#),
        "editor must render the _csrf token"
    );
    assert!(
        editor.contains(r#"name="role[title]""#),
        "editor must render a role select for the `title` field:\n{editor}"
    );
    // Phase 9b: the reorder controls (order index + move buttons) render.
    assert!(
        editor.contains(r#"name="order[title]""#),
        "editor must render an order input per field:\n{editor}"
    );
    assert!(
        editor.contains(r#"data-move="up""#) && editor.contains(r#"data-move="down""#),
        "editor must render up/down reorder buttons"
    );
    let csrf = extract_csrf(&editor).expect("csrf on the editor page");

    // 2. POST save WITHOUT _csrf → 403.
    let no_tok = send(
        addr,
        &post_with_cookie("/admin/notes/view", "role[title]=badge", &cookie),
    )
    .await;
    assert_eq!(
        status_of(&no_tok),
        403,
        "save without _csrf must be rejected:\n{no_tok}"
    );

    // 3. POST save with a VALID token but an UNPARSEABLE role → re-render
    //    the editor (200) with an error, and write nothing.
    let bad = send(
        addr,
        &post_with_cookie(
            "/admin/notes/view",
            &format!("role[title]=banana&_csrf={csrf}"),
            &cookie,
        ),
    )
    .await;
    assert_eq!(
        status_of(&bad),
        200,
        "an unparseable role re-renders the editor"
    );
    assert!(
        bad.contains("Not saved"),
        "the error banner must show:\n{bad}"
    );
    assert!(
        !std::path::Path::new("note.view.json").exists(),
        "a rejected save must write no file"
    );

    // 4. Permission: a non-admin (viewer) is rejected on BOTH the editor GET
    //    and the save POST (defense in depth — not just UI hiding).
    let viewer = login_as(addr, "viewer@example.com", "hunter2").await;
    let v_get = send(addr, &get_with_cookie("/admin/notes/view", &viewer)).await;
    assert_eq!(
        status_of(&v_get),
        403,
        "a non-admin must not reach the editor:\n{v_get}"
    );
    let v_post = send(
        addr,
        &post_with_cookie("/admin/notes/view", "role[title]=badge", &viewer),
    )
    .await;
    assert_eq!(
        status_of(&v_post),
        403,
        "a non-admin save must be rejected:\n{v_post}"
    );

    // Final safety: nothing was ever written by this test.
    assert!(!std::path::Path::new("note.view.json").exists());
}
