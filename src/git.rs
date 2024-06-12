use clap::Parser;
use http::Uri;
use secrecy::Secret;
use std::collections::HashMap;
use std::fmt::{Debug, Write};
use std::iter;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::process::Command;

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
    command
        .current_dir(current_dir)
        .stdin(Stdio::null())
        .arg("config");
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
    let output = f(&mut command).output().await?;
    if output.status.success() {
        Ok(String::from_utf8(output.stdout)?
            .lines()
            .map(ToString::to_string)
            .collect())
    } else {
        Err(anyhow::format_err!(
            String::from_utf8_lossy(&output.stderr).into_owned()
        ))
    }
}

#[derive(Debug)]
pub struct Credential {
    pub username: Option<String>,
    pub password: Option<Secret<String>>,
}

#[tracing::instrument(err, ret)]
pub async fn credential_fill<P>(current_dir: P, url: &Uri) -> anyhow::Result<Credential>
where
    P: AsRef<Path> + Debug,
{
    // https://git-scm.com/docs/git-credential#IOFMT
    let inputs = url
        .scheme_str()
        .map(|scheme| ("protocol", scheme))
        .into_iter()
        .chain(url.authority().map(|authority| ("host", authority.host())))
        .chain(iter::once(("path", url.path().trim_start_matches('/'))))
        .fold(String::new(), |mut inputs, (key, value)| {
            let _ = writeln!(inputs, "{key}={value}");
            inputs
        });

    let mut child = Command::new("git")
        .current_dir(current_dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .arg("credential")
        .arg("fill")
        .spawn()?;

    let stdin = child.stdin.take();
    let (output, _) = futures::future::try_join(child.wait_with_output(), async {
        if let Some(mut stdin) = stdin {
            tokio::io::copy(&mut inputs.as_bytes(), &mut stdin).await?;
        }
        Ok(())
    })
    .await?;

    if output.status.success() {
        // https://git-scm.com/docs/git-credential#IOFMT
        let outputs = String::from_utf8(output.stdout)?;
        let outputs = outputs
            .lines()
            .filter_map(|line| line.split_once('='))
            .collect::<HashMap<_, _>>();
        Ok(Credential {
            username: outputs.get("username").map(ToString::to_string),
            password: outputs
                .get("password")
                .map(ToString::to_string)
                .map(Secret::new),
        })
    } else {
        Err(anyhow::format_err!(
            String::from_utf8_lossy(&output.stderr).into_owned()
        ))
    }
}

#[tracing::instrument(err, ret(Display))]
pub async fn remote_get_url<P>(current_dir: P, remote: &str) -> anyhow::Result<Uri>
where
    P: AsRef<Path> + Debug,
{
    let output = Command::new("git")
        .current_dir(current_dir)
        .stdin(Stdio::null())
        .arg("remote")
        .arg("get-url")
        .arg(remote)
        .output()
        .await?;
    if output.status.success() {
        Ok(String::from_utf8(output.stdout)?.trim().parse()?)
    } else {
        Err(anyhow::format_err!(
            String::from_utf8_lossy(&output.stderr).into_owned()
        ))
    }
}

#[tracing::instrument(err, ret)]
pub async fn rev_parse_absolute_git_dir<P>(current_dir: P) -> anyhow::Result<PathBuf>
where
    P: AsRef<Path> + Debug,
{
    let output = Command::new("git")
        .current_dir(current_dir)
        .stdin(Stdio::null())
        .arg("rev-parse")
        .arg("--absolute-git-dir")
        .output()
        .await?;
    if output.status.success() {
        Ok(String::from_utf8(output.stdout)?.trim().into())
    } else {
        Err(anyhow::format_err!(
            String::from_utf8_lossy(&output.stderr).into_owned()
        ))
    }
}

#[tracing::instrument(err, ret)]
pub async fn rev_parse_show_toplevel<P>(current_dir: P) -> anyhow::Result<PathBuf>
where
    P: AsRef<Path> + Debug,
{
    let output = Command::new("git")
        .current_dir(current_dir)
        .stdin(Stdio::null())
        .arg("rev-parse")
        .arg("--show-toplevel")
        .output()
        .await?;
    if output.status.success() {
        Ok(String::from_utf8(output.stdout)?.trim().into())
    } else {
        Err(anyhow::format_err!(
            String::from_utf8_lossy(&output.stderr).into_owned()
        ))
    }
}
