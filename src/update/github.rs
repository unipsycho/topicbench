//! GitHub Releases API for public repo `unipsycho/topicbench`.

use serde::Deserialize;

/// Owner/repo used for Releases API + HTML fallback links.
pub const GITHUB_REPO: &str = "unipsycho/topicbench";

const USER_AGENT: &str = concat!("TopicBench/", env!("CARGO_PKG_VERSION"));
const API_TIMEOUT_SECS: u64 = 20;

/// Platform binary asset names published with each release.
#[cfg(target_os = "linux")]
pub const ASSET_NAME: &str = "topicbench-linux-x64";
#[cfg(target_os = "windows")]
pub const ASSET_NAME: &str = "topicbench-windows-x64.exe";
#[cfg(not(any(target_os = "linux", target_os = "windows")))]
pub const ASSET_NAME: &str = "topicbench-linux-x64";

pub const SHA256SUMS_NAME: &str = "SHA256SUMS";

#[derive(Debug, Clone)]
pub struct ReleaseInfo {
    /// Tag as published (may include leading `v`).
    pub tag: String,
    /// Semver-ish string without leading `v` (for compare / display).
    pub version: String,
    pub name: String,
    pub body: String,
    pub html_url: String,
    /// Direct download URL for this platform's asset (if present).
    pub asset_url: Option<String>,
    /// Direct download URL for SHA256SUMS (if present).
    pub sha256sums_url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ApiRelease {
    tag_name: String,
    name: Option<String>,
    body: Option<String>,
    html_url: String,
    assets: Vec<ApiAsset>,
}

#[derive(Debug, Deserialize)]
struct ApiAsset {
    name: String,
    browser_download_url: String,
}

/// Fetch the latest published release. Fails gracefully when the repo/release is missing.
pub fn fetch_latest() -> Result<ReleaseInfo, String> {
    let url = format!("https://api.github.com/repos/{GITHUB_REPO}/releases/latest");
    let agent = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(API_TIMEOUT_SECS))
        .build();
    let resp = agent
        .get(&url)
        .set("User-Agent", USER_AGENT)
        .set("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| friendly_http_err(e))?;
    let status = resp.status();
    if status == 404 {
        return Err(format!(
            "no releases at github.com/{GITHUB_REPO} (publish a Release there)"
        ));
    }
    if !(200..300).contains(&status) {
        return Err(format!("GitHub API HTTP {status}"));
    }
    let api: ApiRelease = resp
        .into_json()
        .map_err(|e| format!("parse release JSON: {e}"))?;
    Ok(from_api(api))
}

fn from_api(api: ApiRelease) -> ReleaseInfo {
    let tag = api.tag_name;
    let version = tag.strip_prefix('v').unwrap_or(&tag).to_string();
    let mut asset_url = None;
    let mut sha256sums_url = None;
    for a in &api.assets {
        if a.name == ASSET_NAME {
            asset_url = Some(a.browser_download_url.clone());
        } else if a.name == SHA256SUMS_NAME {
            sha256sums_url = Some(a.browser_download_url.clone());
        }
    }
    ReleaseInfo {
        tag: tag.clone(),
        version,
        name: api.name.unwrap_or(tag),
        body: api.body.unwrap_or_default(),
        html_url: api.html_url,
        asset_url,
        sha256sums_url,
    }
}

fn friendly_http_err(err: ureq::Error) -> String {
    match err {
        ureq::Error::Status(code, _) => format!("GitHub API HTTP {code}"),
        ureq::Error::Transport(t) => format!("network: {t}"),
    }
}

/// GET bytes from a URL (release assets / SHA256SUMS).
pub fn download_bytes(url: &str) -> Result<Vec<u8>, String> {
    let agent = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(120))
        .build();
    let resp = agent
        .get(url)
        .set("User-Agent", USER_AGENT)
        .call()
        .map_err(|e| friendly_http_err(e))?;
    let status = resp.status();
    if !(200..300).contains(&status) {
        return Err(format!("download HTTP {status}"));
    }
    let mut buf = Vec::new();
    resp.into_reader()
        .read_to_end(&mut buf)
        .map_err(|e| format!("download read: {e}"))?;
    Ok(buf)
}

/// Open the release HTML page in the default browser (fallback when assets missing).
pub fn open_release_page(html_url: &str) -> Result<(), String> {
    open_url(html_url)
}

/// Open a URL in the default browser (release page, Pro marketing site, …).
pub fn open_url(url: &str) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("cmd")
            .args(["/C", "start", "", url])
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("open browser: {e}"))
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(url)
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("open browser: {e}"))
    }
    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    {
        std::process::Command::new("xdg-open")
            .arg(url)
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("open browser: {e}"))
    }
}
