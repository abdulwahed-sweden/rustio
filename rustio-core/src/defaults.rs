use crate::error::Error;
use crate::http::{Request, Response, html, text};
use crate::router::{Params, Router};

const HOME_HTML: &str = include_str!("../assets/home.html");

pub fn homepage() -> Response {
    html(HOME_HTML)
}

pub fn admin_placeholder() -> Response {
    text("RustIO admin — coming soon.")
}

pub fn docs_placeholder() -> Response {
    text("RustIO docs — coming soon.")
}

pub fn with_defaults(router: Router) -> Router {
    router
        .get("/", |_req: Request, _p: Params| async {
            Ok::<Response, Error>(homepage())
        })
        .get("/admin", |_req: Request, _p: Params| async {
            Ok::<Response, Error>(admin_placeholder())
        })
        .get("/docs", |_req: Request, _p: Params| async {
            Ok::<Response, Error>(docs_placeholder())
        })
}
