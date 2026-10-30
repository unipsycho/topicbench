//! Probe Pro sidecar update status (localhost only — no clients-host URLs/creds).
//!
//! Free TopicBench shows Pro version lines in Settings → Updates only when the
//! sidecar answers. Check/install are delegated to Pro (`/update/*`).

use serde::Deserialize;
use std::time::Duration;

const DEFAULT_BASE: &str = "http://127.0.0.1:7853";
const TIMEOUT: Duration = Duration::from_millis(400);

/// Snapshot from Pro `/health` or `/update/status` (subset).
#[derive(Debug, Clone, Default)]
pub struct ProUpdateInfo {
    pub current: String,
    pub checking: bool,
    pub installing: bool,
    pub latest: Option<String>,
    pub update_available: bool,
    pub error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct HealthJson {
    #[serde(default)]
    ok: bool,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    update: Option<UpdateBlock>,
}

#[derive(Debug, Deserialize)]
struct UpdateBlock {
    #[serde(default)]
    current: Option<String>,
    #[serde(default)]
    checking: bool,
    #[serde(default)]
    installing: bool,
    #[serde(default)]
    latest: Option<String>,
    #[serde(default)]
    update_available: bool,
    #[serde(default)]
    error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct StatusJson {
    #[serde(default)]
    ok: bool,
    #[serde(default)]
    current: Option<String>,
    #[serde(default)]
    checking: bool,
    #[serde(default)]
    installing: bool,
    #[serde(default)]
    latest: Option<String>,
    #[serde(default)]
    update_available: bool,
    #[serde(default)]
    error: Option<String>,
}

fn base_url() -> String {
    std::env::var("TOPICBENCH_PRO_URL")
        .ok()
        .map(|s| s.trim().trim_end_matches('/').to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| DEFAULT_BASE.to_string())
}

/// Returns `Some` only when the Pro sidecar answers.
pub fn probe() -> Option<ProUpdateInfo> {
    let base = base_url();
    // Prefer /update/status (richer); fall back to /health.version.
    if let Some(info) = get_status(&base) {
        return Some(info);
    }
    get_health(&base)
}

fn get_status(base: &str) -> Option<ProUpdateInfo> {
    let url = format!("{base}/update/status");
    let agent = ureq::AgentBuilder::new().timeout(TIMEOUT).build();
    let resp = agent.get(&url).call().ok()?;
    if !(200..300).contains(&resp.status()) {
        return None;
    }
    let j: StatusJson = resp.into_json().ok()?;
    if !j.ok && j.current.is_none() {
        return None;
    }
    Some(ProUpdateInfo {
        current: j.current.unwrap_or_default(),
        checking: j.checking,
        installing: j.installing,
        latest: j.latest,
        update_available: j.update_available,
        error: j.error,
    })
}

fn get_health(base: &str) -> Option<ProUpdateInfo> {
    let url = format!("{base}/health");
    let agent = ureq::AgentBuilder::new().timeout(TIMEOUT).build();
    let resp = agent.get(&url).call().ok()?;
    if !(200..300).contains(&resp.status()) {
        return None;
    }
    let j: HealthJson = resp.into_json().ok()?;
    if !j.ok {
        return None;
    }
    if let Some(u) = j.update {
        return Some(ProUpdateInfo {
            current: u
                .current
                .or(j.version)
                .unwrap_or_default(),
            checking: u.checking,
            installing: u.installing,
            latest: u.latest,
            update_available: u.update_available,
            error: u.error,
        });
    }
    j.version.map(|v| ProUpdateInfo {
        current: v,
        ..Default::default()
    })
}

/// Ask Pro to check its private update channel (fire-and-forget). No-op if down.
pub fn request_check() {
    let url = format!("{}/update/check", base_url());
    let agent = ureq::AgentBuilder::new().timeout(TIMEOUT).build();
    let _ = agent.post(&url).call();
}

/// Ask Pro to download/install the pending update. Returns Err if sidecar refuses/down.
pub fn request_install() -> Result<(), String> {
    let url = format!("{}/update/install", base_url());
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(2))
        .build();
    let resp = agent
        .post(&url)
        .call()
        .map_err(|e| format!("Pro install: {e}"))?;
    let status = resp.status();
    let body = resp
        .into_string()
        .map_err(|e| format!("Pro install read: {e}"))?;
    if !(200..300).contains(&status) {
        return Err(format!("Pro install HTTP {status}: {body}"));
    }
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&body) {
        if v.get("ok").and_then(|x| x.as_bool()) == Some(false) {
            let err = v
                .get("error")
                .and_then(|x| x.as_str())
                .unwrap_or("install refused");
            return Err(err.to_string());
        }
    }
    Ok(())
}
