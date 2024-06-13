use bytes::Bytes;
use http_body_util::combinators::UnsyncBoxBody;
use hyper_rustls::{HttpsConnector, HttpsConnectorBuilder};
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::rt::TokioExecutor;
use std::error;
use std::io;
use std::process::Stdio;
use tokio::process::Command;

pub type Client = hyper_util::client::legacy::Client<
    HttpsConnector<HttpConnector>,
    UnsyncBoxBody<Bytes, Box<dyn error::Error + Send + Sync>>,
>;
pub fn client() -> Result<Client, io::Error> {
    let connector = HttpsConnectorBuilder::new()
        .with_native_roots()?
        .https_or_http()
        .enable_http1()
        .enable_http2()
        .build();
    Ok(hyper_util::client::legacy::Client::builder(TokioExecutor::new()).build(connector))
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
