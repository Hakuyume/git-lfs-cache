// https://github.com/git-lfs/git-lfs/blob/main/docs/api/server-discovery.md

use super::Operation;
use crate::git;
use futures::TryFutureExt;
use headers::{Authorization, HeaderMapExt};
use http::{header, HeaderMap, HeaderName, HeaderValue};
use secrecy::ExposeSecret;
use std::fmt::Debug;
use std::path::Path;
use url::Url;

#[tracing::instrument(err, ret)]
pub async fn server_discovery<P>(
    current_dir: P,
    operation: Operation,
    remote: &str,
    authorization: bool,
) -> anyhow::Result<Response>
where
    P: AsRef<Path> + Debug,
{
    let current_dir = current_dir.as_ref();
    let (url, custom) = if let Ok(Some(url)) = git::rev_parse_show_toplevel(current_dir)
        .and_then(|toplevel| async move {
            custom_configuration(
                current_dir,
                &git::Location {
                    file: Some(toplevel.join(".lfsconfig")),
                    ..git::Location::default()
                },
                remote,
            )
            .await
        })
        .await
    {
        (url, true)
    } else if let Ok(Some(url)) =
        custom_configuration(current_dir, &git::Location::default(), remote).await
    {
        (url, true)
    } else if let Ok(url) = git::remote_get_url(current_dir, remote).await {
        (url, false)
    } else {
        (remote.parse()?, false)
    };

    match url.scheme() {
        "http" | "https" => {
            let mut header = HeaderMap::new();
            // thanks to @kmaehashi
            if let Ok(lines) = git::config(current_dir, &git::Location::default(), |command| {
                command
                    .arg("--get-urlmatch")
                    .arg("http.extraheader")
                    .arg(url.to_string())
            })
            .await
            {
                header.extend(lines.into_iter().filter_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    Some((
                        HeaderName::try_from(name.trim()).ok()?,
                        HeaderValue::try_from(value.trim()).ok()?,
                    ))
                }));
            }
            if authorization && !header.contains_key(header::AUTHORIZATION) {
                if let Ok(git::Credential {
                    username: Some(username),
                    password: Some(password),
                    ..
                }) = git::credential_fill(current_dir, &url).await
                {
                    header.typed_insert(Authorization::basic(&username, password.expose_secret()));
                }
            }

            let href = if custom {
                url
            } else {
                let mut href = url;
                href.set_path(&format!("{}.git", href.path().trim_end_matches(".git")));
                href.path_segments_mut()
                    .map_err(|_| anyhow::format_err!("cannot-be-a-base"))?
                    .push("info")
                    .push("lfs");
                href
            };

            Ok(Response { href, header })
        }
        "ssh" => {
            if authorization {
                todo!();
            } else {
                let href = if custom {
                    url
                } else {
                    let mut href = Url::parse(&format!(
                        "https://{}",
                        url.host_str()
                            .ok_or_else(|| anyhow::format_err!("missing host"))?
                    ))?;
                    href.set_port(url.port())
                        .map_err(|_| anyhow::format_err!("cannot-be-a-base"))?;
                    href.set_path(url.path());

                    href.set_path(&format!("{}.git", href.path().trim_end_matches(".git")));
                    href.path_segments_mut()
                        .map_err(|_| anyhow::format_err!("cannot-be-a-base"))?
                        .push("info")
                        .push("lfs");
                    href
                };
                Ok(Response {
                    href,
                    header: HeaderMap::new(),
                })
            }
        }
        _ => Err(anyhow::format_err!("unknown scheme")),
    }
}

#[derive(Clone, Debug)]
pub struct Response {
    pub href: Url,
    pub header: HeaderMap,
}

// https://github.com/git-lfs/git-lfs/blob/main/docs/api/server-discovery.md#custom-configuration
#[tracing::instrument(err, ret)]
async fn custom_configuration<P>(
    current_dir: P,
    location: &git::Location,
    remote: &str,
) -> anyhow::Result<Option<Url>>
where
    P: AsRef<Path> + Debug,
{
    let current_dir = current_dir.as_ref();
    let lines = if let Ok(lines) = git::config(current_dir, location, |command| {
        command.arg(format!("remote.{remote}.lfsurl"))
    })
    .await
    {
        Some(lines)
    } else if let Ok(lines) =
        git::config(current_dir, location, |command| command.arg("lfs.url")).await
    {
        Some(lines)
    } else {
        None
    };

    if let Some(lines) = lines {
        let [line] = lines
            .try_into()
            .map_err(|_| anyhow::format_err!("multiple lines"))?;
        Ok(Some(git::parse_url(&line)?))
    } else {
        Ok(None)
    }
}

#[cfg(test)]
mod tests;
