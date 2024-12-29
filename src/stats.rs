use crate::{git, jsonl, logs};
use clap::Parser;
use std::env;
use std::fmt::{self, Display};
use tokio::fs::{self, File};

#[derive(Debug, Parser)]
pub struct Args {}

pub async fn main(_: Args) -> anyhow::Result<()> {
    let current_dir = env::current_dir()?;
    let git_dir = git::rev_parse_absolute_git_dir(&current_dir).await?;
    let logs_dir = logs::dir(&git_dir);

    let mut total = Stat::default();
    let mut hit = Stat::default();
    let mut miss = Stat::default();

    if let Ok(mut read_dir) = fs::read_dir(logs_dir).await {
        while let Some(entry) = read_dir.next_entry().await? {
            if entry.path().extension() == Some("jsonl".as_ref()) {
                let mut reader = jsonl::Reader::new(File::open(entry.path()).await?);
                while let Some(line) = reader.read::<logs::Line>().await? {
                    total.push(&line);
                    if line.cache.is_some() {
                        hit.push(&line);
                    } else {
                        miss.push(&line);
                    }
                }
            }
        }
    }

    println!("total: {total}");
    println!("hit: {hit}");
    println!("miss: {miss}");

    Ok(())
}

#[derive(Default)]
struct Stat {
    count: usize,
    size: u64,
}

impl Stat {
    fn push(&mut self, line: &logs::Line<'_>) {
        self.count += 1;
        self.size += line.size;
    }
}

impl Display for Stat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} objects ({})",
            self.count,
            humansize::format_size(self.size, humansize::BINARY),
        )
    }
}
