// https://github.com/git-lfs/git-lfs/blob/master/docs/api/batch.md

use super::{Error, Operation};
use crate::misc;
use http::{header, HeaderMap};
use http_body_util::{BodyExt, Full};
use serde::{Deserialize, Serialize};
use url::Url;

#[tracing::instrument(err, ret)]
pub async fn batch(
    client: &misc::Client,
    href: &Url,
    header: &HeaderMap,
    request: &Request<'_>,
) -> anyhow::Result<Response> {
    let mut href = href.clone();
    misc::path_segments_mut(&mut href)?
        .push("objects")
        .push("batch");
    let builder = http::Request::post(href.as_ref());
    let builder = header.iter().fold(builder, |builder, (name, value)| {
        builder.header(name, value)
    });
    let request = builder
        .header(header::ACCEPT, "application/vnd.git-lfs+json")
        .header(header::CONTENT_TYPE, "application/vnd.git-lfs+json")
        .body(
            Full::from(serde_json::to_vec(&request)?)
                .map_err(Box::from)
                .boxed_unsync(),
        )?;
    let response = client.request(request).await?;
    let (parts, body) = response.into_parts();
    let body = body.collect().await?.to_bytes();
    if parts.status.is_success() {
        Ok(serde_json::from_slice(&body)?)
    } else {
        #[derive(Deserialize)]
        struct B {
            message: String,
        }

        let message = if let Ok(B { message }) = serde_json::from_slice(&body) {
            message
        } else {
            format!("{body:?}")
        };
        Err(Error {
            code: parts.status,
            message,
        }
        .into())
    }
}

#[derive(Debug, Serialize)]
pub struct Request<'a> {
    pub operation: Operation,
    pub transfers: &'a [request::Transfer],
    pub objects: &'a [request::Object<'a>],
}

pub mod request {
    use serde::Serialize;

    #[derive(Debug, Serialize)]
    #[serde(rename_all = "lowercase")]
    pub enum Transfer {
        Basic,
    }

    #[derive(Debug, Serialize)]
    pub struct Object<'a> {
        pub oid: &'a str,
        pub size: u64,
    }
}

#[derive(Debug, Deserialize)]
pub struct Response {
    pub objects: Vec<response::Object>,
}

pub mod response {
    use super::super::Error;
    use http::HeaderMap;
    use serde::Deserialize;
    use url::Url;

    #[derive(Debug, Deserialize)]
    pub struct Object {
        pub oid: String,
        #[allow(dead_code)]
        pub size: u64,
        #[serde(flatten)]
        pub inner: Inner,
    }

    #[derive(Debug, Deserialize)]
    #[serde(rename_all = "lowercase")]
    pub enum Inner {
        Actions {
            #[allow(dead_code)]
            upload: Option<Box<Action>>,
            #[allow(dead_code)]
            verify: Option<Box<Action>>,
            download: Option<Box<Action>>,
        },
        Error(Error),
    }

    #[derive(Debug, Deserialize)]
    pub struct Action {
        pub href: Url,
        #[serde(default, with = "http_serde::header_map")]
        pub header: HeaderMap,
    }
}
