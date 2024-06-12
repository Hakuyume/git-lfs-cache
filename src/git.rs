use clap::Parser;
use http::Uri;
use secrecy::Secret;
use std::collections::HashMap;
use std::fmt::Write;
use std::iter;
use std::path::PathBuf;
use std::process::Stdio;
use tokio::process::Command;

#[derive(Clone, Debug, Default, Parser)]
#[group(multiple = false)]
pub(super) struct Location {
    #[clap(long, group = "config")]
    system: bool,
    #[clap(long, group = "config")]
    global: bool,
    #[clap(long, group = "config")]
    local: bool,
    #[clap(long)]
    worktree: bool,
    #[clap(long)]
    file: Option<PathBuf>,
}

#[tracing::instrument(err, ret, skip(f))]
pub async fn config<F>(location: &Location, f: F) -> anyhow::Result<Vec<String>>
where
    F: FnOnce(&mut Command) -> &mut Command,
{
    let mut command = Command::new("git");
    command.arg("config");
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
    let output = f(&mut command).stdin(Stdio::null()).output().await?;
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
pub async fn credential_fill(url: &Uri) -> anyhow::Result<Credential> {
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
        .arg("credential")
        .arg("fill")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
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
pub async fn remote_get_url(remote: &str) -> anyhow::Result<Uri> {
    let output = Command::new("git")
        .arg("remote")
        .arg("get-url")
        .arg(remote)
        .stdin(Stdio::null())
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
pub async fn rev_parse_git_dir() -> anyhow::Result<PathBuf> {
    let output = Command::new("git")
        .arg("rev-parse")
        .arg("--git-dir")
        .stdin(Stdio::null())
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
