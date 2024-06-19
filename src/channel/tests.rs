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

    let mut channel = super::new_in(11, temp_dir.path())?;
    let (mut writer, reader) = channel.init()?;
    let body_0 = tokio::spawn(collect(reader.stream()?));
    writer.write(b"hello").await?;
    let body_1 = tokio::spawn(collect(reader.stream()?));
    writer.write(b" world").await?;
    let body_2 = tokio::spawn(collect(reader.stream()?));
    writer.finish().await?;
    let path = channel.keep()?;

    anyhow::ensure!(fs::read(&path).await? == b"hello world");
    anyhow::ensure!(&*body_0.await?? == b"hello world");
    anyhow::ensure!(&*body_1.await?? == b"hello world");
    anyhow::ensure!(&*body_2.await?? == b"hello world");

    Ok(())
}

#[tokio::test]
async fn test_drop() -> anyhow::Result<()> {
    let temp_dir = tempfile::tempdir()?;

    let mut channel = super::new_in(11, temp_dir.path())?;
    let (mut writer, reader) = channel.init()?;
    let body_0 = tokio::spawn(collect(reader.stream()?));
    writer.write(b"hello").await?;
    let body_1 = tokio::spawn(collect(reader.stream()?));
    writer.write(b" world").await?;
    let body_2 = tokio::spawn(collect(reader.stream()?));
    drop(writer);
    drop(channel);

    anyhow::ensure!(fs::read_dir(temp_dir.path())
        .await?
        .next_entry()
        .await?
        .is_none());
    anyhow::ensure!(body_0.await?.is_err());
    anyhow::ensure!(body_1.await?.is_err());
    anyhow::ensure!(body_2.await?.is_err());

    Ok(())
}

#[tokio::test]
async fn test_reset() -> anyhow::Result<()> {
    let temp_dir = tempfile::tempdir()?;

    let mut channel = super::new_in(11, temp_dir.path())?;
    let (mut writer, reader) = channel.init()?;
    let body_0 = tokio::spawn(collect(reader.stream()?));
    writer.write(b"hello").await?;
    let body_1 = tokio::spawn(collect(reader.stream()?));
    writer.reset().await?;
    let body_2 = tokio::spawn(collect(reader.stream()?));
    writer.write(b"hello world").await?;
    writer.finish().await?;
    let path = channel.keep()?;

    anyhow::ensure!(fs::read(&path).await? == b"hello world");
    anyhow::ensure!(&*body_0.await?? == b"hello world");
    anyhow::ensure!(&*body_1.await?? == b"hello world");
    anyhow::ensure!(&*body_2.await?? == b"hello world");

    Ok(())
}

#[tokio::test]
async fn test_large() -> anyhow::Result<()> {
    let mut rng = rand::thread_rng();

    let mut body = vec![0; 1 << 24];
    rng.fill(&mut body[..]);

    let temp_dir = tempfile::tempdir()?;
    let mut channel = super::new_in(body.len() as _, temp_dir.path())?;
    let (mut writer, reader) = channel.init()?;

    let body_0 = tokio::spawn(collect(reader.stream()?));

    let mut pos = 0;
    while pos < body.len() {
        let size = cmp::min(rng.gen_range(1..1 << 16), body.len() - pos);
        writer.write(&body[pos..pos + size]).await?;
        pos += size;
    }

    let body_1 = tokio::spawn(collect(reader.stream()?));
    writer.finish().await?;
    let path = channel.keep()?;

    anyhow::ensure!(fs::read(&path).await? == body);
    anyhow::ensure!(body_0.await?? == body);
    anyhow::ensure!(body_1.await?? == body);

    Ok(())
}
