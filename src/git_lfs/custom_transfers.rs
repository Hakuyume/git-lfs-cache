// https://github.com/git-lfs/git-lfs/blob/master/docs/custom-transfers.md

use super::{Error, Operation};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase", tag = "event")]
pub enum Request {
    Init {
        operation: Operation,
        remote: String,
        #[allow(dead_code)]
        concurrent: bool,
        #[allow(dead_code)]
        concurrenttransfers: usize,
    },
    Upload {
        oid: String,
        #[allow(dead_code)]
        size: u64,
        #[allow(dead_code)]
        path: PathBuf,
    },
    Download {
        oid: String,
        size: u64,
    },
    Terminate,
}

#[derive(Debug, Serialize)]
pub struct InitResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<Error>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "lowercase", tag = "event")]
pub enum Response<'a> {
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
        bytes_so_far: u64,
        bytes_since_last: u64,
    },
}
