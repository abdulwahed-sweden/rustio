//! Unified error type for the framework.
//!
//! Handlers and middleware return `Result<Response, Error>`. The router
//! converts any unhandled `Err` into an HTTP response as a final safety net.
//!
//! `Error::Internal(msg)` keeps the full message for logging (via [`Display`]
//! and [`Error::message`]) but sanitizes it to a generic
//! `"Internal Server Error"` body when converted into an HTTP response.

use std::fmt;

use crate::http::{status_text, Response};

#[non_exhaustive]
#[derive(Debug)]
pub enum Error {
    NotFound,
    MethodNotAllowed,
    BadRequest(String),
    Unauthorized,
    Forbidden,
    Internal(String),
}

impl Error {
    /// HTTP status code associated with this variant.
    pub fn status(&self) -> u16 {
        match self {
            Error::NotFound => 404,
            Error::MethodNotAllowed => 405,
            Error::BadRequest(_) => 400,
            Error::Unauthorized => 401,
            Error::Forbidden => 403,
            Error::Internal(_) => 500,
        }
    }

    /// Human-readable message carried by the variant.
    ///
    /// For `Internal`, this returns the full underlying detail. That detail
    /// is safe for logs but is *not* sent to HTTP clients — see
    /// [`Error::into_response`].
    pub fn message(&self) -> &str {
        match self {
            Error::NotFound => "Not Found",
            Error::MethodNotAllowed => "Method Not Allowed",
            Error::BadRequest(msg) => msg,
            Error::Unauthorized => "Unauthorized",
            Error::Forbidden => "Forbidden",
            Error::Internal(msg) => msg,
        }
    }

    /// Convert this error into an HTTP response.
    ///
    /// The body exposed to clients is sanitized for `Internal` — it always
    /// reads `"Internal Server Error"`, never the original detail.
    pub fn into_response(self) -> Response {
        let status = self.status();
        let body = match self {
            Error::NotFound => String::from("Not Found"),
            Error::MethodNotAllowed => String::from("Method Not Allowed"),
            Error::BadRequest(msg) => msg,
            Error::Unauthorized => String::from("Unauthorized"),
            Error::Forbidden => String::from("Forbidden"),
            Error::Internal(_) => String::from("Internal Server Error"),
        };
        status_text(status, body)
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.status(), self.message())
    }
}

impl std::error::Error for Error {}

impl From<sqlx::Error> for Error {
    fn from(value: sqlx::Error) -> Self {
        Error::Internal(value.to_string())
    }
}

/// Convert a handler result into a definite `Response`.
///
/// Useful in middleware that needs to observe both success and error paths
/// before returning — e.g. attaching an `X-Request-Id` header to every
/// response regardless of outcome.
pub fn resolve(result: Result<Response, Error>) -> Response {
    result.unwrap_or_else(Error::into_response)
}

#[cfg(test)]
mod tests {
    use super::*;
    use http_body_util::BodyExt;

    #[test]
    fn status_codes_match_variant() {
        assert_eq!(Error::NotFound.status(), 404);
        assert_eq!(Error::MethodNotAllowed.status(), 405);
        assert_eq!(Error::BadRequest(String::from("bad")).status(), 400);
        assert_eq!(Error::Unauthorized.status(), 401);
        assert_eq!(Error::Forbidden.status(), 403);
        assert_eq!(Error::Internal(String::from("x")).status(), 500);
    }

    #[test]
    fn parameterless_variants_use_status_phrase_as_message() {
        assert_eq!(Error::NotFound.message(), "Not Found");
        assert_eq!(Error::MethodNotAllowed.message(), "Method Not Allowed");
        assert_eq!(Error::Unauthorized.message(), "Unauthorized");
        assert_eq!(Error::Forbidden.message(), "Forbidden");
    }

    #[test]
    fn parameterised_variants_carry_their_message() {
        assert_eq!(Error::BadRequest(String::from("nope")).message(), "nope");
        assert_eq!(Error::Internal(String::from("oops")).message(), "oops");
    }

    #[test]
    fn into_response_uses_variant_status() {
        assert_eq!(Error::NotFound.into_response().status().as_u16(), 404);
        assert_eq!(Error::Forbidden.into_response().status().as_u16(), 403);
        assert_eq!(
            Error::BadRequest(String::from("x"))
                .into_response()
                .status()
                .as_u16(),
            400,
        );
        assert_eq!(
            Error::Internal(String::from("x"))
                .into_response()
                .status()
                .as_u16(),
            500,
        );
    }

    #[test]
    fn display_shows_status_and_message() {
        assert_eq!(format!("{}", Error::NotFound), "404 Not Found");
        assert_eq!(format!("{}", Error::Forbidden), "403 Forbidden");
        assert_eq!(
            format!("{}", Error::Internal(String::from("oops"))),
            "500 oops"
        );
    }

    #[test]
    fn resolve_passes_ok_through() {
        let resp = status_text(204, "");
        let resolved = resolve(Ok(resp));
        assert_eq!(resolved.status().as_u16(), 204);
    }

    #[test]
    fn resolve_converts_err_to_response() {
        let resolved = resolve(Err(Error::Unauthorized));
        assert_eq!(resolved.status().as_u16(), 401);
    }

    #[tokio::test]
    async fn internal_response_body_is_sanitized() {
        let resp = Error::Internal(String::from("db password: hunter2")).into_response();
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        let body = std::str::from_utf8(&bytes).unwrap();
        assert_eq!(body, "Internal Server Error");
        assert!(!body.contains("hunter2"));
    }

    #[tokio::test]
    async fn public_error_bodies_use_status_phrase_or_message() {
        async fn body_of(err: Error) -> String {
            let bytes = err
                .into_response()
                .into_body()
                .collect()
                .await
                .unwrap()
                .to_bytes();
            String::from_utf8(bytes.to_vec()).unwrap()
        }
        assert_eq!(body_of(Error::NotFound).await, "Not Found");
        assert_eq!(body_of(Error::Unauthorized).await, "Unauthorized");
        assert_eq!(body_of(Error::Forbidden).await, "Forbidden");
        assert_eq!(body_of(Error::BadRequest(String::from("bad"))).await, "bad");
    }

    #[test]
    fn internal_display_and_message_retain_detail_for_logging() {
        let err = Error::Internal(String::from("leaked secret"));
        assert_eq!(err.message(), "leaked secret");
        assert!(format!("{err}").contains("leaked secret"));
    }
}
