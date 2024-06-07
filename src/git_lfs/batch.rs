// https://github.com/git-lfs/git-lfs/blob/master/docs/api/batch.md

use super::{Error, Operation};
use crate::misc;
use http::{header, HeaderMap, Uri};
use http_body_util::{BodyExt, Full};
use serde::{Deserialize, Serialize};

#[tracing::instrument(err, ret)]
pub(crate) async fn batch(
    client: &misc::Client,
    href: &Uri,
    header: &HeaderMap,
    request: &Request<'_>,
) -> anyhow::Result<Response> {
    let builder = http::Request::post(misc::patch_path(href.clone(), |path| {
        format!("{path}/objects/batch")
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
        struct E {
            message: String,
        }
        let E { message } = serde_json::from_slice(&body)?;
        Err(Error {
            code: parts.status,
            message,
        }
        .into())
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct Request<'a> {
    pub(crate) operation: Operation,
    pub(crate) transfers: &'a [request::Transfer],
    pub(crate) objects: &'a [request::Object<'a>],
}

pub(crate) mod request {
    use serde::Serialize;

    #[derive(Debug, Serialize)]
    #[serde(rename_all = "lowercase")]
    pub(crate) enum Transfer {
        Basic,
    }

    #[derive(Debug, Serialize)]
    pub(crate) struct Object<'a> {
        pub(crate) oid: &'a str,
        pub(crate) size: u64,
    }
}

#[derive(Debug, Deserialize)]
pub(crate) struct Response {
    pub(crate) objects: Vec<response::Object>,
}

pub(crate) mod response {
    use super::super::Error;
    use http::{HeaderMap, Uri};
    use serde::Deserialize;

    #[derive(Debug, Deserialize)]
    pub(crate) struct Object {
        pub(crate) oid: String,
        pub(crate) size: u64,
        #[serde(flatten)]
        pub(crate) inner: Inner,
    }

    #[derive(Debug, Deserialize)]
    #[serde(rename_all = "lowercase")]
    pub(crate) enum Inner {
        Actions {
            upload: Option<Action>,
            verify: Option<Action>,
            download: Option<Action>,
        },
        Error(Error),
    }

    #[derive(Debug, Deserialize)]
    pub(crate) struct Action {
        #[serde(default, with = "http_serde::uri")]
        pub(crate) href: Uri,
        #[serde(default, with = "http_serde::header_map")]
        pub(crate) header: HeaderMap,
    }
}
