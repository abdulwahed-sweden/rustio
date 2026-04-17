//! Path router with `:param` support.
//!
//! Routes are registered against a [`Router`] and dispatched by path +
//! method. Paths that match but with the wrong method produce `405 Method
//! Not Allowed` rather than collapsing to `404`.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use hyper::Method;

use crate::error::Error;
use crate::http::{Request, Response};
use crate::middleware::{MiddlewareFn, Next};

type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send>>;
type HandlerFn = Arc<dyn Fn(Request, Params) -> BoxFuture<Result<Response, Error>> + Send + Sync>;

pub struct Params {
    pairs: Vec<(String, String)>,
}

impl Params {
    fn empty() -> Self {
        Self { pairs: Vec::new() }
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        self.pairs
            .iter()
            .find_map(|(k, v)| (k == name).then_some(v.as_str()))
    }

    pub fn len(&self) -> usize {
        self.pairs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.pairs.is_empty()
    }
}

enum Segment {
    Literal(String),
    Param(String),
}

struct Route {
    method: Method,
    segments: Vec<Segment>,
    handler: HandlerFn,
}

pub struct Router {
    routes: Vec<Route>,
    middlewares: Vec<MiddlewareFn>,
}

impl Router {
    pub fn new() -> Self {
        Self {
            routes: Vec::new(),
            middlewares: Vec::new(),
        }
    }

    pub fn wrap<F, Fut>(mut self, middleware: F) -> Self
    where
        F: Fn(Request, Next) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Response, Error>> + Send + 'static,
    {
        self.middlewares
            .push(Arc::new(move |req, next| Box::pin(middleware(req, next))));
        self
    }

    pub fn get<F, Fut>(self, path: &str, handler: F) -> Self
    where
        F: Fn(Request, Params) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Response, Error>> + Send + 'static,
    {
        self.route(Method::GET, path, handler)
    }

    pub fn post<F, Fut>(self, path: &str, handler: F) -> Self
    where
        F: Fn(Request, Params) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Response, Error>> + Send + 'static,
    {
        self.route(Method::POST, path, handler)
    }

    fn route<F, Fut>(mut self, method: Method, path: &str, handler: F) -> Self
    where
        F: Fn(Request, Params) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Response, Error>> + Send + 'static,
    {
        let handler: HandlerFn = Arc::new(move |req, params| Box::pin(handler(req, params)));
        self.routes.push(Route {
            method,
            segments: parse_path(path),
            handler,
        });
        self
    }

    pub async fn dispatch(&self, req: Request) -> Response {
        let path = req.uri().path().to_owned();
        let actual: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        let method = req.method().clone();

        let mut found: Option<(HandlerFn, Params)> = None;
        let mut path_matched = false;

        for route in &self.routes {
            if let Some(params) = match_segments(&route.segments, &actual) {
                path_matched = true;
                if route.method == method {
                    found = Some((route.handler.clone(), params));
                    break;
                }
            }
        }

        let (handler, params) = found.unwrap_or_else(|| {
            let method_not_allowed = path_matched;
            let fallback: HandlerFn = Arc::new(move |_req, _params| {
                let err = if method_not_allowed {
                    Error::MethodNotAllowed
                } else {
                    Error::NotFound
                };
                Box::pin(async move { Err(err) })
            });
            (fallback, Params::empty())
        });

        let chain = build_chain(&self.middlewares, handler, params);
        match chain(req).await {
            Ok(resp) => resp,
            Err(err) => err.into_response(),
        }
    }
}

impl Default for Router {
    fn default() -> Self {
        Self::new()
    }
}

fn build_chain(
    middlewares: &[MiddlewareFn],
    handler: HandlerFn,
    params: Params,
) -> Box<dyn FnOnce(Request) -> BoxFuture<Result<Response, Error>> + Send> {
    let mut chain: Box<dyn FnOnce(Request) -> BoxFuture<Result<Response, Error>> + Send> =
        Box::new(move |req| handler(req, params));

    for mw in middlewares.iter().rev() {
        let mw = mw.clone();
        let inner = chain;
        chain = Box::new(move |req| {
            let next = Next::new(inner);
            mw(req, next)
        });
    }
    chain
}

fn parse_path(path: &str) -> Vec<Segment> {
    path.split('/')
        .filter(|s| !s.is_empty())
        .map(|s| match s.strip_prefix(':') {
            Some(name) => Segment::Param(name.to_owned()),
            None => Segment::Literal(s.to_owned()),
        })
        .collect()
}

fn match_segments(patterns: &[Segment], actual: &[&str]) -> Option<Params> {
    if patterns.len() != actual.len() {
        return None;
    }
    let mut params = Params::empty();
    for (pat, seg) in patterns.iter().zip(actual.iter()) {
        match pat {
            Segment::Literal(lit) => {
                if lit != seg {
                    return None;
                }
            }
            Segment::Param(name) => {
                params.pairs.push((name.clone(), (*seg).to_owned()));
            }
        }
    }
    Some(params)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn segs(path: &str) -> Vec<Segment> {
        parse_path(path)
    }

    fn parts(path: &str) -> Vec<&str> {
        path.split('/').filter(|s| !s.is_empty()).collect()
    }

    #[test]
    fn root_path_is_empty_segment_list() {
        assert!(parse_path("/").is_empty());
    }

    #[test]
    fn literal_match() {
        assert!(match_segments(&segs("/users"), &parts("/users")).is_some());
        assert!(match_segments(&segs("/users"), &parts("/posts")).is_none());
    }

    #[test]
    fn param_captures_value() {
        let params = match_segments(&segs("/users/:id"), &parts("/users/42")).unwrap();
        assert_eq!(params.get("id"), Some("42"));
    }

    #[test]
    fn length_mismatch_does_not_match() {
        assert!(match_segments(&segs("/users/:id"), &parts("/users")).is_none());
        assert!(match_segments(&segs("/users"), &parts("/users/42")).is_none());
    }

    #[test]
    fn multiple_params_captured_by_name() {
        let params = match_segments(&segs("/a/:x/b/:y"), &parts("/a/first/b/second")).unwrap();
        assert_eq!(params.get("x"), Some("first"));
        assert_eq!(params.get("y"), Some("second"));
    }
}
