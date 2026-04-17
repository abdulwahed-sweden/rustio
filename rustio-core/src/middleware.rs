use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use crate::error::Error;
use crate::http::{Request, Response};

type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send>>;

pub type MiddlewareFn =
    Arc<dyn Fn(Request, Next) -> BoxFuture<Result<Response, Error>> + Send + Sync>;

pub struct Next {
    inner: Box<dyn FnOnce(Request) -> BoxFuture<Result<Response, Error>> + Send>,
}

impl Next {
    pub(crate) fn new(
        inner: Box<dyn FnOnce(Request) -> BoxFuture<Result<Response, Error>> + Send>,
    ) -> Self {
        Self { inner }
    }

    pub async fn run(self, req: Request) -> Result<Response, Error> {
        (self.inner)(req).await
    }
}
