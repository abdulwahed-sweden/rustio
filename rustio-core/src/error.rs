use std::fmt;

use crate::http::{Response, status_text};

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

    pub fn into_response(self) -> Response {
        let status = self.status();
        let message = match self {
            Error::NotFound => String::from("Not Found"),
            Error::MethodNotAllowed => String::from("Method Not Allowed"),
            Error::BadRequest(msg) => msg,
            Error::Unauthorized => String::from("Unauthorized"),
            Error::Forbidden => String::from("Forbidden"),
            Error::Internal(msg) => msg,
        };
        status_text(status, message)
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

pub fn resolve(result: Result<Response, Error>) -> Response {
    result.unwrap_or_else(Error::into_response)
}

#[cfg(test)]
mod tests {
    use super::*;

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
            Error::BadRequest(String::from("x")).into_response().status().as_u16(),
            400,
        );
        assert_eq!(
            Error::Internal(String::from("x")).into_response().status().as_u16(),
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
}
