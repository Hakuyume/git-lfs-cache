mod cache;
mod channel;
mod git;
mod git_lfs;
mod install;
mod jsonl;
mod logs;
mod misc;
mod stats;
mod transfer_agent;

use clap::Parser;

#[derive(Debug, Parser)]
struct Args {
    #[clap(subcommand)]
    command: Command,
}

#[derive(Debug, Parser)]
enum Command {
    Install(install::Args),
    Stats(stats::Args),
    TransferAgent(transfer_agent::Args),
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    match args.command {
        Command::Install(args) => install::main(args).await,
        Command::Stats(args) => stats::main(args).await,
        Command::TransferAgent(args) => transfer_agent::main(args).await,
    }
}
