use crate::{cache, git};
use clap::Parser;
use std::borrow::{Borrow, Cow};
use std::env;

#[derive(Clone, Debug, Parser)]
pub struct Opts {
    #[clap(flatten)]
    location: git::Location,
    #[clap(long)]
    cache: Option<cache::Opts>,
}

pub async fn main(opts: Opts) -> anyhow::Result<()> {
    let path = env::current_exe()?;

    let mut args = vec![Cow::Borrowed("transfer-agent")];
    if let Some(cache) = &opts.cache {
        args.push(Cow::Borrowed("--cache"));
        args.push(Cow::Owned(serde_json::to_string(cache)?));
    }
    let args = shlex::Quoter::new().join(args.iter().map(Borrow::borrow))?;

    git::config(&opts.location, |command| {
        command
            .arg(concat!(
                "lfs.customtransfer.",
                env!("CARGO_PKG_NAME"),
                ".path"
            ))
            .arg(path)
    })
    .await?;
    git::config(&opts.location, |command| {
        command
            .arg(concat!(
                "lfs.customtransfer.",
                env!("CARGO_PKG_NAME"),
                ".args"
            ))
            .arg(args)
    })
    .await?;
    git::config(&opts.location, |command| {
        command
            .arg(concat!(
                "lfs.customtransfer.",
                env!("CARGO_PKG_NAME"),
                ".direction"
            ))
            .arg("download")
    })
    .await?;
    git::config(&opts.location, |command| {
        command
            .arg("lfs.standalonetransferagent")
            .arg(env!("CARGO_PKG_NAME"))
    })
    .await?;

    Ok(())
}
