use crate::{cache, git_lfs};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize, Serialize)]
pub struct Line<'a> {
    pub operation: git_lfs::Operation,
    pub oid: Cow<'a, str>,
    pub size: u64,
    pub cache: Option<cache::Source>,
    pub start: DateTime<Utc>,
    pub finish: DateTime<Utc>,
}

pub fn dir<P>(git_dir: P) -> PathBuf
where
    P: AsRef<Path>,
{
    git_dir
        .as_ref()
        .join(env!("CARGO_PKG_NAME").trim_start_matches("git-"))
        .join("logs")
}
