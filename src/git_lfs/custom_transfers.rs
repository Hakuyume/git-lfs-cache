// https://github.com/git-lfs/git-lfs/blob/master/docs/custom-transfers.md

use super::{Error, Operation};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase", tag = "event")]
pub(crate) enum Request {
    Init {
        operation: Operation,
        remote: String,
        concurrent: bool,
        concurrenttransfers: usize,
    },
    Upload {
        oid: String,
        size: u64,
        path: PathBuf,
    },
    Download {
        oid: String,
        size: u64,
    },
    Terminate,
}

#[derive(Debug, Serialize)]
pub(crate) struct InitResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) error: Option<Error>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "lowercase", tag = "event")]
pub(crate) enum Response<'a> {
    Complete {
        oid: &'a str,
        #[serde(skip_serializing_if = "Option::is_none")]
        path: Option<&'a Path>,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<Error>,
    },
    #[serde(rename_all = "camelCase")]
    Progress {
        oid: &'a str,
        bytes_so_far: usize,
        bytes_since_last: usize,
    },
}
