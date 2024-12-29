use crate::{cache, channel, git, git_lfs, jsonl, logs, misc};
use chrono::Utc;
use clap::Parser;
use futures::TryStreamExt;
use http::{Request, StatusCode};
use http_body_util::{BodyExt, Empty};
use sha2::{Digest, Sha256};
use std::borrow::Cow;
use std::env;
use std::fmt::Debug;
use std::path::PathBuf;
use std::pin;
use std::sync::Arc;
use tokio::fs::{self, File};
use tokio::io;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

#[derive(Debug, Parser)]
pub struct Args {
    #[clap(long)]
    cache: Option<cache::Args>,
}

pub async fn main(args: Args) -> anyhow::Result<()> {
    let current_dir = env::current_dir()?;
    let git_dir = git::rev_parse_absolute_git_dir(&current_dir).await?;
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

    let mut context = Context::new(args, current_dir, git_dir, logs_dir).await?;

    let mut stdin = jsonl::Reader::new(io::stdin());
    let mut stdout = jsonl::Writer::new(io::stdout());

    while let Some(line) = stdin.read().await? {
        match line {
            git_lfs::custom_transfers::Request::Init {
                operation, remote, ..
            } => {
                let error = context.init(operation, remote).await.err().map(error);
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
            message: format!("{e:?}"),
        },
    }
}

#[derive(Debug)]
struct Context {
    client: misc::Client,
    current_dir: PathBuf,
    git_dir: PathBuf,
    logs: jsonl::Writer<File>,
    cache: Option<cache::Cache>,
    operation: Option<git_lfs::Operation>,
    remote: Option<String>,
    server_discovery: Option<Arc<git_lfs::server_discovery::Response>>,
}

impl Context {
    #[tracing::instrument(err, ret)]
    async fn new(
        args: Args,
        current_dir: PathBuf,
        git_dir: PathBuf,
        logs_dir: PathBuf,
    ) -> anyhow::Result<Self> {
        let (logs, _) = tempfile::Builder::new()
            .prefix("")
            .suffix(".jsonl")
            .tempfile_in(logs_dir)?
            .keep()?;

        let cache = if let Some(args) = args.cache {
            Some(cache::Cache::new(args).await?)
        } else {
            None
        };

        Ok(Self {
            client: misc::client(misc::connector()?),
            current_dir,
            git_dir,
            logs: jsonl::Writer::new(File::from_std(logs)),
            cache,
            operation: None,
            remote: None,
            server_discovery: None,
        })
    }

    #[tracing::instrument(err, ret)]
    async fn init(&mut self, operation: git_lfs::Operation, remote: String) -> anyhow::Result<()> {
        self.operation = Some(operation);
        self.remote = Some(remote);
        Ok(())
    }

    async fn server_discovery(
        &mut self,
        authorization: bool,
    ) -> anyhow::Result<Arc<git_lfs::server_discovery::Response>> {
        let response = match (self.server_discovery.clone(), authorization) {
            (None, _) | (_, true) => {
                let operation = self
                    .operation
                    .ok_or_else(|| anyhow::format_err!("uninitialized"))?;
                let remote = self
                    .remote
                    .as_ref()
                    .ok_or_else(|| anyhow::format_err!("uninitialized"))?;
                let response =
                    git_lfs::server_discovery(&self.current_dir, operation, remote, authorization)
                        .await?;
                self.server_discovery.insert(Arc::new(response)).clone()
            }
            (Some(response), _) => response,
        };
        Ok(response)
    }

    #[tracing::instrument(err, ret, skip(stdout))]
    async fn download(
        &mut self,
        oid: &str,
        size: u64,
        stdout: &mut jsonl::Writer<io::Stdout>,
    ) -> anyhow::Result<PathBuf> {
        let start = Utc::now();

        let temp_dir = self.git_dir.join("lfs").join("tmp");
        fs::create_dir_all(&temp_dir).await?;

        let path = if let Some(cache) = &self.cache {
            let mut channel = channel::new_in(size, &temp_dir)?;
            let (writer, reader) = channel.init()?;
            if let Ok((source, _, _)) = futures::future::try_join3(
                cache.get(oid, size, writer),
                async {
                    let mut hasher = Sha256::new();
                    let mut body = pin::pin!(reader.stream()?);
                    while let Some(data) = body.try_next().await? {
                        hasher.update(data);
                    }
                    anyhow::ensure!(oid == hex::encode(hasher.finalize()));
                    Ok(())
                },
                progress(oid, &reader, &mut *stdout),
            )
            .await
            {
                let path = channel.keep()?;
                self.logs
                    .write(&logs::Line {
                        operation: git_lfs::Operation::Download,
                        oid: Cow::Borrowed(oid),
                        size,
                        cache: Some(source),
                        start,
                        finish: Utc::now(),
                    })
                    .await?;
                Some(path)
            } else {
                None
            }
        } else {
            None
        };

        if let Some(path) = path {
            Ok(path)
        } else {
            let request = git_lfs::batch::Request {
                operation: git_lfs::Operation::Download,
                transfers: &[git_lfs::batch::request::Transfer::Basic],
                objects: &[git_lfs::batch::request::Object { oid, size }],
            };
            let server_discovery = self.server_discovery(false).await?;
            let response = git_lfs::batch(
                &self.client,
                &server_discovery.href,
                &server_discovery.header,
                &request,
            )
            .await;
            let response = match response {
                Ok(response) => Ok(response),
                Err(e) => match e.downcast::<git_lfs::Error>() {
                    Ok(e) if e.code == StatusCode::UNAUTHORIZED => {
                        let server_discovery = self.server_discovery(true).await?;
                        git_lfs::batch(
                            &self.client,
                            &server_discovery.href,
                            &server_discovery.header,
                            &request,
                        )
                        .await
                    }
                    Ok(e) => Err(e.into()),
                    Err(e) => Err(e),
                },
            }?;

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
                    let builder = Request::get(download.href.as_ref());
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
                        let mut channel = channel::new_in(size, &temp_dir)?;
                        let (mut writer, reader) = channel.init()?;
                        futures::future::try_join3(
                            async {
                                while let Some(frame) = body.frame().await.transpose()? {
                                    if let Ok(data) = frame.into_data() {
                                        writer.write(&data).await?;
                                    }
                                }
                                Ok(writer.finish().await?)
                            },
                            async {
                                if let Some(cache) = &self.cache {
                                    cache.put(oid, size, &reader).await?;
                                }
                                Ok(())
                            },
                            progress(oid, &reader, &mut *stdout),
                        )
                        .await?;
                        let path = channel.keep()?;
                        self.logs
                            .write(&logs::Line {
                                operation: git_lfs::Operation::Download,
                                oid: Cow::Borrowed(oid),
                                size,
                                cache: None,
                                start,
                                finish: Utc::now(),
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

async fn progress(
    oid: &str,
    reader: &channel::Reader<'_>,
    stdout: &mut jsonl::Writer<io::Stdout>,
) -> anyhow::Result<()> {
    let mut bytes_so_far = 0;
    let mut bytes_since_last = 0;

    let mut body = pin::pin!(reader.stream()?);
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
