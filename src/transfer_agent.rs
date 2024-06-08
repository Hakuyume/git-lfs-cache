use crate::{git, git_lfs, misc, writer};
use clap::Parser;
use futures::{TryFutureExt, TryStreamExt};
use http::{HeaderMap, Request, StatusCode, Uri};
use http_body_util::{BodyExt, Empty};
use serde::Serialize;
use std::fmt::Debug;
use std::future;
use std::path::PathBuf;
use std::pin::Pin;
use tokio::io::{self, AsyncBufReadExt, AsyncWriteExt, BufReader};

#[derive(Debug, Parser)]
pub struct Opts {}

pub async fn main(_: Opts) -> anyhow::Result<()> {
    let client = misc::client()?;

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

    let mut context = None;
    while let Some(request) = requests.try_next().await? {
        match request {
            git_lfs::custom_transfers::Request::Init {
                operation, remote, ..
            } => {
                let error = match init(operation, &remote).await {
                    Ok(v) => {
                        context = Some(v);
                        None
                    }
                    Err(e) => Some(error(e)),
                };
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
                let (path, error) = match download(&client, &context, &oid, size).await {
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

#[derive(Debug)]
struct Context {
    git_dir: PathBuf,
    href: Uri,
    header: HeaderMap,
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

#[tracing::instrument(err, ret)]
async fn init(operation: git_lfs::Operation, remote: &str) -> anyhow::Result<Context> {
    let git_dir = git::rev_parse_git_dir().await?.canonicalize()?;
    let url = if let Ok(url) = git::remote_get_url(remote).await {
        url
    } else {
        remote.parse()?
    };
    let response = git_lfs::server_discovery(&url, operation).await?;
    Ok(Context {
        git_dir,
        href: response.href,
        header: response.header,
    })
}

#[tracing::instrument(err, ret)]
async fn download(
    client: &misc::Client,
    context: &Option<Context>,
    oid: &str,
    size: u64,
) -> anyhow::Result<PathBuf> {
    let context = context
        .as_ref()
        .ok_or_else(|| anyhow::format_err!("uninitialized"))?;
    let response = git_lfs::batch(
        client,
        &context.href,
        &context.header,
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
            let response = client.request(request).await?;
            let (parts, mut body) = response.into_parts();
            if parts.status.is_success() {
                let mut writer = writer::new_in(context.git_dir.join("lfs").join("tmp")).await?;
                while let Some(frame) = body.frame().await.transpose()? {
                    if let Ok(data) = frame.into_data() {
                        writer.write(&data).await?;
                    }
                }
                Ok(writer.finish().await?)
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
