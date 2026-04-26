//! TODO: docs.

use core::fmt;
use core::pin::pin;
use std::io::{self, Write};
use std::path::PathBuf;
use std::process;

use futures::SinkExt;
use tokio::net::UnixStream;

use crate::protocol;
use crate::tokio::Tokio;

#[derive(Debug, clap::Args)]
pub struct StopArgs {
    #[arg(long)]
    socket: PathBuf,
}

enum StopError {
    ConnectToSocket(io::Error),
    WriteMessage(io::Error),
}

pub(crate) fn stop(args: StopArgs) {
    let stop_result: Result<(), StopError> =
        Tokio::new().block_on(async move {
            let socket = UnixStream::connect(&args.socket)
                .await
                .map_err(StopError::ConnectToSocket)?;

            let (mut read_half, write_half) = socket.into_split();

            pin!(protocol::Sender::new(async_compat::Compat::new(write_half)))
                .send(protocol::Message::StopDaemon)
                .await
                .map_err(StopError::WriteMessage)?;

            let mut stdout = tokio::io::stdout();

            if let Err(io_err) =
                tokio::io::copy(&mut read_half, &mut stdout).await
            {
                let _ = writeln!(
                    io::stderr(),
                    "Couldn't copy bytes to stdout: {io_err}"
                );
            }

            Ok(())
        });

    if let Err(stop_error) = stop_result {
        eprintln!("{stop_error}");
        process::exit(1);
    }
}

impl fmt::Display for StopError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ConnectToSocket(error) => {
                write!(f, "Couldn't connect to Unix domain socket: {error}")
            },
            Self::WriteMessage(error) => {
                write!(
                    f,
                    "Couldn't write message to Unix domain socket: {error}"
                )
            },
        }
    }
}
