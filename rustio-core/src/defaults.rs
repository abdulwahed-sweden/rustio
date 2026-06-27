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

/// HTML-escape config-sourced text before splicing it into the landing page.
fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Turn a project's display name into a CLI-safe slug (lowercase, runs of
/// non-alphanumerics collapsed to a single `-`), for the `init`/`cd` commands
/// shown in the landing-page terminal. Empty input falls back to `myproject`.
fn project_slug(name: &str) -> String {
    let mut out = String::new();
    let mut pending_dash = false;
    for c in name.trim().chars() {
        if c.is_alphanumeric() {
            if pending_dash && !out.is_empty() {
                out.push('-');
            }
            pending_dash = false;
            out.extend(c.to_lowercase());
        } else {
            pending_dash = true;
        }
    }
    if out.is_empty() {
        "myproject".to_string()
    } else {
        out
    }
}

/// Stamp the running framework version and the project's own identity (name,
/// logo initial, and a CLI-safe slug from `rustio.design.json`) into a landing
/// page. Config text is HTML-escaped; the slug is alphanumeric-and-dash only.
fn stamp_home(src: &str) -> String {
    let design = crate::admin::design::Design::global();
    src.replace("__RUSTIO_VERSION__", env!("CARGO_PKG_VERSION"))
        .replace("__PROJECT_NAME__", &esc(&design.project_name))
        .replace("__PROJECT_INITIAL__", &esc(&design.logo_initial))
        .replace(
            "__PROJECT_SLUG__",
            &esc(&project_slug(&design.project_name)),
        )
}

pub fn homepage() -> Response {
    // Project override (as the in-file note promises): a `templates/home.html`
    // in the project root replaces the built-in page, with the same placeholder
    // substitution. Any read error (absent / unreadable) falls back to the
    // embedded default, so `/` always renders something.
    let source =
        std::fs::read_to_string("templates/home.html").unwrap_or_else(|_| HOME_HTML.to_string());
    html(stamp_home(&source))
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

pub fn with_defaults(mut router: Router) -> Router {
    // Register each default only if the project hasn't already claimed
    // that path. Without this check, registering `with_defaults` after
    // your own `/` handler would silently shadow it (router matches in
    // registration order), which is a nasty footgun. Ordering now
    // doesn't matter: whichever `/` or `/docs` the project registers
    // first takes precedence, the framework fills in any gap.
    if !router.has_route(&hyper::Method::GET, "/") {
        router = router.get("/", |_req: Request, _p: Params| async {
            Ok::<Response, Error>(homepage())
        });
    }
    if !router.has_route(&hyper::Method::GET, "/docs") {
        router = router.get("/docs", |_req: Request, _p: Params| async {
            Ok::<Response, Error>(docs_placeholder())
        });
    }
    // `wrap` adds middleware that runs on every request — so the
    // body-size cap applies uniformly to admin, user, and default
    // routes without each handler having to opt in.
    router.wrap(body_limit)
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
    fn homepage_stamps_all_placeholders_and_marks_itself_a_dev_page() {
        let stamped = HOME_HTML
            .replace("__RUSTIO_VERSION__", env!("CARGO_PKG_VERSION"))
            .replace("__PROJECT_NAME__", "X")
            .replace("__PROJECT_INITIAL__", "X")
            .replace("__PROJECT_SLUG__", "x");
        // Every placeholder is filled in — none leak to the page.
        for ph in [
            "__RUSTIO_VERSION__",
            "__PROJECT_NAME__",
            "__PROJECT_INITIAL__",
            "__PROJECT_SLUG__",
        ] {
            assert!(!stamped.contains(ph), "placeholder {ph} not filled");
        }
        assert!(stamped.contains(env!("CARGO_PKG_VERSION")));
        // It is unmistakably a developer page to be replaced in production,
        // and carries the Swedish toggle the project relies on.
        assert!(HOME_HTML.contains("replace before production"));
        assert!(HOME_HTML.contains("data-set-lang=\"sv\""));
    }

    #[test]
    fn stamp_home_fills_every_placeholder() {
        let out = stamp_home(
            "<i>__PROJECT_INITIAL__</i> __PROJECT_NAME__ / __PROJECT_SLUG__ / v__RUSTIO_VERSION__",
        );
        for ph in [
            "__PROJECT_NAME__",
            "__PROJECT_INITIAL__",
            "__PROJECT_SLUG__",
            "__RUSTIO_VERSION__",
        ] {
            assert!(!out.contains(ph));
        }
        assert!(out.contains(env!("CARGO_PKG_VERSION")));
    }

    #[test]
    fn project_slug_is_cli_safe() {
        assert_eq!(project_slug("Aurora"), "aurora");
        assert_eq!(project_slug("My Clinic"), "my-clinic");
        assert_eq!(project_slug("RustIO"), "rustio");
        assert_eq!(project_slug("  spaced   out  "), "spaced-out");
        assert_eq!(project_slug("clinic_v2"), "clinic-v2");
        assert_eq!(project_slug("***"), "myproject");
        assert_eq!(project_slug(""), "myproject");
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
