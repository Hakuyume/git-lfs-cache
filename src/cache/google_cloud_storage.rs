use crate::{channel, git_lfs, misc};
use futures::{TryFutureExt, TryStreamExt};
use headers::ContentLength;
use http_body::Frame;
use http_body_util::{BodyExt, StreamBody};
use serde::{Deserialize, Serialize};
use std::fmt;
use tower::Layer;

pub struct Cache {
    service: google_cloud_storage::middleware::yup_oauth2::Service<misc::Client, misc::Connector>,
    bucket: String,
    prefix: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Args {
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
        f.debug_struct("Cache")
            .field("bucket", &self.bucket)
            .field("prefix", &self.prefix)
            .finish()
    }
}

impl Cache {
    pub async fn new(args: Args) -> anyhow::Result<Self> {
        let connector = misc::connector()?;
        let client = misc::client(connector.clone());
        let service = google_cloud_storage::middleware::yup_oauth2::with_connector(connector)
            .await?
            .layer(client);
        Ok(Self {
            service,
            bucket: args.bucket,
            prefix: args.prefix,
        })
    }

    #[tracing::instrument(err, ret)]
    pub async fn get(
        &self,
        oid: &str,
        size: u64,
        mut writer: channel::Writer<'_>,
    ) -> anyhow::Result<Source> {
        let name = self.name(oid);
        let response = google_cloud_storage::api::xml::get_object::builder(&self.bucket, &name)
            .send(self.service.clone())
            .map_err(map_err)
            .await?;
        let mut body = response.into_body();
        while let Some(frame) = body.frame().await.transpose()? {
            if let Ok(data) = frame.into_data() {
                writer.write(&data).await?;
            }
        }
        writer.finish().await?;
        Ok(Source {
            bucket: self.bucket.clone(),
            name,
        })
    }

    #[tracing::instrument(err, ret)]
    pub async fn put(
        &self,
        oid: &str,
        size: u64,
        reader: &channel::Reader<'_>,
    ) -> anyhow::Result<()> {
        let body = BodyExt::map_err(StreamBody::new(reader.stream()?.map_ok(Frame::data)), |e| {
            Box::from(anyhow::Error::from(e))
        })
        .boxed_unsync();
        google_cloud_storage::api::xml::put_object::builder(&self.bucket, self.name(oid), body)
            .header(ContentLength(size))
            .send(self.service.clone())
            .map_err(map_err)
            .await?;
        Ok(())
    }

    fn name(&self, oid: &str) -> String {
        if let Some(prefix) = &self.prefix {
            format!("{prefix}{oid}")
        } else {
            oid.to_string()
        }
    }
}

fn map_err<S, B>(e: google_cloud_storage::api::Error<S, B>) -> anyhow::Error
where
    S: std::error::Error + Send + Sync + 'static,
    B: std::error::Error + Send + Sync + 'static,
{
    match e {
        google_cloud_storage::api::Error::Api(e) => {
            let (parts, body) = e.into_parts();
            git_lfs::Error {
                code: parts.status,
                message: format!("{body:?}"),
            }
            .into()
        }
        e => e.into(),
    }
}
