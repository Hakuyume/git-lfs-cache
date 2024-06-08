mod cache;
mod git;
mod git_lfs;
mod misc;
mod transfer_agent;
mod writer;

use clap::Parser;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

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
    if let Ok(layer) = tracing_journald::layer() {
        tracing_subscriber::registry().with(layer).init();
    }

    match Opts::try_parse() {
        Ok(opts) => match opts.command {
            Command::TransferAgent(opts) => transfer_agent::main(opts).await,
        },
        Err(e) => {
            tracing::error!(error = e.to_string());
            Err(e.into())
        }
    }
}
