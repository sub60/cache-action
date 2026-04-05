//! TODO: docs.

use core::fmt;
use std::path::PathBuf;
use std::{io, process};

use nix_types::{CacheName, UserName};
use tokio::net::UnixListener;

use crate::AuthToken;
use crate::context::Context;
use crate::real_context::{self, RealContext};
use crate::tokio::Tokio;

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

pub(crate) fn start(args: StartArgs) {
    let tokio = Tokio::new();

    let mut ctx = RealContext::new(tokio.spawner());

    let start_result: Result<i64, StartError> = tokio.block_on(async {
        let _cache = ctx
            .cache(args.user, args.cache, args.auth_token)
            .await
            .map_err(StartError::GetCache)?;

        let _listener =
            UnixListener::bind(&args.socket).map_err(StartError::BindSocket)?;

        todo!("fork process, pass cache and listener to fork");
    });

    match start_result {
        Ok(daemon_pid) => {
            println!(
                "Started daemon with process ID {daemon_pid}, listening on {}",
                args.socket.display()
            );
        },
        Err(start_error) => {
            eprintln!("{start_error}");
            process::exit(1);
        },
    }
}

enum StartError {
    BindSocket(io::Error),
    GetCache(real_context::CacheError),
}

impl fmt::Display for StartError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BindSocket(io_error) => {
                write!(f, "Couldn't bind to Unix domain socket: {io_error}")
            },
            Self::GetCache(error) => error.fmt(f),
        }
    }
}
