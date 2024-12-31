use crate::misc;
use clap::Parser;
use secrecy::SecretString;
use std::collections::HashMap;
use std::fmt::{Debug, Write};
use std::path::{Path, PathBuf};
use tokio::process::Command;
use url::Url;

#[derive(Clone, Debug, Default, Parser)]
#[group(multiple = false)]
pub struct Location {
    #[clap(long, group = "config")]
    pub system: bool,
    #[clap(long, group = "config")]
    pub global: bool,
    #[clap(long, group = "config")]
    pub local: bool,
    #[clap(long)]
    pub worktree: bool,
    #[clap(long)]
    pub file: Option<PathBuf>,
}

#[tracing::instrument(err, ret, skip(f))]
pub async fn config<P, F>(current_dir: P, location: &Location, f: F) -> anyhow::Result<Vec<String>>
where
    P: AsRef<Path> + Debug,
    F: FnOnce(&mut Command) -> &mut Command,
{
    let mut command = Command::new("git");
    command.current_dir(current_dir).arg("config");
    if location.system {
        command.arg("--system");
    }
    if location.global {
        command.arg("--global");
    }
    if location.local {
        command.arg("--local");
    }
    if location.worktree {
        command.arg("--worktree");
    }
    if let Some(file) = &location.file {
        command.arg("--file").arg(file);
    }
    let stdout = misc::spawn(f(&mut command), None).await?;
    Ok(String::from_utf8(stdout)?
        .lines()
        .map(ToString::to_string)
        .collect())
}

#[derive(Debug)]
pub struct Credential {
    pub username: Option<String>,
    pub password: Option<SecretString>,
}

#[tracing::instrument(err, ret)]
pub async fn credential_fill<P>(current_dir: P, url: &Url) -> anyhow::Result<Credential>
where
    P: AsRef<Path> + Debug,
{
    // https://git-scm.com/docs/git-credential#IOFMT
    let inputs = [
        ("protocol", url.scheme()),
        ("host", url.authority()),
        ("path", url.path().trim_start_matches('/')),
    ]
    .into_iter()
    .fold(String::new(), |mut inputs, (key, value)| {
        let _ = writeln!(inputs, "{key}={value}");
        inputs
    });

    let stdout = misc::spawn(
        Command::new("git")
            .current_dir(current_dir)
            .arg("credential")
            .arg("fill"),
        Some(inputs.as_bytes()),
    )
    .await?;

    // https://git-scm.com/docs/git-credential#IOFMT
    let stdout = String::from_utf8(stdout)?;
    let outputs = stdout
        .lines()
        .filter_map(|line| line.split_once('='))
        .collect::<HashMap<_, _>>();
    Ok(Credential {
        username: outputs.get("username").map(ToString::to_string),
        password: outputs
            .get("password")
            .copied()
            .map(Box::from)
            .map(SecretString::new),
    })
}

#[tracing::instrument(err, ret)]
pub fn parse_url(s: &str) -> anyhow::Result<Url> {
    match s.parse() {
        Ok(url) => Ok(url),
        Err(e) => {
            if let Some(Ok(url)) = s
                .split_once(':')
                .map(|(authority, path)| format!("ssh://{authority}/{path}").parse())
            {
                // scp-like syntax
                Ok(url)
            } else {
                Err(e.into())
            }
        }
    }
}

#[tracing::instrument(err, ret)]
pub async fn remote_get_url<P>(current_dir: P, remote: &str) -> anyhow::Result<Url>
where
    P: AsRef<Path> + Debug,
{
    let stdout = misc::spawn(
        Command::new("git")
            .current_dir(current_dir)
            .arg("remote")
            .arg("get-url")
            .arg(remote),
        None,
    )
    .await?;
    parse_url(String::from_utf8(stdout)?.trim())
}

#[tracing::instrument(err, ret)]
pub async fn rev_parse_absolute_git_dir<P>(current_dir: P) -> anyhow::Result<PathBuf>
where
    P: AsRef<Path> + Debug,
{
    let stdout = misc::spawn(
        Command::new("git")
            .current_dir(current_dir)
            .arg("rev-parse")
            .arg("--absolute-git-dir"),
        None,
    )
    .await?;
    Ok(String::from_utf8(stdout)?.trim().parse()?)
}

#[tracing::instrument(err, ret)]
pub async fn rev_parse_show_toplevel<P>(current_dir: P) -> anyhow::Result<PathBuf>
where
    P: AsRef<Path> + Debug,
{
    let stdout = misc::spawn(
        Command::new("git")
            .current_dir(current_dir)
            .arg("rev-parse")
            .arg("--show-toplevel"),
        None,
    )
    .await?;
    Ok(String::from_utf8(stdout)?.trim().parse()?)
}
