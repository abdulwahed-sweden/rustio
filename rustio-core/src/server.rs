//! Hyper-backed HTTP/1 server.
//!
//! Bind an address with [`Server::bind`] and serve either a raw handler
//! via [`Server::serve`] or a [`Router`] via [`Server::serve_router`].

use std::convert::Infallible;
use std::future::Future;
use std::net::SocketAddr;
use std::sync::Arc;

use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper_util::rt::TokioIo;
use tokio::net::TcpListener;

use crate::http::{Request, Response};
use crate::router::Router;

pub struct Server {
    addr: SocketAddr,
}

impl Server {
    pub fn bind(addr: SocketAddr) -> Self {
        Self { addr }
    }

    pub async fn serve<F, Fut>(self, handler: F) -> std::io::Result<()>
    where
        F: Fn(Request) -> Fut + Clone + Send + Sync + 'static,
        Fut: Future<Output = Response> + Send + 'static,
    {
        let listener = TcpListener::bind(self.addr).await?;
        eprintln!("rustio-core: listening on http://{}", self.addr);

        loop {
            let (stream, peer) = listener.accept().await?;
            let io = TokioIo::new(stream);
            let handler = handler.clone();

            tokio::spawn(async move {
                let service = service_fn(move |raw: hyper::Request<hyper::body::Incoming>| {
                    let handler = handler.clone();
                    async move {
                        // `peer` is captured by `Copy` (SocketAddr is
                        // Copy); attached to every request off this
                        // connection so handlers can read it via
                        // `Request::peer_addr`.
                        let req = Request::new(raw, Some(peer));
                        Ok::<Response, Infallible>(handler(req).await)
                    }
                });

                if let Err(err) = http1::Builder::new().serve_connection(io, service).await {
                    eprintln!("rustio-core: connection error: {err}");
                }
            });
        }
    }

    pub async fn serve_router(self, router: Router) -> std::io::Result<()> {
        let router = Arc::new(router);
        self.serve(move |req| {
            let router = router.clone();
            async move { router.dispatch(req).await }
        })
        .await
    }

    /// Serve a router on an already-bound `TcpListener`.
    ///
    /// Use when the caller needs to own the socket — for example to
    /// bind to port 0 and read back the kernel-assigned address
    /// before spawning the server (integration tests, pre-fork
    /// servers that drop privileges after binding).
    pub async fn serve_router_on(listener: TcpListener, router: Router) -> std::io::Result<()> {
        let router = Arc::new(router);
        loop {
            let (stream, peer) = listener.accept().await?;
            let io = TokioIo::new(stream);
            let router = router.clone();

            tokio::spawn(async move {
                let service = service_fn(move |raw: hyper::Request<hyper::body::Incoming>| {
                    let router = router.clone();
                    async move {
                        let req = Request::new(raw, Some(peer));
                        Ok::<Response, Infallible>(router.dispatch(req).await)
                    }
                });
                if let Err(err) = http1::Builder::new().serve_connection(io, service).await {
                    eprintln!("rustio-core: connection error: {err}");
                }
            });
        }
    }
}
