//! TODO: docs.

use std::path::PathBuf;

#[derive(Debug, clap::Args)]
pub struct StartArgs {
    #[arg(long)]
    socket: PathBuf,
    #[arg(long)]
    auth_token: String,
}

pub(crate) fn start(args: StartArgs) {
    println!("Starting {:?}", args.socket);
}
