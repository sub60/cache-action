//! TODO: docs.

use core::fmt;
use core::pin::Pin;
use core::task::{Context, Poll};
use std::fs::File;
use std::io::Write;
use std::os::fd::{FromRawFd, RawFd};
use std::path::PathBuf;
use std::{io, process};

use async_compat::Compat;
use futures::Stream;
use futures::stream::FusedStream;
use smallvec::SmallVec;
use tokio::net::{UnixListener, UnixStream};

use crate::event_loop;
use crate::nix_store::NixStore;
use crate::real_context::RealContext;
use crate::tokio::{Tokio, TokioSpawner};

#[derive(Debug, clap::Args)]
pub struct RunArgs {
    #[arg(long)]
    socket: PathBuf,

    #[arg(long)]
    ready_fd: Option<RawFd>,

    #[arg(
        long,
        value_parser = url::Url::parse,
        value_delimiter = ',',
        num_args = 0..,
        default_values = ["https://cache.nixos.org"],
    )]
    pub(crate) upstream_caches: SmallVec<[url::Url; 2]>,

    #[cfg(feature = "sub60-cache")]
    #[command(flatten)]
    pub(crate) sub60_cache_args: crate::sub60_cache::Sub60CacheRunArgs,
}

enum StartError {
    BindSocket(io::Error),
    #[cfg(feature = "sub60-cache")]
    ConnectToCache(crate::sub60_cache::CacheConnectError),
    OpenNixStore(nixb::Error),
}

/// A [`Stream`] of accepted connections from a [`UnixListener`].
struct UnixStreams {
    listener: UnixListener,
}

pub(crate) fn run(args: RunArgs) {
    let tokio = Tokio::new();

    let ready_fd = args.ready_fd;

    let start_res = tokio.block_on(start(args, tokio.spawner()));

    if let Some(ready_fd) = ready_fd {
        let mut ready_file = unsafe { File::from_raw_fd(ready_fd) };

        let mut signal_readiness = || match &start_res {
            Ok(_) => ready_file.write_all(&[0]),
            Err(start_error) => {
                ready_file.write_all(&[1])?;
                ready_file.write_all(start_error.to_string().as_bytes())
            },
        };

        if let Err(io_err) = signal_readiness() {
            eprintln!("Couldn't write to file descriptor {ready_fd}: {io_err}");
        }
    }

    match start_res {
        Ok(run_event_loop) => tokio.block_on(run_event_loop),
        Err(start_error) => {
            if ready_fd.is_none() {
                eprintln!("{start_error}");
            }
            process::exit(1);
        },
    }
}

async fn start(
    args: RunArgs,
    spawner: TokioSpawner,
) -> Result<impl Future<Output = ()> + use<>, StartError> {
    let listener =
        UnixListener::bind(&args.socket).map_err(StartError::BindSocket)?;

    let nix_store = NixStore::open().map_err(StartError::OpenNixStore)?;

    let cache = cfg_select! {
        feature = "noop-cache" => crate::noop_cache::NoopCache::default(),
        feature = "sub60-cache" => {
            crate::sub60_cache::Sub60Cache::connect(&args)
                .await
                .map_err(StartError::ConnectToCache)?
        },
        _ => unreachable!(),
    };

    let mut ctx = RealContext::new(nix_store, spawner);

    Ok(async move {
        event_loop::run(args, cache, UnixStreams { listener }, &mut ctx).await;
    })
}

impl Stream for UnixStreams {
    type Item = UnixStream;

    fn poll_next(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        loop {
            match self.listener.poll_accept(cx) {
                Poll::Ready(Ok((stream, _addr))) => {
                    return Poll::Ready(Some(stream));
                },
                Poll::Ready(Err(err)) => {
                    eprintln!("Couldn't accept stream: {err}");
                    continue;
                },
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

impl FusedStream for UnixStreams {
    fn is_terminated(&self) -> bool {
        false
    }
}

impl event_loop::Io for tokio::net::UnixStream {
    type Reader = Compat<tokio::net::unix::OwnedReadHalf>;
    type Writer = Compat<tokio::net::unix::OwnedWriteHalf>;

    fn split(self) -> (Self::Reader, Self::Writer) {
        let (reader, writer) = self.into_split();
        (Compat::new(reader), Compat::new(writer))
    }
}

impl fmt::Display for StartError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BindSocket(io_error) => {
                write!(f, "Couldn't bind to Unix domain socket: {io_error}")
            },
            #[cfg(feature = "sub60-cache")]
            Self::ConnectToCache(error) => error.fmt(f),
            Self::OpenNixStore(error) => {
                write!(f, "Couldn't open Nix store: {error}")
            },
        }
    }
}
