use bytes::Bytes;
use futures::Stream;
use std::io;
use std::path::{Path, PathBuf};
use tokio::fs::{self, File};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter};
use tokio::sync::watch;
use uuid::Uuid;

pub async fn new_in<P>(dir: P) -> Result<Writer, io::Error>
where
    P: AsRef<Path>,
{
    let path = dir.as_ref().join(Uuid::new_v4().to_string());
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await?;
    }
    let writer = BufWriter::new(File::create(&path).await?);
    Ok(Writer {
        path: Some(path),
        writer,
        state: watch::Sender::new((0, false)),
    })
}

pub struct Writer {
    path: Option<PathBuf>,
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
        Ok(self.path.take().unwrap())
    }

    pub async fn subscribe(
        &self,
    ) -> Result<impl Stream<Item = Result<Bytes, io::Error>> + Send + Sync + 'static, io::Error>
    {
        let reader = BufReader::new(File::open(self.path.as_ref().unwrap()).await?);
        Ok(futures::stream::try_unfold(
            (reader, self.state.subscribe(), 0),
            |(mut reader, mut state, mut position)| async move {
                let (size, eof) = *state
                    .wait_for(|(size, eof)| *size > position || *eof)
                    .await
                    .map_err(|_| io::ErrorKind::BrokenPipe)?;
                if position < size {
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
                            position += data.len() as u64;
                            break Ok(Some((data, (reader, state, position))));
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

impl Drop for Writer {
    fn drop(&mut self) {
        if let Some(path) = self.path.take() {
            let _ = std::fs::remove_file(path);
        }
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

    async fn collect<S, E>(stream: S) -> Result<Bytes, E>
    where
        S: Stream<Item = Result<Bytes, E>>,
    {
        Ok(StreamBody::new(stream.map_ok(Frame::data))
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

        let mut data = vec![0; 1 << 24];
        rng.fill(&mut data[..]);

        let temp_dir = tempfile::tempdir()?;
        let mut writer = super::new_in(temp_dir.path()).await?;

        let subscribe_0 = tokio::spawn(collect(writer.subscribe().await?));

        let mut position = 0;
        while position < data.len() {
            let size = cmp::min(rng.gen_range(1..1 << 16), data.len() - position);
            writer.write(&data[position..position + size]).await?;
            position += size;
        }

        let subscribe_1 = tokio::spawn(collect(writer.subscribe().await?));
        let path = writer.finish().await?;

        anyhow::ensure!(fs::read(&path).await? == data);
        anyhow::ensure!(subscribe_0.await?? == data);
        anyhow::ensure!(subscribe_1.await?? == data);

        Ok(())
    }
}
