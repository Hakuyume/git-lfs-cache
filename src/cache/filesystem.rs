use crate::channel;
use futures::TryStreamExt;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::pin;
use tokio::fs::{self, File};
use tokio::io::{AsyncBufReadExt, BufReader};

#[derive(Debug)]
pub struct Cache {
    dir: PathBuf,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Opts {
    dir: PathBuf,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Source {
    path: PathBuf,
}

impl Cache {
    pub async fn new(opts: Opts) -> anyhow::Result<Self> {
        fs::create_dir_all(&opts.dir).await?;
        Ok(Self {
            dir: opts.dir.canonicalize()?,
        })
    }

    #[tracing::instrument(err, ret)]
    pub async fn get(
        &self,
        oid: &str,
        size: u64,
        mut writer: channel::Writer<'_>,
    ) -> anyhow::Result<Source> {
        let path = self.path(oid);
        let mut reader = BufReader::new(File::open(&path).await?);
        loop {
            let data = reader.fill_buf().await?;
            if data.is_empty() {
                break;
            } else {
                let len = data.len();
                writer.write(data).await?;
                reader.consume(len);
            }
        }
        writer.finish().await?;
        Ok(Source { path })
    }

    #[tracing::instrument(err, ret)]
    pub async fn put(
        &self,
        oid: &str,
        size: u64,
        reader: &channel::Reader<'_>,
    ) -> anyhow::Result<()> {
        let path = self.path(oid);

        let parent = path
            .parent()
            .ok_or_else(|| anyhow::format_err!("missing parent"))?;
        fs::create_dir_all(&parent).await?;
        let mut channel = channel::new_in(size, parent)?;
        let (mut writer, _) = channel.init()?;

        let mut body = pin::pin!(reader.stream()?);
        while let Some(data) = body.try_next().await? {
            writer.write(&data).await?;
        }
        writer.finish().await?;
        fs::rename(channel.keep()?, path).await?;
        Ok(())
    }

    fn path(&self, oid: &str) -> PathBuf {
        self.dir.join(&oid[..2]).join(&oid[2..4]).join(oid)
    }
}
