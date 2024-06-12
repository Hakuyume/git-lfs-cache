// https://github.com/git-lfs/git-lfs/blob/master/docs/api/batch.md

use super::{Error, Operation};
use crate::misc;
use http::{header, HeaderMap, Uri};
use http_body_util::{BodyExt, Full};
use serde::{Deserialize, Serialize};

#[tracing::instrument(err, ret)]
pub async fn batch(
    client: &misc::Client,
    href: &Uri,
    header: &HeaderMap,
    request: &Request<'_>,
) -> anyhow::Result<Response> {
    let builder = http::Request::post(misc::patch_path(href.clone(), |path| {
        format!("{}/objects/batch", path.trim_end_matches('/'))
    })?);
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

        let B { message } = serde_json::from_slice(&body)?;
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
    use http::{HeaderMap, Uri};
    use serde::Deserialize;

    #[derive(Debug, Deserialize)]
    pub struct Object {
        pub oid: String,
        pub size: u64,
        #[serde(flatten)]
        pub inner: Inner,
    }

    #[derive(Debug, Deserialize)]
    #[serde(rename_all = "lowercase")]
    pub enum Inner {
        Actions {
            upload: Option<Action>,
            verify: Option<Action>,
            download: Option<Action>,
        },
        Error(Error),
    }

    #[derive(Debug, Deserialize)]
    pub struct Action {
        #[serde(default, with = "http_serde::uri")]
        pub href: Uri,
        #[serde(default, with = "http_serde::header_map")]
        pub header: HeaderMap,
    }
}
