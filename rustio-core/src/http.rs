use std::ops::{Deref, DerefMut};

use bytes::Bytes;
use http_body_util::Full;

use crate::context::Context;

pub type Response = hyper::Response<Full<Bytes>>;

pub struct Request {
    inner: hyper::Request<hyper::body::Incoming>,
    ctx: Context,
}

impl Request {
    pub(crate) fn new(inner: hyper::Request<hyper::body::Incoming>) -> Self {
        Self {
            inner,
            ctx: Context::new(),
        }
    }

    pub fn ctx(&self) -> &Context {
        &self.ctx
    }

    pub fn ctx_mut(&mut self) -> &mut Context {
        &mut self.ctx
    }

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

pub fn text(body: impl Into<String>) -> Response {
    response(200, "text/plain; charset=utf-8", body.into().into_bytes())
}

pub fn html(body: impl Into<String>) -> Response {
    response(200, "text/html; charset=utf-8", body.into().into_bytes())
}

pub fn status_text(status: u16, body: impl Into<String>) -> Response {
    response(
        status,
        "text/plain; charset=utf-8",
        body.into().into_bytes(),
    )
}

fn response(status: u16, content_type: &'static str, body: Vec<u8>) -> Response {
    hyper::Response::builder()
        .status(status)
        .header("content-type", content_type)
        .body(Full::new(Bytes::from(body)))
        .expect("valid response")
}
