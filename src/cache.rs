mod filesystem;

use crate::writer;
use bytes::Bytes;
use futures::Stream;
use futures::TryFutureExt;
use serde::Deserialize;
use std::path::PathBuf;
use std::str::FromStr;

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Opts {
    Filesystem(filesystem::Opts),
}

impl FromStr for Opts {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        serde_json::from_str(s).map_err(|e| e.to_string())
    }
}

#[derive(Debug)]
pub enum Cache {
    Filesystem(filesystem::Cache),
}

impl Cache {
    pub async fn new(opts: Opts) -> anyhow::Result<Self> {
        match opts {
            Opts::Filesystem(opts) => filesystem::Cache::new(opts).map_ok(Cache::Filesystem).await,
        }
    }

    pub async fn get(
        &self,
        oid: &str,
        size: u64,
        writer: writer::Writer,
    ) -> anyhow::Result<PathBuf> {
        match self {
            Self::Filesystem(cache) => cache.get(oid, size, writer).await,
        }
    }

    pub async fn put<B, E>(&self, oid: &str, size: u64, body: B) -> anyhow::Result<()>
    where
        B: Stream<Item = Result<Bytes, E>>,
        anyhow::Error: From<E>,
    {
        match self {
            Self::Filesystem(cache) => cache.put(oid, size, body).await,
        }
    }
}
