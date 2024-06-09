use serde::{Deserialize, Serialize};
use std::fmt::Debug;
use tokio::io::{
    AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader, BufWriter, Lines,
};

#[derive(Debug)]
pub struct Reader<R> {
    inner: Lines<BufReader<R>>,
}

impl<R> Reader<R>
where
    R: Debug + AsyncRead + Unpin,
{
    pub fn new(inner: R) -> Self {
        Self {
            inner: BufReader::new(inner).lines(),
        }
    }

    #[tracing::instrument(err, ret)]
    pub async fn read<T>(&mut self) -> anyhow::Result<Option<T>>
    where
        T: Debug + for<'de> Deserialize<'de>,
    {
        if let Some(line) = self.inner.next_line().await? {
            Ok(Some(serde_json::from_str(&line)?))
        } else {
            Ok(None)
        }
    }
}

#[derive(Debug)]
pub struct Writer<W> {
    inner: BufWriter<W>,
}

impl<W> Writer<W>
where
    W: Debug + AsyncWrite + Unpin,
{
    pub fn new(inner: W) -> Self {
        Self {
            inner: BufWriter::new(inner),
        }
    }

    #[tracing::instrument(err, ret)]
    pub async fn write<T>(&mut self, line: &T) -> anyhow::Result<()>
    where
        T: Debug + Serialize,
    {
        self.inner.write_all(&serde_json::to_vec(line)?).await?;
        self.inner.write_all(b"\n").await?;
        self.inner.flush().await?;
        Ok(())
    }
}
