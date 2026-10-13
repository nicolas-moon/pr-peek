//! `pr-peek update`: replace the running binary with the latest release.

use anyhow::{Context, Result};
use reqwest::{
    Client,
    header::{ACCEPT, AUTHORIZATION},
};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    env,
    io::Read,
    path::{Path, PathBuf},
    process::Command,
};

use crate::github;

/// Where this binary is published. Each release ships a
/// `pr-peek-<target>.tar.xz` per platform plus a `.sha256` checksum file.
const UPDATE_REPOSITORY: &str = "nicolas-moon/pr-peek";

/// The releases API endpoint; `PR_PEEK_UPDATE_API_URL` points the updater
/// elsewhere (tests, mirrors, forks).
fn releases_api_url() -> String {
    env::var("PR_PEEK_UPDATE_API_URL").unwrap_or_else(|_| {
        format!("https://api.github.com/repos/{UPDATE_REPOSITORY}/releases/latest")
    })
}

#[derive(Debug, Deserialize)]
struct ReleaseInfo {
    tag_name: String,
    assets: Vec<ReleaseAsset>,
}

#[derive(Debug, Deserialize)]
struct ReleaseAsset {
    name: String,
    browser_download_url: String,
}

/// Strip the leading "v" from a release tag such as "v0.3.0".
fn version_from_tag(tag: &str) -> &str {
    tag.strip_prefix('v').unwrap_or(tag)
}

/// Numeric version segments; non-numeric tails ("0.2.0-beta.1") keep only
/// their leading digits so versions always compare.
fn version_key(version: &str) -> Vec<u64> {
    version
        .split('.')
        .map(|segment| {
            let digits: String = segment.chars().take_while(char::is_ascii_digit).collect();
            digits.parse().unwrap_or(0)
        })
        .collect()
}

/// True when `a` is strictly newer than `b`. Missing segments count as
/// zero, so "0.2" and "0.2.0" compare equal.
fn is_newer_version(a: &str, b: &str) -> bool {
    let (a, b) = (version_key(a), version_key(b));
    for i in 0..a.len().max(b.len()) {
        let a_segment = a.get(i).copied().unwrap_or(0);
        let b_segment = b.get(i).copied().unwrap_or(0);
        if a_segment != b_segment {
            return a_segment > b_segment;
        }
    }
    false
}

/// The target triple for this machine, mirroring the targets in
/// dist-workspace.toml; `None` when no prebuilt binary is published for it.
fn platform_target(os: &str, arch: &str) -> Option<&'static str> {
    Some(match (os, arch) {
        ("macos", "aarch64") => "aarch64-apple-darwin",
        ("macos", "x86_64") => "x86_64-apple-darwin",
        ("linux", "aarch64") => "aarch64-unknown-linux-gnu",
        ("linux", "x86_64") => "x86_64-unknown-linux-gnu",
        _ => return None,
    })
}

/// The hash from a published `.sha256` file, whose line looks like
/// `<hash> *<asset name>`.
fn expected_checksum(text: &str) -> Option<String> {
    text.lines()
        .next()
        .and_then(|line| line.split_whitespace().next())
        .map(str::to_ascii_lowercase)
}

fn sha256_hex(path: &Path) -> Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 8192];

    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }

    Ok(hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

/// Removes its temp directory when it goes out of scope.
struct TempDir {
    path: PathBuf,
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// Download `url` into `path`.
async fn download_file(client: &Client, url: &str, path: &Path) -> Result<()> {
    let bytes = client
        .get(url)
        .send()
        .await
        .with_context(|| format!("failed to download {url}"))?
        .error_for_status()
        .with_context(|| format!("GitHub returned an error downloading {url}"))?
        .bytes()
        .await
        .context("failed to read the download")?;

    std::fs::write(path, bytes).with_context(|| format!("failed to write {}", path.display()))?;
    Ok(())
}

fn make_executable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let mut permissions = std::fs::metadata(path)?.permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(path, permissions)?;
    Ok(())
}

pub async fn run(client: &Client) -> Result<()> {
    let current = env!("CARGO_PKG_VERSION");

    // The releases API is public; a token is only used to raise rate
    // limits, so it is optional here (unlike the GraphQL flow).
    let mut request = client
        .get(releases_api_url())
        .header(ACCEPT, "application/vnd.github+json");

    if let Some(token) = github::token(None) {
        request = request.header(AUTHORIZATION, format!("Bearer {token}"));
    }

    let release: ReleaseInfo = request
        .send()
        .await
        .context("failed to request the latest release")?
        .error_for_status()
        .context("GitHub returned an error")?
        .json()
        .await
        .context("failed to decode the release")?;

    let latest = version_from_tag(&release.tag_name);

    if !is_newer_version(latest, current) {
        println!(
            "pr-peek {current} is up to date (latest release is {}.)",
            release.tag_name
        );

        return Ok(());
    }

    let Some(triple) = platform_target(env::consts::OS, env::consts::ARCH) else {
        anyhow::bail!(
            "no prebuilt binary is published for {}/{}; install from source instead (see the README)",
            env::consts::OS,
            env::consts::ARCH
        );
    };

    let asset_name = format!("pr-peek-{triple}.tar.xz");
    let asset = release
        .assets
        .iter()
        .find(|asset| asset.name == asset_name)
        .with_context(|| format!("release {} does not include {asset_name}", release.tag_name))?;

    println!("Updating pr-peek {current} → {} ...", release.tag_name);

    let work_dir = env::temp_dir().join(format!("pr-peek-update-{}", std::process::id()));
    std::fs::create_dir_all(&work_dir).context("failed to create a temp directory")?;
    let _work = TempDir {
        path: work_dir.clone(),
    };

    let archive = work_dir.join(&asset_name);
    download_file(client, &asset.browser_download_url, &archive).await?;

    // Verify the download against the checksum published alongside it.
    let checksum_text = client
        .get(format!("{}.sha256", asset.browser_download_url))
        .send()
        .await
        .context("failed to request the published checksum")?
        .error_for_status()
        .context("GitHub returned an error")?
        .text()
        .await
        .context("failed to read the published checksum")?;

    let Some(expected) = expected_checksum(&checksum_text) else {
        anyhow::bail!("the published checksum could not be parsed");
    };
    if sha256_hex(&archive).context("failed to hash the downloaded archive")? != expected {
        anyhow::bail!(
            "checksum mismatch: the download does not match the published SHA-256, aborting"
        );
    }

    // The archive extracts to `pr-peek-<target>/` holding the binary.
    let extract_dir = work_dir.join("extract");
    std::fs::create_dir(&extract_dir)?;
    let status = Command::new("tar")
        .arg("-xf")
        .arg(&archive)
        .arg("-C")
        .arg(&extract_dir)
        .status()
        .context("failed to run `tar` (is it installed?)")?;
    if !status.success() {
        anyhow::bail!("`tar` failed to extract {asset_name}");
    }

    let new_binary = extract_dir
        .join(format!("pr-peek-{triple}"))
        .join("pr-peek");
    if !new_binary.is_file() {
        anyhow::bail!(
            "unexpected archive layout: no binary at {}",
            new_binary.display()
        );
    }

    // Rename over the running executable: safe on Linux and macOS, the
    // running process keeps its open inode.
    let current_exe = env::current_exe().context("failed to locate the running binary")?;
    let staging = current_exe.with_extension("update-tmp");
    std::fs::copy(&new_binary, &staging).context("failed to stage the new binary")?;
    make_executable(&staging)?;
    std::fs::rename(&staging, &current_exe).with_context(|| {
        format!(
            "failed to replace {}; if the directory is not writable, re-run with sudo or reinstall with the installer script",
            current_exe.display()
        )
    })?;

    println!("Updated pr-peek {current} → {}.", release.tag_name);
    println!(
        "Release notes: https://github.com/{UPDATE_REPOSITORY}/releases/tag/{}",
        release.tag_name
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_RELEASE: &str = r#"{
      "tag_name": "v0.3.0",
      "assets": [
        {
          "name": "pr-peek-x86_64-unknown-linux-gnu.tar.xz",
          "browser_download_url": "https://github.com/nicolas-moon/pr-peek/releases/download/v0.3.0/pr-peek-x86_64-unknown-linux-gnu.tar.xz"
        },
        {
          "name": "pr-peek-aarch64-apple-darwin.tar.xz",
          "browser_download_url": "https://github.com/nicolas-moon/pr-peek/releases/download/v0.3.0/pr-peek-aarch64-apple-darwin.tar.xz"
        }
      ]
    }"#;

    #[test]
    fn decodes_release_response() {
        let release: ReleaseInfo = serde_json::from_str(SAMPLE_RELEASE).unwrap();
        assert_eq!(release.tag_name, "v0.3.0");
        assert_eq!(release.assets.len(), 2);

        let asset = release
            .assets
            .iter()
            .find(|asset| asset.name == "pr-peek-x86_64-unknown-linux-gnu.tar.xz")
            .unwrap();
        assert!(asset.browser_download_url.ends_with(".tar.xz"));
    }

    #[test]
    fn version_from_tag_strips_leading_v() {
        assert_eq!(version_from_tag("v0.2.1"), "0.2.1");
        assert_eq!(version_from_tag("0.2.1"), "0.2.1");
    }

    #[test]
    fn version_comparison_orders_releases() {
        assert!(is_newer_version("0.2.2", "0.2.1"));
        assert!(is_newer_version("0.3.0", "0.2.9"));
        assert!(is_newer_version("0.10.0", "0.9.9"));
        assert!(is_newer_version("1.0.0", "0.99.99"));
        assert!(!is_newer_version("0.2.1", "0.2.1"));
        assert!(!is_newer_version("0.2.1", "0.2.2"));

        // missing segments count as zero
        assert!(!is_newer_version("0.2.0", "0.2"));
        assert!(is_newer_version("0.2.1", "0.2"));
        assert!(!is_newer_version("0.2", "0.2.1"));
    }

    #[test]
    fn platform_target_matches_published_targets() {
        assert_eq!(
            platform_target("macos", "aarch64"),
            Some("aarch64-apple-darwin")
        );
        assert_eq!(
            platform_target("macos", "x86_64"),
            Some("x86_64-apple-darwin")
        );
        assert_eq!(
            platform_target("linux", "aarch64"),
            Some("aarch64-unknown-linux-gnu")
        );
        assert_eq!(
            platform_target("linux", "x86_64"),
            Some("x86_64-unknown-linux-gnu")
        );
        assert_eq!(platform_target("windows", "x86_64"), None);
        assert_eq!(platform_target("linux", "riscv64"), None);
    }

    #[test]
    fn expected_checksum_reads_sha256_file() {
        let text = "3028cba207479c2c25fc47f82858251aadb05bc04dd824376861a52a47209958 *pr-peek-x86_64-unknown-linux-gnu.tar.xz\n";
        assert_eq!(
            expected_checksum(text).as_deref(),
            Some("3028cba207479c2c25fc47f82858251aadb05bc04dd824376861a52a47209958")
        );
        assert_eq!(expected_checksum("   \n"), None);
    }

    #[test]
    fn sha256_hex_of_empty_file_matches_known_vector() {
        let dir = std::env::temp_dir().join("pr-peek-sha256-test");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("empty");
        std::fs::write(&file, b"").unwrap();

        assert_eq!(
            sha256_hex(&file).unwrap(),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
