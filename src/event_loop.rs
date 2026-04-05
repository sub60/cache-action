use core::pin::pin;

use futures::stream::FusedStream;
use futures::{AsyncRead, AsyncWrite, StreamExt, select};
use nix_types::NixStorePath;

use crate::context::{Context, JoinHandle, Spawner};
use crate::protocol::{self, StoreDir};

pub(crate) trait Io: Send + 'static {
    type Reader: AsyncRead + Send;
    type Writer: AsyncWrite + Send;
    fn split(self) -> (Self::Reader, Self::Writer);
}

pub(crate) async fn run<Ctx: Context>(
    cache: Ctx::Cache,
    mut io_stream: impl FusedStream<Item = impl Io> + Unpin,
    ctx: &mut Ctx,
) {
    let (message_tx, message_rx) = flume::unbounded();

    let _result_writer = loop {
        let _store_path = select! {
            io = io_stream.select_next_some() => {
                ctx.spawner().spawn(handle_io(io, message_tx.clone())).detach();
                continue;
            },
            message_res = message_rx.recv_async() => {
                match message_res.expect("message_tx is still alive") {
                    Ok(Event::PushStorePath(store_path)) => store_path,
                    Ok(Event::Drain(writer)) => break writer,
                    Err(rx_err) => {
                        ctx.handle_rx_error(rx_err);
                        continue;
                    },
                }
            },
        };
    };
}

enum Event<Writer> {
    PushStorePath(NixStorePath<StoreDir>),
    Drain(Writer),
}

async fn handle_io<I: Io>(
    io: I,
    message_tx: flume::Sender<Result<Event<I::Writer>, protocol::ReceiveError>>,
) {
    let (reader, writer) = io.split();
    let mut message_rx = pin!(protocol::Receiver::new(reader));
    loop {
        let Some(message_res) = message_rx.next().await else { return };
        match message_res {
            Ok(protocol::Message::PushStorePath(store_path)) => {
                let _ = message_tx.send(Ok(Event::PushStorePath(store_path)));
            },
            Ok(protocol::Message::DrainDaemon) => {
                let _ = message_tx.send(Ok(Event::Drain(writer)));
                return;
            },
            Err(err) => {
                let _ = message_tx.send(Err(err));
            },
        }
    }
}
