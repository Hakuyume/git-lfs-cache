mod cache;
mod git;
mod git_lfs;
mod install;
mod jsonl;
mod logs;
mod misc;
mod stats;
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
    Install(install::Opts),
    Stats(stats::Opts),
    TransferAgent(transfer_agent::Opts),
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let opts = Opts::parse();
    match opts.command {
        Command::Install(opts) => install::main(opts).await,
        Command::Stats(opts) => stats::main(opts).await,
        Command::TransferAgent(opts) => transfer_agent::main(opts).await,
    }
}
