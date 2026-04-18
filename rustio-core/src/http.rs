//! HTTP primitives: [`Request`], [`Response`], response builders, and a small
//! [`FormData`] parser shared by query strings and form bodies.
//!
//! [`Request`] wraps [`hyper::Request`] and adds a typed per-request
//! [`Context`]. It derefs to the underlying hyper request so the usual
//! accessors (`.method()`, `.uri()`, `.headers()`, `.body_mut()`) are
//! available directly.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::ops::{Deref, DerefMut};

use bytes::Bytes;
use http_body_util::Full;

use crate::context::Context;

pub type Response = hyper::Response<Full<Bytes>>;

/// Maximum bytes accepted from a request body across the framework.
///
/// The global body-limit middleware rejects any request whose
/// `Content-Length` exceeds this value with `Error::PayloadTooLarge`
/// (HTTP 413); the admin form reader enforces the same cap while
/// collecting the body so chunked or mis-labelled requests can't slip
/// past. Custom handlers that read bodies directly should use the
/// same constant.
pub const MAX_REQUEST_BODY_BYTES: usize = 2 * 1024 * 1024;

/// Incoming HTTP request with an attached per-request [`Context`]
/// and, when known, the peer address the TCP connection came from.
pub struct Request {
    inner: hyper::Request<hyper::body::Incoming>,
    ctx: Context,
    peer: Option<SocketAddr>,
}

impl Request {
    pub(crate) fn new(
        inner: hyper::Request<hyper::body::Incoming>,
        peer: Option<SocketAddr>,
    ) -> Self {
        Self {
            inner,
            ctx: Context::new(),
            peer,
        }
    }

    /// The client's socket address, if the server could determine it.
    ///
    /// Populated by [`crate::server::Server`] from the accept result.
    /// May be `None` when the request is constructed from a source
    /// that doesn't carry it (tests, reverse proxies that terminate
    /// the connection — the `X-Forwarded-For` header is not parsed
    /// here; projects that need the upstream IP must parse it
    /// themselves).
    pub fn peer_addr(&self) -> Option<SocketAddr> {
        self.peer
    }

    /// Read-only access to the per-request [`Context`].
    pub fn ctx(&self) -> &Context {
        &self.ctx
    }

    /// Mutable access to the per-request [`Context`].
    pub fn ctx_mut(&mut self) -> &mut Context {
        &mut self.ctx
    }

    /// Parse the URL query string into a [`FormData`].
    ///
    /// Returns an empty `FormData` when the request has no query string.
    pub fn query(&self) -> FormData {
        FormData::parse(self.inner.uri().query().unwrap_or(""))
    }

    /// Look up a single cookie value by name.
    ///
    /// Returns `None` if the request has no `Cookie` header, the header is
    /// not valid UTF-8, or no cookie with this name is present. The value
    /// is returned as-is (not URL-decoded).
    pub fn cookie(&self, name: &str) -> Option<String> {
        let header = self
            .inner
            .headers()
            .get(hyper::header::COOKIE)?
            .to_str()
            .ok()?;
        for pair in header.split(';') {
            let pair = pair.trim();
            if let Some((k, v)) = pair.split_once('=') {
                if k == name {
                    return Some(v.to_string());
                }
            }
        }
        None
    }

    /// Consume this request, returning the underlying hyper parts, body,
    /// and the attached context.
    pub fn into_parts(self) -> (hyper::http::request::Parts, hyper::body::Incoming, Context) {
        let (parts, body) = self.inner.into_parts();
        (parts, body, self.ctx)
    }
}

impl Deref for Request {
    type Target = hyper::Request<hyper::body::Incoming>;
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl DerefMut for Request {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

/// Build a `200 OK` response with `text/plain` content type.
pub fn text(body: impl Into<String>) -> Response {
    response(200, "text/plain; charset=utf-8", body.into().into_bytes())
}

/// Build a `200 OK` response with `text/html` content type.
pub fn html(body: impl Into<String>) -> Response {
    response(200, "text/html; charset=utf-8", body.into().into_bytes())
}

/// Build a `200 OK` response with `application/json` content type.
///
/// The body is written verbatim; it is the caller's responsibility to pass
/// a valid JSON document (e.g. from `serde_json::to_string(&value)?`).
pub fn json_raw(body: impl Into<String>) -> Response {
    response(
        200,
        "application/json; charset=utf-8",
        body.into().into_bytes(),
    )
}

/// Build a response with an arbitrary status code and a `text/plain` body.
pub fn status_text(status: u16, body: impl Into<String>) -> Response {
    response(
        status,
        "text/plain; charset=utf-8",
        body.into().into_bytes(),
    )
}

/// Append a `Set-Cookie` header to a response.
///
/// The caller is responsible for formatting `value` as a valid
/// `Set-Cookie` string (e.g. `"name=val; Path=/; HttpOnly; SameSite=Lax"`).
/// Returns silently if `value` contains characters that aren't valid in
/// an HTTP header.
pub fn set_cookie(resp: &mut Response, value: &str) {
    if let Ok(hv) = value.parse() {
        resp.headers_mut().append(hyper::header::SET_COOKIE, hv);
    }
}

fn response(status: u16, content_type: &'static str, body: Vec<u8>) -> Response {
    hyper::Response::builder()
        .status(status)
        .header("content-type", content_type)
        .body(Full::new(Bytes::from(body)))
        .expect("valid response")
}

/// Parsed `application/x-www-form-urlencoded` data.
///
/// Used for both URL query strings (via [`Request::query`]) and POST
/// request bodies (the admin layer reads form submissions this way).
pub struct FormData {
    map: HashMap<String, String>,
}

impl FormData {
    /// Parse a URL-encoded key/value string.
    pub fn parse(body: &str) -> Self {
        let mut map = HashMap::new();
        for pair in body.split('&') {
            if pair.is_empty() {
                continue;
            }
            let mut iter = pair.splitn(2, '=');
            let raw_key = match iter.next() {
                Some(k) if !k.is_empty() => k,
                _ => continue,
            };
            let raw_val = iter.next().unwrap_or("");
            map.insert(percent_decode(raw_key), percent_decode(raw_val));
        }
        FormData { map }
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.map.get(key).map(String::as_str)
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

pub(crate) fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'+' {
            out.push(b' ');
            i += 1;
        } else if b == b'%' && i + 2 < bytes.len() {
            if let (Some(h), Some(l)) = (hex_digit(bytes[i + 1]), hex_digit(bytes[i + 2])) {
                out.push((h << 4) | l);
                i += 3;
                continue;
            }
            out.push(b);
            i += 1;
        } else {
            out.push(b);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_digit(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn form_parse_decodes_basic_pairs() {
        let form = FormData::parse("a=1&b=2");
        assert_eq!(form.get("a"), Some("1"));
        assert_eq!(form.get("b"), Some("2"));
    }

    #[test]
    fn form_parse_decodes_plus_as_space() {
        let form = FormData::parse("name=John+Doe");
        assert_eq!(form.get("name"), Some("John Doe"));
    }

    #[test]
    fn form_parse_decodes_percent_encoded() {
        let form = FormData::parse("q=hello%20world%21");
        assert_eq!(form.get("q"), Some("hello world!"));
    }

    #[test]
    fn form_parse_handles_empty_values() {
        let form = FormData::parse("a=&b=x");
        assert_eq!(form.get("a"), Some(""));
        assert_eq!(form.get("b"), Some("x"));
    }

    #[test]
    fn form_parse_ignores_empty_pairs() {
        let form = FormData::parse("&a=1&&b=2&");
        assert_eq!(form.get("a"), Some("1"));
        assert_eq!(form.get("b"), Some("2"));
        assert_eq!(form.len(), 2);
    }

    #[test]
    fn form_missing_key_is_none() {
        let form = FormData::parse("a=1");
        assert!(form.get("missing").is_none());
    }

    #[test]
    fn percent_decode_passes_through_unreserved() {
        assert_eq!(percent_decode("abcXYZ123-_.~"), "abcXYZ123-_.~");
    }

    #[test]
    fn percent_decode_handles_lowercase_and_uppercase_hex() {
        assert_eq!(percent_decode("%2f%2F"), "//");
    }

    #[test]
    fn percent_decode_leaves_invalid_percent_sequences_alone() {
        assert_eq!(percent_decode("%GG"), "%GG");
        assert_eq!(percent_decode("end%"), "end%");
    }
}
