use super::{server_discovery, Operation};
use crate::git;
use headers::{Authorization, HeaderMapExt};

async fn init() -> anyhow::Result<tempfile::TempDir> {
    let temp_dir = tempfile::tempdir()?;
    git::spawn(&temp_dir, None, |command| command.arg("init")).await?;

    let credentials = temp_dir.path().join(".git").join("credentials");
    git::spawn(&temp_dir, None, |command| {
        command
            .arg("config")
            .arg("--replace-all")
            .arg("credential.helper")
            .arg(format!("store --file={}", credentials.display()))
    })
    .await?;
    git::spawn(
        &temp_dir,
        Some(b"protocol=https\nhost=git-server.com\nusername=qux\npassword=quux\n"),
        |command| {
            command
                .arg("credential-store")
                .arg(format!("--file={}", credentials.display()))
                .arg("store")
        },
    )
    .await?;
    Ok(temp_dir)
}

#[tokio::test]
async fn test_http() -> anyhow::Result<()> {
    let temp_dir = init().await?;
    git::spawn(&temp_dir, None, |command| {
        command
            .arg("remote")
            .arg("add")
            .arg("baz")
            .arg("https://git-server.com/foo/bar")
    })
    .await?;
    let response = server_discovery(&temp_dir, Operation::Upload, "baz", false).await?;
    anyhow::ensure!(response.href == "https://git-server.com/foo/bar.git/info/lfs");
    anyhow::ensure!(response.header.is_empty());
    Ok(())
}

#[tokio::test]
async fn test_http_suffix() -> anyhow::Result<()> {
    let temp_dir = init().await?;
    git::spawn(&temp_dir, None, |command| {
        command
            .arg("remote")
            .arg("add")
            .arg("baz")
            .arg("https://git-server.com/foo/bar.git")
    })
    .await?;
    let response = server_discovery(&temp_dir, Operation::Upload, "baz", false).await?;
    anyhow::ensure!(response.href == "https://git-server.com/foo/bar.git/info/lfs");
    anyhow::ensure!(response.header.is_empty());
    Ok(())
}

#[tokio::test]
async fn test_http_lfs_url() -> anyhow::Result<()> {
    let temp_dir = init().await?;
    git::spawn(&temp_dir, None, |command| {
        command
            .arg("config")
            .arg("lfs.url")
            .arg("https://lfs-server.com/foo/bar")
    })
    .await?;
    let response = server_discovery(&temp_dir, Operation::Upload, "baz", false).await?;
    anyhow::ensure!(response.href == "https://lfs-server.com/foo/bar");
    anyhow::ensure!(response.header.is_empty());
    Ok(())
}

#[tokio::test]
async fn test_http_remote_lfsurl() -> anyhow::Result<()> {
    let temp_dir = init().await?;
    git::spawn(&temp_dir, None, |command| {
        command
            .arg("config")
            .arg("remote.dev.lfsurl")
            .arg("http://lfs-server.dev/foo/bar")
    })
    .await?;
    let response = server_discovery(&temp_dir, Operation::Upload, "dev", false).await?;
    anyhow::ensure!(response.href == "http://lfs-server.dev/foo/bar");
    anyhow::ensure!(response.header.is_empty());
    Ok(())
}

#[tokio::test]
async fn test_http_lfsconfig_lfs_url() -> anyhow::Result<()> {
    let temp_dir = init().await?;
    git::spawn(&temp_dir, None, |command| {
        command
            .arg("config")
            .arg("--file=.lfsconfig")
            .arg("lfs.url")
            .arg("https://lfs-server.com/foo/bar")
    })
    .await?;
    let response = server_discovery(&temp_dir, Operation::Upload, "baz", false).await?;
    anyhow::ensure!(response.href == "https://lfs-server.com/foo/bar");
    anyhow::ensure!(response.header.is_empty());
    Ok(())
}

#[tokio::test]
async fn test_http_lfsconfig_remote_lfsurl() -> anyhow::Result<()> {
    let temp_dir = init().await?;
    git::spawn(&temp_dir, None, |command| {
        command
            .arg("config")
            .arg("--file=.lfsconfig")
            .arg("remote.dev.lfsurl")
            .arg("http://lfs-server.dev/foo/bar")
    })
    .await?;
    let response = server_discovery(&temp_dir, Operation::Upload, "dev", false).await?;
    anyhow::ensure!(response.href == "http://lfs-server.dev/foo/bar");
    anyhow::ensure!(response.header.is_empty());
    Ok(())
}

#[tokio::test]
async fn test_http_authorization() -> anyhow::Result<()> {
    let temp_dir = init().await?;
    git::spawn(&temp_dir, None, |command| {
        command
            .arg("remote")
            .arg("add")
            .arg("baz")
            .arg("https://git-server.com/foo/bar")
    })
    .await?;
    let response = server_discovery(&temp_dir, Operation::Upload, "baz", true).await?;
    anyhow::ensure!(response.href == "https://git-server.com/foo/bar.git/info/lfs");
    anyhow::ensure!(response.header.typed_get() == Some(Authorization::basic("qux", "quux")));
    Ok(())
}
