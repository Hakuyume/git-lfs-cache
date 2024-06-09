mod filesystem;
mod google_cloud_storage;

use crate::writer;
use bytes::Bytes;
use futures::{Stream, TryFutureExt};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::str::FromStr;

#[derive(Debug)]
pub enum Cache {
    Filesystem(filesystem::Cache),
    GoogleCloudStorage(google_cloud_storage::Cache),
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Opts {
    Filesystem(filesystem::Opts),
    GoogleCloudStorage(google_cloud_storage::Opts),
}

impl FromStr for Opts {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        serde_json::from_str(s).map_err(|e| e.to_string())
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub enum Source {
    Filesystem(filesystem::Source),
    GoogleCloudStorage(google_cloud_storage::Source),
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
        }
    }

    pub async fn get(
        &self,
        oid: &str,
        size: u64,
        writer: writer::Writer,
    ) -> anyhow::Result<(PathBuf, Source)> {
        match self {
            Self::Filesystem(cache) => {
                cache
                    .get(oid, size, writer)
                    .map_ok(|(path, source)| (path, Source::Filesystem(source)))
                    .await
            }
            Self::GoogleCloudStorage(cache) => {
                cache
                    .get(oid, size, writer)
                    .map_ok(|(path, source)| (path, Source::GoogleCloudStorage(source)))
                    .await
            }
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
        }
    }
}
