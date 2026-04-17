use bytes::Bytes;
use http_body_util::Full;

pub type Request = hyper::Request<hyper::body::Incoming>;
pub type Response = hyper::Response<Full<Bytes>>;

pub fn text(body: impl Into<String>) -> Response {
    hyper::Response::builder()
        .header("content-type", "text/plain; charset=utf-8")
        .body(Full::new(Bytes::from(body.into())))
        .expect("valid response")
}

pub fn html(body: impl Into<String>) -> Response {
    hyper::Response::builder()
        .header("content-type", "text/html; charset=utf-8")
        .body(Full::new(Bytes::from(body.into())))
        .expect("valid response")
}
