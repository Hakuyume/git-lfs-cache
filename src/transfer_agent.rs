use crate::{cache, git, git_lfs, jsonl, logs, misc, writer};
use bytes::Bytes;
use clap::Parser;
use futures::future::OptionFuture;
use futures::{FutureExt, Stream, TryStreamExt};
use http::{Request, StatusCode, Uri};
use http_body_util::{BodyExt, Empty};
use sha2::{Digest, Sha256};
use std::borrow::Cow;
use std::fmt::Debug;
use std::path::PathBuf;
use std::pin;
use std::sync::Arc;
use tokio::fs::{self, File};
use tokio::io;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

#[derive(Debug, Parser)]
pub struct Opts {
    #[clap(long)]
    cache: Option<cache::Opts>,
}

pub async fn main(opts: Opts) -> anyhow::Result<()> {
    let git_dir = git::rev_parse_git_dir().await?.canonicalize()?;
    let logs_dir = logs::dir(&git_dir);
    fs::create_dir_all(&logs_dir).await?;

    tracing_subscriber::Registry::default()
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(
                    tempfile::Builder::new()
                        .prefix("")
                        .suffix(".log")
                        .tempfile_in(&logs_dir)?
                        .keep()?
                        .0,
                )
                .with_ansi(false),
        )
        .with(tracing_subscriber::filter::EnvFilter::from_default_env())
        .try_init()?;

    let mut context = Context::new(opts, git_dir, logs_dir).await?;

    let mut stdin = jsonl::Reader::new(io::stdin());
    let mut stdout = jsonl::Writer::new(io::stdout());

    while let Some(line) = stdin.read().await? {
        match line {
            git_lfs::custom_transfers::Request::Init {
                operation, remote, ..
            } => {
                let error = context.init(operation, &remote).await.err().map(error);
                stdout
                    .write(&git_lfs::custom_transfers::InitResponse { error })
                    .await?;
            }
            git_lfs::custom_transfers::Request::Upload { oid, .. } => {
                stdout
                    .write(&git_lfs::custom_transfers::Response::Complete {
                        oid: &oid,
                        path: None,
                        error: Some(error(anyhow::format_err!("unimplemented"))),
                    })
                    .await?
            }
            git_lfs::custom_transfers::Request::Download { oid, size } => {
                let (path, error) = match context.download(&oid, size, &mut stdout).await {
                    Ok(v) => (Some(v), None),
                    Err(e) => (None, Some(error(e))),
                };
                stdout
                    .write(&git_lfs::custom_transfers::Response::Complete {
                        oid: &oid,
                        path: path.as_deref(),
                        error,
                    })
                    .await?
            }
            git_lfs::custom_transfers::Request::Terminate => break,
        }
    }

    Ok(())
}

fn error(e: anyhow::Error) -> git_lfs::Error {
    match e.downcast::<git_lfs::Error>() {
        Ok(e) => e,
        Err(e) => git_lfs::Error {
            code: StatusCode::INTERNAL_SERVER_ERROR,
            message: e.to_string(),
        },
    }
}

#[derive(Debug)]
struct Context {
    client: misc::Client,
    git_dir: PathBuf,
    logs: jsonl::Writer<File>,
    cache: Option<cache::Cache>,
    operation: Option<git_lfs::Operation>,
    url: Option<Uri>,
    server_discovery: Option<Arc<git_lfs::server_discovery::Response>>,
}

impl Context {
    #[tracing::instrument(err, ret)]
    async fn new(opts: Opts, git_dir: PathBuf, logs_dir: PathBuf) -> anyhow::Result<Self> {
        let (logs, _) = tempfile::Builder::new()
            .prefix("")
            .suffix(".jsonl")
            .tempfile_in(logs_dir)?
            .keep()?;

        let cache = if let Some(opts) = opts.cache {
            Some(cache::Cache::new(opts).await?)
        } else {
            None
        };

        Ok(Self {
            client: misc::client()?,
            git_dir,
            logs: jsonl::Writer::new(File::from_std(logs)),
            cache,
            operation: None,
            url: None,
            server_discovery: None,
        })
    }

    #[tracing::instrument(err, ret)]
    async fn init(&mut self, operation: git_lfs::Operation, remote: &str) -> anyhow::Result<()> {
        self.operation = Some(operation);
        let url = if let Ok(url) = git::remote_get_url(remote).await {
            url
        } else {
            remote.parse()?
        };
        self.url = Some(url);
        Ok(())
    }

    async fn server_discovery(
        &mut self,
    ) -> anyhow::Result<Arc<git_lfs::server_discovery::Response>> {
        if let Some(response) = self.server_discovery.clone() {
            Ok(response)
        } else {
            let url = self
                .url
                .as_ref()
                .ok_or_else(|| anyhow::format_err!("uninitialized"))?;
            let operation = self
                .operation
                .ok_or_else(|| anyhow::format_err!("uninitialized"))?;
            let response = git_lfs::server_discovery(url, operation).await?;
            Ok(self.server_discovery.insert(Arc::new(response)).clone())
        }
    }

    #[tracing::instrument(err, ret, skip(stdout))]
    async fn download(
        &mut self,
        oid: &str,
        size: u64,
        stdout: &mut jsonl::Writer<io::Stdout>,
    ) -> anyhow::Result<PathBuf> {
        let temp_dir = self.git_dir.join("lfs").join("tmp");
        fs::create_dir_all(&temp_dir).await?;

        let path = if let Some(cache) = &self.cache {
            let writer = writer::new_in(&temp_dir).await?;
            let check = {
                let body = writer.subscribe().await?;
                async move {
                    let mut hasher = Sha256::new();
                    let mut body = pin::pin!(body);
                    while let Some(data) = body.try_next().await? {
                        hasher.update(data);
                    }
                    anyhow::ensure!(oid == hex::encode(hasher.finalize()));
                    Ok(())
                }
            };
            let progress = progress(oid, writer.subscribe().await?, &mut *stdout);
            match futures::future::join3(cache.get(oid, size, writer), check, progress).await {
                (Ok((path, source)), Ok(_), _) => {
                    self.logs
                        .write(&logs::Line {
                            operation: git_lfs::Operation::Download,
                            oid: Cow::Borrowed(oid),
                            size,
                            cache: Some(source),
                        })
                        .await?;
                    Some(path)
                }
                (Ok((path, _)), _, _) => {
                    fs::remove_file(path).await?;
                    None
                }
                _ => None,
            }
        } else {
            None
        };

        if let Some(path) = path {
            Ok(path)
        } else {
            let server_discovery = self.server_discovery().await?;
            let response = git_lfs::batch(
                &self.client,
                &server_discovery.href,
                &server_discovery.header,
                &git_lfs::batch::Request {
                    operation: git_lfs::Operation::Download,
                    transfers: &[git_lfs::batch::request::Transfer::Basic],
                    objects: &[git_lfs::batch::request::Object { oid, size }],
                },
            )
            .await?;
            let object = response
                .objects
                .into_iter()
                .find(|object| object.oid == oid)
                .ok_or_else(|| anyhow::format_err!("missing object"))?;
            match object.inner {
                git_lfs::batch::response::Inner::Actions {
                    download: Some(download),
                    ..
                } => {
                    let builder = Request::get(download.href);
                    let builder = download
                        .header
                        .iter()
                        .fold(builder, |builder, (name, value)| {
                            builder.header(name, value)
                        });
                    let request = builder.body(Empty::new().map_err(Box::from).boxed_unsync())?;
                    let response = self.client.request(request).await?;
                    let (parts, mut body) = response.into_parts();
                    if parts.status.is_success() {
                        let mut writer = writer::new_in(&temp_dir).await?;
                        let put = if let Some(cache) = &self.cache {
                            Some(cache.put(oid, size, writer.subscribe().await?))
                        } else {
                            None
                        };
                        let progress = progress(oid, writer.subscribe().await?, &mut *stdout);
                        let (path, _, _) = futures::future::try_join3(
                            async {
                                while let Some(frame) = body.frame().await.transpose()? {
                                    if let Ok(data) = frame.into_data() {
                                        writer.write(&data).await?;
                                    }
                                }
                                Ok(writer.finish().await?)
                            },
                            OptionFuture::from(put).map(Option::transpose),
                            progress,
                        )
                        .await?;
                        self.logs
                            .write(&logs::Line {
                                operation: git_lfs::Operation::Download,
                                oid: Cow::Borrowed(oid),
                                size,
                                cache: None,
                            })
                            .await?;
                        Ok(path)
                    } else {
                        let body = body.collect().await?.to_bytes();
                        Err(git_lfs::Error {
                            code: parts.status,
                            message: format!("{body:?}"),
                        }
                        .into())
                    }
                }
                git_lfs::batch::response::Inner::Actions { download: None, .. } => {
                    Err(anyhow::format_err!("missing action"))
                }
                git_lfs::batch::response::Inner::Error(e) => Err(e.into()),
            }
        }
    }
}

async fn progress<B, E>(
    oid: &str,
    body: B,
    stdout: &mut jsonl::Writer<io::Stdout>,
) -> anyhow::Result<()>
where
    B: Stream<Item = Result<Bytes, E>>,
    anyhow::Error: From<E>,
{
    let mut bytes_so_far = 0;
    let mut bytes_since_last = 0;

    let mut body = pin::pin!(body);
    while let Some(data) = body.try_next().await? {
        bytes_so_far += data.len() as u64;
        bytes_since_last += data.len() as u64;

        if bytes_since_last >= 1 << 16 {
            stdout
                .write(&git_lfs::custom_transfers::Response::Progress {
                    oid,
                    bytes_so_far,
                    bytes_since_last,
                })
                .await?;
            bytes_since_last = 0;
        }
    }
    if bytes_since_last > 0 {
        stdout
            .write(&git_lfs::custom_transfers::Response::Progress {
                oid,
                bytes_so_far,
                bytes_since_last,
            })
            .await?;
    }
    Ok(())
}
