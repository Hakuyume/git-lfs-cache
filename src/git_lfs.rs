pub mod batch;
pub mod custom_transfers;
pub mod server_discovery;

pub use batch::batch;
use http::StatusCode;
use serde::{Deserialize, Serialize};
pub use server_discovery::server_discovery;

#[derive(Clone, Debug, Deserialize, Serialize, thiserror::Error)]
#[error("[{code:?}] {message}")]
pub struct Error {
    #[serde(with = "http_serde::status_code")]
    pub code: StatusCode,
    pub message: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Operation {
    Upload,
    Download,
}
