mod cache;
mod git;
mod git_lfs;
mod misc;
mod transfer_agent;
mod writer;

use clap::Parser;

#[derive(Debug, Parser)]
struct Opts {
    #[clap(subcommand)]
    command: Command,
}

#[derive(Debug, Parser)]
enum Command {
    TransferAgent(transfer_agent::Opts),
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let opts = Opts::parse();
    match opts.command {
        Command::TransferAgent(opts) => transfer_agent::main(opts).await,
    }
}
