use super::{server_discovery, Operation};
use crate::misc;
use headers::authorization::Basic;
use headers::{Authorization, Header, HeaderMapExt};
use http::HeaderValue;
use std::env;
use tokio::process::Command;

async fn init(
    http_extraheader: bool,
    credential_helper: bool,
) -> anyhow::Result<tempfile::TempDir> {
    let temp_dir = tempfile::tempdir()?;
    misc::spawn(Command::new("git").current_dir(&temp_dir).arg("init"), None).await?;

    if http_extraheader {
        let mut values = Vec::new();
        Authorization::basic("qux", "quux").encode(&mut values);
        for value in values {
            let header = format!("{}: {}", Authorization::<Basic>::name(), value.to_str()?);
            misc::spawn(
                Command::new("git")
                    .current_dir(&temp_dir)
                    .arg("config")
                    .arg("http.https://git-server.com/.extraheader")
                    .arg(header),
                None,
            )
            .await?;
        }
    }

    if credential_helper {
        let credentials = temp_dir.path().join(".git").join("credentials");
        misc::spawn(
            Command::new("git")
                .current_dir(&temp_dir)
                .arg("config")
                .arg("credential.helper")
                .arg(format!("store --file={}", credentials.display())),
            None,
        )
        .await?;
        misc::spawn(
            Command::new("git")
                .current_dir(&temp_dir)
                .arg("credential-store")
                .arg(format!("--file={}", credentials.display()))
                .arg("store")
                .current_dir(&temp_dir),
            Some(b"protocol=https\nhost=git-server.com\nusername=corge\npassword=grault\n"),
        )
        .await?;
    }

    env::set_var(
        "GIT_SSH_COMMAND",
        concat!(
            "jq --args --null-input ",
            r#"'{href: "https://git-server.com/foo/bar.git/info/lfs", "#,
            r#"header: ($ARGS.positional | to_entries | map({(.key | tostring): .value}) | add)}' "#,
            "--",
        ),
    );

    Ok(temp_dir)
}

#[tokio::test]
async fn test_http() -> anyhow::Result<()> {
    let temp_dir = init(false, false).await?;
    misc::spawn(
        Command::new("git")
            .current_dir(&temp_dir)
            .arg("remote")
            .arg("add")
            .arg("baz")
            .arg("https://git-server.com/foo/bar"),
        None,
    )
    .await?;
    let response = server_discovery(&temp_dir, Operation::Upload, "baz", false).await?;
    anyhow::ensure!(response.href.as_ref() == "https://git-server.com/foo/bar.git/info/lfs");
    anyhow::ensure!(response.header.is_empty());
    Ok(())
}

#[tokio::test]
async fn test_http_suffix() -> anyhow::Result<()> {
    let temp_dir = init(false, false).await?;
    misc::spawn(
        Command::new("git")
            .current_dir(&temp_dir)
            .arg("remote")
            .arg("add")
            .arg("baz")
            .arg("https://git-server.com/foo/bar.git"),
        None,
    )
    .await?;
    let response = server_discovery(&temp_dir, Operation::Upload, "baz", false).await?;
    anyhow::ensure!(response.href.as_ref() == "https://git-server.com/foo/bar.git/info/lfs");
    anyhow::ensure!(response.header.is_empty());
    Ok(())
}

#[tokio::test]
async fn test_http_lfs_url() -> anyhow::Result<()> {
    let temp_dir = init(false, false).await?;
    misc::spawn(
        Command::new("git")
            .current_dir(&temp_dir)
            .arg("config")
            .arg("lfs.url")
            .arg("https://lfs-server.com/foo/bar"),
        None,
    )
    .await?;
    let response = server_discovery(&temp_dir, Operation::Upload, "baz", false).await?;
    anyhow::ensure!(response.href.as_ref() == "https://lfs-server.com/foo/bar");
    anyhow::ensure!(response.header.is_empty());
    Ok(())
}

#[tokio::test]
async fn test_http_remote_lfsurl() -> anyhow::Result<()> {
    let temp_dir = init(false, false).await?;
    misc::spawn(
        Command::new("git")
            .current_dir(&temp_dir)
            .arg("config")
            .arg("remote.dev.lfsurl")
            .arg("http://lfs-server.dev/foo/bar"),
        None,
    )
    .await?;
    let response = server_discovery(&temp_dir, Operation::Upload, "dev", false).await?;
    anyhow::ensure!(response.href.as_ref() == "http://lfs-server.dev/foo/bar");
    anyhow::ensure!(response.header.is_empty());
    Ok(())
}

#[tokio::test]
async fn test_http_lfsconfig_lfs_url() -> anyhow::Result<()> {
    let temp_dir = init(false, false).await?;
    misc::spawn(
        Command::new("git")
            .current_dir(&temp_dir)
            .arg("config")
            .arg("--file=.lfsconfig")
            .arg("lfs.url")
            .arg("https://lfs-server.com/foo/bar"),
        None,
    )
    .await?;
    let response = server_discovery(&temp_dir, Operation::Upload, "baz", false).await?;
    anyhow::ensure!(response.href.as_ref() == "https://lfs-server.com/foo/bar");
    anyhow::ensure!(response.header.is_empty());
    Ok(())
}

#[tokio::test]
async fn test_http_lfsconfig_remote_lfsurl() -> anyhow::Result<()> {
    let temp_dir = init(false, false).await?;
    misc::spawn(
        Command::new("git")
            .current_dir(&temp_dir)
            .arg("config")
            .arg("--file=.lfsconfig")
            .arg("remote.dev.lfsurl")
            .arg("http://lfs-server.dev/foo/bar"),
        None,
    )
    .await?;
    let response = server_discovery(&temp_dir, Operation::Upload, "dev", false).await?;
    anyhow::ensure!(response.href.as_ref() == "http://lfs-server.dev/foo/bar");
    anyhow::ensure!(response.header.is_empty());
    Ok(())
}

#[tokio::test]
async fn test_http_authorization_http_extraheader() -> anyhow::Result<()> {
    let temp_dir = init(true, false).await?;
    misc::spawn(
        Command::new("git")
            .current_dir(&temp_dir)
            .arg("remote")
            .arg("add")
            .arg("baz")
            .arg("https://git-server.com/foo/bar"),
        None,
    )
    .await?;
    let response = server_discovery(&temp_dir, Operation::Upload, "baz", false).await?;
    anyhow::ensure!(response.href.as_ref() == "https://git-server.com/foo/bar.git/info/lfs");
    anyhow::ensure!(dbg!(response.header.typed_get()) == Some(Authorization::basic("qux", "quux")));
    Ok(())
}

#[tokio::test]
async fn test_http_authorization_credential_helper() -> anyhow::Result<()> {
    let temp_dir = init(false, true).await?;
    misc::spawn(
        Command::new("git")
            .current_dir(&temp_dir)
            .arg("remote")
            .arg("add")
            .arg("baz")
            .arg("https://git-server.com/foo/bar"),
        None,
    )
    .await?;
    let response = server_discovery(&temp_dir, Operation::Upload, "baz", true).await?;
    anyhow::ensure!(response.href.as_ref() == "https://git-server.com/foo/bar.git/info/lfs");
    anyhow::ensure!(response.header.typed_get() == Some(Authorization::basic("corge", "grault")));
    Ok(())
}

#[tokio::test]
async fn test_http_authorization_both() -> anyhow::Result<()> {
    let temp_dir = init(true, false).await?;
    misc::spawn(
        Command::new("git")
            .current_dir(&temp_dir)
            .arg("remote")
            .arg("add")
            .arg("baz")
            .arg("https://git-server.com/foo/bar"),
        None,
    )
    .await?;
    let response = server_discovery(&temp_dir, Operation::Upload, "baz", false).await?;
    anyhow::ensure!(response.href.as_ref() == "https://git-server.com/foo/bar.git/info/lfs");
    anyhow::ensure!(response.header.typed_get() == Some(Authorization::basic("qux", "quux")));
    Ok(())
}

#[tokio::test]
async fn test_git() -> anyhow::Result<()> {
    let temp_dir = init(false, false).await?;
    misc::spawn(
        Command::new("git")
            .current_dir(&temp_dir)
            .arg("remote")
            .arg("add")
            .arg("baz")
            .arg("git@git-server.com:foo/bar.git"),
        None,
    )
    .await?;
    let response = server_discovery(&temp_dir, Operation::Upload, "baz", false).await?;
    anyhow::ensure!(response.href.as_ref() == "https://git-server.com/foo/bar.git/info/lfs");
    anyhow::ensure!(response.header.is_empty());
    Ok(())
}

#[tokio::test]
async fn test_ssh() -> anyhow::Result<()> {
    let temp_dir = init(false, false).await?;
    misc::spawn(
        Command::new("git")
            .current_dir(&temp_dir)
            .arg("remote")
            .arg("add")
            .arg("baz")
            .arg("ssh://git-server.com/foo/bar.git"),
        None,
    )
    .await?;
    let response = server_discovery(&temp_dir, Operation::Upload, "baz", false).await?;
    anyhow::ensure!(response.href.as_ref() == "https://git-server.com/foo/bar.git/info/lfs");
    anyhow::ensure!(response.header.is_empty());
    Ok(())
}

#[tokio::test]
async fn test_ssh_authorization() -> anyhow::Result<()> {
    let temp_dir = init(false, false).await?;
    misc::spawn(
        Command::new("git")
            .current_dir(&temp_dir)
            .arg("remote")
            .arg("add")
            .arg("baz")
            .arg("git@git-server.com:foo/bar.git"),
        None,
    )
    .await?;
    let mut response = server_discovery(&temp_dir, Operation::Upload, "baz", true).await?;
    anyhow::ensure!(response.href.as_ref() == "https://git-server.com/foo/bar.git/info/lfs");
    anyhow::ensure!(response.header.remove("0") == Some(HeaderValue::from_static("-l")));
    anyhow::ensure!(response.header.remove("1") == Some(HeaderValue::from_static("git")));
    anyhow::ensure!(
        response.header.remove("2") == Some(HeaderValue::from_static("git-server.com"))
    );
    anyhow::ensure!(
        response.header.remove("3") == Some(HeaderValue::from_static("git-lfs-authenticate"))
    );
    anyhow::ensure!(response.header.remove("4") == Some(HeaderValue::from_static("/foo/bar.git")));
    anyhow::ensure!(response.header.remove("5") == Some(HeaderValue::from_static("upload")));
    anyhow::ensure!(response.header.is_empty());
    Ok(())
}
