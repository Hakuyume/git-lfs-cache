use bytes::Bytes;
use futures::Stream;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use tempfile::NamedTempFile;
use tokio::fs::File;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter};
use tokio::sync::watch;

pub fn new_in<P>(dir: P) -> io::Result<Channel>
where
    P: AsRef<Path>,
{
    let temp = NamedTempFile::new_in(dir)?;
    Ok(Channel { temp })
}

pub struct Channel {
    temp: NamedTempFile,
}

impl Channel {
    pub fn init(&mut self) -> io::Result<(Writer<'_>, Reader<'_>)> {
        self.temp.as_file().set_len(0)?;
        let (tx, rx) = watch::channel((0, false));
        Ok((
            Writer {
                temp: &self.temp,
                writer: BufWriter::new(File::from_std(self.temp.reopen()?)),
                state: tx,
            },
            Reader {
                temp: &self.temp,
                state: rx,
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
    state: watch::Sender<(u64, bool)>,
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
        self.state
            .send_modify(|(size, _)| *size += data.len() as u64);
        Ok(())
    }

    pub async fn finish(mut self) -> io::Result<()> {
        self.writer.flush().await?;
        self.state.send_modify(|(_, eof)| *eof = true);
        Ok(())
    }
}

pub struct Reader<'a> {
    temp: &'a NamedTempFile,
    state: watch::Receiver<(u64, bool)>,
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
        let reader = BufReader::new(File::from_std(self.temp.reopen()?));
        Ok(futures::stream::try_unfold(
            (reader, self.state.clone(), 0),
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

#[cfg(test)]
mod tests;
