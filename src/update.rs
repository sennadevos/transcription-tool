//! Self-update from the latest GitHub release: check on startup, download, verify, swap the executable, restart.

use std::ffi::OsString;
use std::io::Read;
use std::path::PathBuf;

use anyhow::{Context, Result, anyhow};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::openrouter::{agent, net_error};

const REPO: &str = "sennadevos/transcription-tool";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
const ASSET: &str = if cfg!(windows) { "transcription-tool.exe" } else { "transcription-tool-linux-x86_64" };
const MAX_DOWNLOAD: u64 = 200 * 1024 * 1024;

#[derive(Clone)]
pub struct Release {
    pub version: String,
    url: String,
    sha256: String,
}

fn parse(version: &str) -> Vec<u64> {
    version.trim().trim_start_matches('v').split('.').map(|p| p.parse().unwrap_or(0)).collect()
}

fn is_newer(candidate: &str, current: &str) -> bool {
    let (mut a, mut b) = (parse(candidate), parse(current));
    let n = a.len().max(b.len());
    a.resize(n, 0);
    b.resize(n, 0);
    a > b
}

/// The newest release when it is newer than this program, `None` when we are up to date.
pub fn check() -> Result<Option<Release>> {
    let mut resp = agent()
        .get(format!("https://api.github.com/repos/{REPO}/releases/latest"))
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", format!("transcription-tool/{VERSION}"))
        .call()
        .map_err(net_error)?;
    let status = resp.status().as_u16();
    if status == 404 {
        return Ok(None); // no release published yet
    }
    if status != 200 {
        return Err(anyhow!("GitHub answered with error {status}"));
    }
    let body: Value = resp.body_mut().read_json().map_err(net_error)?;
    release_from(&body, VERSION)
}

fn release_from(body: &Value, current: &str) -> Result<Option<Release>> {
    let tag = body["tag_name"].as_str().context("release has no version")?;
    if !is_newer(tag, current) {
        return Ok(None);
    }
    let asset = body["assets"].as_array().into_iter().flatten().find(|a| a["name"] == ASSET).context("release has no file for this system")?;
    let url = asset["browser_download_url"].as_str().context("asset has no link")?.to_string();
    let sha256 = asset["digest"].as_str().and_then(|d| d.strip_prefix("sha256:")).context("release file has no checksum")?.to_lowercase();
    Ok(Some(Release { version: tag.trim_start_matches('v').to_string(), url, sha256 }))
}

fn sibling(exe: &std::path::Path, suffix: &str) -> PathBuf {
    let mut s: OsString = exe.as_os_str().to_owned();
    s.push(suffix);
    s.into()
}

/// Removes the leftover of the previous version, which Windows could not delete while it was still running.
pub fn cleanup() {
    if let Ok(exe) = std::env::current_exe() {
        let _ = std::fs::remove_file(sibling(&exe, ".old"));
    }
}

/// Downloads and checks the new version, puts it in place of this program and starts it. The caller must then exit.
pub fn apply(rel: &Release) -> Result<()> {
    let mut resp = agent().get(&rel.url).header("User-Agent", format!("transcription-tool/{VERSION}")).call().map_err(net_error)?;
    if resp.status().as_u16() != 200 {
        return Err(anyhow!("The update could not be downloaded (error {}). Try again later.", resp.status().as_u16()));
    }
    let mut bytes = Vec::new();
    resp.body_mut().as_reader().take(MAX_DOWNLOAD).read_to_end(&mut bytes).map_err(|_| anyhow!("The download was interrupted. Try again."))?;
    let got: String = Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect();
    if got != rel.sha256 {
        return Err(anyhow!("The downloaded update is damaged, so it was not installed. Try again."));
    }

    let exe = std::env::current_exe()?;
    let new = sibling(&exe, ".new");
    let old = sibling(&exe, ".old");
    let cant = |_| anyhow!("Can't update the program in its current place. Download the new version from github.com/{REPO}/releases instead.");
    std::fs::write(&new, &bytes).map_err(cant)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&new, std::fs::Permissions::from_mode(0o755)).map_err(cant)?;
    }
    swap(&exe, &new, &old).map_err(cant)?;
    std::process::Command::new(&exe).spawn().context("The update was installed, but the program could not restart. Open it again yourself.")?;
    Ok(())
}

#[cfg(unix)]
fn swap(exe: &std::path::Path, new: &std::path::Path, _old: &std::path::Path) -> std::io::Result<()> {
    std::fs::rename(new, exe) // replaces the file a running program was started from
}

/// Windows refuses to overwrite a running .exe but allows renaming it.
#[cfg(windows)]
fn swap(exe: &std::path::Path, new: &std::path::Path, old: &std::path::Path) -> std::io::Result<()> {
    let _ = std::fs::remove_file(old);
    std::fs::rename(exe, old)?;
    std::fs::rename(new, exe).inspect_err(|_| {
        let _ = std::fs::rename(old, exe);
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn versions_compare_numerically() {
        assert!(is_newer("v0.10.0", "0.9.0"));
        assert!(is_newer("1.0", "0.9.9"));
        assert!(!is_newer("v0.1.0", "0.1.0"));
        assert!(!is_newer("0.1.0", "0.2.0"));
    }

    #[test]
    fn picks_our_asset_and_needs_a_checksum() {
        let rel = |digest: Value| json!({"tag_name": "v9.0.0", "assets": [
            {"name": "other", "browser_download_url": "https://x/other", "digest": "sha256:aa"},
            {"name": ASSET, "browser_download_url": "https://x/ours", "digest": digest},
        ]});
        let r = release_from(&rel(json!("sha256:ABCD")), "0.1.0").unwrap().unwrap();
        assert_eq!((r.version.as_str(), r.url.as_str(), r.sha256.as_str()), ("9.0.0", "https://x/ours", "abcd"));
        assert!(release_from(&rel(Value::Null), "0.1.0").is_err());
        assert!(release_from(&rel(json!("sha256:ab")), "9.0.0").unwrap().is_none());
    }
}
