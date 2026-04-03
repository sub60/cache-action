//! TODO: docs.

use std::path::PathBuf;

#[derive(Debug, clap::Args)]
pub struct DrainArgs {
    #[arg(long)]
    socket: PathBuf,
}

pub(crate) fn drain(args: DrainArgs) {
    println!("Draining {:?}", args.socket);
}
