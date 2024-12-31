use crate::{cache, git};
use clap::Parser;
use std::borrow::{Borrow, Cow};
use std::env;

#[derive(Clone, Debug, Parser)]
pub struct Args {
    #[clap(flatten)]
    location: git::Location,
    #[clap(long)]
    cache: Option<cache::Args>,
}

pub async fn main(args: Args) -> anyhow::Result<()> {
    let current_dir = env::current_dir()?;
    let path = env::current_exe()?;

    let mut transfer_agent = vec![Cow::Borrowed("transfer-agent")];
    if let Some(cache) = &args.cache {
        transfer_agent.push(Cow::Borrowed("--cache"));
        transfer_agent.push(Cow::Owned(serde_json::to_string(cache)?));
    }
    let transfer_agent = shlex::Quoter::new().join(transfer_agent.iter().map(Borrow::borrow))?;

    git::config(&current_dir, &args.location, |command| {
        command
            .arg(concat!(
                "lfs.customtransfer.",
                env!("CARGO_PKG_NAME"),
                ".path"
            ))
            .arg(path)
    })
    .await?;
    git::config(&current_dir, &args.location, |command| {
        command
            .arg(concat!(
                "lfs.customtransfer.",
                env!("CARGO_PKG_NAME"),
                ".args"
            ))
            .arg(transfer_agent)
    })
    .await?;
    git::config(&current_dir, &args.location, |command| {
        command
            .arg(concat!(
                "lfs.customtransfer.",
                env!("CARGO_PKG_NAME"),
                ".direction"
            ))
            .arg("download")
    })
    .await?;
    git::config(&current_dir, &args.location, |command| {
        command
            .arg("lfs.standalonetransferagent")
            .arg(env!("CARGO_PKG_NAME"))
    })
    .await?;

    Ok(())
}
