use crate::writer;
use bytes::Bytes;
use futures::{Stream, TryStreamExt};
use serde::Deserialize;
use std::path::PathBuf;
use std::pin;
use tokio::fs::{self, File};
use tokio::io::{AsyncBufReadExt, BufReader};

#[derive(Clone, Debug, Deserialize)]
pub struct Opts {
    dir: PathBuf,
}

#[derive(Debug)]
pub struct Cache {
    dir: PathBuf,
}

impl Cache {
    pub async fn new(opts: Opts) -> anyhow::Result<Self> {
        Ok(Self { dir: opts.dir })
    }

    #[tracing::instrument(err, ret)]
    pub async fn get(
        &self,
        oid: &str,
        size: u64,
        mut writer: writer::Writer,
    ) -> anyhow::Result<PathBuf> {
        let mut reader = BufReader::new(File::open(self.path(oid)).await?);
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
        Ok(writer.finish().await?)
    }

    #[tracing::instrument(err, ret, skip(body))]
    pub async fn put<B, E>(&self, oid: &str, size: u64, body: B) -> anyhow::Result<()>
    where
        B: Stream<Item = Result<Bytes, E>> + Send + Sync + 'static,
        anyhow::Error: From<E>,
    {
        let path = self.path(oid);

        let parent = path
            .parent()
            .ok_or_else(|| anyhow::format_err!("missing parent"))?;
        fs::create_dir_all(&parent).await?;
        let mut writer = writer::new_in(&parent).await?;

        let mut body = pin::pin!(body);
        while let Some(data) = body.try_next().await? {
            writer.write(&data).await?;
        }

        fs::rename(writer.finish().await?, path).await?;
        Ok(())
    }

    fn path(&self, oid: &str) -> PathBuf {
        self.dir.join(&oid[..2]).join(&oid[2..4]).join(oid)
    }
}
