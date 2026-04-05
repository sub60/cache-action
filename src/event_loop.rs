use futures::{AsyncRead, AsyncWrite, Stream};

use crate::context::Context;

pub(crate) async fn run<Ctx: Context>(
    _cache: Ctx::Cache,
    _io_stream: impl Stream<Item = impl Io>,
    _ctx: &Ctx,
) {
    todo!()
}

pub(crate) trait Io: AsyncRead + AsyncWrite + Send + 'static {}

impl<T: AsyncRead + AsyncWrite + Send + 'static> Io for T {}
