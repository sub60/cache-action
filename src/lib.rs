#![allow(incomplete_features)]
#![feature(generic_const_exprs)]

mod async_read_ext;
mod context;
mod event_loop;
mod nix_store;
#[cfg(feature = "noop-cache")]
mod noop_cache;
mod protocol;
mod push;
mod real_context;
mod real_drain_progress_reporter;
mod run;
mod stop;
#[cfg(feature = "sub60-cache")]
mod sub60_cache;
mod tokio;

#[derive(Debug, clap::Subcommand)]
pub enum Command {
    Run(run::RunArgs),
    Push(push::PushArgs),
    Stop(stop::StopArgs),
}

pub fn run(command: Command) {
    match command {
        Command::Run(args) => run::run(args),
        Command::Push(args) => push::push(args),
        Command::Stop(args) => stop::stop(args),
    }
}
