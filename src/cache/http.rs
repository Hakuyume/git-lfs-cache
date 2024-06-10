use crate::{git_lfs, misc, writer};
use bytes::Bytes;
use futures::{Stream, TryStreamExt};
use headers::HeaderMapExt;
use http::{header, Request, Uri};
use http_body::Frame;
use http_body_util::{BodyExt, Empty, StreamBody};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::PathBuf;
use tokio::fs;

pub struct Cache {
    client: misc::Client,
    endpoint: Uri,
    authorization: Option<Authorization>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Opts {
    #[serde(with = "http_serde::uri")]
    endpoint: Uri,
    authorization: Option<Authorization>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Source {
    #[serde(with = "http_serde::uri")]
    url: Uri,
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
        f.debug_struct("GoogleCloudStorage")
            .field("url", &self.endpoint)
            .finish()
    }
}

impl Cache {
    pub async fn new(opts: Opts) -> anyhow::Result<Self> {
        Ok(Self {
            client: misc::client()?,
            endpoint: opts.endpoint,
            authorization: opts.authorization,
        })
    }

    #[tracing::instrument(err, ret)]
    pub async fn get(
        &self,
        oid: &str,
        size: u64,
        mut writer: writer::Writer,
    ) -> anyhow::Result<(PathBuf, Source)> {
        let url = self.url(oid)?;

        let builder = Request::get(url.clone());
        let builder = self.authorization(builder).await?;
        let request = builder.body(Empty::new().map_err(Box::from).boxed_unsync())?;
        let response = self.client.request(request).await?;
        let (parts, mut body) = response.into_parts();
        if parts.status.is_success() {
            while let Some(frame) = body.frame().await.transpose()? {
                if let Ok(data) = frame.into_data() {
                    writer.write(&data).await?;
                }
            }
            Ok((writer.finish().await?, Source { url }))
        } else {
            let body = body.collect().await?.to_bytes();
            Err(git_lfs::Error {
                code: parts.status,
                message: format!("{body:?}"),
            }
            .into())
        }
    }

    #[tracing::instrument(err, ret, skip(body))]
    pub async fn put<B, E>(&self, oid: &str, size: u64, body: B) -> anyhow::Result<()>
    where
        B: Stream<Item = Result<Bytes, E>> + Send + Sync + 'static,
        anyhow::Error: From<E>,
    {
        let url = self.url(oid)?;

        let builder = Request::put(url).header(header::CONTENT_LENGTH, size);
        let builder = self.authorization(builder).await?;
        let request = builder.body(
            BodyExt::map_err(StreamBody::new(body.map_ok(Frame::data)), |e| {
                Box::from(anyhow::Error::from(e))
            })
            .boxed_unsync(),
        )?;
        let response = self.client.request(request).await?;
        let (parts, body) = response.into_parts();
        if parts.status.is_success() {
            Ok(())
        } else {
            let body = body.collect().await?.to_bytes();
            Err(git_lfs::Error {
                code: parts.status,
                message: format!("{body:?}"),
            }
            .into())
        }
    }

    fn url(&self, oid: &str) -> anyhow::Result<Uri> {
        Ok(misc::patch_path(self.endpoint.clone(), |path| {
            format!("{path}{oid}")
        })?)
    }

    async fn authorization(
        &self,
        mut builder: http::request::Builder,
    ) -> anyhow::Result<http::request::Builder> {
        if let Some(headers) = builder.headers_mut() {
            match &self.authorization {
                Some(Authorization::Bearer(bearer)) => {
                    let token = match bearer {
                        Bearer::TokenPath(path) => fs::read_to_string(path).await?,
                    };
                    headers.typed_insert(headers::Authorization::bearer(token.trim())?);
                }
                None => (),
            }
        }
        Ok(builder)
    }
}
