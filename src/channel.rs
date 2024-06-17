use bytes::Bytes;
use futures::Stream;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use tempfile::NamedTempFile;
use tokio::fs::File;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter};
use tokio::sync::watch;

pub fn new_in<P>(size: u64, dir: P) -> io::Result<Channel>
where
    P: AsRef<Path>,
{
    let temp = NamedTempFile::new_in(dir)?;
    Ok(Channel { temp, size })
}

pub struct Channel {
    temp: NamedTempFile,
    size: u64,
}

impl Channel {
    pub fn init(&mut self) -> io::Result<(Writer<'_>, Reader<'_>)> {
        self.temp.as_file().set_len(0)?;
        let (tx, rx) = watch::channel(());
        Ok((
            Writer {
                temp: &self.temp,
                writer: BufWriter::new(File::from_std(self.temp.reopen()?)),
                notify: tx,
            },
            Reader {
                temp: &self.temp,
                size: self.size,
                notify: rx,
            },
        ))
    }

    pub fn keep(self) -> io::Result<PathBuf> {
        Ok(self.temp.keep()?.1)
    }
}

pub struct Writer<'a> {
    temp: &'a NamedTempFile,
    writer: BufWriter<File>,
    notify: watch::Sender<()>,
}

impl fmt::Debug for Writer<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Writer")
            .field("path", &self.temp.path())
            .finish()
    }
}

impl Writer<'_> {
    pub async fn write(&mut self, data: &[u8]) -> io::Result<()> {
        self.writer.write_all(data).await?;
        let _ = self.notify.send(());
        Ok(())
    }

    pub async fn finish(mut self) -> io::Result<()> {
        self.writer.flush().await?;
        let _ = self.notify.send(());
        Ok(())
    }
}

pub struct Reader<'a> {
    temp: &'a NamedTempFile,
    size: u64,
    notify: watch::Receiver<()>,
}

impl fmt::Debug for Reader<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Reader")
            .field("path", &self.temp.path())
            .finish()
    }
}

impl Reader<'_> {
    pub fn stream(
        &self,
    ) -> io::Result<impl Stream<Item = io::Result<Bytes>> + Send + Sync + 'static> {
        let size = self.size;
        Ok(futures::stream::try_unfold(
            (
                BufReader::new(File::from_std(self.temp.reopen()?)),
                self.notify.clone(),
                0,
            ),
            move |(mut reader, mut notify, pos)| async move {
                if pos < size {
                    loop {
                        let data = reader.fill_buf().await?;
                        if data.is_empty() {
                            notify
                                .changed()
                                .await
                                .map_err(|_| io::ErrorKind::BrokenPipe)?;
                        } else {
                            let data = Bytes::copy_from_slice(data);
                            reader.consume(data.len());
                            let pos = pos + data.len() as u64;
                            break Ok(Some((data, (reader, notify, pos))));
                        }
                    }
                } else {
                    Ok(None)
                }
            },
        ))
    }
}

#[cfg(test)]
mod tests;
