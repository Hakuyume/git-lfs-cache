use bytes::Bytes;
use http_body_util::combinators::UnsyncBoxBody;
use hyper_rustls::{HttpsConnector, HttpsConnectorBuilder};
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::rt::TokioExecutor;
use std::error;
use std::io;

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

pub fn patch_path<F>(url: http::Uri, f: F) -> Result<http::Uri, http::Error>
where
    F: FnOnce(&str) -> String,
{
    let mut parts = url.into_parts();

    let (path, query) = if let Some(path_and_query) = &parts.path_and_query {
        (path_and_query.path(), path_and_query.query())
    } else {
        ("", None)
    };
    let path = f(path);
    let path_and_query = if let Some(query) = query {
        format!("{path}?{query}")
    } else {
        path
    };
    parts.path_and_query = Some(path_and_query.parse()?);

    Ok(http::Uri::from_parts(parts)?)
}
