use bytes::Bytes;
use http_body_util::combinators::UnsyncBoxBody;
use hyper_rustls::ConfigBuilderExt;
use std::process::Stdio;
use std::sync::Arc;
use tokio::process::Command;
use url::{PathSegmentsMut, Url};

pub type Connector =
    hyper_rustls::HttpsConnector<hyper_util::client::legacy::connect::HttpConnector>;
pub type Client<B = UnsyncBoxBody<Bytes, Box<dyn std::error::Error + Send + Sync>>> =
    hyper_util::client::legacy::Client<Connector, B>;
pub fn client<B>() -> anyhow::Result<Client<B>>
where
    B: http_body::Body + Send,
    B::Data: Send,
{
    let tls_config = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()?
    .with_native_roots()?
    .with_no_client_auth();
    let connector = hyper_rustls::HttpsConnectorBuilder::new()
        .with_tls_config(tls_config)
        .https_or_http()
        .enable_http1()
        .enable_http2()
        .build();
    Ok(
        hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new())
            .build(connector),
    )
}

pub async fn spawn(command: &mut Command, stdin: Option<&[u8]>) -> anyhow::Result<Vec<u8>> {
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    tracing::info!(?command);
    let mut child = command.spawn()?;

    let copy = {
        let mut reader = stdin.unwrap_or_default();
        let writer = child.stdin.take();
        async move {
            if let Some(mut writer) = writer {
                tokio::io::copy(&mut reader, &mut writer).await?;
            }
            Ok(())
        }
    };

    let (output, _) = futures::future::try_join(child.wait_with_output(), copy).await?;
    if output.status.success() {
        Ok(output.stdout)
    } else {
        Err(anyhow::format_err!(
            String::from_utf8_lossy(&output.stderr).into_owned()
        ))
    }
}

pub fn path_segments_mut(url: &mut Url) -> anyhow::Result<PathSegmentsMut<'_>> {
    let mut path_segments = url
        .path_segments_mut()
        .map_err(|_| anyhow::format_err!("cannot be base"))?;
    path_segments.pop_if_empty();
    Ok(path_segments)
}

pub fn backoff_permanent<E>(e: E) -> backoff::Error<anyhow::Error>
where
    anyhow::Error: From<E>,
{
    backoff::Error::permanent(anyhow::Error::from(e))
}

pub fn backoff_transient<E>(e: E) -> backoff::Error<anyhow::Error>
where
    anyhow::Error: From<E>,
{
    backoff::Error::transient(anyhow::Error::from(e))
}
