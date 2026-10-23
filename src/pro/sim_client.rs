//! Thin HTTP client for the Pro sidecar simulation API (`http://127.0.0.1:7853/sim`).
//!
//! List/status polls run on a background thread — never block the egui UI thread.
//! Mutations (upsert/start/…) stay sync with short timeouts (user-initiated).

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};

const DEFAULT_BASE: &str = "http://127.0.0.1:7853";

/// Failures before the panel/status shows “sidecar down” (avoids flash on one blip).
const FAIL_STREAK_BANNER: u32 = 3;
/// Healthy poll cadence while sidecar is up.
const POLL_HEALTHY_MS: u64 = 750;
/// Cap for exponential backoff while down.
const BACKOFF_MAX_MS: u64 = 8_000;

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

struct ListSnapshot {
    rows: Vec<SimRow>,
    statuses: Vec<SimStatus>,
    playback_running: bool,
}

/// Cached list + last error for the Simulation panel.
#[derive(Default)]
pub struct SimClientState {
    pub rows: Vec<SimRow>,
    pub statuses: Vec<SimStatus>,
    /// Sticky error shown only after sustained failures (not every blip).
    pub last_error: Option<String>,
    pub sidecar_up: bool,
    pub selected: Option<String>,
    /// CSV tape playback active on the sidecar.
    pub playback_running: bool,
    token: Option<String>,
    base: String,
    next_due: Option<Instant>,
    backoff_ms: u64,
    fail_streak: u32,
    /// True once FAIL_STREAK_BANNER consecutive failures — drives banner + quiet status.
    show_down: bool,
    inflight: Option<Receiver<Result<ListSnapshot, String>>>,
}

impl SimClientState {
    pub fn new() -> Self {
        let base = std::env::var("TOPICBENCH_PRO_URL")
            .unwrap_or_else(|_| DEFAULT_BASE.to_string())
            .trim_end_matches('/')
            .to_string();
        Self {
            base,
            backoff_ms: POLL_HEALTHY_MS,
            ..Default::default()
        }
    }

    /// Count of running simulation rows (for the status strip).
    pub fn running_count(&self) -> usize {
        self.statuses.iter().filter(|s| s.running).count()
    }

    /// Compact status-strip label, or `None` when nothing is publishing.
    pub fn status_strip_label(&self) -> Option<String> {
        let n = self.running_count();
        let pb = self.playback_running;
        if n == 0 && !pb {
            return None;
        }
        Some(if n > 0 && pb {
            format!("Simulation: running — {n} publishing + playback")
        } else if n > 0 {
            format!("Simulation: running — {n} publishing")
        } else {
            "Simulation: playback".into()
        })
    }

    /// Error text for the panel banner (only after sustained failure).
    pub fn banner_error(&self) -> Option<&str> {
        if self.show_down {
            self.last_error.as_deref()
        } else {
            None
        }
    }

    /// Refresh list off the UI thread (never blocks the egui frame).
    pub fn poll_if_due(&mut self, force: bool) {
        self.drain_inflight();

        if self.inflight.is_some() {
            return;
        }
        let now = Instant::now();
        let due = force
            || self
                .next_due
                .map(|t| now >= t)
                .unwrap_or(true);
        if !due {
            return;
        }

        let base = self.base.clone();
        let token = match self.token_cached() {
            Ok(t) => t,
            Err(e) => {
                self.on_poll_err(e);
                return;
            }
        };
        let (tx, rx) = mpsc::channel();
        self.inflight = Some(rx);
        // Background GET /sim — short timeouts so a starved sidecar fails fast.
        thread::Builder::new()
            .name("topicbench-sim-poll".into())
            .spawn(move || {
                let r = http_json(
                    &base,
                    &token,
                    "GET",
                    "/sim",
                    None,
                    Duration::from_millis(250),
                    Duration::from_millis(600),
                )
                .and_then(parse_list);
                let _ = tx.send(r);
            })
            .ok();
    }

    fn drain_inflight(&mut self) {
        let Some(rx) = &self.inflight else {
            return;
        };
        match rx.try_recv() {
            Ok(Ok(snap)) => {
                self.rows = snap.rows;
                self.statuses = snap.statuses;
                self.playback_running = snap.playback_running;
                self.sidecar_up = true;
                self.fail_streak = 0;
                self.show_down = false;
                self.last_error = None;
                self.backoff_ms = POLL_HEALTHY_MS;
                self.next_due = Some(Instant::now() + Duration::from_millis(POLL_HEALTHY_MS));
                self.inflight = None;
            }
            Ok(Err(e)) => {
                self.inflight = None;
                self.on_poll_err(e);
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                self.inflight = None;
                self.on_poll_err("sidecar poll thread ended".into());
            }
        }
    }

    fn on_poll_err(&mut self, e: String) {
        self.sidecar_up = false;
        self.fail_streak = self.fail_streak.saturating_add(1);
        // Quiet sticky message — no cargo-run spam.
        let quiet = if e.contains("timed out") || e.contains("TimedOut") {
            "sidecar unreachable (timed out) — start topicbench-pro".to_string()
        } else if e.contains("Connection refused") || e.contains("connection refused") {
            "sidecar unreachable — start topicbench-pro".to_string()
        } else {
            format!("sidecar unreachable — start topicbench-pro ({e})")
        };
        self.last_error = Some(quiet);
        if self.fail_streak >= FAIL_STREAK_BANNER {
            self.show_down = true;
        }
        // Exponential backoff while down so we don't hammer a dead process.
        self.backoff_ms = (self.backoff_ms.saturating_mul(2)).clamp(POLL_HEALTHY_MS, BACKOFF_MAX_MS);
        self.next_due = Some(Instant::now() + Duration::from_millis(self.backoff_ms));
    }

    pub fn status_for(&self, id: &str) -> Option<&SimStatus> {
        self.statuses.iter().find(|s| s.id == id)
    }

    pub fn upsert(&mut self, row: &SimRow) -> Result<SimRow, String> {
        let body = serde_json::to_vec(row).map_err(|e| e.to_string())?;
        let v = self.http_mut("PUT", "/sim", Some(body))?;
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
        let _ = self.http_mut("DELETE", &format!("/sim/{id}"), None)?;
        if self.selected.as_deref() == Some(id) {
            self.selected = None;
        }
        self.poll_if_due(true);
        Ok(())
    }

    pub fn start(&mut self, id: &str) -> Result<(), String> {
        let _ = self.http_mut("POST", &format!("/sim/{id}/start"), None)?;
        self.poll_if_due(true);
        Ok(())
    }

    pub fn start_all(&mut self) -> Result<(), String> {
        let _ = self.http_mut("POST", "/sim/start_all", None)?;
        self.poll_if_due(true);
        Ok(())
    }

    pub fn stop(&mut self, id: &str) -> Result<(), String> {
        let _ = self.http_mut("POST", &format!("/sim/{id}/stop"), None)?;
        self.poll_if_due(true);
        Ok(())
    }

    pub fn stop_all(&mut self) -> Result<(), String> {
        // Also cancels CSV playback on the sidecar.
        let _ = self.http_mut("POST", "/sim/stop_all", None)?;
        self.poll_if_due(true);
        Ok(())
    }

    /// Import sims. `mode` is `"replace"` (full list) or `"merge"` (by profile+topic+field).
    /// `json` may be a bare array or `{simulations:[…]}` (Export shape).
    pub fn import_sims(&mut self, json: &Value, mode: &str) -> Result<usize, String> {
        let mode = if mode.eq_ignore_ascii_case("merge") {
            "merge"
        } else {
            "replace"
        };
        let body_val = if let Some(obj) = json.as_object() {
            let mut o = obj.clone();
            o.insert("mode".into(), Value::String(mode.into()));
            if !o.contains_key("simulations") {
                return Err("expected {simulations:[…]} or a JSON array".into());
            }
            Value::Object(o)
        } else if json.is_array() {
            serde_json::json!({ "mode": mode, "simulations": json })
        } else {
            return Err("expected array or {simulations:[…]}".into());
        };
        let body = serde_json::to_vec(&body_val).map_err(|e| e.to_string())?;
        let v = self.http_mut("POST", "/sim/import", Some(body))?;
        self.poll_if_due(true);
        Ok(v.get("simulations")
            .and_then(|a| a.as_array())
            .map(|a| a.len())
            .unwrap_or(self.rows.len()))
    }

    /// Current defs as `{simulations:[…]}`.
    pub fn export_sims(&mut self) -> Result<Value, String> {
        self.http_mut("GET", "/sim/export", None)
    }

    /// Start CSV tape playback (all topics). Empty profile → CSV hint / server default.
    pub fn start_playback(&mut self, csv: &str, profile: &str) -> Result<(), String> {
        let body = serde_json::to_vec(&serde_json::json!({
            "csv": csv,
            "profile": profile,
        }))
        .map_err(|e| e.to_string())?;
        let v = self.http_mut("POST", "/sim/playback", Some(body))?;
        if v.get("ok").and_then(|o| o.as_bool()).unwrap_or(false)
            || v.get("running").and_then(|o| o.as_bool()).unwrap_or(false)
        {
            self.playback_running = true;
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
        let _ = self.http_mut("POST", "/sim/playback/stop", None)?;
        self.playback_running = false;
        Ok(())
    }

    /// Syntax-check a formula via the sidecar (topic/sim stubs resolve to 0).
    /// Short timeout — never stall typing for long when sidecar is busy.
    pub fn validate_formula(&mut self, formula: &str) -> Result<(), String> {
        let body = serde_json::to_vec(&serde_json::json!({ "formula": formula }))
            .map_err(|e| e.to_string())?;
        let token = self.token_cached()?;
        let v = http_json(
            &self.base,
            &token,
            "POST",
            "/sim/validate_formula",
            Some(body),
            Duration::from_millis(250),
            Duration::from_millis(400),
        )?;
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

    fn token_cached(&mut self) -> Result<String, String> {
        if let Some(t) = &self.token {
            if !t.is_empty() {
                return Ok(t.clone());
            }
        }
        let path = token_path();
        let t = fs::read_to_string(&path)
            .map_err(|e| {
                format!(
                    "cannot read {} ({e}) — is topicbench-pro running?",
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

    /// User-initiated mutation — sync but with a tight read budget.
    fn http_mut(
        &mut self,
        method: &str,
        path: &str,
        body: Option<Vec<u8>>,
    ) -> Result<Value, String> {
        let token = self.token_cached()?;
        let base = self.base.clone();
        match http_json(
            &base,
            &token,
            method,
            path,
            body,
            Duration::from_millis(300),
            Duration::from_millis(1200),
        ) {
            Ok(v) => {
                // A successful mutation means the sidecar answered — clear sticky down.
                self.fail_streak = 0;
                self.show_down = false;
                self.sidecar_up = true;
                self.last_error = None;
                self.backoff_ms = POLL_HEALTHY_MS;
                Ok(v)
            }
            Err(e) => {
                self.on_poll_err(e.clone());
                Err(e)
            }
        }
    }
}

fn parse_list(v: Value) -> Result<ListSnapshot, String> {
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
    let playback_running = v
        .get("playback")
        .and_then(|p| p.get("running"))
        .and_then(|r| r.as_bool())
        .unwrap_or(false);
    Ok(ListSnapshot {
        rows,
        statuses,
        playback_running,
    })
}

fn http_json(
    base: &str,
    token: &str,
    method: &str,
    path: &str,
    body: Option<Vec<u8>>,
    connect: Duration,
    read: Duration,
) -> Result<Value, String> {
    let url = format!("{base}{path}");
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(connect)
        .timeout_read(read)
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
        Err(ureq::Error::Transport(t)) => Err(format!("{t}")),
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
