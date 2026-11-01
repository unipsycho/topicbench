//! Probe Pro sidecar update status (localhost only — no clients-host URLs/creds).
//!
//! Free TopicBench shows Pro version lines in Settings → Updates only when the
//! sidecar answers. Uses the **same base URL + Bearer `mcp-token`** as `/sim`
//! (and other Pro HTTP clients). Check/install are delegated to Pro (`/update/*`).

use serde::Deserialize;
use std::fs;
use std::path::PathBuf;
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

/// Same base resolution as sim / launch (PRO_URL, else MCP_URL stripped of `/mcp`).
fn base_url() -> String {
    if let Ok(s) = std::env::var("TOPICBENCH_PRO_URL") {
        let s = s.trim().trim_end_matches('/').to_string();
        if !s.is_empty() {
            return s;
        }
    }
    if let Ok(u) = std::env::var("TOPICBENCH_MCP_URL") {
        let u = u.trim().trim_end_matches('/');
        if u.is_empty() {
            // fall through
        } else if let Some(rest) = u.strip_suffix("/mcp") {
            return rest.to_string();
        } else {
            return u.to_string();
        }
    }
    DEFAULT_BASE.to_string()
}

fn token_path() -> PathBuf {
    crate::mqtt::paths::config_file("mcp-token")
}

fn read_token() -> Option<String> {
    let t = fs::read_to_string(token_path()).ok()?.trim().to_string();
    if t.is_empty() {
        None
    } else {
        Some(t)
    }
}

/// GET/POST with the same Bearer header sim uses (required on Pro `/update/*`).
fn call(method: &str, path: &str) -> Option<ureq::Response> {
    let url = format!("{}{path}", base_url());
    let agent = ureq::AgentBuilder::new().timeout(TIMEOUT).build();
    let mut req = match method {
        "GET" => agent.get(&url),
        "POST" => agent.post(&url),
        _ => return None,
    };
    if let Some(tok) = read_token() {
        req = req.set("Authorization", &format!("Bearer {tok}"));
    }
    req.call().ok().filter(|r| (200..300).contains(&r.status()))
}

/// Returns `Some` only when the Pro sidecar answers on the live port.
pub fn probe() -> Option<ProUpdateInfo> {
    // Prefer /update/status (richer); fall back to /health.version.
    if let Some(info) = get_status() {
        return Some(info);
    }
    if let Some(info) = get_health() {
        return Some(info);
    }
    // Last resort: same Bearer path Simulation uses — proves sidecar is the live one.
    probe_via_sim()
}

fn get_status() -> Option<ProUpdateInfo> {
    let resp = call("GET", "/update/status")?;
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

fn get_health() -> Option<ProUpdateInfo> {
    let resp = call("GET", "/health")?;
    let j: HealthJson = resp.into_json().ok()?;
    info_from_health(j)
}

/// `/health` alone means the sidecar is up — even older Pro builds that omit
/// `version` / `update` (UI shows “Running (version unknown)”).
fn info_from_health(j: HealthJson) -> Option<ProUpdateInfo> {
    if !j.ok {
        return None;
    }
    if let Some(u) = j.update {
        return Some(ProUpdateInfo {
            current: u.current.or(j.version).unwrap_or_default(),
            checking: u.checking,
            installing: u.installing,
            latest: u.latest,
            update_available: u.update_available,
            error: u.error,
        });
    }
    Some(ProUpdateInfo {
        current: j.version.unwrap_or_default(),
        ..Default::default()
    })
}

/// Bearer GET `/sim` — same connection as Simulation when update routes are absent.
fn probe_via_sim() -> Option<ProUpdateInfo> {
    let _resp = call("GET", "/sim")?;
    Some(ProUpdateInfo::default())
}

/// Ask Pro to check its private update channel (fire-and-forget). No-op if down.
pub fn request_check() {
    let _ = call("POST", "/update/check");
}

/// Ask Pro to download/install the pending update. Returns Err if sidecar refuses/down.
pub fn request_install() -> Result<(), String> {
    let url = format!("{}/update/install", base_url());
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(2))
        .build();
    let mut req = agent.post(&url);
    if let Some(tok) = read_token() {
        req = req.set("Authorization", &format!("Bearer {tok}"));
    }
    let resp = req.call().map_err(|e| format!("Pro install: {e}"))?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn health_ok_without_version_still_counts_as_up() {
        let j: HealthJson = serde_json::from_str(
            r#"{"ok":true,"mcp":"http://127.0.0.1:7853/mcp","sim":"http://127.0.0.1:7853/sim","bridge":true}"#,
        )
        .unwrap();
        let info = info_from_health(j).expect("sidecar answering");
        assert!(info.current.is_empty());
        assert!(!info.update_available);
    }

    #[test]
    fn health_with_version_and_update_block() {
        let j: HealthJson = serde_json::from_str(
            r#"{"ok":true,"version":"1.2.6","update":{"current":"1.2.6","latest":"1.2.6","update_available":false}}"#,
        )
        .unwrap();
        let info = info_from_health(j).unwrap();
        assert_eq!(info.current, "1.2.6");
        assert_eq!(info.latest.as_deref(), Some("1.2.6"));
    }

    #[test]
    fn health_not_ok_is_down() {
        let j: HealthJson = serde_json::from_str(r#"{"ok":false}"#).unwrap();
        assert!(info_from_health(j).is_none());
    }
}
