mod filesystem;
mod google_cloud_storage;
mod http;

use crate::channel;
use futures::TryFutureExt;
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
pub enum Args {
    Filesystem(filesystem::Args),
    GoogleCloudStorage(google_cloud_storage::Args),
    Http(http::Args),
}

impl FromStr for Args {
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
    pub async fn new(args: Args) -> anyhow::Result<Self> {
        match args {
            Args::Filesystem(args) => filesystem::Cache::new(args).map_ok(Self::Filesystem).await,
            Args::GoogleCloudStorage(args) => {
                google_cloud_storage::Cache::new(args)
                    .map_ok(Self::GoogleCloudStorage)
                    .await
            }
            Args::Http(args) => http::Cache::new(args).map_ok(Self::Http).await,
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

    pub async fn put(
        &self,
        oid: &str,
        size: u64,
        reader: &channel::Reader<'_>,
    ) -> anyhow::Result<()> {
        match self {
            Self::Filesystem(cache) => cache.put(oid, size, reader).await,
            Self::GoogleCloudStorage(cache) => cache.put(oid, size, reader).await,
            Self::Http(cache) => cache.put(oid, size, reader).await,
        }
    }
}
