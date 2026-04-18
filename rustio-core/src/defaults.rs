//! Default routes that scaffolded projects mount via [`with_defaults`]:
//! `/` (homepage) and `/docs` (placeholder).
//!
//! `/admin` is intentionally **not** registered here — it is owned by the
//! admin layer (see [`crate::admin::Admin::register`]). If no admin models
//! are registered, `/admin` is simply absent.

use crate::error::Error;
use crate::http::{html, text, Request, Response, MAX_REQUEST_BODY_BYTES};
use crate::middleware::Next;
use crate::router::{Params, Router};

const HOME_HTML: &str = include_str!("../assets/home.html");

pub fn homepage() -> Response {
    html(HOME_HTML)
}

pub fn docs_placeholder() -> Response {
    text("RustIO docs — coming soon.")
}

/// Reject requests whose `Content-Length` exceeds
/// [`MAX_REQUEST_BODY_BYTES`] before any handler runs.
///
/// This is a cheap upfront defence — clients that advertise a
/// multi-megabyte body are refused with HTTP 413 immediately. Clients
/// that under-report or use chunked transfer still pay the ceiling at
/// the body-reader layer (see `admin::read_form`, which wraps the body
/// in `http_body_util::Limited`). Both paths end in
/// `Error::PayloadTooLarge`.
///
/// `with_defaults` wraps every router with this middleware so custom
/// handlers that don't explicitly limit their bodies still benefit.
pub async fn body_limit(req: Request, next: Next) -> Result<Response, Error> {
    if let Some(header) = req.headers().get(hyper::header::CONTENT_LENGTH) {
        // A `Content-Length` header that doesn't parse is a malformed
        // request; the router's downstream body reader will reject it,
        // but we can also short-circuit here. We conservatively
        // *forward* on parse failure rather than rejecting — a bad
        // header is a 400 concern, not ours.
        if let Ok(s) = header.to_str() {
            if let Ok(n) = s.parse::<u64>() {
                if n as u128 > MAX_REQUEST_BODY_BYTES as u128 {
                    return Err(Error::PayloadTooLarge);
                }
            }
        }
    }
    next.run(req).await
}

pub fn with_defaults(router: Router) -> Router {
    // `wrap` adds middleware that runs on every request — so the
    // body-size cap applies uniformly to admin, user, and default
    // routes without each handler having to opt in.
    router
        .get("/", |_req: Request, _p: Params| async {
            Ok::<Response, Error>(homepage())
        })
        .get("/docs", |_req: Request, _p: Params| async {
            Ok::<Response, Error>(docs_placeholder())
        })
        .wrap(body_limit)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Re-parse a Content-Length value against the same check the
    /// middleware uses. This unit-level test avoids spinning up a
    /// server — the integration test in `tests/login_flow.rs` covers
    /// the end-to-end wiring.
    fn check_content_length(value: &str) -> Result<(), ()> {
        let n: u64 = value.parse().map_err(|_| ())?;
        if n as u128 > MAX_REQUEST_BODY_BYTES as u128 {
            Err(())
        } else {
            Ok(())
        }
    }

    #[test]
    fn content_length_at_limit_is_accepted() {
        let at_limit = MAX_REQUEST_BODY_BYTES.to_string();
        assert!(check_content_length(&at_limit).is_ok());
    }

    #[test]
    fn content_length_over_limit_is_rejected() {
        let over = (MAX_REQUEST_BODY_BYTES + 1).to_string();
        assert!(check_content_length(&over).is_err());
    }

    #[test]
    fn content_length_way_over_limit_is_rejected() {
        // Even obviously-huge values don't overflow the u128 compare.
        let huge = format!("{}", u64::MAX);
        assert!(check_content_length(&huge).is_err());
    }
}
