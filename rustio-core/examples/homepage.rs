use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use rustio_core::auth::{authenticate, require_admin, require_auth};
use rustio_core::defaults::with_defaults;
use rustio_core::{resolve, text, Error, Next, Request, Response, Router, Server};

#[derive(Debug)]
struct RequestId(u64);

static REQUEST_COUNTER: AtomicU64 = AtomicU64::new(0);

async fn request_id(mut req: Request, next: Next) -> Result<Response, Error> {
    let id = REQUEST_COUNTER.fetch_add(1, Ordering::Relaxed);
    req.ctx_mut().insert(RequestId(id));
    let mut resp = resolve(next.run(req).await);
    if let Ok(header) = format!("req-{id}").parse() {
        resp.headers_mut().insert("x-request-id", header);
    }
    Ok(resp)
}

async fn logger(req: Request, next: Next) -> Result<Response, Error> {
    let method = req.method().clone();
    let path = req.uri().path().to_owned();
    let id = req.ctx().get::<RequestId>().map(|r| r.0);
    let user = rustio_core::auth::identity(req.ctx()).map(|i| i.user_id.clone());
    let started = Instant::now();
    let result = next.run(req).await;
    let status = match &result {
        Ok(resp) => resp.status().as_u16(),
        Err(err) => err.status(),
    };
    let id_display = id.map(|i| format!("req-{i}")).unwrap_or_else(|| "-".into());
    let user_display = user.unwrap_or_else(|| "-".into());
    eprintln!(
        "[{:>3}] {:>4} {} id={} user={} ({:?})",
        status,
        method,
        path,
        id_display,
        user_display,
        started.elapsed()
    );
    result
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let addr: SocketAddr = ([127, 0, 0, 1], 3000).into();
    let router = with_defaults(Router::new())
        .get("/whoami", |req, _params| async move {
            let id = req
                .ctx()
                .get::<RequestId>()
                .map(|r| r.0.to_string())
                .unwrap_or_else(|| "unknown".into());
            Ok::<Response, Error>(text(format!("your request id is req-{id}\n")))
        })
        .get("/me", |req, _params| async move {
            let id = require_auth(req.ctx())?;
            Ok::<Response, Error>(text(format!("hello {}\n", id.user_id)))
        })
        .get("/admin-only", |req, _params| async move {
            let id = require_admin(req.ctx())?;
            Ok::<Response, Error>(text(format!("hello admin {}\n", id.user_id)))
        })
        .get("/crash", |_req, _params| async {
            Err::<Response, Error>(Error::Internal("simulated failure".into()))
        })
        .get("/unauth", |_req, _params| async {
            Err::<Response, Error>(Error::Unauthorized)
        })
        .wrap(request_id)
        .wrap(authenticate)
        .wrap(logger);
    Server::bind(addr).serve_router(router).await
}
