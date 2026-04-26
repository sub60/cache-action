//! TODO: docs.

use core::fmt;
use core::pin::Pin;
use core::task::{Context, Poll};
use std::os::unix::net::UnixListener as StdUnixListener;
use std::path::PathBuf;
use std::{fs, io, process};

use async_compat::Compat;
use futures::Stream;
use futures::stream::FusedStream;
use nix::unistd;
use tokio::net::{UnixListener as TokioUnixListener, UnixStream};

use crate::event_loop;
use crate::nix_store::NixStore;
use crate::real_context::RealContext;
use crate::tokio::Tokio;

#[derive(Debug, clap::Args)]
pub struct StartArgs {
    #[arg(long)]
    socket: PathBuf,

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
    CreatePipe(nix::Error),
    DaemonDidntStart,
    DaemonStartupError(String),
    ForkProcess(nix::Error),
    OpenNixStore(nixb::Error),
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
    let std_listener =
        StdUnixListener::bind(&args.socket).map_err(StartError::BindSocket)?;

    let nix_store = NixStore::open().map_err(StartError::OpenNixStore)?;

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
            let mut status = [0];
            match unistd::read(&read_fd, &mut status) {
                Ok(1) if status[0] == 0 => {},
                Ok(1) if status[0] == 1 => {
                    let mut message = Vec::new();
                    let mut buf = [0; 1024];

                    loop {
                        match unistd::read(&read_fd, &mut buf) {
                            Ok(0) => break,
                            Ok(n) => message.extend_from_slice(&buf[..n]),
                            Err(_) => return Err(StartError::DaemonDidntStart),
                        }
                    }

                    return Err(StartError::DaemonStartupError(
                        String::from_utf8_lossy(&message).into_owned(),
                    ));
                },
                Ok(0) | Err(_) => return Err(StartError::DaemonDidntStart),
                _ => unreachable!(),
            }

            let socket_path =
                fs::canonicalize(&args.socket).map_err(|err| {
                    StartError::CanonicalizeSocketPath(args.socket, err)
                })?;

            println!(
                "Started daemon with process ID {child}, listening on {}",
                socket_path.display()
            );
        },

        unistd::ForkResult::Child => {
            // We only write to the parent process.
            drop(read_fd);

            let cache = match cfg_select! {
                feature = "noop-cache" => Ok::<_, StartError>(crate::noop_cache::NoopCache::default()),
                feature = "sub60-cache" => {
                    futures::executor::block_on(crate::sub60_cache::Sub60Cache::connect(
                        args.user,
                        args.cache,
                        args.auth_token,
                    ))
                    .map_err(StartError::ConnectToCache)
                },
                _ => unreachable!(),
            } {
                Ok(cache) => cache,
                Err(error) => {
                    let error = error.to_string();
                    let _ = unistd::write(&write_fd, &[1]);
                    let _ = unistd::write(&write_fd, error.as_bytes());
                    drop(write_fd);
                    process::exit(1);
                },
            };

            let tokio = Tokio::new();

            let mut context = RealContext::new(nix_store, tokio.spawner());

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
                let _ = unistd::write(&write_fd, &[0]);
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
            Self::CanonicalizeSocketPath(path, io_err) => {
                write!(
                    f,
                    "Couldn't canonicalize '{}': {io_err}",
                    path.display()
                )
            },
            #[cfg(feature = "sub60-cache")]
            Self::ConnectToCache(error) => error.fmt(f),
            Self::CreatePipe(error) => {
                write!(f, "Couldn't create pipe: {error}")
            },
            Self::DaemonDidntStart => {
                write!(f, "Couldn't start daemon")
            },
            Self::DaemonStartupError(message) => f.write_str(message),
            Self::ForkProcess(error) => {
                write!(f, "Couldn't fork process: {error}")
            },
            Self::OpenNixStore(error) => {
                write!(f, "Couldn't open Nix store: {error}")
            },
        }
    }
}
