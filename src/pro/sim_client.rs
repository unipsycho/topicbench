//! Thin HTTP client for the Pro sidecar simulation API (`http://127.0.0.1:7853/sim`).
//!
//! No formula engine here — CRUD/start/stop only. Bearer token from `mcp-token`.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant};

const DEFAULT_BASE: &str = "http://127.0.0.1:7853";

/// One sim row as returned by the sidecar (subset mirrored for the panel).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SimRow {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub profile: String,
    #[serde(default)]
    pub topic: String,
    #[serde(default = "default_interval")]
    pub interval_ms: u64,
    #[serde(default)]
    pub qos: u8,
    #[serde(default)]
    pub retain: bool,
    #[serde(default = "default_mode")]
    pub mode: String,
    #[serde(default)]
    pub template: String,
    #[serde(default)]
    pub field_path: String,
    #[serde(default)]
    pub generator: Value,
    /// Decimal digits when publishing numbers (default 3).
    #[serde(default = "default_precision")]
    pub precision: u8,
    #[serde(default)]
    pub auto_start: bool,
}

fn default_interval() -> u64 {
    1000
}
fn default_mode() -> String {
    "whole_payload".into()
}
fn default_precision() -> u8 {
    3
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SimStatus {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub running: bool,
    #[serde(default)]
    pub tick: u64,
    pub last_value: Option<Value>,
    pub last_payload: Option<String>,
    pub last_error: Option<String>,
}

/// Cached list + last error for the Simulation panel.
#[derive(Default)]
pub struct SimClientState {
    pub rows: Vec<SimRow>,
    pub statuses: Vec<SimStatus>,
    pub last_error: Option<String>,
    pub sidecar_up: bool,
    pub selected: Option<String>,
    last_fetch: Option<Instant>,
    token: Option<String>,
    base: String,
}

impl SimClientState {
    pub fn new() -> Self {
        let base = std::env::var("TOPICBENCH_PRO_URL")
            .unwrap_or_else(|_| DEFAULT_BASE.to_string())
            .trim_end_matches('/')
            .to_string();
        Self {
            base,
            ..Default::default()
        }
    }

    /// Refresh list when panel is open (throttled).
    pub fn poll_if_due(&mut self, force: bool) {
        let due = force
            || self
                .last_fetch
                .map(|t| t.elapsed() >= Duration::from_millis(500))
                .unwrap_or(true);
        if !due {
            return;
        }
        self.last_fetch = Some(Instant::now());
        match self.fetch_list() {
            Ok((rows, statuses)) => {
                self.rows = rows;
                self.statuses = statuses;
                self.sidecar_up = true;
                self.last_error = None;
            }
            Err(e) => {
                self.sidecar_up = false;
                self.last_error = Some(e);
            }
        }
    }

    pub fn status_for(&self, id: &str) -> Option<&SimStatus> {
        self.statuses.iter().find(|s| s.id == id)
    }

    pub fn upsert(&mut self, row: &SimRow) -> Result<SimRow, String> {
        let body = serde_json::to_vec(row).map_err(|e| e.to_string())?;
        let v = self.http("PUT", "/sim", Some(body))?;
        let sim = v
            .get("simulation")
            .cloned()
            .ok_or_else(|| "missing simulation in response".to_string())?;
        let row: SimRow = serde_json::from_value(sim).map_err(|e| e.to_string())?;
        // Apply response locally first so the next paint keeps the new mode
        // even if a concurrent list poll would have been stale mid-edit.
        if let Some(slot) = self.rows.iter_mut().find(|r| r.id == row.id) {
            *slot = row.clone();
        } else {
            self.rows.push(row.clone());
        }
        self.poll_if_due(true);
        Ok(row)
    }

    pub fn delete(&mut self, id: &str) -> Result<(), String> {
        let _ = self.http("DELETE", &format!("/sim/{id}"), None)?;
        if self.selected.as_deref() == Some(id) {
            self.selected = None;
        }
        self.poll_if_due(true);
        Ok(())
    }

    pub fn start(&mut self, id: &str) -> Result<(), String> {
        let _ = self.http("POST", &format!("/sim/{id}/start"), None)?;
        self.poll_if_due(true);
        Ok(())
    }

    pub fn start_all(&mut self) -> Result<(), String> {
        let _ = self.http("POST", "/sim/start_all", None)?;
        self.poll_if_due(true);
        Ok(())
    }

    pub fn stop(&mut self, id: &str) -> Result<(), String> {
        let _ = self.http("POST", &format!("/sim/{id}/stop"), None)?;
        self.poll_if_due(true);
        Ok(())
    }

    pub fn stop_all(&mut self) -> Result<(), String> {
        // Also cancels CSV playback on the sidecar.
        let _ = self.http("POST", "/sim/stop_all", None)?;
        self.poll_if_due(true);
        Ok(())
    }

    /// Replace all sims. Accepts bare `[…]` or `{simulations|sims:[…]}`.
    pub fn import_sims(&mut self, json: &Value) -> Result<usize, String> {
        self.import_sims_mode(json, "replace")
    }

    /// Import with `replace` or `merge`. Accepts bare or wrapped sims JSON.
    pub fn import_sims_mode(&mut self, json: &Value, mode: &str) -> Result<usize, String> {
        // Normalize to bare array for the wire (sidecar also accepts wrapped).
        let sims = parse_simulations_array(json)?;
        let mode = if mode.eq_ignore_ascii_case("merge") {
            "merge"
        } else {
            "replace"
        };
        let body = serde_json::to_vec(sims).map_err(|e| e.to_string())?;
        let v = self.http("POST", &format!("/sim/import?mode={mode}"), Some(body))?;
        if let Some(err) = v.get("error").and_then(|e| e.as_str()) {
            return Err(err.to_string());
        }
        self.poll_if_due(true);
        let n = v
            .get("simulations")
            .and_then(|a| a.as_array())
            .map(|a| a.len())
            .ok_or_else(|| {
                "import response missing simulations array — is TopicBench Pro up to date?"
                    .to_string()
            })?;
        Ok(n)
    }

    /// Current defs as a bare JSON array (Export file shape).
    pub fn export_sims(&mut self) -> Result<Value, String> {
        let v = self.http("GET", "/sim/export", None)?;
        // Sidecar returns a bare array; tolerate a wrapper if present.
        match parse_simulations_array(&v) {
            Ok(arr) => Ok(Value::Array(arr.to_vec())),
            Err(_) => Err("export response was not a JSON array".into()),
        }
    }

    /// Start CSV tape playback (all topics). Empty profile → CSV hint / server default.
    pub fn start_playback(&mut self, csv: &str, profile: &str) -> Result<(), String> {
        let body = serde_json::to_vec(&serde_json::json!({
            "csv": csv,
            "profile": profile,
        }))
        .map_err(|e| e.to_string())?;
        let v = self.http("POST", "/sim/playback", Some(body))?;
        if v.get("ok").and_then(|o| o.as_bool()).unwrap_or(false)
            || v.get("running").and_then(|o| o.as_bool()).unwrap_or(false)
        {
            Ok(())
        } else {
            Err(v
                .get("error")
                .and_then(|e| e.as_str())
                .unwrap_or("playback failed")
                .to_string())
        }
    }

    pub fn stop_playback(&mut self) -> Result<(), String> {
        let _ = self.http("POST", "/sim/playback/stop", None)?;
        Ok(())
    }

    /// Syntax-check a formula via the sidecar (topic/sim stubs resolve to 0).
    pub fn validate_formula(&mut self, formula: &str) -> Result<(), String> {
        let body = serde_json::to_vec(&serde_json::json!({ "formula": formula }))
            .map_err(|e| e.to_string())?;
        let v = self.http("POST", "/sim/validate_formula", Some(body))?;
        if v.get("ok").and_then(|o| o.as_bool()).unwrap_or(false) {
            Ok(())
        } else {
            Err(v
                .get("error")
                .and_then(|e| e.as_str())
                .unwrap_or("invalid formula")
                .to_string())
        }
    }

    fn token(&mut self) -> Result<String, String> {
        if let Some(t) = &self.token {
            if !t.is_empty() {
                return Ok(t.clone());
            }
        }
        let path = token_path();
        let t = fs::read_to_string(&path)
            .map_err(|e| {
                format!(
                    "cannot read {} ({e}) — is TopicBench Pro running?",
                    path.display()
                )
            })?
            .trim()
            .to_string();
        if t.is_empty() {
            return Err("mcp-token file is empty".into());
        }
        self.token = Some(t.clone());
        Ok(t)
    }

    fn fetch_list(&mut self) -> Result<(Vec<SimRow>, Vec<SimStatus>), String> {
        let v = self.http("GET", "/sim", None)?;
        let rows: Vec<SimRow> = serde_json::from_value(
            v.get("simulations")
                .cloned()
                .unwrap_or(Value::Array(vec![])),
        )
        .map_err(|e| format!("simulations: {e}"))?;
        let statuses: Vec<SimStatus> = serde_json::from_value(
            v.get("statuses").cloned().unwrap_or(Value::Array(vec![])),
        )
        .map_err(|e| format!("statuses: {e}"))?;
        Ok((rows, statuses))
    }

    fn http(&mut self, method: &str, path: &str, body: Option<Vec<u8>>) -> Result<Value, String> {
        let token = self.token()?;
        let url = format!("{}{path}", self.base);
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_millis(400))
            .timeout_read(Duration::from_secs(3))
            .build();
        let req = match method {
            "GET" => agent.get(&url),
            "PUT" => agent.put(&url),
            "POST" => agent.post(&url),
            "DELETE" => agent.request("DELETE", &url),
            other => return Err(format!("unsupported method {other}")),
        }
        .set("Authorization", &format!("Bearer {token}"))
        .set("Content-Type", "application/json");

        let resp = match body {
            Some(b) => req.send_bytes(&b),
            None => {
                if method == "GET" || method == "DELETE" {
                    req.call()
                } else {
                    req.send_bytes(b"{}")
                }
            }
        };
        match resp {
            Ok(r) => {
                let text = r.into_string().map_err(|e| format!("read body: {e}"))?;
                if text.trim().is_empty() {
                    return Ok(Value::Null);
                }
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
                "TopicBench Pro unreachable ({t}) — is it running?"
            )),
        }
    }
}

/// Import/load shape: bare `[…]` or wrapped `{ "simulations"|"sims": […] }`.
pub fn parse_simulations_array(json: &Value) -> Result<&[Value], String> {
    if let Some(arr) = json.as_array() {
        return Ok(arr.as_slice());
    }
    if let Some(obj) = json.as_object() {
        for key in ["simulations", "sims"] {
            if let Some(arr) = obj.get(key).and_then(|x| x.as_array()) {
                return Ok(arr.as_slice());
            }
        }
        return Err(
            "import JSON must be a bare array [ … ] or { \"simulations\": [ … ] }"
                .into(),
        );
    }
    Err(format!(
        "import JSON must be a bare array [ … ] or {{ \"simulations\": [ … ] }} — found {}",
        match json {
            Value::Null => "null",
            Value::Bool(_) => "bool",
            Value::Number(_) => "number",
            Value::String(_) => "string",
            Value::Array(_) => "array",
            Value::Object(_) => "object",
        }
    ))
}

#[cfg(test)]
mod parse_simulations_tests {
    use super::parse_simulations_array;
    use serde_json::json;

    #[test]
    fn accepts_bare_and_wrapped() {
        assert_eq!(
            parse_simulations_array(&json!([{ "id": "a" }]))
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            parse_simulations_array(&json!({ "simulations": [{ "id": "a" }, { "id": "b" }] }))
                .unwrap()
                .len(),
            2
        );
        assert_eq!(
            parse_simulations_array(&json!({ "sims": [{ "id": "a" }] }))
                .unwrap()
                .len(),
            1
        );
        assert!(parse_simulations_array(&json!({ "other": [] })).is_err());
    }
}

fn token_path() -> PathBuf {
    crate::mqtt::paths::config_file("mcp-token")
}

/// Blank row ready for Add (caller fills profile/topic).
pub fn blank_row(profile: &str, topic: &str) -> SimRow {
    SimRow {
        id: String::new(),
        name: if topic.is_empty() {
            "New simulation".into()
        } else {
            topic.to_string()
        },
        profile: profile.to_string(),
        topic: topic.to_string(),
        interval_ms: 1000,
        qos: 0,
        retain: false,
        mode: "whole_payload".into(),
        template: String::new(),
        field_path: String::new(),
        generator: serde_json::json!({"type": "sine", "min": 0.0, "max": 100.0, "period_s": 10.0}),
        precision: 3,
        auto_start: false,
    }
}

/// Wire/schema generator type id (stable for persistence / match arms).
pub fn generator_label(gen: &Value) -> String {
    match gen.get("type").and_then(|t| t.as_str()).unwrap_or("?") {
        "expr" | "formula" => "formula".into(),
        "enum_list" => "enum_cycle".into(),
        other => other.to_string(),
    }
}

/// Human Title Case label for the generator combo (display only).
pub fn generator_display_name(type_id: &str) -> &'static str {
    match type_id {
        "constant" => "Constant",
        "counter" => "Counter",
        "sine" => "Sine",
        "triangle" => "Triangle",
        "ramp" => "Ramp",
        "uniform" => "Random",
        "gaussian" => "Gaussian",
        "random_walk" => "Random Walk",
        "bool_toggle" => "Bool Toggle",
        "iso_timestamp" => "ISO Timestamp",
        "enum_cycle" | "enum_list" => "List",
        "formula" | "expr" => "Formula",
        _ => "Unknown",
    }
}

/// Human-readable current value cell.
pub fn value_label(st: Option<&SimStatus>) -> String {
    let Some(st) = st else {
        return "—".into();
    };
    if let Some(err) = &st.last_error {
        return format!("err: {err}");
    }
    if let Some(v) = &st.last_value {
        return match v {
            Value::String(s) => s.clone(),
            other => other.to_string(),
        };
    }
    "—".into()
}
