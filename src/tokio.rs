use core::pin::Pin;
use core::task::{Context, Poll, ready};
use std::panic;

use crate::context;

pub(crate) struct Tokio {
    inner: tokio::runtime::Runtime,
}

pin_project_lite::pin_project! {
    pub(crate) struct TokioHandle<T> {
        #[pin]
        inner: tokio::task::JoinHandle<T>,
    }
}

impl Tokio {
    #[track_caller]
    pub(crate) fn new() -> Self {
        tokio::runtime::Builder::new_multi_thread()
            .build()
            .map(|inner| Self { inner })
            .expect("couldn't create tokio runtime")
    }
}

impl context::Runtime for Tokio {
    type Handle<Fut: Future> = TokioHandle<Fut::Output>;

    fn block_on<Fut: Future>(&self, future: Fut) -> Fut::Output {
        self.inner.block_on(future)
    }

    fn spawn<Fut>(&self, future: Fut) -> Self::Handle<Fut>
    where
        Fut: Future + Send + Sync + 'static,
        Fut::Output: Send + Sync + 'static,
    {
        TokioHandle { inner: self.inner.handle().spawn(future) }
    }
}

impl<T> Future for TokioHandle<T> {
    type Output = T;

    fn poll(self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<Self::Output> {
        match ready!(self.project().inner.poll(ctx)) {
            Ok(output) => Poll::Ready(output),
            Err(join_err) => panic::resume_unwind(join_err.into_panic()),
        }
    }
}

impl<T> context::RuntimeHandle<T> for TokioHandle<T> {
    fn detach(self) {}
}
