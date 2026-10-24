//! Localhost IPC bridge for TopicBench Pro (MCP / future engines).
//!
//! **v1 transport:** Unix domain socket at `~/.config/topicbench/bridge.sock`
//! with one JSON object per line (request → response). MQTT truth stays in
//! SessionHub; this only serializes tool calls onto the same UI paths.
//!
//! Started only when Pro is licensed (`pro::has(Entitlement::McpServer)` /
//! `pro::is_licensed()`). Without a license the socket is never bound.
//!
//! Settings → MCP panel will plug in later (enable/token/port); for v1 Pro owns
//! the HTTP listener and reads token from `~/.config/topicbench/mcp-token`.

use crate::mqtt::paths;
use crate::mqtt::publish::Qos;
use crate::mqtt::session::{SessionManager, SessionState};
use crate::mqtt::subscribe;
use crate::mqtt::profile::ProfileStore;
use eframe::egui::Context;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::VecDeque;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Socket filename under the TopicBench config dir.
pub const BRIDGE_SOCK_NAME: &str = "bridge.sock";
/// Default ring-buffer depth for MCP `read_messages`.
const MSG_BUF_CAP: usize = 500;
/// Soft cap on pending waiters.
const WAITER_CAP: usize = 64;

/// Path to the bridge Unix socket (`~/.config/topicbench/bridge.sock`).
pub fn socket_path() -> PathBuf {
    paths::config_file(BRIDGE_SOCK_NAME)
}

/// One inbound MQTT message retained for MCP read/wait tools.
#[derive(Clone, Debug, Serialize)]
pub struct BufferedMessage {
    pub profile_id: String,
    pub profile_name: String,
    pub topic: String,
    pub payload: String,
    pub retain: bool,
    /// Unix epoch millis when applied on the UI thread.
    pub ts_ms: u64,
}

/// Pending `wait_for_message` registered from the accept thread.
struct Waiter {
    profile_id: Option<String>,
    topic_filter: String,
    /// Only match messages with `ts_ms >= since_ms` (0 = any).
    since_ms: u64,
    deadline: Instant,
    done: Arc<(Mutex<Option<Result<BufferedMessage, String>>>, Condvar)>,
}

struct Shared {
    messages: VecDeque<BufferedMessage>,
    waiters: Vec<Waiter>,
}

/// Handle owned by the egui app: drain requests each frame, feed message buffer.
pub struct BridgeHandle {
    rx: Receiver<PendingCall>,
    shared: Arc<Mutex<Shared>>,
    stop: Arc<AtomicBool>,
    /// Wake egui when a bridge call arrives (idle UI would otherwise never drain).
    egui_ctx: Arc<Mutex<Option<Context>>>,
    /// True while the accept thread is running.
    pub listening: bool,
    pub socket_path: PathBuf,
}

struct PendingCall {
    req: BridgeRequest,
    reply: Sender<BridgeResponse>,
}

#[derive(Debug, Deserialize)]
struct BridgeRequest {
    id: u64,
    method: String,
    #[serde(default)]
    params: serde_json::Value,
}

#[derive(Debug, Serialize)]
struct BridgeResponse {
    id: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

impl BridgeHandle {
    /// Bind the Unix socket and spawn the accept loop. Call only when entitled.
    pub fn start(ctx: &Context) -> Result<Self, String> {
        let path = socket_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("mkdir config: {e}"))?;
        }
        let _ = fs::remove_file(&path);
        let listener =
            UnixListener::bind(&path).map_err(|e| format!("bind {}: {e}", path.display()))?;
        // Restrict to the owning user (best-effort on Linux).
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o600));
        }

        let (tx, rx) = mpsc::channel::<PendingCall>();
        let shared = Arc::new(Mutex::new(Shared {
            messages: VecDeque::with_capacity(MSG_BUF_CAP),
            waiters: Vec::new(),
        }));
        let stop = Arc::new(AtomicBool::new(false));
        let egui_ctx = Arc::new(Mutex::new(Some(ctx.clone())));
        let stop_t = stop.clone();
        let shared_t = shared.clone();
        let path_t = path.clone();
        let wake = egui_ctx.clone();

        thread::Builder::new()
            .name("topicbench-bridge".into())
            .spawn(move || accept_loop(listener, tx, shared_t, stop_t, path_t, wake))
            .map_err(|e| format!("spawn bridge: {e}"))?;

        Ok(Self {
            rx,
            shared,
            stop,
            egui_ctx,
            listening: true,
            socket_path: path,
        })
    }

    /// Keep the wake handle current (call from `App::update`).
    pub fn bind_egui_ctx(&self, ctx: &Context) {
        *self.egui_ctx.lock().unwrap_or_else(|e| e.into_inner()) = Some(ctx.clone());
    }

    /// Push one applied MQTT message into the ring buffer and wake waiters.
    pub fn on_message(
        &self,
        profile_id: &str,
        profile_name: &str,
        topic: &str,
        payload: &str,
        retain: bool,
    ) {
        let ts_ms = now_ms();
        let msg = BufferedMessage {
            profile_id: profile_id.to_string(),
            profile_name: profile_name.to_string(),
            topic: topic.to_string(),
            payload: payload.to_string(),
            retain,
            ts_ms,
        };
        let mut g = self.shared.lock().unwrap_or_else(|e| e.into_inner());
        g.messages.push_back(msg.clone());
        while g.messages.len() > MSG_BUF_CAP {
            g.messages.pop_front();
        }
        // Complete matching waiters (first match wins per waiter).
        let mut i = 0;
        while i < g.waiters.len() {
            let w = &g.waiters[i];
            let profile_ok = w
                .profile_id
                .as_ref()
                .map(|id| id == profile_id)
                .unwrap_or(true);
            let topic_ok = subscribe::topic_matches_filter(topic, &w.topic_filter);
            let since_ok = ts_ms >= w.since_ms;
            if profile_ok && topic_ok && since_ok {
                let w = g.waiters.remove(i);
                let mut slot = w.done.0.lock().unwrap_or_else(|e| e.into_inner());
                *slot = Some(Ok(msg.clone()));
                w.done.1.notify_one();
            } else {
                i += 1;
            }
        }
    }

    /// Expire waiters whose deadline has passed (call each UI frame).
    pub fn tick_waiters(&self) {
        let now = Instant::now();
        let mut g = self.shared.lock().unwrap_or_else(|e| e.into_inner());
        let mut i = 0;
        while i < g.waiters.len() {
            if g.waiters[i].deadline <= now {
                let w = g.waiters.remove(i);
                let mut slot = w.done.0.lock().unwrap_or_else(|e| e.into_inner());
                if slot.is_none() {
                    *slot = Some(Err("timeout".into()));
                }
                w.done.1.notify_one();
            } else {
                i += 1;
            }
        }
    }

    /// Drain pending bridge calls on the UI thread (same SessionHub paths as the GUI).
    pub fn poll(
        &self,
        profiles: &ProfileStore,
        sessions: &mut SessionManager,
        max_packet_bytes: usize,
        ctx: &Context,
        status_out: &mut String,
    ) {
        self.tick_waiters();
        loop {
            let call = match self.rx.try_recv() {
                Ok(c) => c,
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => break,
            };
            let resp = dispatch(call.req, profiles, sessions, max_packet_bytes, ctx, &self.shared, status_out);
            let _ = call.reply.send(resp);
        }
    }
}

impl Drop for BridgeHandle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // Connect briefly so accept unblocks, then remove the socket file.
        let _ = UnixStream::connect(&self.socket_path);
        let _ = fs::remove_file(&self.socket_path);
    }
}

fn accept_loop(
    listener: UnixListener,
    tx: Sender<PendingCall>,
    shared: Arc<Mutex<Shared>>,
    stop: Arc<AtomicBool>,
    path: PathBuf,
    wake: Arc<Mutex<Option<Context>>>,
) {
    let _ = listener.set_nonblocking(false);
    while !stop.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((stream, _)) => {
                if stop.load(Ordering::Relaxed) {
                    break;
                }
                let tx = tx.clone();
                let shared = shared.clone();
                let stop_c = stop.clone();
                let wake_c = wake.clone();
                let _ = thread::Builder::new()
                    .name("topicbench-bridge-conn".into())
                    .spawn(move || handle_conn(stream, tx, shared, stop_c, wake_c));
            }
            Err(_) => {
                if stop.load(Ordering::Relaxed) {
                    break;
                }
                thread::sleep(Duration::from_millis(20));
            }
        }
    }
    let _ = fs::remove_file(&path);
}

fn wake_ui(wake: &Arc<Mutex<Option<Context>>>) {
    if let Some(ctx) = wake.lock().unwrap_or_else(|e| e.into_inner()).clone() {
        ctx.request_repaint();
    }
}

fn handle_conn(
    stream: UnixStream,
    tx: Sender<PendingCall>,
    shared: Arc<Mutex<Shared>>,
    stop: Arc<AtomicBool>,
    wake: Arc<Mutex<Option<Context>>>,
) {
    let Ok(cloned) = stream.try_clone() else {
        return;
    };
    let mut reader = BufReader::new(cloned);
    let mut writer = stream;
    let mut line = String::new();
    while !stop.load(Ordering::Relaxed) {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {}
            Err(_) => break,
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let req: BridgeRequest = match serde_json::from_str(trimmed) {
            Ok(r) => r,
            Err(e) => {
                let _ = writeln!(
                    writer,
                    "{}",
                    serde_json::to_string(&BridgeResponse {
                        id: 0,
                        result: None,
                        error: Some(format!("bad request: {e}")),
                    })
                    .unwrap_or_else(|_| r#"{"id":0,"error":"bad request"}"#.into())
                );
                continue;
            }
        };

        // wait_for_message is handled on the connection thread (blocks until match/timeout).
        if req.method == "wait_for_message" {
            let resp = wait_for_message_conn(&req, &shared);
            let _ = writeln!(
                writer,
                "{}",
                serde_json::to_string(&resp).unwrap_or_else(|_| format!(r#"{{"id":{},"error":"encode"}}"#, req.id))
            );
            continue;
        }

        let (reply_tx, reply_rx) = mpsc::channel();
        if tx
            .send(PendingCall {
                req,
                reply: reply_tx,
            })
            .is_err()
        {
            break;
        }
        // Idle egui would never drain — poke the UI thread.
        wake_ui(&wake);
        // Block until the UI thread answers (or the app exits).
        let resp = match reply_rx.recv_timeout(Duration::from_secs(120)) {
            Ok(r) => r,
            Err(_) => BridgeResponse {
                id: 0,
                result: None,
                error: Some("UI bridge timeout".into()),
            },
        };
        let _ = writeln!(
            writer,
            "{}",
            serde_json::to_string(&resp).unwrap_or_else(|_| r#"{"id":0,"error":"encode"}"#.into())
        );
    }
}

fn wait_for_message_conn(req: &BridgeRequest, shared: &Arc<Mutex<Shared>>) -> BridgeResponse {
    let id = req.id;
    let profile = req
        .params
        .get("profile")
        .or_else(|| req.params.get("profile_id"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let topic_filter = req
        .params
        .get("topic_filter")
        .or_else(|| req.params.get("topic"))
        .and_then(|v| v.as_str())
        .unwrap_or("#")
        .to_string();
    let timeout_ms = req
        .params
        .get("timeout_ms")
        .and_then(|v| v.as_u64())
        .unwrap_or(10_000)
        .min(120_000);
    let since_ms = req
        .params
        .get("since_ms")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    // Fast path: already-buffered matching message.
    {
        let g = shared.lock().unwrap_or_else(|e| e.into_inner());
        for m in g.messages.iter().rev() {
            let profile_ok = profile.as_ref().map(|p| p == &m.profile_id).unwrap_or(true);
            if profile_ok
                && m.ts_ms >= since_ms
                && subscribe::topic_matches_filter(&m.topic, &topic_filter)
            {
                return BridgeResponse {
                    id,
                    result: Some(serde_json::to_value(m).unwrap_or(json!({}))),
                    error: None,
                };
            }
        }
    }

    let done = Arc::new((Mutex::new(None), Condvar::new()));
    {
        let mut g = shared.lock().unwrap_or_else(|e| e.into_inner());
        if g.waiters.len() >= WAITER_CAP {
            return BridgeResponse {
                id,
                result: None,
                error: Some("too many waiters".into()),
            };
        }
        g.waiters.push(Waiter {
            profile_id: profile,
            topic_filter,
            since_ms,
            deadline: Instant::now() + Duration::from_millis(timeout_ms),
            done: done.clone(),
        });
    }

    let (lock, cvar) = &*done;
    let mut slot = lock.lock().unwrap_or_else(|e| e.into_inner());
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    while slot.is_none() {
        let now = Instant::now();
        if now >= deadline {
            break;
        }
        let (next, timeout_res) = cvar
            .wait_timeout(slot, deadline.saturating_duration_since(now))
            .unwrap_or_else(|e| e.into_inner());
        slot = next;
        if timeout_res.timed_out() && slot.is_none() {
            break;
        }
    }
    match slot.take() {
        Some(Ok(msg)) => BridgeResponse {
            id,
            result: Some(serde_json::to_value(msg).unwrap_or(json!({}))),
            error: None,
        },
        Some(Err(e)) => BridgeResponse {
            id,
            result: None,
            error: Some(e),
        },
        None => BridgeResponse {
            id,
            result: None,
            error: Some("timeout".into()),
        },
    }
}

fn dispatch(
    req: BridgeRequest,
    profiles: &ProfileStore,
    sessions: &mut SessionManager,
    max_packet_bytes: usize,
    ctx: &Context,
    shared: &Arc<Mutex<Shared>>,
    status_out: &mut String,
) -> BridgeResponse {
    let id = req.id;
    let result = match req.method.as_str() {
        "ping" => Ok(json!({"pong": true})),
        "list_profiles" => Ok(list_profiles(profiles, sessions)),
        "list_sessions" => Ok(list_sessions(sessions)),
        "connect_profile" => connect_profile(&req.params, profiles, sessions, max_packet_bytes, ctx, status_out),
        "disconnect_profile" => disconnect_profile(&req.params, sessions, status_out),
        "subscribe" => subscribe_runtime(&req.params, sessions, status_out),
        "unsubscribe" => unsubscribe_runtime(&req.params, sessions, status_out),
        "publish" => publish_msg(&req.params, sessions, status_out),
        "read_messages" => Ok(read_messages(&req.params, shared)),
        "get_last_payload" => get_last_payload(&req.params, sessions, shared),
        "server_status" => Ok(server_status(sessions, shared)),
        other => Err(format!("unknown method: {other}")),
    };
    match result {
        Ok(v) => BridgeResponse {
            id,
            result: Some(v),
            error: None,
        },
        Err(e) => BridgeResponse {
            id,
            result: None,
            error: Some(e),
        },
    }
}

fn list_profiles(profiles: &ProfileStore, sessions: &SessionManager) -> serde_json::Value {
    let mut out = Vec::new();
    for id in profiles.ids_in_order() {
        let Some(p) = profiles.profiles.get(&id) else {
            continue;
        };
        let connected = sessions
            .open
            .get(&id)
            .map(|s| s.state == SessionState::Connected)
            .unwrap_or(false);
        let state = sessions
            .open
            .get(&id)
            .map(|s| s.state.label())
            .unwrap_or("disconnected");
        // Redact credentials — never return password / key material.
        out.push(json!({
            "id": p.id,
            "name": p.name,
            "host": p.host,
            "port": p.port,
            "connected": connected,
            "state": state,
            "use_tls": p.use_tls,
        }));
    }
    json!(out)
}

fn list_sessions(sessions: &SessionManager) -> serde_json::Value {
    let mut out = Vec::new();
    for s in sessions.open.values() {
        out.push(json!({
            "profile_id": s.profile_id,
            "profile_name": s.profile_name,
            "state": s.state.label(),
            "detail": s.detail,
            "subscriptions": s.subscriptions,
            "message_count": s.message_count,
        }));
    }
    json!(out)
}

fn resolve_profile_id(profiles: &ProfileStore, key: &str) -> Result<String, String> {
    let key = key.trim();
    if key.is_empty() {
        return Err("profile required".into());
    }
    if profiles.profiles.contains_key(key) {
        return Ok(key.to_string());
    }
    // Match by display name (case-insensitive).
    let lower = key.to_ascii_lowercase();
    let mut hits: Vec<String> = profiles
        .profiles
        .values()
        .filter(|p| p.name.to_ascii_lowercase() == lower)
        .map(|p| p.id.clone())
        .collect();
    if hits.len() == 1 {
        return Ok(hits.remove(0));
    }
    if hits.is_empty() {
        Err(format!("profile not found: {key}"))
    } else {
        Err(format!("ambiguous profile name: {key}"))
    }
}

fn connect_profile(
    params: &serde_json::Value,
    profiles: &ProfileStore,
    sessions: &mut SessionManager,
    max_packet_bytes: usize,
    ctx: &Context,
    status_out: &mut String,
) -> Result<serde_json::Value, String> {
    let key = params
        .get("profile")
        .or_else(|| params.get("profile_id"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| "profile required".to_string())?;
    let id = resolve_profile_id(profiles, key)?;
    let profile = profiles
        .profiles
        .get(&id)
        .cloned()
        .ok_or_else(|| format!("profile not found: {id}"))?;
    sessions.connect(&profile, max_packet_bytes, ctx)?;
    *status_out = format!("Pro: connecting {}", profile.name);
    Ok(json!({
        "profile_id": id,
        "profile_name": profile.name,
        "state": "connecting",
    }))
}

fn disconnect_profile(
    params: &serde_json::Value,
    sessions: &mut SessionManager,
    status_out: &mut String,
) -> Result<serde_json::Value, String> {
    let key = params
        .get("profile")
        .or_else(|| params.get("profile_id"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| "profile required".to_string())?;
    // Prefer open session id, else treat key as id.
    let id = if sessions.open.contains_key(key) {
        key.to_string()
    } else {
        sessions
            .open
            .values()
            .find(|s| s.profile_name.eq_ignore_ascii_case(key))
            .map(|s| s.profile_id.clone())
            .unwrap_or_else(|| key.to_string())
    };
    if !sessions.open.contains_key(&id) {
        return Err(format!("no open session: {key}"));
    }
    let name = sessions
        .open
        .get(&id)
        .map(|s| s.profile_name.clone())
        .unwrap_or_else(|| id.clone());
    sessions.disconnect(&id);
    *status_out = format!("Pro: disconnected {name}");
    Ok(json!({"profile_id": id, "state": "disconnected"}))
}

fn subscribe_runtime(
    params: &serde_json::Value,
    sessions: &mut SessionManager,
    status_out: &mut String,
) -> Result<serde_json::Value, String> {
    let key = params
        .get("profile")
        .or_else(|| params.get("profile_id"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| "profile required".to_string())?;
    let filter = params
        .get("topic_filter")
        .or_else(|| params.get("topic"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| "topic_filter required".to_string())?;
    let id = resolve_open_session(sessions, key)?;
    let mut filters = sessions
        .open
        .get(&id)
        .ok_or_else(|| "no open session".to_string())?
        .subscriptions
        .clone();
    subscribe::add_subscription(&mut filters, filter)?;
    // Session-only for tests — do not persist into the profile file.
    sessions.apply_profile_subscriptions(&id, &filters)?;
    *status_out = format!("Pro: subscribed {filter}");
    Ok(json!({"profile_id": id, "subscriptions": filters}))
}

fn unsubscribe_runtime(
    params: &serde_json::Value,
    sessions: &mut SessionManager,
    status_out: &mut String,
) -> Result<serde_json::Value, String> {
    let key = params
        .get("profile")
        .or_else(|| params.get("profile_id"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| "profile required".to_string())?;
    let filter = params
        .get("topic_filter")
        .or_else(|| params.get("topic"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| "topic_filter required".to_string())?;
    let id = resolve_open_session(sessions, key)?;
    let mut filters = sessions
        .open
        .get(&id)
        .ok_or_else(|| "no open session".to_string())?
        .subscriptions
        .clone();
    if !subscribe::remove_subscription(&mut filters, filter) {
        return Err(format!("filter not in session: {filter}"));
    }
    sessions.apply_profile_subscriptions(&id, &filters)?;
    *status_out = format!("Pro: unsubscribed {filter}");
    Ok(json!({"profile_id": id, "subscriptions": filters}))
}

fn publish_msg(
    params: &serde_json::Value,
    sessions: &mut SessionManager,
    status_out: &mut String,
) -> Result<serde_json::Value, String> {
    let key = params
        .get("profile")
        .or_else(|| params.get("profile_id"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| "profile required".to_string())?;
    let topic = params
        .get("topic")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "topic required".to_string())?;
    let payload = params
        .get("payload")
        .map(|v| {
            if let Some(s) = v.as_str() {
                s.to_string()
            } else {
                v.to_string()
            }
        })
        .unwrap_or_default();
    let qos = match params.get("qos").and_then(|v| v.as_u64()).unwrap_or(0) {
        2 => Qos::ExactlyOnce,
        1 => Qos::AtLeastOnce,
        _ => Qos::AtMostOnce,
    };
    let retain = params
        .get("retain")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let id = resolve_open_session(sessions, key)?;
    sessions.publish_live(&id, topic, &payload, qos, retain)?;
    // Do not dump per-topic lines into the status strip — sim/bridge publishes
    // are high-rate; the strip shows compact "Simulation: running — N …" instead.
    let _ = status_out;
    Ok(json!({
        "profile_id": id,
        "topic": topic,
        "bytes": payload.len(),
        "qos": qos.as_u8(),
        "retain": retain,
    }))
}

fn read_messages(params: &serde_json::Value, shared: &Arc<Mutex<Shared>>) -> serde_json::Value {
    let profile = params
        .get("profile")
        .or_else(|| params.get("profile_id"))
        .and_then(|v| v.as_str());
    let pattern = params
        .get("topic_pattern")
        .or_else(|| params.get("topic_filter"))
        .or_else(|| params.get("topic"))
        .and_then(|v| v.as_str())
        .unwrap_or("#");
    let since_ms = params
        .get("since_ms")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let limit = params
        .get("limit")
        .and_then(|v| v.as_u64())
        .unwrap_or(50)
        .min(MSG_BUF_CAP as u64) as usize;

    let g = shared.lock().unwrap_or_else(|e| e.into_inner());
    let mut out: Vec<&BufferedMessage> = g
        .messages
        .iter()
        .filter(|m| {
            let profile_ok = profile.map(|p| p == m.profile_id).unwrap_or(true);
            profile_ok
                && m.ts_ms >= since_ms
                && subscribe::topic_matches_filter(&m.topic, pattern)
        })
        .collect();
    // Newest last in buffer — return newest-first, capped.
    out.reverse();
    out.truncate(limit);
    json!(out)
}

fn get_last_payload(
    params: &serde_json::Value,
    sessions: &SessionManager,
    shared: &Arc<Mutex<Shared>>,
) -> Result<serde_json::Value, String> {
    let key = params
        .get("profile")
        .or_else(|| params.get("profile_id"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| "profile required".to_string())?;
    let topic = params
        .get("topic")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "topic required".to_string())?;
    let id = resolve_open_session(sessions, key)?;

    // Prefer live topic tree (what the GUI shows).
    if let Some(session) = sessions.open.get(&id) {
        if let Some(node) = session.topic_tree.root.node_at(topic) {
            if let Some(payload) = &node.last_payload {
                return Ok(json!({
                    "profile_id": id,
                    "topic": topic,
                    "payload": payload,
                    "retain": node.last_retain,
                    "source": "topic_tree",
                }));
            }
        }
    }

    // Fallback: newest matching buffer entry.
    let g = shared.lock().unwrap_or_else(|e| e.into_inner());
    for m in g.messages.iter().rev() {
        if m.profile_id == id && m.topic == topic {
            return Ok(json!({
                "profile_id": id,
                "topic": topic,
                "payload": m.payload,
                "retain": m.retain,
                "ts_ms": m.ts_ms,
                "source": "buffer",
            }));
        }
    }
    Err(format!("no payload for {id} / {topic}"))
}

fn server_status(sessions: &SessionManager, shared: &Arc<Mutex<Shared>>) -> serde_json::Value {
    let buf_len = shared
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .messages
        .len();
    json!({
        "bridge": "up",
        "socket": socket_path().display().to_string(),
        "app_version": env!("CARGO_PKG_VERSION"),
        "sessions": sessions.open.len(),
        "buffer_len": buf_len,
        "buffer_cap": MSG_BUF_CAP,
        "licensed": true,
    })
}

fn resolve_open_session(sessions: &SessionManager, key: &str) -> Result<String, String> {
    let key = key.trim();
    if sessions.open.contains_key(key) {
        return Ok(key.to_string());
    }
    let lower = key.to_ascii_lowercase();
    let mut hits: Vec<String> = sessions
        .open
        .values()
        .filter(|s| s.profile_name.to_ascii_lowercase() == lower || s.profile_id == key)
        .map(|s| s.profile_id.clone())
        .collect();
    if hits.len() == 1 {
        Ok(hits.remove(0))
    } else if hits.is_empty() {
        Err(format!("no open session for: {key}"))
    } else {
        Err(format!("ambiguous session: {key}"))
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
