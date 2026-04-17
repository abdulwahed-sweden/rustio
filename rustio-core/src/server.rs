use std::convert::Infallible;
use std::future::Future;
use std::net::SocketAddr;

use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper_util::rt::TokioIo;
use tokio::net::TcpListener;

use crate::http::{Request, Response};

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
            let (stream, _peer) = listener.accept().await?;
            let io = TokioIo::new(stream);
            let handler = handler.clone();

            tokio::spawn(async move {
                let service = service_fn(move |req: Request| {
                    let handler = handler.clone();
                    async move { Ok::<Response, Infallible>(handler(req).await) }
                });

                if let Err(err) = http1::Builder::new().serve_connection(io, service).await {
                    eprintln!("rustio-core: connection error: {err}");
                }
            });
        }
    }
}
