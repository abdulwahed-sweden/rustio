//! End-to-end integration test for the login → cookie → authenticated
//! request cycle.
//!
//! Spins up a full hyper server on a kernel-assigned port, speaks raw
//! HTTP/1.1 over a TCP socket, and asserts the observable surface:
//! 401 before login, 303 with a session cookie on success, 200 on a
//! subsequent authenticated request, 401 after logout, and 413 for
//! an oversized form body.
//!
//! Written at the socket layer (no hyper-client dep) so the test stays
//! self-contained against the crate's existing dependency set.

use std::net::SocketAddr;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use rustio_core::admin::Admin;
use rustio_core::auth::{self, authenticate, ROLE_ADMIN};
use rustio_core::defaults::with_defaults;
use rustio_core::{Db, Router, Server};

/// Spin up a router backed by an in-memory DB with one admin user,
/// bind it to 127.0.0.1:0, and spawn the hyper accept loop on a
/// background tokio task. Returns the socket address the test client
/// should dial.
async fn spawn_test_server() -> SocketAddr {
    let db = Db::memory().await.expect("db");
    auth::ensure_core_tables(&db).await.expect("tables");
    auth::user::create(&db, "admin@example.com", "hunter2", ROLE_ADMIN)
        .await
        .expect("seed admin");

    let router = with_defaults(Router::new()).wrap(authenticate(db.clone()));
    let router = Admin::new().register(router, &db);

    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local_addr");

    tokio::spawn(async move {
        let _ = Server::serve_router_on(listener, router).await;
    });

    // Small pause so the accept loop is polling before the first
    // test client connect.
    tokio::time::sleep(Duration::from_millis(20)).await;

    addr
}

/// Send one HTTP request and return the full raw response bytes as a
/// string. Uses `Connection: close` so the server ends the stream
/// after the response and `read_to_end` terminates.
///
/// Write errors are tolerated: when the server rejects a large body
/// with 413, it closes the connection before the client has finished
/// writing, producing a broken-pipe on the write side. What matters is
/// whatever response bytes we can still read from the socket.
async fn send(addr: SocketAddr, request: &str) -> String {
    let mut stream = TcpStream::connect(addr).await.expect("connect");
    let _ = stream.write_all(request.as_bytes()).await;
    let mut buf = Vec::new();
    let _ = stream.read_to_end(&mut buf).await;
    String::from_utf8_lossy(&buf).into_owned()
}

/// Parse the status code out of an HTTP/1.1 response.
fn status_of(resp: &str) -> u16 {
    let first = resp.lines().next().expect("response is empty");
    first
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or_else(|| panic!("could not parse status from: {first}"))
}

/// Extract a specific cookie value from a response's `Set-Cookie`
/// header. Returns `None` if absent.
fn extract_cookie(resp: &str, name: &str) -> Option<String> {
    // Response headers end at the first blank line; only scan headers
    // so a body that happens to contain "set-cookie" isn't picked up.
    let headers_end = resp.find("\r\n\r\n").unwrap_or(resp.len());
    let headers = &resp[..headers_end];
    for line in headers.lines() {
        let line = line.trim();
        let Some(value) = line
            .strip_prefix("set-cookie: ")
            .or_else(|| line.strip_prefix("Set-Cookie: "))
        else {
            continue;
        };
        // First `name=value` pair is the cookie; the rest are attrs.
        if let Some(first) = value.split(';').next() {
            if let Some((k, v)) = first.split_once('=') {
                if k == name {
                    return Some(v.to_string());
                }
            }
        }
    }
    None
}

fn form_post(path: &str, body: &str) -> String {
    format!(
        "POST {path} HTTP/1.1\r\n\
         Host: test.local\r\n\
         Connection: close\r\n\
         Content-Type: application/x-www-form-urlencoded\r\n\
         Content-Length: {len}\r\n\
         \r\n\
         {body}",
        len = body.len(),
    )
}

fn form_post_with_cookie(path: &str, body: &str, cookie: &str) -> String {
    format!(
        "POST {path} HTTP/1.1\r\n\
         Host: test.local\r\n\
         Connection: close\r\n\
         Cookie: rustio_session={cookie}\r\n\
         Content-Type: application/x-www-form-urlencoded\r\n\
         Content-Length: {len}\r\n\
         \r\n\
         {body}",
        len = body.len(),
    )
}

fn get_with_cookie(path: &str, cookie: &str) -> String {
    format!(
        "GET {path} HTTP/1.1\r\n\
         Host: test.local\r\n\
         Connection: close\r\n\
         Cookie: rustio_session={cookie}\r\n\
         \r\n"
    )
}

/// Scrape the `_csrf` hidden input out of a rendered admin page.
/// Matches `<input ... name="_csrf" value="HEX">` with either attribute
/// order. Returns `None` if no such input is found.
fn extract_csrf(html: &str) -> Option<String> {
    for input in html.split("<input") {
        if !input.contains(r#"name="_csrf""#) {
            continue;
        }
        // Find value="..."; accept any order of attributes.
        if let Some(value_start) = input.find("value=\"") {
            let rest = &input[value_start + "value=\"".len()..];
            if let Some(end) = rest.find('"') {
                return Some(rest[..end].to_string());
            }
        }
    }
    None
}

#[tokio::test]
async fn full_login_flow_admin_cookie_auth_logout() {
    let addr = spawn_test_server().await;

    // 1. Anonymous GET /admin → 401.
    let resp = send(
        addr,
        "GET /admin HTTP/1.1\r\nHost: test.local\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert_eq!(status_of(&resp), 401, "anonymous /admin must be 401");

    // 2. Wrong password → 401 with the generic message.
    let resp = send(
        addr,
        &form_post("/admin/login", "email=admin@example.com&password=WRONG"),
    )
    .await;
    assert_eq!(status_of(&resp), 401);
    assert!(
        resp.contains("Invalid email or password"),
        "wrong password must use the generic credential error"
    );

    // 3. Unknown email → also 401 with the same generic message,
    //    confirming no enumeration via response text.
    let resp = send(
        addr,
        &form_post("/admin/login", "email=ghost@example.com&password=whatever"),
    )
    .await;
    assert_eq!(status_of(&resp), 401);
    assert!(resp.contains("Invalid email or password"));

    // 4. Correct credentials → 303 + session cookie.
    let resp = send(
        addr,
        &form_post("/admin/login", "email=admin@example.com&password=hunter2"),
    )
    .await;
    assert_eq!(
        status_of(&resp),
        303,
        "successful login must redirect; response was:\n{resp}"
    );
    let token = extract_cookie(&resp, "rustio_session")
        .unwrap_or_else(|| panic!("session cookie not set; response was:\n{resp}"));
    assert!(!token.is_empty());
    assert!(
        resp.to_lowercase().contains("httponly"),
        "session cookie must be HttpOnly"
    );
    assert!(
        resp.contains("SameSite=Strict"),
        "session cookie must be SameSite=Strict"
    );

    // 5. GET /admin with cookie → 200. Scrape the CSRF token out of
    //    the rendered admin index so we can use it in logout below.
    let resp = send(addr, &get_with_cookie("/admin", &token)).await;
    assert_eq!(status_of(&resp), 200, "cookie must grant admin access");
    let csrf = extract_csrf(&resp).expect("admin index must render _csrf input");
    assert!(!csrf.is_empty());

    // 5a. Security headers must be present on the rendered admin
    //     index (dev mode — no HSTS).
    let lower = resp.to_lowercase();
    assert!(
        lower.contains("x-frame-options: deny"),
        "admin responses must ship X-Frame-Options: DENY"
    );
    assert!(
        lower.contains("x-content-type-options: nosniff"),
        "admin responses must ship X-Content-Type-Options: nosniff"
    );
    assert!(
        lower.contains("referrer-policy: no-referrer"),
        "admin responses must ship Referrer-Policy: no-referrer"
    );
    assert!(
        !lower.contains("strict-transport-security"),
        "dev mode must not emit HSTS"
    );

    // 6. POST /admin/logout without CSRF → 403 (new Pass D guarantee).
    let resp = send(addr, &form_post_with_cookie("/admin/logout", "", &token)).await;
    assert_eq!(
        status_of(&resp),
        403,
        "logout without CSRF must be rejected; response was:\n{resp}",
    );

    // 6a. POST /admin/logout WITH CSRF → 303, cookie expired.
    let logout_body = format!("_csrf={csrf}");
    let resp = send(
        addr,
        &form_post_with_cookie("/admin/logout", &logout_body, &token),
    )
    .await;
    assert_eq!(status_of(&resp), 303);
    assert!(
        resp.contains("Max-Age=0"),
        "logout must emit a Max-Age=0 cookie"
    );

    // 7. Replaying the old token after logout → 401.
    let resp = send(addr, &get_with_cookie("/admin", &token)).await;
    assert_eq!(
        status_of(&resp),
        401,
        "replayed token after logout must be rejected"
    );
}

#[tokio::test]
async fn oversized_form_body_returns_413() {
    let addr = spawn_test_server().await;

    // 3 MB body — above the 2 MB cap in `admin::MAX_FORM_BODY_BYTES`.
    let big = "a".repeat(3 * 1024 * 1024);
    let resp = send(addr, &form_post("/admin/login", &big)).await;
    assert_eq!(
        status_of(&resp),
        413,
        "oversized form bodies must be rejected with 413"
    );
}

// --- Pass D coverage -----------------------------------------------------

#[tokio::test]
async fn logout_without_csrf_returns_403() {
    // Guards against CSRF regressions: a valid session cookie is
    // not sufficient to log the user out. The POST body must carry
    // the session's `_csrf` token.
    let addr = spawn_test_server().await;

    // Log in to get a valid session cookie.
    let resp = send(
        addr,
        &form_post("/admin/login", "email=admin@example.com&password=hunter2"),
    )
    .await;
    assert_eq!(status_of(&resp), 303);
    let token = extract_cookie(&resp, "rustio_session").unwrap();

    // No _csrf in body → 403.
    let resp = send(addr, &form_post_with_cookie("/admin/logout", "", &token)).await;
    assert_eq!(
        status_of(&resp),
        403,
        "logout without _csrf must be 403; response was:\n{resp}",
    );

    // Wrong _csrf in body → 403.
    let resp = send(
        addr,
        &form_post_with_cookie("/admin/logout", "_csrf=not-the-token", &token),
    )
    .await;
    assert_eq!(
        status_of(&resp),
        403,
        "logout with wrong _csrf must be 403; response was:\n{resp}",
    );

    // The session must still be live — a rejected CSRF request does
    // not accidentally invalidate it.
    let resp = send(addr, &get_with_cookie("/admin", &token)).await;
    assert_eq!(
        status_of(&resp),
        200,
        "failed CSRF logout must not delete the underlying session",
    );
}

#[tokio::test]
async fn anonymous_post_admin_logout_is_rejected() {
    // An unauthenticated cross-origin POST — no session cookie, no
    // CSRF token — hits `require_csrf` with an empty expected token,
    // which verify_token rejects. 403 regardless of body content.
    let addr = spawn_test_server().await;
    let resp = send(addr, &form_post("/admin/logout", "_csrf=anything")).await;
    assert_eq!(
        status_of(&resp),
        403,
        "unauthenticated logout POST must be 403",
    );
}

#[tokio::test]
async fn global_body_limit_rejects_large_non_admin_post() {
    // The Pass D body-limit middleware sits at `with_defaults`, so a
    // 3 MB POST to a non-admin route (e.g. `/` which doesn't accept
    // bodies but still sees the request) must be rejected upfront.
    let addr = spawn_test_server().await;
    let big = "a".repeat(3 * 1024 * 1024);
    let resp = send(addr, &form_post("/", &big)).await;
    assert_eq!(
        status_of(&resp),
        413,
        "oversized body on ANY route must be 413"
    );
}

#[tokio::test]
async fn admin_response_headers_are_present() {
    // Dedicated assertion that every admin render ships the full
    // Pass D header set. The login-flow test already checks the
    // authenticated admin index; this repeats the check against the
    // 401 login page so the guard path is covered too.
    let addr = spawn_test_server().await;
    let resp = send(
        addr,
        "GET /admin HTTP/1.1\r\nHost: test.local\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert_eq!(status_of(&resp), 401);
    let lower = resp.to_lowercase();
    assert!(lower.contains("x-frame-options: deny"));
    assert!(lower.contains("x-content-type-options: nosniff"));
    assert!(lower.contains("referrer-policy: no-referrer"));
}

#[tokio::test]
async fn login_rate_limiter_triggers_lockout() {
    let addr = spawn_test_server().await;

    // The global limiter is process-wide; use an unusual email so
    // concurrent tests don't collide on the failure counter.
    let email = "ratelimit-probe@example.com";

    // First 5 failures should return 401 (generic credential error).
    for _ in 0..5 {
        let resp = send(
            addr,
            &form_post("/admin/login", &format!("email={email}&password=WRONG")),
        )
        .await;
        assert_eq!(status_of(&resp), 401);
    }

    // The 6th attempt should be rejected up-front with 429.
    let resp = send(
        addr,
        &form_post("/admin/login", &format!("email={email}&password=WRONG")),
    )
    .await;
    assert_eq!(
        status_of(&resp),
        429,
        "sixth failed attempt must trip the rate limiter"
    );
    assert!(resp.contains("Too many failed attempts"));
}
