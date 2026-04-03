#[derive(Debug, clap::Parser)]
#[command(version, about = "sub60 cache action daemon CLI")]
struct Cli {
    #[command(subcommand)]
    command: cache_action::Command,
}

fn main() {
    cache_action::run(<Cli as clap::Parser>::parse().command)
}
