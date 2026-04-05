//! TODO: docs.

use core::fmt;
use core::pin::Pin;
use core::task::{Context, Poll};
use std::os::unix::net::UnixListener as StdUnixListener;
use std::path::PathBuf;
use std::{io, process};

use async_compat::Compat;
use futures::Stream;
use futures::stream::FusedStream;
use nix::unistd;
use nix_types::{CacheName, UserName};
use tokio::net::{UnixListener as TokioUnixListener, UnixStream};

use crate::real_cache::{CacheConnectError, RealCache};
use crate::real_context::RealContext;
use crate::tokio::Tokio;
use crate::{AuthToken, event_loop};

#[derive(Debug, clap::Args)]
pub struct StartArgs {
    #[arg(long)]
    socket: PathBuf,
    #[arg(long)]
    user: UserName,
    #[arg(long)]
    cache: CacheName,
    #[arg(long)]
    auth_token: AuthToken,
}

enum StartError {
    BindSocket(io::Error),
    ConnectToCache(CacheConnectError),
    CreatePipe(nix::Error),
    DaemonDidntStart,
    ForkProcess(nix::Error),
}

/// A [`Stream`] of accepted connections from a [`TokioUnixListener`].
struct UnixStreams {
    listener: TokioUnixListener,
}

pub(crate) fn start(args: StartArgs) {
    if let Err(start_error) = start_inner(args) {
        eprintln!("{start_error}");
        process::exit(1);
    }
}

fn start_inner(args: StartArgs) -> Result<(), StartError> {
    let cache = futures::executor::block_on(RealCache::connect(
        args.user,
        args.cache,
        args.auth_token,
    ))
    .map_err(StartError::ConnectToCache)?;

    let std_listener =
        StdUnixListener::bind(&args.socket).map_err(StartError::BindSocket)?;

    // Create a pipe for the daemon process to signal readiness.
    let (read_fd, write_fd) = unistd::pipe().map_err(StartError::CreatePipe)?;

    // SAFETY: the process is fully single-threaded at this point.
    let fork_result =
        unsafe { unistd::fork() }.map_err(StartError::ForkProcess)?;

    match fork_result {
        unistd::ForkResult::Parent { child } => {
            // We only read from the child process.
            drop(write_fd);

            // Block until the child signals readiness.
            unistd::read(&read_fd, &mut [0u8; 1])
                .map_err(|_err| StartError::DaemonDidntStart)?;

            println!(
                "Started daemon with process ID {child}, listening on {}",
                args.socket.display()
            );
        },

        unistd::ForkResult::Child => {
            // We only write to the parent process.
            drop(read_fd);

            let tokio = Tokio::new();

            let mut context = RealContext::new(tokio.spawner());

            tokio.block_on(async {
                std_listener
                    .set_nonblocking(true)
                    .expect("couldn't set socket to non-blocking");

                let listener = TokioUnixListener::from_std(std_listener)
                    .expect(
                        "couldn't convert std's UnixListener to tokio's \
                         UnixListener",
                    );

                // Signal to the parent that we're ready.
                let _ = unistd::write(&write_fd, &[1]);
                drop(write_fd);

                event_loop::run(cache, UnixStreams { listener }, &mut context)
                    .await;
            });
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
            Self::ConnectToCache(error) => error.fmt(f),
            Self::CreatePipe(error) => {
                write!(f, "Couldn't create pipe: {error}")
            },
            Self::DaemonDidntStart => {
                write!(f, "Couldn't start daemon")
            },
            Self::ForkProcess(error) => {
                write!(f, "Couldn't fork process: {error}")
            },
        }
    }
}
