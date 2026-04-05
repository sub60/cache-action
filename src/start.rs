//! TODO: docs.

use std::os::unix::net::UnixListener;
use std::path::PathBuf;

#[derive(Debug, clap::Args)]
pub struct StartArgs {
    #[arg(long)]
    socket: PathBuf,
    #[arg(long)]
    auth_token: String,
    #[arg(long)]
    user: String,
    #[arg(long)]
    cache: String,
}

pub(crate) fn start(args: StartArgs) {
    let _listener = match UnixListener::bind(&args.socket) {
        Ok(socket) => socket,
        Err(err) => panic!(
            "Failed to create Unix domain socket at {:?}: {err}",
            args.socket
        ),
    };

    println!("Started {:?}", args.socket);
}
