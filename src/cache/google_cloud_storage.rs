use crate::{git_lfs, misc, writer};
use bytes::Bytes;
use futures::{Stream, TryStreamExt};
use headers::HeaderMapExt;
use http::{header, Request};
use http_body::Frame;
use http_body_util::{BodyExt, Empty, StreamBody};
use serde::{Deserialize, Serialize};
use std::env;
use std::fmt;
use std::path::PathBuf;
use url::Url;

pub struct Cache {
    client: misc::Client,
    authenticator: yup_oauth2::authenticator::DefaultAuthenticator,
    bucket: String,
    prefix: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Opts {
    bucket: String,
    prefix: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Source {
    bucket: String,
    name: String,
}

impl fmt::Debug for Cache {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GoogleCloudStorage")
            .field("bucket", &self.bucket)
            .field("prefix", &self.prefix)
            .finish()
    }
}

impl Cache {
    pub async fn new(opts: Opts) -> anyhow::Result<Self> {
        let client = misc::client()?;

        let authenticator = if let Ok(path) = env::var("GOOGLE_APPLICATION_CREDENTIALS") {
            if let Ok(secret) = yup_oauth2::read_authorized_user_secret(&path).await {
                yup_oauth2::AuthorizedUserAuthenticator::builder(secret)
                    .build()
                    .await?
            } else if let Ok(secret) = yup_oauth2::read_external_account_secret(&path).await {
                yup_oauth2::ExternalAccountAuthenticator::builder(secret)
                    .build()
                    .await?
            } else {
                anyhow::bail!("unknown credentials type")
            }
        } else {
            // https://cloud.google.com/compute/docs/access/create-enable-service-accounts-for-instances#applications
            let request = Request::get(concat!(
                "http://metadata.google.internal/computeMetadata/v1",
                "/instance/service-accounts/default/token",
            ))
            .header("Metadata-Flavor", "Google")
            .body(Empty::new().map_err(Box::from).boxed_unsync())?;
            let response = client.request(request).await?;
            let (parts, body) = response.into_parts();
            let body = body.collect().await?.to_bytes();
            let access_token = if parts.status.is_success() {
                #[derive(Deserialize)]
                struct B {
                    access_token: String,
                }

                let B { access_token } = serde_json::from_slice(&body)?;
                Ok(access_token)
            } else {
                Err(git_lfs::Error {
                    code: parts.status,
                    message: format!("{body:?}"),
                })
            }?;

            yup_oauth2::AccessTokenAuthenticator::builder(access_token)
                .build()
                .await?
        };

        Ok(Self {
            client,
            authenticator,
            bucket: opts.bucket,
            prefix: opts.prefix,
        })
    }

    #[tracing::instrument(err, ret)]
    pub async fn get(
        &self,
        oid: &str,
        size: u64,
        mut writer: writer::Writer,
    ) -> anyhow::Result<(PathBuf, Source)> {
        let name = self.name(oid);

        // https://cloud.google.com/storage/docs/json_api/v1/objects/get
        let mut url = Url::parse_with_params(
            "https://storage.googleapis.com/storage/v1/b",
            [("alt", "media")],
        )?;
        misc::path_segments_mut(&mut url)?
            .push(&self.bucket)
            .push("o")
            .push(&name);
        let builder = Request::get(url.as_ref());
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
            Ok((
                writer.finish().await?,
                Source {
                    bucket: self.bucket.clone(),
                    name,
                },
            ))
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
        // https://cloud.google.com/storage/docs/json_api/v1/objects/insert
        let mut url = Url::parse_with_params(
            "https://storage.googleapis.com/upload/storage/v1/b",
            [("uploadType", "media"), ("name", &self.name(oid))],
        )?;
        misc::path_segments_mut(&mut url)?
            .push(&self.bucket)
            .push("o");
        let builder = Request::post(url.as_ref()).header(header::CONTENT_LENGTH, size);
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

    fn name(&self, oid: &str) -> String {
        if let Some(prefix) = &self.prefix {
            format!("{prefix}{oid}")
        } else {
            oid.to_string()
        }
    }

    async fn authorization(
        &self,
        mut builder: http::request::Builder,
    ) -> anyhow::Result<http::request::Builder> {
        if let Some(headers) = builder.headers_mut() {
            let token = self
                .authenticator
                .token(&["https://www.googleapis.com/auth/cloud-platform"])
                .await?;
            headers.typed_insert(headers::Authorization::bearer(
                token
                    .token()
                    .ok_or_else(|| anyhow::format_err!("missing token"))?,
            )?);
        }
        Ok(builder)
    }
}
