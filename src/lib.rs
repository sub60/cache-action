pub mod drain;
pub mod push;
pub mod start;

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
