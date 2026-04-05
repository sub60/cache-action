use tokio::net::UnixListener;

use crate::context::Context;

pub(crate) async fn run<Ctx: Context>(
    _cache: Ctx::Cache,
    _connection_stream: UnixListener,
    _ctx: &Ctx,
) {
    todo!()
}
