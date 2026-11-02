//! Thin HTTP client for the Pro sidecar scenario API (`/scenario`).
//!
//! No engine here — CRUD/start/pause/resume/stop/step only. Bearer from `mcp-token`.
//! UI thread never calls ureq (same invariant as [`super::sim_client`]).

use eframe::egui::Context;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use std::fs;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

/// Accept string or null (older step overrides used `Option`).
fn deser_profile<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    Ok(Option::<String>::deserialize(d)?.unwrap_or_default())
}

const DEFAULT_BASE: &str = "http://127.0.0.1:7853";
const POLL_INTERVAL: Duration = Duration::from_millis(500);
const BACKOFF_START: Duration = Duration::from_secs(2);
const BACKOFF_MAX: Duration = Duration::from_secs(30);
const CMD_CAP: usize = 8;

/// Wait-value compare op (wire).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WaitOp {
    #[default]
    Eq,
    Neq,
    Gt,
    Gte,
    Lt,
    Lte,
    Contains,
    NotContains,
}

/// One step (serde `kind` tag — matches Pro schema).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ScenarioStep {
    Publish {
        #[serde(default)]
        id: String,
        #[serde(default)]
        name: String,
        #[serde(default = "default_true")]
        enabled: bool,
        /// Broker profile for this publish (per-step; cross-broker scenarios).
        #[serde(default, deserialize_with = "deser_profile")]
        profile: String,
        #[serde(default)]
        topic: String,
        #[serde(default)]
        payload: String,
        #[serde(default)]
        qos: u8,
        #[serde(default)]
        retain: bool,
    },
    WaitTime {
        #[serde(default)]
        id: String,
        #[serde(default)]
        name: String,
        #[serde(default = "default_true")]
        enabled: bool,
        #[serde(default)]
        duration_ms: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        max_ms: Option<u64>,
    },
    WaitValue {
        #[serde(default)]
        id: String,
        #[serde(default)]
        name: String,
        #[serde(default = "default_true")]
        enabled: bool,
        /// Broker profile to watch (per-step; cross-broker scenarios).
        #[serde(default, deserialize_with = "deser_profile")]
        profile: String,
        #[serde(default)]
        topic: String,
        #[serde(default)]
        field: String,
        #[serde(default)]
        op: WaitOp,
        #[serde(default)]
        value: String,
        #[serde(default = "default_true")]
        match_retained: bool,
    },
}

fn default_true() -> bool {
    true
}

impl ScenarioStep {
    pub fn id(&self) -> &str {
        match self {
            Self::Publish { id, .. } | Self::WaitTime { id, .. } | Self::WaitValue { id, .. } => id,
        }
    }

    /// Wire/schema kind id (`wait_time`, not UI copy).
    pub fn kind_label(&self) -> &'static str {
        match self {
            Self::Publish { .. } => "publish",
            Self::WaitTime { .. } => "wait_time",
            Self::WaitValue { .. } => "wait_value",
        }
    }

    /// Human label for Scenarios UI rows / menus (`wait time`).
    pub fn kind_ui_label(&self) -> &'static str {
        match self {
            Self::Publish { .. } => "publish",
            Self::WaitTime { .. } => "wait time",
            Self::WaitValue { .. } => "wait value",
        }
    }

    pub fn enabled(&self) -> bool {
        match self {
            Self::Publish { enabled, .. }
            | Self::WaitTime { enabled, .. }
            | Self::WaitValue { enabled, .. } => *enabled,
        }
    }

    pub fn set_enabled(&mut self, on: bool) {
        match self {
            Self::Publish { enabled, .. }
            | Self::WaitTime { enabled, .. }
            | Self::WaitValue { enabled, .. } => *enabled = on,
        }
    }

    pub fn name(&self) -> &str {
        match self {
            Self::Publish { name, .. }
            | Self::WaitTime { name, .. }
            | Self::WaitValue { name, .. } => name,
        }
    }

    pub fn name_mut(&mut self) -> &mut String {
        match self {
            Self::Publish { name, .. }
            | Self::WaitTime { name, .. }
            | Self::WaitValue { name, .. } => name,
        }
    }

    /// One-line muted summary of key settings for the left step list.
    pub fn settings_summary(&self) -> String {
        match self {
            Self::Publish {
                topic, profile, ..
            } => {
                let topic = topic.trim();
                let profile = profile.trim();
                match (topic.is_empty(), profile.is_empty()) {
                    (true, true) => String::new(),
                    (false, true) => topic.to_string(),
                    (true, false) => format!("@{profile}"),
                    (false, false) => format!("{topic} @{profile}"),
                }
            }
            Self::WaitTime {
                duration_ms, max_ms, ..
            } => match max_ms {
                Some(m) if *m > *duration_ms => format!("{duration_ms}–{m} ms"),
                _ => format!("{duration_ms} ms"),
            },
            Self::WaitValue {
                topic,
                field,
                op,
                value,
                ..
            } => {
                let topic = topic.trim();
                let field = field.trim();
                let value = value.trim();
                let left = if topic.is_empty() && field.is_empty() {
                    String::new()
                } else if field.is_empty() {
                    topic.to_string()
                } else if topic.is_empty() {
                    field.to_string()
                } else {
                    format!("{topic}.{field}")
                };
                let op_s = match op {
                    WaitOp::Eq => "=",
                    WaitOp::Neq => "≠",
                    WaitOp::Gt => ">",
                    WaitOp::Gte => "≥",
                    WaitOp::Lt => "<",
                    WaitOp::Lte => "≤",
                    WaitOp::Contains => "contains",
                    WaitOp::NotContains => "!contains",
                };
                match (left.is_empty(), value.is_empty()) {
                    (true, true) => String::new(),
                    (false, true) => format!("{left} {op_s}"),
                    (true, false) => format!("{op_s} {value}"),
                    (false, false) => format!("{left} {op_s} {value}"),
                }
            }
        }
    }
}

/// Named folder for organizing scenarios (empty `group_id` on a row = Ungrouped).
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ScenarioGroup {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
}

/// DnD / reorder placement for one scenario.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScenarioPlacement {
    pub id: String,
    #[serde(default)]
    pub group_id: String,
    #[serde(default)]
    pub sort: i64,
}

/// One scenario definition from the sidecar.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ScenarioRow {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    /// Legacy scenario-level profile (Pro migrates onto steps; not sent on upsert).
    #[serde(default, skip_serializing)]
    pub profile: String,
    /// Owning group id; empty = Ungrouped.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub group_id: String,
    /// Order within the group (and among ungrouped); lower first.
    #[serde(default)]
    pub sort: i64,
    #[serde(default, rename = "loop")]
    pub loop_: bool,
    #[serde(default)]
    pub steps: Vec<ScenarioStep>,
    #[serde(default)]
    pub auto_start: bool,
}

/// Runner status snapshot.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ScenarioStatus {
    #[serde(default)]
    pub id: String,
    /// `stopped` | `running` | `paused`
    #[serde(default)]
    pub state: String,
    pub step_index: Option<usize>,
    pub step_id: Option<String>,
    #[serde(default)]
    pub loop_n: u64,
    pub remaining_ms: Option<u64>,
    pub last_error: Option<String>,
}

impl ScenarioStatus {
    pub fn is_running(&self) -> bool {
        self.state == "running"
    }
    pub fn is_paused(&self) -> bool {
        self.state == "paused"
    }
    pub fn is_active(&self) -> bool {
        self.is_running() || self.is_paused()
    }
}

#[derive(Clone, Default)]
struct SharedSnap {
    rows: Vec<ScenarioRow>,
    groups: Vec<ScenarioGroup>,
    statuses: Vec<ScenarioStatus>,
    sidecar_up: bool,
    last_error: Option<String>,
    generation: u64,
    next_probe_at: Option<Instant>,
    backoff: Duration,
    egui_ctx: Option<Context>,
}

enum ScenarioCmd {
    Refresh {
        force: bool,
    },
    Http {
        method: String,
        path: String,
        body: Option<Vec<u8>>,
        reply: SyncSender<Result<Value, String>>,
    },
}

/// Cached list + selection for the Scenarios panel (UI-facing).
pub struct ScenarioClientState {
    pub rows: Vec<ScenarioRow>,
    pub groups: Vec<ScenarioGroup>,
    pub statuses: Vec<ScenarioStatus>,
    pub last_error: Option<String>,
    pub sidecar_up: bool,
    /// Selected scenario id (left list); mutually exclusive with [`Self::selected_group_id`].
    pub selected_id: Option<String>,
    /// Selected group id; `Some("")` = Ungrouped, `Some(id)` = named group.
    pub selected_group_id: Option<String>,
    /// Selected step index within the selected scenario (right editor).
    pub selected_step: Option<usize>,
    pub toolbar_arm_at: Option<Instant>,
    last_fetch: Option<Instant>,
    last_gen: u64,
    cmd_tx: SyncSender<ScenarioCmd>,
    shared: Arc<Mutex<SharedSnap>>,
}

impl Default for ScenarioClientState {
    fn default() -> Self {
        Self::new()
    }
}

impl ScenarioClientState {
    pub fn new() -> Self {
        let base = std::env::var("TOPICBENCH_PRO_URL")
            .unwrap_or_else(|_| DEFAULT_BASE.to_string())
            .trim_end_matches('/')
            .to_string();
        let shared = Arc::new(Mutex::new(SharedSnap {
            backoff: BACKOFF_START,
            ..Default::default()
        }));
        let (cmd_tx, cmd_rx) = mpsc::sync_channel(CMD_CAP);
        let shared_t = Arc::clone(&shared);
        let _ = thread::Builder::new()
            .name("tb-scenario-poll".into())
            .spawn(move || worker_loop(cmd_rx, shared_t, base));
        Self {
            rows: Vec::new(),
            groups: Vec::new(),
            statuses: Vec::new(),
            last_error: None,
            sidecar_up: false,
            selected_id: None,
            selected_group_id: None,
            selected_step: None,
            toolbar_arm_at: None,
            last_fetch: None,
            last_gen: 0,
            cmd_tx,
            shared,
        }
    }

    pub fn bind_egui_ctx(&self, ctx: &Context) {
        if let Ok(mut g) = self.shared.lock() {
            g.egui_ctx = Some(ctx.clone());
        }
    }

    pub fn running_count(&self) -> usize {
        self.statuses.iter().filter(|s| s.is_running()).count()
    }

    pub fn status_strip_label(&self) -> Option<String> {
        let n = self.running_count();
        let paused = self.statuses.iter().filter(|s| s.is_paused()).count();
        if n == 0 && paused == 0 {
            return None;
        }
        Some(if n > 0 && paused > 0 {
            format!("Scenarios: {n} running, {paused} paused")
        } else if n > 0 {
            format!("Scenarios: {n} running")
        } else {
            format!("Scenarios: {paused} paused")
        })
    }

    pub fn status_for(&self, id: &str) -> Option<&ScenarioStatus> {
        self.statuses.iter().find(|s| s.id == id)
    }

    pub fn selected_row(&self) -> Option<&ScenarioRow> {
        let id = self.selected_id.as_deref()?;
        self.rows.iter().find(|r| r.id == id)
    }

    pub fn selected_row_mut(&mut self) -> Option<&mut ScenarioRow> {
        let id = self.selected_id.clone()?;
        self.rows.iter_mut().find(|r| r.id == id)
    }

    /// Select a scenario; clears group selection.
    pub fn select_scenario(&mut self, id: String) {
        self.selected_id = Some(id);
        self.selected_group_id = None;
        self.selected_step = None;
    }

    /// Select a group (`""` = Ungrouped); clears scenario/step selection.
    pub fn select_group(&mut self, group_id: String) {
        self.selected_group_id = Some(group_id);
        self.selected_id = None;
        self.selected_step = None;
    }

    /// Alias kept for older call sites.
    pub fn select(&mut self, id: String) {
        self.select_scenario(id);
    }

    /// Wire id for group transport (`ungrouped` when empty).
    pub fn group_wire_id(group_id: &str) -> &str {
        if group_id.is_empty() {
            "ungrouped"
        } else {
            group_id
        }
    }

    pub fn poll_if_due(&mut self, force: bool) {
        self.pull_shared();
        let due = force
            || self
                .last_fetch
                .map(|t| t.elapsed() >= POLL_INTERVAL)
                .unwrap_or(true);
        if !due {
            return;
        }
        if !force && !self.sidecar_up {
            if let Ok(g) = self.shared.lock() {
                if let Some(at) = g.next_probe_at {
                    if Instant::now() < at {
                        return;
                    }
                }
            }
        }
        self.last_fetch = Some(Instant::now());
        let _ = self.cmd_tx.try_send(ScenarioCmd::Refresh { force });
    }

    pub fn upsert(&mut self, row: &ScenarioRow) -> Result<ScenarioRow, String> {
        let body = serde_json::to_vec(row).map_err(|e| e.to_string())?;
        let v = self.http("PUT", "/scenario", Some(body))?;
        let scn = v
            .get("scenario")
            .cloned()
            .ok_or_else(|| "missing scenario in response".to_string())?;
        let row: ScenarioRow = serde_json::from_value(scn).map_err(|e| e.to_string())?;
        // Keep selected_step when updating the already-selected scenario — calling
        // select_scenario() would clear it and collapse the step editor every keystroke.
        let keep_step = if self.selected_id.as_deref() == Some(row.id.as_str()) {
            self.selected_step
        } else {
            None
        };
        if let Some(slot) = self.rows.iter_mut().find(|r| r.id == row.id) {
            *slot = row.clone();
        } else {
            self.rows.push(row.clone());
        }
        self.selected_id = Some(row.id.clone());
        self.selected_group_id = None;
        self.selected_step = keep_step;
        self.poll_if_due(true);
        Ok(row)
    }

    pub fn delete(&mut self, id: &str) -> Result<(), String> {
        let _ = self.http("DELETE", &format!("/scenario/{id}"), None)?;
        self.rows.retain(|r| r.id != id);
        self.statuses.retain(|s| s.id != id);
        if self.selected_id.as_deref() == Some(id) {
            self.selected_id = self.rows.first().map(|r| r.id.clone());
            self.selected_step = None;
        }
        self.poll_if_due(true);
        Ok(())
    }

    pub fn upsert_group(&mut self, id: Option<&str>, name: &str) -> Result<ScenarioGroup, String> {
        let body = serde_json::json!({
            "id": id.unwrap_or(""),
            "name": name,
        });
        let v = self.http(
            "PUT",
            "/scenario/groups",
            Some(serde_json::to_vec(&body).map_err(|e| e.to_string())?),
        )?;
        let g = v
            .get("group")
            .cloned()
            .ok_or_else(|| "missing group in response".to_string())?;
        let group: ScenarioGroup = serde_json::from_value(g).map_err(|e| e.to_string())?;
        if let Some(slot) = self.groups.iter_mut().find(|x| x.id == group.id) {
            *slot = group.clone();
        } else {
            self.groups.push(group.clone());
        }
        // Keep selection without bouncing selected_step / remounting the rename field.
        self.selected_group_id = Some(group.id.clone());
        self.selected_id = None;
        self.poll_if_due(true);
        Ok(group)
    }

    pub fn delete_group(&mut self, id: &str) -> Result<(), String> {
        let _ = self.http("DELETE", &format!("/scenario/groups/{id}"), None)?;
        self.groups.retain(|g| g.id != id);
        for r in &mut self.rows {
            if r.group_id == id {
                r.group_id.clear();
            }
        }
        if self.selected_group_id.as_deref() == Some(id) {
            self.selected_group_id = None;
        }
        self.poll_if_due(true);
        Ok(())
    }

    pub fn start_group(&mut self, group_id: &str) -> Result<(), String> {
        let wire = Self::group_wire_id(group_id);
        let _ = self.http("POST", &format!("/scenario/groups/{wire}/start"), None)?;
        self.poll_if_due(true);
        Ok(())
    }

    pub fn pause_group(&mut self, group_id: &str) -> Result<(), String> {
        let wire = Self::group_wire_id(group_id);
        let _ = self.http("POST", &format!("/scenario/groups/{wire}/pause"), None)?;
        self.poll_if_due(true);
        Ok(())
    }

    pub fn resume_group(&mut self, group_id: &str) -> Result<(), String> {
        let wire = Self::group_wire_id(group_id);
        let _ = self.http("POST", &format!("/scenario/groups/{wire}/resume"), None)?;
        self.poll_if_due(true);
        Ok(())
    }

    pub fn stop_group(&mut self, group_id: &str) -> Result<(), String> {
        let wire = Self::group_wire_id(group_id);
        let _ = self.http("POST", &format!("/scenario/groups/{wire}/stop"), None)?;
        self.poll_if_due(true);
        Ok(())
    }

    pub fn reorder(
        &mut self,
        group_ids: Option<Vec<String>>,
        placements: Vec<ScenarioPlacement>,
    ) -> Result<(), String> {
        let body = serde_json::json!({
            "group_ids": group_ids,
            "placements": placements,
        });
        let _ = self.http(
            "POST",
            "/scenario/reorder",
            Some(serde_json::to_vec(&body).map_err(|e| e.to_string())?),
        )?;
        self.poll_if_due(true);
        Ok(())
    }

    pub fn start(&mut self, id: &str) -> Result<(), String> {
        let _ = self.http("POST", &format!("/scenario/{id}/start"), None)?;
        self.poll_if_due(true);
        Ok(())
    }

    pub fn pause(&mut self, id: &str) -> Result<(), String> {
        let _ = self.http("POST", &format!("/scenario/{id}/pause"), None)?;
        self.poll_if_due(true);
        Ok(())
    }

    pub fn resume(&mut self, id: &str) -> Result<(), String> {
        let _ = self.http("POST", &format!("/scenario/{id}/resume"), None)?;
        self.poll_if_due(true);
        Ok(())
    }

    pub fn stop(&mut self, id: &str) -> Result<(), String> {
        let _ = self.http("POST", &format!("/scenario/{id}/stop"), None)?;
        self.poll_if_due(true);
        Ok(())
    }

    pub fn step(&mut self, id: &str) -> Result<(), String> {
        let _ = self.http("POST", &format!("/scenario/{id}/step"), None)?;
        self.poll_if_due(true);
        Ok(())
    }

    /// Import: `replace` | `merge` | `into_group` (+ optional group_id / group_name).
    /// Body is the raw document (wrapped or bare array); sidecar parses it.
    pub fn import_mode(
        &mut self,
        json: &Value,
        mode: &str,
        group_id: Option<&str>,
        group_name: Option<&str>,
    ) -> Result<usize, String> {
        let mode = mode.trim().to_ascii_lowercase();
        let mode = match mode.as_str() {
            "merge" => "merge",
            "into_group" => "into_group",
            _ => "replace",
        };
        let mut path = format!("/scenario/import?mode={mode}");
        if mode == "into_group" {
            if let Some(id) = group_id.map(str::trim).filter(|s| !s.is_empty()) {
                path.push_str(&format!("&group_id={}", query_escape(id)));
            }
            if let Some(name) = group_name.map(str::trim).filter(|s| !s.is_empty()) {
                path.push_str(&format!("&group_name={}", query_escape(name)));
            }
        }
        let body = serde_json::to_vec(json).map_err(|e| e.to_string())?;
        let v = self.http("POST", &path, Some(body))?;
        if let Some(err) = v.get("error").and_then(|e| e.as_str()) {
            return Err(err.to_string());
        }
        self.poll_if_due(true);
        Ok(v.get("scenarios")
            .and_then(|a| a.as_array())
            .map(|a| a.len())
            .unwrap_or(0))
    }

    /// Full wrapped `{ groups, scenarios }` document.
    pub fn export_all(&mut self) -> Result<Value, String> {
        self.http("GET", "/scenario/export", None)
    }

    /// One group (`""` / ungrouped → Ungrouped members only).
    pub fn export_group(&mut self, group_id: &str) -> Result<Value, String> {
        let wire = Self::group_wire_id(group_id);
        self.http("GET", &format!("/scenario/export?group_id={}", query_escape(wire)), None)
    }

    /// Legacy: export everything (prefer [`Self::export_all`]).
    pub fn export(&mut self) -> Result<Value, String> {
        self.export_all()
    }

    pub fn arm_toolbar_after_open(&mut self) {
        if self.toolbar_arm_at.is_none() {
            self.toolbar_arm_at = Some(Instant::now() + Duration::from_millis(450));
        }
    }

    pub fn disarm_toolbar(&mut self) {
        self.toolbar_arm_at = None;
    }

    pub fn toolbar_armed(&self) -> bool {
        match self.toolbar_arm_at {
            Some(t) => Instant::now() >= t,
            None => false,
        }
    }

    fn pull_shared(&mut self) {
        let Ok(g) = self.shared.lock() else {
            return;
        };
        if g.generation == self.last_gen {
            return;
        }
        self.last_gen = g.generation;
        self.rows = g.rows.clone();
        self.groups = g.groups.clone();
        self.statuses = g.statuses.clone();
        self.sidecar_up = g.sidecar_up;
        self.last_error = g.last_error.clone();
        drop(g);
        // Keep selection if still present (group XOR scenario).
        if let Some(gid) = &self.selected_group_id {
            let ok = gid.is_empty() || self.groups.iter().any(|g| g.id == *gid);
            if !ok {
                self.selected_group_id = None;
            }
        }
        if let Some(id) = &self.selected_id {
            if !self.rows.iter().any(|r| r.id == *id) {
                self.selected_id = None;
                self.selected_step = None;
            }
        }
        if self.selected_id.is_none() && self.selected_group_id.is_none() {
            if let Some(first) = self.rows.first() {
                self.selected_id = Some(first.id.clone());
            }
        }
    }

    fn http(&self, method: &str, path: &str, body: Option<Vec<u8>>) -> Result<Value, String> {
        let (reply_tx, reply_rx) = mpsc::sync_channel(1);
        self.cmd_tx
            .send(ScenarioCmd::Http {
                method: method.to_string(),
                path: path.to_string(),
                body,
                reply: reply_tx,
            })
            .map_err(|_| "scenario worker disconnected".to_string())?;
        reply_rx
            .recv()
            .map_err(|_| "scenario worker disconnected".to_string())?
    }
}

fn worker_loop(rx: Receiver<ScenarioCmd>, shared: Arc<Mutex<SharedSnap>>, base: String) {
    let mut token: Option<String> = None;
    while let Ok(cmd) = rx.recv() {
        match cmd {
            ScenarioCmd::Refresh { force } => {
                {
                    let Ok(g) = shared.lock() else {
                        continue;
                    };
                    if !force {
                        if let Some(at) = g.next_probe_at {
                            if Instant::now() < at {
                                continue;
                            }
                        }
                    }
                }
                while let Ok(ScenarioCmd::Refresh { .. }) = rx.try_recv() {}
                match fetch_list(&base, &mut token) {
                    Ok((rows, groups, statuses)) => {
                        let ctx = {
                            let Ok(mut g) = shared.lock() else {
                                continue;
                            };
                            g.rows = rows;
                            g.groups = groups;
                            g.statuses = statuses;
                            g.sidecar_up = true;
                            g.last_error = None;
                            g.backoff = BACKOFF_START;
                            g.next_probe_at = None;
                            g.generation = g.generation.wrapping_add(1);
                            g.egui_ctx.clone()
                        };
                        if let Some(ctx) = ctx {
                            ctx.request_repaint();
                        }
                    }
                    Err(e) => {
                        let ctx = {
                            let Ok(mut g) = shared.lock() else {
                                continue;
                            };
                            let was_up = g.sidecar_up;
                            g.sidecar_up = false;
                            g.last_error = Some(e);
                            let next = g.backoff;
                            g.next_probe_at = Some(Instant::now() + next);
                            g.backoff = (g.backoff.saturating_mul(2)).min(BACKOFF_MAX);
                            g.generation = g.generation.wrapping_add(1);
                            if was_up {
                                g.egui_ctx.clone()
                            } else {
                                None
                            }
                        };
                        if let Some(ctx) = ctx {
                            ctx.request_repaint();
                        }
                    }
                }
            }
            ScenarioCmd::Http {
                method,
                path,
                body,
                reply,
            } => {
                let result = http_call(&base, &mut token, &method, &path, body);
                match &result {
                    Ok(_) => {
                        if let Ok(mut g) = shared.lock() {
                            g.sidecar_up = true;
                            g.last_error = None;
                            g.backoff = BACKOFF_START;
                            g.next_probe_at = None;
                        }
                    }
                    Err(e) if e.contains("unreachable") || e.contains("cannot read") => {
                        if let Ok(mut g) = shared.lock() {
                            g.sidecar_up = false;
                            g.last_error = Some(e.clone());
                            let next = g.backoff;
                            g.next_probe_at = Some(Instant::now() + next);
                            g.backoff = (g.backoff.saturating_mul(2)).min(BACKOFF_MAX);
                            g.generation = g.generation.wrapping_add(1);
                        }
                    }
                    Err(_) => {}
                }
                let _ = reply.send(result);
            }
        }
    }
}

fn fetch_list(
    base: &str,
    token: &mut Option<String>,
) -> Result<(Vec<ScenarioRow>, Vec<ScenarioGroup>, Vec<ScenarioStatus>), String> {
    let v = http_call(base, token, "GET", "/scenario", None)?;
    let rows: Vec<ScenarioRow> =
        serde_json::from_value(v.get("scenarios").cloned().unwrap_or(Value::Array(vec![])))
            .map_err(|e| format!("scenarios: {e}"))?;
    let groups: Vec<ScenarioGroup> =
        serde_json::from_value(v.get("groups").cloned().unwrap_or(Value::Array(vec![])))
            .map_err(|e| format!("groups: {e}"))?;
    let statuses: Vec<ScenarioStatus> =
        serde_json::from_value(v.get("statuses").cloned().unwrap_or(Value::Array(vec![])))
            .map_err(|e| format!("statuses: {e}"))?;
    Ok((rows, groups, statuses))
}

fn read_token_cached(token: &mut Option<String>) -> Result<String, String> {
    if let Some(t) = token.as_ref() {
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
    *token = Some(t.clone());
    Ok(t)
}

fn http_call(
    base: &str,
    token: &mut Option<String>,
    method: &str,
    path: &str,
    body: Option<Vec<u8>>,
) -> Result<Value, String> {
    let tok = read_token_cached(token)?;
    let url = format!("{base}{path}");
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
    .set("Authorization", &format!("Bearer {tok}"))
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
        Err(ureq::Error::Transport(t)) => {
            Err(format!("TopicBench Pro unreachable ({t}) — is it running?"))
        }
    }
}

/// Import/load: bare `[…]` or `{ "scenarios"|"scenario": […] }`.
pub fn parse_scenarios_array(json: &Value) -> Result<&[Value], String> {
    if let Some(arr) = json.as_array() {
        return Ok(arr.as_slice());
    }
    if let Some(obj) = json.as_object() {
        for key in ["scenarios", "scenario"] {
            if let Some(arr) = obj.get(key).and_then(|x| x.as_array()) {
                return Ok(arr.as_slice());
            }
        }
        return Err("import JSON must be a bare array [ … ] or { \"scenarios\": [ … ] }".into());
    }
    Err(format!(
        "import JSON must be a bare array [ … ] or {{ \"scenarios\": [ … ] }} — found {}",
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

/// Groups from a wrapped document (`groups` key); bare arrays → none.
pub fn parse_groups_array(json: &Value) -> Result<Vec<ScenarioGroup>, String> {
    let Some(obj) = json.as_object() else {
        return Ok(Vec::new());
    };
    let Some(arr) = obj.get("groups").and_then(|x| x.as_array()) else {
        return Ok(Vec::new());
    };
    serde_json::from_value(Value::Array(arr.clone())).map_err(|e| format!("groups: {e}"))
}

fn query_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn token_path() -> PathBuf {
    crate::mqtt::paths::config_file("mcp-token")
}

/// Blank scenario ready for Add (`default_profile` seeds the first publish step).
pub fn blank_scenario(default_profile: &str) -> ScenarioRow {
    ScenarioRow {
        id: String::new(),
        name: "New scenario".into(),
        profile: String::new(),
        group_id: String::new(),
        sort: 0,
        loop_: false,
        steps: vec![blank_step("publish", default_profile)],
        auto_start: false,
    }
}

/// Blank step; `default_profile` used for publish / wait_value.
pub fn blank_step(kind: &str, default_profile: &str) -> ScenarioStep {
    match kind {
        "wait_time" => ScenarioStep::WaitTime {
            id: String::new(),
            name: "wait".into(),
            enabled: true,
            duration_ms: 500,
            max_ms: None,
        },
        "wait_value" => ScenarioStep::WaitValue {
            id: String::new(),
            name: "wait value".into(),
            enabled: true,
            profile: default_profile.to_string(),
            topic: String::new(),
            field: String::new(),
            op: WaitOp::Eq,
            value: String::new(),
            match_retained: true,
        },
        _ => ScenarioStep::Publish {
            id: String::new(),
            name: "publish".into(),
            enabled: true,
            profile: default_profile.to_string(),
            topic: String::new(),
            payload: String::new(),
            qos: 0,
            retain: false,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_bare_and_wrapped() {
        assert_eq!(
            parse_scenarios_array(&json!([{ "id": "a" }]))
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            parse_scenarios_array(&json!({ "scenarios": [{ "id": "a" }, { "id": "b" }] }))
                .unwrap()
                .len(),
            2
        );
        assert!(parse_scenarios_array(&json!({ "other": [] })).is_err());
        assert!(parse_groups_array(&json!([{ "id": "a" }])).unwrap().is_empty());
        let groups = parse_groups_array(&json!({
            "groups": [{ "id": "g1", "name": "Lab" }],
            "scenarios": []
        }))
        .unwrap();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].id, "g1");
    }

    #[test]
    fn blank_has_group_defaults() {
        let s = blank_scenario("p1");
        assert!(s.group_id.is_empty());
        assert_eq!(s.sort, 0);
    }

    #[test]
    fn step_round_trip() {
        let s = blank_step("wait_time", "");
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(v.get("kind").and_then(|k| k.as_str()), Some("wait_time"));
        let back: ScenarioStep = serde_json::from_value(v).unwrap();
        assert_eq!(back.kind_label(), "wait_time");
    }

    #[test]
    fn publish_blank_topic_serializes() {
        let s = blank_step("publish", "p1");
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(v.get("profile").and_then(|p| p.as_str()), Some("p1"));
        assert_eq!(v.get("topic").and_then(|t| t.as_str()), Some(""));
    }
}
