//! Thin HTTP client for Pro sidecar topic-tree Import (`/tree/preview`, `/tree/import`).
//!
//! Expand / remap / republish live in `topicbench-pro`; OSS only posts JSON + mappings.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

const DEFAULT_BASE: &str = "http://127.0.0.1:7853";
const RECENT_CAP: usize = 10;
const RECENT_FILE: &str = "topic-tree-import-recent.json";

/// One mapping row from `/tree/preview`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PreviewRow {
    pub broker_index: usize,
    pub source_broker_name: String,
    pub segment: String,
}

/// Row state edited in the Import modal.
#[derive(Clone, Debug)]
pub struct MapRow {
    pub broker_index: usize,
    pub source_broker_name: String,
    pub segment: String,
    pub include: bool,
    pub under_path: String,
}

impl From<PreviewRow> for MapRow {
    fn from(r: PreviewRow) -> Self {
        Self {
            broker_index: r.broker_index,
            source_broker_name: r.source_broker_name,
            segment: r.segment,
            include: true,
            under_path: String::new(),
        }
    }
}

/// Last imported file paths (MRU, capped).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ImportRecent {
    #[serde(default)]
    pub paths: Vec<String>,
}

impl ImportRecent {
    pub fn path() -> PathBuf {
        crate::mqtt::paths::config_dir()
            .join("pro")
            .join(RECENT_FILE)
    }

    pub fn load() -> Self {
        let path = Self::path();
        match fs::read_to_string(&path) {
            Ok(text) => {
                let mut h: Self = serde_json::from_str(&text).unwrap_or_default();
                h.paths.truncate(RECENT_CAP);
                h
            }
            Err(_) => Self::default(),
        }
    }

    pub fn push_and_save(&mut self, file: &Path) {
        let s = file.display().to_string();
        self.paths.retain(|p| p != &s);
        self.paths.insert(0, s);
        self.paths.truncate(RECENT_CAP);
        let path = Self::path();
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Ok(text) = serde_json::to_string_pretty(self) {
            let _ = fs::write(&path, text);
        }
    }
}

fn base_url() -> String {
    std::env::var("TOPICBENCH_MCP_URL")
        .ok()
        .and_then(|u| {
            // Accept full mcp URL or host:port base.
            let u = u.trim_end_matches('/').to_string();
            if let Some(rest) = u.strip_suffix("/mcp") {
                Some(rest.to_string())
            } else {
                Some(u)
            }
        })
        .unwrap_or_else(|| DEFAULT_BASE.to_string())
}

fn token_path() -> PathBuf {
    crate::mqtt::paths::config_file("mcp-token")
}

fn read_token() -> Result<String, String> {
    let text = fs::read_to_string(token_path()).map_err(|_| {
        "Pro token missing — is TopicBench Pro running?".to_string()
    })?;
    let t = text.trim().to_string();
    if t.is_empty() {
        return Err("mcp-token file is empty".into());
    }
    Ok(t)
}

fn http_post(path: &str, body: &Value) -> Result<Value, String> {
    let tok = read_token()?;
    let url = format!("{}{path}", base_url());
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_millis(400))
        // Import can publish many retained topics — allow longer than sim CRUD.
        .timeout_read(Duration::from_secs(120))
        .build();
    let bytes = serde_json::to_vec(body).map_err(|e| e.to_string())?;
    let resp = agent
        .post(&url)
        .set("Authorization", &format!("Bearer {tok}"))
        .set("Content-Type", "application/json")
        .send_bytes(&bytes);
    match resp {
        Ok(r) => {
            let text = r.into_string().map_err(|e| format!("read body: {e}"))?;
            serde_json::from_str(&text).map_err(|e| format!("json: {e}"))
        }
        Err(ureq::Error::Status(code, resp)) => {
            let text = resp.into_string().unwrap_or_default();
            let msg = serde_json::from_str::<Value>(&text)
                .ok()
                .and_then(|v| {
                    v.get("error")
                        .and_then(|e| e.as_str())
                        .map(|s| s.to_string())
                })
                .unwrap_or(text);
            Err(format!("HTTP {code}: {msg}"))
        }
        Err(ureq::Error::Transport(t)) => Err(format!(
            "TopicBench Pro unreachable ({t}) — start Pro to import"
        )),
    }
}

/// Ask sidecar for top-level mapping rows.
pub fn preview(doc: &Value) -> Result<Vec<PreviewRow>, String> {
    let v = http_post("/tree/preview", doc)?;
    if let Some(err) = v.get("error").and_then(|e| e.as_str()) {
        return Err(err.to_string());
    }
    let rows = v
        .get("rows")
        .cloned()
        .ok_or_else(|| "preview response missing rows".to_string())?;
    serde_json::from_value(rows).map_err(|e| format!("preview rows: {e}"))
}

/// Republish via sidecar (bridge → live sessions).
pub fn import_mapped(
    doc: &Value,
    target_profile_id: &str,
    rows: &[MapRow],
) -> Result<usize, String> {
    let mappings: Vec<Value> = rows
        .iter()
        .map(|r| {
            json!({
                "broker_index": r.broker_index,
                "segment": r.segment,
                "include": r.include,
                "under_path": r.under_path,
            })
        })
        .collect();
    let body = json!({
        "doc": doc,
        "target_profile_id": target_profile_id,
        "mappings": mappings,
    });
    let v = http_post("/tree/import", &body)?;
    if let Some(err) = v.get("error").and_then(|e| e.as_str()) {
        return Err(err.to_string());
    }
    v.get("published")
        .and_then(|n| n.as_u64())
        .map(|n| n as usize)
        .ok_or_else(|| "import response missing published".into())
}
