//! TODO: docs.

use core::fmt;
use std::io::{self, Write};
use std::path::PathBuf;
use std::process;

use futures::SinkExt;
use nix_types::NixStorePath;
use tokio::net::UnixStream;

use crate::context::Runtime;
use crate::protocol::{self, StoreDir};
use crate::tokio::Tokio;

#[derive(Debug, clap::Args)]
pub struct PushArgs {
    #[arg(long)]
    socket: PathBuf,
    #[arg(required = true)]
    store_paths: Vec<NixStorePath<StoreDir>>,
}

pub(crate) fn push(args: PushArgs) {
    let push_result: Result<(), PushError> =
        Tokio::new().block_on(async move {
            let mut message_tx = UnixStream::connect(&args.socket)
                .await
                .map(async_compat::Compat::new)
                .map(protocol::Sender::new)
                .map_err(PushError::ConnectToSocket)?;

            for store_path in args.store_paths {
                message_tx
                    .send(protocol::Message::PushStorePath(store_path))
                    .await
                    .map_err(PushError::WriteMessage)?;
            }

            Ok(())
        });

    if let Err(push_error) = push_result {
        let _ = writeln!(io::stderr(), "{}", push_error);
        process::exit(1);
    }
}

enum PushError {
    ConnectToSocket(io::Error),
    WriteMessage(io::Error),
}

impl fmt::Display for PushError {
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
