// https://github.com/git-lfs/git-lfs/blob/main/docs/api/server-discovery.md

use super::Operation;
use crate::{git, misc};
use headers::{Authorization, HeaderMapExt};
use http::{header, HeaderMap, HeaderName, HeaderValue, Uri};
use secrecy::ExposeSecret;

#[tracing::instrument(err, ret)]
pub async fn server_discovery(url: &Uri, operation: Operation) -> anyhow::Result<Response> {
    match url.scheme_str() {
        Some("http") | Some("https") => {
            let href = misc::patch_path(url.clone(), |path| {
                format!("{}.git/info/lfs", path.trim_end_matches(".git"))
            })?;

            let mut header = HeaderMap::new();
            // thanks to @kmaehashi
            if let Ok(lines) = git::config(&git::Location::default(), |command| {
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
                }) = git::credential_fill(url).await
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
