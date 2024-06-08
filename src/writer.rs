use bytes::Bytes;
use futures::Stream;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use tempfile::NamedTempFile;
use tokio::fs::File;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter};
use tokio::sync::watch;

pub async fn new_in<P>(dir: P) -> Result<Writer, io::Error>
where
    P: AsRef<Path>,
{
    let temp = NamedTempFile::new_in(dir)?;
    let writer = BufWriter::new(File::from_std(temp.reopen()?));
    Ok(Writer {
        temp,
        writer,
        state: watch::Sender::new((0, false)),
    })
}

pub struct Writer {
    temp: NamedTempFile,
    writer: BufWriter<File>,
    state: watch::Sender<(u64, bool)>,
}

impl Writer {
    pub async fn write(&mut self, data: &[u8]) -> Result<(), io::Error> {
        self.writer.write_all(data).await?;
        self.state
            .send_modify(|(size, _)| *size += data.len() as u64);
        Ok(())
    }

    pub async fn finish(mut self) -> Result<PathBuf, io::Error> {
        self.writer.flush().await?;
        self.state.send_modify(|(_, eof)| *eof = true);
        Ok(self.temp.keep()?.1)
    }

    pub async fn subscribe(
        &self,
    ) -> Result<impl Stream<Item = Result<Bytes, io::Error>> + Send + Sync + 'static, io::Error>
    {
        let reader = BufReader::new(File::from_std(self.temp.reopen()?));
        Ok(futures::stream::try_unfold(
            (reader, self.state.subscribe(), 0),
            |(mut reader, mut state, mut pos)| async move {
                let (size, eof) = *state
                    .wait_for(|(size, eof)| *size > pos || *eof)
                    .await
                    .map_err(|_| io::ErrorKind::BrokenPipe)?;
                if pos < size {
                    loop {
                        let data = reader.fill_buf().await?;
                        if data.is_empty() {
                            state
                                .changed()
                                .await
                                .map_err(|_| io::ErrorKind::BrokenPipe)?;
                        } else {
                            let data = Bytes::copy_from_slice(data);
                            reader.consume(data.len());
                            pos += data.len() as u64;
                            break Ok(Some((data, (reader, state, pos))));
                        }
                    }
                } else {
                    assert!(eof);
                    Ok(None)
                }
            },
        ))
    }
}

impl fmt::Debug for Writer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Writer")
            .field("path", &self.temp.path())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use bytes::Bytes;
    use futures::{Stream, TryStreamExt};
    use http_body::Frame;
    use http_body_util::{BodyExt, StreamBody};
    use rand::Rng;
    use std::cmp;
    use tokio::fs;

    async fn collect<B, E>(body: B) -> Result<Bytes, E>
    where
        B: Stream<Item = Result<Bytes, E>>,
    {
        Ok(StreamBody::new(body.map_ok(Frame::data))
            .collect()
            .await?
            .to_bytes())
    }

    #[tokio::test]
    async fn test() -> anyhow::Result<()> {
        let temp_dir = tempfile::tempdir()?;
        let mut writer = super::new_in(temp_dir.path()).await?;

        let subscribe_0 = tokio::spawn(collect(writer.subscribe().await?));
        writer.write(b"hello").await?;
        let subscribe_1 = tokio::spawn(collect(writer.subscribe().await?));
        writer.write(b" world").await?;
        let subscribe_2 = tokio::spawn(collect(writer.subscribe().await?));
        let path = writer.finish().await?;

        anyhow::ensure!(fs::read(&path).await? == b"hello world");
        anyhow::ensure!(&*subscribe_0.await?? == b"hello world");
        anyhow::ensure!(&*subscribe_1.await?? == b"hello world");
        anyhow::ensure!(&*subscribe_2.await?? == b"hello world");

        Ok(())
    }

    #[tokio::test]
    async fn test_drop() -> anyhow::Result<()> {
        let temp_dir = tempfile::tempdir()?;
        let mut writer = super::new_in(temp_dir.path()).await?;

        let subscribe_0 = tokio::spawn(collect(writer.subscribe().await?));
        writer.write(b"hello").await?;
        let subscribe_1 = tokio::spawn(collect(writer.subscribe().await?));
        writer.write(b" world").await?;
        let subscribe_2 = tokio::spawn(collect(writer.subscribe().await?));
        drop(writer);

        anyhow::ensure!(fs::read_dir(temp_dir.path())
            .await?
            .next_entry()
            .await?
            .is_none());
        anyhow::ensure!(subscribe_0.await?.is_err());
        anyhow::ensure!(subscribe_1.await?.is_err());
        anyhow::ensure!(subscribe_2.await?.is_err());

        Ok(())
    }

    #[tokio::test]
    async fn test_large() -> anyhow::Result<()> {
        let mut rng = rand::thread_rng();

        let mut body = vec![0; 1 << 24];
        rng.fill(&mut body[..]);

        let temp_dir = tempfile::tempdir()?;
        let mut writer = super::new_in(temp_dir.path()).await?;

        let subscribe_0 = tokio::spawn(collect(writer.subscribe().await?));

        let mut pos = 0;
        while pos < body.len() {
            let size = cmp::min(rng.gen_range(1..1 << 16), body.len() - pos);
            writer.write(&body[pos..pos + size]).await?;
            pos += size;
        }

        let subscribe_1 = tokio::spawn(collect(writer.subscribe().await?));
        let path = writer.finish().await?;

        anyhow::ensure!(fs::read(&path).await? == body);
        anyhow::ensure!(subscribe_0.await?? == body);
        anyhow::ensure!(subscribe_1.await?? == body);

        Ok(())
    }
}
