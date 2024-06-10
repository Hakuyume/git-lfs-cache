// https://github.com/git-lfs/git-lfs/blob/main/docs/api/server-discovery.md

use super::Operation;
use crate::{git, misc};
use headers::{Authorization, HeaderMapExt};
use http::{HeaderMap, HeaderName, HeaderValue, Uri};
use secrecy::ExposeSecret;

#[tracing::instrument(err, ret)]
pub async fn server_discovery(url: &Uri, operation: Operation) -> anyhow::Result<Response> {
    match url.scheme_str() {
        Some("http") | Some("https") => {
            let href = misc::patch_path(url.clone(), |path| {
                format!("{}.git/info/lfs", path.trim_end_matches(".git"))
            })?;

            let mut header = HeaderMap::new();
            if let Ok(git::Credential {
                username: Some(username),
                password: Some(password),
                ..
            }) = git::credential_fill(url).await
            {
                header.typed_insert(Authorization::basic(&username, password.expose_secret()));
            }
            // thanks to @kmaehashi
            if let Ok(lines) = git::config_get_urlmatch("http.extraheader", url).await {
                header.extend(lines.into_iter().filter_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    Some((
                        HeaderName::try_from(name.trim()).ok()?,
                        HeaderValue::try_from(value.trim()).ok()?,
                    ))
                }));
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
