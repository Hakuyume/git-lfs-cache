use crate::{channel, git_lfs, misc};
use futures::{TryFutureExt, TryStreamExt};
use headers::HeaderMapExt;
use http::{header, Request, StatusCode};
use http_body::Frame;
use http_body_util::{BodyExt, Empty, StreamBody};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::PathBuf;
use tokio::fs;
use tokio::sync::Mutex;
use url::Url;

pub struct Cache {
    client: misc::Client,
    endpoint: Url,
    authorization: Option<Authorization>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Args {
    endpoint: Url,
    authorization: Option<Authorization>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Source {
    url: Url,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum Authorization {
    Bearer(Bearer),
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum Bearer {
    TokenPath(PathBuf),
}

impl fmt::Debug for Cache {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Cache").field("url", &self.endpoint).finish()
    }
}

impl Cache {
    pub async fn new(args: Args) -> anyhow::Result<Self> {
        Ok(Self {
            client: misc::client(misc::connector()?),
            endpoint: args.endpoint,
            authorization: args.authorization,
        })
    }

    #[tracing::instrument(err, ret)]
    pub async fn get(
        &self,
        oid: &str,
        size: u64,
        writer: channel::Writer<'_>,
    ) -> anyhow::Result<Source> {
        let url = self.url(oid)?;
        let writer = Mutex::new(writer);

        backoff::future::retry(backoff::ExponentialBackoff::default(), || {
            let url = &url;
            let writer = &writer;
            async move {
                let builder = Request::get(url.as_ref());
                let builder = self.authorization(builder).await?;
                let request = builder
                    .body(Empty::new().map_err(Box::from).boxed_unsync())
                    .map_err(misc::backoff_permanent)?;
                let response = self
                    .client
                    .request(request)
                    .map_err(misc::backoff_transient)
                    .await?;
                let (parts, mut body) = response.into_parts();
                if parts.status.is_success() {
                    let mut writer = writer.lock().await;
                    writer.reset().map_err(misc::backoff_permanent).await?;
                    while let Some(frame) = body
                        .frame()
                        .await
                        .transpose()
                        .map_err(misc::backoff_transient)?
                    {
                        if let Ok(data) = frame.into_data() {
                            writer.write(&data).map_err(misc::backoff_permanent).await?;
                        }
                    }
                    Ok(())
                } else {
                    let body = body
                        .collect()
                        .map_err(misc::backoff_transient)
                        .await?
                        .to_bytes();
                    let e = git_lfs::Error {
                        code: parts.status,
                        message: format!("{body:?}"),
                    };
                    if parts.status == StatusCode::REQUEST_TIMEOUT || parts.status.is_server_error()
                    {
                        Err(misc::backoff_transient(e))
                    } else {
                        Err(misc::backoff_permanent(e))
                    }
                }
            }
        })
        .await?;
        writer.into_inner().finish().await?;
        Ok(Source { url })
    }

    #[tracing::instrument(err, ret)]
    pub async fn put(
        &self,
        oid: &str,
        size: u64,
        reader: &channel::Reader<'_>,
    ) -> anyhow::Result<()> {
        let url = self.url(oid)?;

        backoff::future::retry(backoff::ExponentialBackoff::default(), || async {
            let builder = Request::put(url.as_ref()).header(header::CONTENT_LENGTH, size);
            let builder = self.authorization(builder).await?;
            let request = builder
                .body(
                    BodyExt::map_err(
                        StreamBody::new(
                            reader
                                .stream()
                                .map_err(misc::backoff_permanent)?
                                .map_ok(Frame::data),
                        ),
                        |e| Box::from(anyhow::Error::from(e)),
                    )
                    .boxed_unsync(),
                )
                .map_err(misc::backoff_permanent)?;
            let response = self
                .client
                .request(request)
                .map_err(misc::backoff_transient)
                .await?;
            let (parts, body) = response.into_parts();
            if parts.status.is_success() {
                Ok(())
            } else {
                let body = body
                    .collect()
                    .map_err(misc::backoff_transient)
                    .await?
                    .to_bytes();
                let e = git_lfs::Error {
                    code: parts.status,
                    message: format!("{body:?}"),
                };
                if parts.status == StatusCode::REQUEST_TIMEOUT || parts.status.is_server_error() {
                    Err(misc::backoff_transient(e))
                } else {
                    Err(misc::backoff_permanent(e))
                }
            }
        })
        .await
    }

    fn url(&self, oid: &str) -> anyhow::Result<Url> {
        let mut url = self.endpoint.clone();
        misc::path_segments_mut(&mut url)?.push(oid);
        Ok(url)
    }

    async fn authorization(
        &self,
        mut builder: http::request::Builder,
    ) -> Result<http::request::Builder, backoff::Error<anyhow::Error>> {
        if let Some(headers) = builder.headers_mut() {
            match &self.authorization {
                Some(Authorization::Bearer(bearer)) => {
                    let token = match bearer {
                        Bearer::TokenPath(path) => {
                            fs::read_to_string(path)
                                .map_err(anyhow::Error::from)
                                .map_err(backoff::Error::permanent)
                                .await?
                        }
                    };
                    headers.typed_insert(
                        headers::Authorization::bearer(token.trim())
                            .map_err(anyhow::Error::from)
                            .map_err(backoff::Error::permanent)?,
                    );
                }
                None => (),
            }
        }
        Ok(builder)
    }
}
