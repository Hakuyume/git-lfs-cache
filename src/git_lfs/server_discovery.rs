// https://github.com/git-lfs/git-lfs/blob/main/docs/api/server-discovery.md

use super::Operation;
use crate::{git, misc};
use futures::TryFutureExt;
use headers::{Authorization, HeaderMapExt};
use http::{header, HeaderMap, HeaderName, HeaderValue, Uri};
use secrecy::ExposeSecret;
use std::fmt::Debug;
use std::path::Path;

#[tracing::instrument(err, ret)]
pub async fn server_discovery<P>(
    current_dir: P,
    operation: Operation,
    remote: &str,
) -> anyhow::Result<Response>
where
    P: AsRef<Path> + Debug,
{
    let current_dir = current_dir.as_ref();
    let url = if let Ok(Some(url)) = git::rev_parse_show_toplevel(current_dir)
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
        url
    } else if let Ok(Some(url)) =
        custom_configuration(current_dir, &git::Location::default(), remote).await
    {
        url
    } else if let Ok(url) = git::remote_get_url(current_dir, remote).await {
        url
    } else {
        remote.parse()?
    };

    match url.scheme_str() {
        Some("http") | Some("https") => {
            let href = misc::patch_path(url.clone(), |path| {
                format!("{}.git/info/lfs", path.trim_end_matches(".git"))
            })?;

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
            if !header.contains_key(header::AUTHORIZATION) {
                if let Ok(git::Credential {
                    username: Some(username),
                    password: Some(password),
                    ..
                }) = git::credential_fill(current_dir, &url).await
                {
                    header.typed_insert(Authorization::basic(&username, password.expose_secret()));
                }
            }

            Ok(Response { href, header })
        }
        // TODO: support ssh
        _ => Err(anyhow::format_err!("unknown scheme")),
    }
}

#[derive(Clone, Debug)]
pub struct Response {
    pub href: Uri,
    pub header: HeaderMap,
}

// https://github.com/git-lfs/git-lfs/blob/main/docs/api/server-discovery.md#custom-configuration
#[tracing::instrument(err, ret)]
async fn custom_configuration<P>(
    current_dir: P,
    location: &git::Location,
    remote: &str,
) -> anyhow::Result<Option<Uri>>
where
    P: AsRef<Path> + Debug,
{
    let current_dir = current_dir.as_ref();
    if let Ok(lines) = git::config(current_dir, location, |command| {
        command.arg(format!("remote.{remote}.lfsurl"))
    })
    .await
    {
        let [line] = lines
            .try_into()
            .map_err(|_| anyhow::format_err!("multiple lines"))?;
        Ok(Some(line.parse()?))
    } else if let Ok(lines) =
        git::config(current_dir, location, |command| command.arg("lfs.url")).await
    {
        let [line] = lines
            .try_into()
            .map_err(|_| anyhow::format_err!("multiple lines"))?;
        Ok(Some(line.parse()?))
    } else {
        Ok(None)
    }
}
