mod filesystem;
mod google_cloud_storage;
mod http;

use crate::channel;
use bytes::Bytes;
use futures::{Stream, TryFutureExt};
use serde::{Deserialize, Serialize};
use std::str::FromStr;

#[derive(Debug)]
pub enum Cache {
    Filesystem(filesystem::Cache),
    GoogleCloudStorage(google_cloud_storage::Cache),
    Http(http::Cache),
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Opts {
    Filesystem(filesystem::Opts),
    GoogleCloudStorage(google_cloud_storage::Opts),
    Http(http::Opts),
}

impl FromStr for Opts {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        serde_json::from_str(s).map_err(|e| e.to_string())
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    Filesystem(filesystem::Source),
    GoogleCloudStorage(google_cloud_storage::Source),
    Http(http::Source),
}

impl Cache {
    pub async fn new(opts: Opts) -> anyhow::Result<Self> {
        match opts {
            Opts::Filesystem(opts) => filesystem::Cache::new(opts).map_ok(Self::Filesystem).await,
            Opts::GoogleCloudStorage(opts) => {
                google_cloud_storage::Cache::new(opts)
                    .map_ok(Self::GoogleCloudStorage)
                    .await
            }
            Opts::Http(opts) => http::Cache::new(opts).map_ok(Self::Http).await,
        }
    }

    pub async fn get(
        &self,
        oid: &str,
        size: u64,
        writer: channel::Writer<'_>,
    ) -> anyhow::Result<Source> {
        match self {
            Self::Filesystem(cache) => {
                cache
                    .get(oid, size, writer)
                    .map_ok(Source::Filesystem)
                    .await
            }
            Self::GoogleCloudStorage(cache) => {
                cache
                    .get(oid, size, writer)
                    .map_ok(Source::GoogleCloudStorage)
                    .await
            }
            Self::Http(cache) => cache.get(oid, size, writer).map_ok(Source::Http).await,
        }
    }

    pub async fn put<B, E>(&self, oid: &str, size: u64, body: B) -> anyhow::Result<()>
    where
        B: Stream<Item = Result<Bytes, E>> + Send + Sync + 'static,
        anyhow::Error: From<E>,
    {
        match self {
            Self::Filesystem(cache) => cache.put(oid, size, body).await,
            Self::GoogleCloudStorage(cache) => cache.put(oid, size, body).await,
            Self::Http(cache) => cache.put(oid, size, body).await,
        }
    }
}
