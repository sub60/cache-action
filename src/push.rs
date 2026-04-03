//! TODO: docs.

use std::path::PathBuf;

#[derive(Debug, clap::Args)]
pub struct PushArgs {
    #[arg(long)]
    socket: PathBuf,
    #[arg(required = true)]
    store_paths: Vec<String>,
}

pub(crate) fn push(args: PushArgs) {
    println!("Pushing {:?} to {:?}", args.store_paths, args.socket);
}
