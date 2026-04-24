#![allow(incomplete_features)]
#![feature(generic_const_exprs)]

mod context;
mod drain;
mod event_loop;
mod nix_store;
#[cfg(feature = "noop-cache")]
mod noop_cache;
mod protocol;
mod push;
mod real_context;
mod real_drain_progress_reporter;
mod start;
#[cfg(feature = "sub60-cache")]
mod sub60_cache;
mod tokio;

#[derive(Debug, clap::Subcommand)]
pub enum Command {
    Start(start::StartArgs),
    Push(push::PushArgs),
    Drain(drain::DrainArgs),
}

pub fn run(command: Command) {
    match command {
        Command::Start(args) => start::start(args),
        Command::Push(args) => push::push(args),
        Command::Drain(args) => drain::drain(args),
    }
}
