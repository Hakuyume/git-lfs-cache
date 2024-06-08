use crate::{cache, git, git_lfs, misc, writer};
use clap::Parser;
use futures::future::OptionFuture;
use futures::{FutureExt, TryFutureExt, TryStreamExt};
use http::{Request, StatusCode, Uri};
use http_body_util::{BodyExt, Empty};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::fmt::Debug;
use std::future;
use std::path::PathBuf;
use std::pin::{self, Pin};
use std::sync::Arc;
use tokio::fs;
use tokio::io::{self, AsyncBufReadExt, AsyncWriteExt, BufReader};

#[derive(Clone, Debug, Parser)]
pub struct Opts {
    #[clap(long = "cache")]
    cache: Option<cache::Opts>,
}

pub async fn main(opts: Opts) -> anyhow::Result<()> {
    let mut context = Context::new(opts).await?;

    let stdin = io::stdin();
    let mut stdout = io::stdout();

    let mut requests = futures::stream::poll_fn({
        let mut lines = BufReader::new(stdin).lines();
        move |cx| {
            Pin::new(&mut lines)
                .poll_next_line(cx)
                .map(Result::transpose)
        }
    })
    .and_then(|line| future::ready(serde_json::from_str(&line)).err_into());

    while let Some(request) = requests.try_next().await? {
        match request {
            git_lfs::custom_transfers::Request::Init {
                operation, remote, ..
            } => {
                let error = context.init(operation, &remote).await.err().map(error);
                respond(
                    &mut stdout,
                    &git_lfs::custom_transfers::InitResponse { error },
                )
                .await?;
            }
            git_lfs::custom_transfers::Request::Upload { oid, .. } => {
                respond(
                    &mut stdout,
                    &git_lfs::custom_transfers::Response::Complete {
                        oid: &oid,
                        path: None,
                        error: Some(error(anyhow::format_err!("unimplemented"))),
                    },
                )
                .await?
            }
            git_lfs::custom_transfers::Request::Download { oid, size } => {
                let (path, error) = match context.download(&oid, size).await {
                    Ok(v) => (Some(v), None),
                    Err(e) => (None, Some(error(e))),
                };
                respond(
                    &mut stdout,
                    &git_lfs::custom_transfers::Response::Complete {
                        oid: &oid,
                        path: path.as_deref(),
                        error,
                    },
                )
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

#[tracing::instrument(err)]
async fn respond<T>(stdout: &mut io::Stdout, response: &T) -> anyhow::Result<()>
where
    T: Debug + Serialize,
{
    stdout.write_all(&serde_json::to_vec(response)?).await?;
    stdout.write_all(b"\n").await?;
    stdout.flush().await?;
    Ok(())
}

#[derive(Debug)]
struct Context {
    client: misc::Client,
    git_dir: PathBuf,
    cache: Option<cache::Cache>,
    operation: Option<git_lfs::Operation>,
    url: Option<Uri>,
    server_discovery: Option<Arc<git_lfs::server_discovery::Response>>,
}

impl Context {
    #[tracing::instrument(err, ret)]
    async fn new(opts: Opts) -> anyhow::Result<Self> {
        let git_dir = git::rev_parse_git_dir().await?.canonicalize()?;

        let cache = if let Some(opts) = opts.cache {
            Some(cache::Cache::new(opts).await?)
        } else {
            None
        };

        Ok(Self {
            client: misc::client()?,
            git_dir,
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

    #[tracing::instrument(err, ret)]
    async fn download(&mut self, oid: &str, size: u64) -> anyhow::Result<PathBuf> {
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
            match futures::future::join(cache.get(oid, size, writer), check).await {
                (Ok(path), Ok(_)) => Some(path),
                (Ok(path), _) => {
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
                        let (path, _) = futures::future::try_join(
                            async {
                                while let Some(frame) = body.frame().await.transpose()? {
                                    if let Ok(data) = frame.into_data() {
                                        writer.write(&data).await?;
                                    }
                                }
                                Ok(writer.finish().await?)
                            },
                            OptionFuture::from(put).map(Option::transpose),
                        )
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
