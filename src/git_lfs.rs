pub(crate) mod batch;
pub(crate) mod custom_transfers;
pub(crate) mod server_discovery;

pub(crate) use batch::batch;
use http::StatusCode;
use serde::{Deserialize, Serialize};
pub(crate) use server_discovery::server_discovery;

#[derive(Clone, Debug, Deserialize, Serialize, thiserror::Error)]
#[error("[{code:?}] {message}")]
pub(crate) struct Error {
    #[serde(with = "http_serde::status_code")]
    pub(crate) code: StatusCode,
    pub(crate) message: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Operation {
    Upload,
    Download,
}
