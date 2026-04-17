//! Default routes that scaffolded projects mount via [`with_defaults`]:
//! `/` (homepage) and `/docs` (placeholder).
//!
//! `/admin` is intentionally **not** registered here — it is owned by the
//! admin layer (see [`crate::admin::Admin::register`]). If no admin models
//! are registered, `/admin` is simply absent.

use crate::error::Error;
use crate::http::{html, text, Request, Response};
use crate::router::{Params, Router};

const HOME_HTML: &str = include_str!("../assets/home.html");

pub fn homepage() -> Response {
    html(HOME_HTML)
}

pub fn docs_placeholder() -> Response {
    text("RustIO docs — coming soon.")
}

pub fn with_defaults(router: Router) -> Router {
    router
        .get("/", |_req: Request, _p: Params| async {
            Ok::<Response, Error>(homepage())
        })
        .get("/docs", |_req: Request, _p: Params| async {
            Ok::<Response, Error>(docs_placeholder())
        })
}
