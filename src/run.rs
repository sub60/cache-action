//! TODO: docs.

use core::fmt;
use core::pin::Pin;
use core::task::{Context, Poll};
use std::fs::File;
use std::io::Write;
use std::os::fd::{FromRawFd, RawFd};
use std::os::unix::net::UnixListener as StdUnixListener;
use std::path::PathBuf;
use std::time::Instant;
use std::{env, fs, io, process};

use async_compat::Compat;
use futures::Stream;
use futures::stream::FusedStream;
use tokio::net::{UnixListener as TokioUnixListener, UnixStream};

use crate::event_loop;
use crate::nix_store::NixStore;
use crate::real_context::RealContext;
use crate::tokio::Tokio;

#[derive(Debug, clap::Args)]
pub struct RunArgs {
    #[arg(long)]
    socket: PathBuf,

    #[arg(long)]
    ready_fd: Option<RawFd>,

    #[cfg(feature = "sub60-cache")]
    #[arg(long)]
    user: nix_types::sub60::UserName,

    #[cfg(feature = "sub60-cache")]
    #[arg(long)]
    cache: nix_types::sub60::CacheName,

    #[cfg(feature = "sub60-cache")]
    #[arg(long)]
    auth_token: crate::sub60_cache::AuthToken,
}

enum StartError {
    BindSocket(io::Error),
    CanonicalizeSocketPath(PathBuf, io::Error),
    #[cfg(feature = "sub60-cache")]
    ConnectToCache(crate::sub60_cache::CacheConnectError),
    OpenNixStore(nixb::Error),
    SignalReadiness(io::Error),
}

/// A [`Stream`] of accepted connections from a [`TokioUnixListener`].
struct UnixStreams {
    listener: TokioUnixListener,
}

pub(crate) fn run(args: RunArgs) {
    let ready_fd = args.ready_fd;

    if let Err(start_error) = start(args) {
        if let Some(ready_fd) = ready_fd
            && !matches!(start_error, StartError::SignalReadiness(_))
        {
            let _ = write_ready_status(ready_fd, Err(start_error.to_string()));
        } else {
            eprintln!("{start_error}");
        }

        process::exit(1);
    }
}

fn start(args: RunArgs) -> Result<(), StartError> {
    let std_listener =
        StdUnixListener::bind(&args.socket).map_err(StartError::BindSocket)?;

    let nix_store = NixStore::open().map_err(StartError::OpenNixStore)?;

    let cache = cfg_select! {
        feature = "noop-cache" => crate::noop_cache::NoopCache::default(),
        feature = "sub60-cache" => {
            futures::executor::block_on(crate::sub60_cache::Sub60Cache::connect(
                args.user,
                args.cache,
                args.auth_token,
            ))
            .map_err(StartError::ConnectToCache)?
        },
        _ => unreachable!(),
    };

    let tokio = Tokio::new();

    let mut context = RealContext::new(nix_store, tokio.spawner());

    std_listener
        .set_nonblocking(true)
        .expect("couldn't set socket to non-blocking");

    let listener = TokioUnixListener::from_std(std_listener)
        .expect("couldn't convert std's UnixListener to tokio's UnixListener");

    let socket_path = fs::canonicalize(&args.socket)
        .map_err(|err| StartError::CanonicalizeSocketPath(args.socket, err))?;

    if let Some(ready_fd) = args.ready_fd {
        write_ready_status(ready_fd, Ok(()))
            .map_err(StartError::SignalReadiness)?;
    } else {
        println!("Started daemon, listening on {}", socket_path.display());
    }

    tokio.block_on(async {
        event_loop::run(cache, UnixStreams { listener }, &mut context).await;
    });

    Ok(())
}

fn write_ready_status(
    ready_fd: RawFd,
    status: Result<(), String>,
) -> io::Result<()> {
    // SAFETY: `--ready-fd` is only used by the JS action, which passes an open
    // pipe fd that this process owns and should close after writing status.
    let mut ready_file = unsafe { File::from_raw_fd(ready_fd) };

    match status {
        Ok(()) => ready_file.write_all(&[0])?,
        Err(message) => {
            ready_file.write_all(&[1])?;
            ready_file.write_all(message.as_bytes())?;
        },
    }

    Ok(())
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
            Self::CanonicalizeSocketPath(path, io_err) => {
                write!(
                    f,
                    "Couldn't canonicalize '{}': {io_err}",
                    path.display()
                )
            },
            #[cfg(feature = "sub60-cache")]
            Self::ConnectToCache(error) => error.fmt(f),
            Self::OpenNixStore(error) => {
                write!(f, "Couldn't open Nix store: {error}")
            },
            Self::SignalReadiness(error) => {
                write!(f, "Couldn't signal daemon readiness: {error}")
            },
        }
    }
}
