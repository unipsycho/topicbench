//! Thin HTTP client + background forwarder for Pro `/history/*`.
//!
//! Deep store lives only in topicbench-pro. OSS enqueues topic payloads
//! without blocking MQTT ingest (bounded channel; drop + backoff if Pro is down).
//!
//! **Paint path never calls ureq.** `query_cached` / `meta` return cache (or empty)
//! and schedule a background fetch; critical on Windows where a down sidecar
//! blocks `timeout_connect` for hundreds of ms.

use eframe::egui::Context;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const DEFAULT_BASE: &str = "http://127.0.0.1:7853";
/// Drop rather than block ingest when the forwarder is behind.
const QUEUE_CAP: usize = 4096;
const BATCH_MAX: usize = 128;
const BATCH_WAIT: Duration = Duration::from_millis(40);
const QUERY_CAP: usize = 32;
const DEEP_FRESH: Duration = Duration::from_secs(1);
const META_FRESH: Duration = Duration::from_secs(2);
const QUERY_BACKOFF: Duration = Duration::from_secs(2);

/// One sample to append (matches Pro `AppendItem`).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FwdSample {
    pub series_id: String,
    #[serde(default)]
    pub profile: String,
    #[serde(default)]
    pub topic: String,
    #[serde(default)]
    pub field: String,
    pub t_ms: i64,
    #[serde(default)]
    pub payload: String,
    #[serde(default)]
    pub v: Option<f64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DeepSample {
    pub t_ms: i64,
    #[serde(default)]
    pub payload: String,
    #[serde(default)]
    pub v: Option<f64>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SeriesMeta {
    #[serde(default)]
    pub series_id: String,
    #[serde(default)]
    pub count: usize,
    pub from_ms: Option<i64>,
    pub to_ms: Option<i64>,
    #[serde(default)]
    pub persist: bool,
}

/// Shared handle: try_enqueue from UI thread; query/meta never block paint.
#[derive(Clone)]
pub struct HistoryForwarder {
    tx: SyncSender<FwdSample>,
    query_tx: SyncSender<QueryCmd>,
    shared: Arc<Mutex<FwdShared>>,
}

#[derive(Default)]
struct FwdShared {
    sidecar_up: bool,
    last_error: Option<String>,
    /// Cached deep samples per series (for scrub merge).
    deep_cache: HashMap<String, CachedDeep>,
    meta_cache: HashMap<String, (Instant, SeriesMeta)>,
    /// Series ids with an in-flight query (avoid enqueue spam every frame).
    pending_query: HashSet<String>,
    pending_meta: HashSet<String>,
    /// After a transport failure, skip new probes until this instant.
    next_probe_at: Option<Instant>,
    egui_ctx: Option<Context>,
}

struct CachedDeep {
    fetched_at: Instant,
    from_ms: i64,
    to_ms: i64,
    samples: Vec<DeepSample>,
}

enum QueryCmd {
    Query {
        series_id: String,
        from_ms: i64,
        to_ms: i64,
        limit: usize,
    },
    Meta {
        series_id: String,
    },
    Persist {
        series_id: String,
        persist: bool,
    },
}

impl HistoryForwarder {
    /// Spawn background append + query workers (call once when HistoryScrubber entitled).
    pub fn start() -> Self {
        let base = std::env::var("TOPICBENCH_PRO_URL")
            .unwrap_or_else(|_| DEFAULT_BASE.to_string())
            .trim_end_matches('/')
            .to_string();
        let (tx, rx) = mpsc::sync_channel(QUEUE_CAP);
        let (query_tx, query_rx) = mpsc::sync_channel(QUERY_CAP);
        let shared = Arc::new(Mutex::new(FwdShared::default()));
        let shared_t = Arc::clone(&shared);
        let base_t = base.clone();
        let _ = thread::Builder::new()
            .name("tb-hist-fwd".into())
            .spawn(move || forward_loop(rx, shared_t, base_t));
        let shared_q = Arc::clone(&shared);
        let base_q = base.clone();
        let _ = thread::Builder::new()
            .name("tb-hist-query".into())
            .spawn(move || query_loop(query_rx, shared_q, base_q));
        Self {
            tx,
            query_tx,
            shared,
        }
    }

    /// So query completion can wake egui when deep cache fills.
    pub fn bind_egui_ctx(&self, ctx: &Context) {
        if let Ok(mut g) = self.shared.lock() {
            g.egui_ctx = Some(ctx.clone());
        }
    }

    /// Non-blocking; drops when the queue is full or Pro is lagging.
    pub fn try_enqueue(&self, sample: FwdSample) {
        if sample.series_id.is_empty() || sample.payload.is_empty() {
            return;
        }
        match self.tx.try_send(sample) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_)) => {}
        }
    }

    pub fn sidecar_up(&self) -> bool {
        self.shared
            .lock()
            .map(|g| g.sidecar_up)
            .unwrap_or(false)
    }

    /// Series meta (range/count/persist); never blocks — schedules a fetch if stale.
    pub fn meta(&self, series_id: &str) -> Option<SeriesMeta> {
        let mut need_fetch = false;
        let cached = {
            let Ok(mut g) = self.shared.lock() else {
                return None;
            };
            let hit = g.meta_cache.get(series_id).and_then(|(at, m)| {
                if at.elapsed() < META_FRESH {
                    Some(m.clone())
                } else {
                    None
                }
            });
            if hit.is_none() && !g.pending_meta.contains(series_id) && probe_allowed(&g) {
                g.pending_meta.insert(series_id.to_string());
                need_fetch = true;
            }
            // Stale-but-present: still return for paint.
            hit.or_else(|| g.meta_cache.get(series_id).map(|(_, m)| m.clone()))
        };
        if need_fetch {
            let _ = self.query_tx.try_send(QueryCmd::Meta {
                series_id: series_id.to_string(),
            });
        }
        cached
    }

    /// Deep samples for scrub; never blocks paint. Returns cache (or empty) and
    /// schedules a background refresh when missing/stale.
    pub fn query_cached(
        &self,
        series_id: &str,
        from_ms: i64,
        to_ms: i64,
        limit: usize,
    ) -> Vec<DeepSample> {
        let mut need_fetch = false;
        let samples = {
            let Ok(mut g) = self.shared.lock() else {
                return Vec::new();
            };
            let fresh = g.deep_cache.get(series_id).and_then(|c| {
                if c.fetched_at.elapsed() < DEEP_FRESH
                    && c.from_ms <= from_ms
                    && c.to_ms >= to_ms
                {
                    Some(
                        c.samples
                            .iter()
                            .filter(|s| s.t_ms >= from_ms && s.t_ms <= to_ms)
                            .cloned()
                            .collect::<Vec<_>>(),
                    )
                } else {
                    None
                }
            });
            if fresh.is_none() && !g.pending_query.contains(series_id) && probe_allowed(&g) {
                g.pending_query.insert(series_id.to_string());
                need_fetch = true;
            }
            // Prefer fresh; else any overlapping cache for this paint.
            fresh.unwrap_or_else(|| {
                g.deep_cache
                    .get(series_id)
                    .map(|c| {
                        c.samples
                            .iter()
                            .filter(|s| s.t_ms >= from_ms && s.t_ms <= to_ms)
                            .cloned()
                            .collect()
                    })
                    .unwrap_or_default()
            })
        };
        if need_fetch {
            let _ = self.query_tx.try_send(QueryCmd::Query {
                series_id: series_id.to_string(),
                from_ms,
                to_ms,
                limit,
            });
        }
        samples
    }

    /// Persist flag — enqueued off-thread (never blocks UI).
    pub fn set_persist(&self, series_id: &str, persist: bool) -> Result<(), String> {
        if let Ok(mut g) = self.shared.lock() {
            g.meta_cache.remove(series_id);
        }
        self.query_tx
            .try_send(QueryCmd::Persist {
                series_id: series_id.to_string(),
                persist,
            })
            .map_err(|_| "history query worker busy".to_string())?;
        Ok(())
    }
}

fn probe_allowed(g: &FwdShared) -> bool {
    match g.next_probe_at {
        Some(at) if Instant::now() < at => false,
        _ => true,
    }
}

fn wake(shared: &Arc<Mutex<FwdShared>>) {
    if let Ok(g) = shared.lock() {
        if let Some(ctx) = g.egui_ctx.as_ref() {
            ctx.request_repaint();
        }
    }
}

fn mark_down(shared: &Arc<Mutex<FwdShared>>, err: String) {
    if let Ok(mut g) = shared.lock() {
        g.sidecar_up = false;
        g.last_error = Some(err);
        g.next_probe_at = Some(Instant::now() + QUERY_BACKOFF);
    }
}

fn mark_up(shared: &Arc<Mutex<FwdShared>>) {
    if let Ok(mut g) = shared.lock() {
        g.sidecar_up = true;
        g.last_error = None;
        g.next_probe_at = None;
    }
}

fn query_loop(rx: Receiver<QueryCmd>, shared: Arc<Mutex<FwdShared>>, base: String) {
    while let Ok(cmd) = rx.recv() {
        match cmd {
            QueryCmd::Query {
                series_id,
                from_ms,
                to_ms,
                limit,
            } => {
                let path = format!(
                    "/history/query?series={}&from_ms={from_ms}&to_ms={to_ms}&limit={limit}",
                    urlencoding_lite(&series_id)
                );
                match http_get(&base, &path) {
                    Ok(v) => {
                        let samples: Vec<DeepSample> = v
                            .get("samples")
                            .cloned()
                            .and_then(|x| serde_json::from_value(x).ok())
                            .unwrap_or_default();
                        if let Ok(mut g) = shared.lock() {
                            g.deep_cache.insert(
                                series_id.clone(),
                                CachedDeep {
                                    fetched_at: Instant::now(),
                                    from_ms,
                                    to_ms,
                                    samples,
                                },
                            );
                            g.pending_query.remove(&series_id);
                            g.sidecar_up = true;
                            g.last_error = None;
                            g.next_probe_at = None;
                        }
                        wake(&shared);
                    }
                    Err(e) => {
                        if let Ok(mut g) = shared.lock() {
                            g.pending_query.remove(&series_id);
                        }
                        mark_down(&shared, e);
                    }
                }
            }
            QueryCmd::Meta { series_id } => {
                let path = format!(
                    "/history/meta?series={}",
                    urlencoding_lite(&series_id)
                );
                match http_get(&base, &path) {
                    Ok(v) => {
                        if let Ok(m) = serde_json::from_value::<SeriesMeta>(v) {
                            if let Ok(mut g) = shared.lock() {
                                g.meta_cache
                                    .insert(series_id.clone(), (Instant::now(), m));
                                g.pending_meta.remove(&series_id);
                                g.sidecar_up = true;
                                g.last_error = None;
                                g.next_probe_at = None;
                            }
                            wake(&shared);
                        } else if let Ok(mut g) = shared.lock() {
                            g.pending_meta.remove(&series_id);
                        }
                    }
                    Err(e) => {
                        if let Ok(mut g) = shared.lock() {
                            g.pending_meta.remove(&series_id);
                        }
                        mark_down(&shared, e);
                    }
                }
            }
            QueryCmd::Persist { series_id, persist } => {
                let body = match serde_json::to_vec(&serde_json::json!({
                    "series_id": series_id,
                    "persist": persist,
                })) {
                    Ok(b) => b,
                    Err(_) => continue,
                };
                match http(&base, "POST", "/history/persist", Some(body)) {
                    Ok(_) => {
                        mark_up(&shared);
                        if let Ok(mut g) = shared.lock() {
                            g.meta_cache.remove(&series_id);
                        }
                    }
                    Err(e) => mark_down(&shared, e),
                }
            }
        }
    }
}

fn forward_loop(rx: Receiver<FwdSample>, shared: Arc<Mutex<FwdShared>>, base: String) {
    let mut backoff = Duration::from_millis(200);
    let max_backoff = Duration::from_secs(8);
    loop {
        let first = match rx.recv() {
            Ok(s) => s,
            Err(_) => return,
        };
        let mut batch = vec![first];
        let deadline = Instant::now() + BATCH_WAIT;
        while batch.len() < BATCH_MAX {
            let wait = deadline.saturating_duration_since(Instant::now());
            if wait.is_zero() {
                break;
            }
            match rx.recv_timeout(wait) {
                Ok(s) => batch.push(s),
                Err(mpsc::RecvTimeoutError::Timeout) => break,
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
        // Drain a bit more without waiting.
        while batch.len() < BATCH_MAX {
            match rx.try_recv() {
                Ok(s) => batch.push(s),
                Err(_) => break,
            }
        }

        match post_append(&base, &batch) {
            Ok(()) => {
                if let Ok(mut g) = shared.lock() {
                    g.sidecar_up = true;
                    g.last_error = None;
                    g.next_probe_at = None;
                }
                backoff = Duration::from_millis(200);
            }
            Err(e) => {
                if let Ok(mut g) = shared.lock() {
                    g.sidecar_up = false;
                    g.last_error = Some(e);
                    g.next_probe_at = Some(Instant::now() + backoff);
                }
                thread::sleep(backoff);
                backoff = (backoff * 2).min(max_backoff);
            }
        }
    }
}

fn post_append(base: &str, batch: &[FwdSample]) -> Result<(), String> {
    let token = read_token()?;
    let body = serde_json::to_vec(&serde_json::json!({ "samples": batch }))
        .map_err(|e| e.to_string())?;
    let url = format!("{base}/history/append");
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_millis(400))
        .timeout_read(Duration::from_secs(3))
        .build();
    match agent
        .post(&url)
        .set("Authorization", &format!("Bearer {token}"))
        .set("Content-Type", "application/json")
        .send_bytes(&body)
    {
        Ok(_) => Ok(()),
        Err(ureq::Error::Status(code, resp)) => {
            let text = resp.into_string().unwrap_or_default();
            Err(format!("HTTP {code}: {text}"))
        }
        Err(ureq::Error::Transport(t)) => Err(format!("transport: {t}")),
    }
}

fn http_get(base: &str, path: &str) -> Result<Value, String> {
    http(base, "GET", path, None)
}

fn http(
    base: &str,
    method: &str,
    path: &str,
    body: Option<Vec<u8>>,
) -> Result<Value, String> {
    let token = read_token()?;
    let url = format!("{base}{path}");
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_millis(300))
        .timeout_read(Duration::from_secs(2))
        .build();
    let req = match method {
        "GET" => agent.get(&url),
        "POST" => agent.post(&url),
        other => return Err(format!("unsupported method {other}")),
    }
    .set("Authorization", &format!("Bearer {token}"))
    .set("Content-Type", "application/json");
    let resp = match body {
        Some(b) => req.send_bytes(&b),
        None => req.call(),
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
            Err(format!("HTTP {code}: {text}"))
        }
        Err(ureq::Error::Transport(t)) => Err(format!("TopicBench Pro unreachable ({t})")),
    }
}

fn read_token() -> Result<String, String> {
    let path = token_path();
    let t = fs::read_to_string(&path)
        .map_err(|e| format!("cannot read {} ({e})", path.display()))?
        .trim()
        .to_string();
    if t.is_empty() {
        return Err("mcp-token empty".into());
    }
    Ok(t)
}

fn token_path() -> PathBuf {
    crate::mqtt::paths::config_file("mcp-token")
}

/// Minimal query escape for series ids (may contain `\0`).
fn urlencoding_lite(s: &str) -> String {
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

/// Wall-clock millis for an Instant (via age from now).
pub fn instant_to_t_ms(at: Instant) -> i64 {
    let age = Instant::now().saturating_duration_since(at);
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    now_ms.saturating_sub(age.as_millis() as i64)
}

/// Instant approximating a wall-clock millis (for merging Pro samples into plots).
pub fn t_ms_to_instant(t_ms: i64) -> Instant {
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    let age_ms = now_ms.saturating_sub(t_ms).max(0) as u64;
    Instant::now()
        .checked_sub(Duration::from_millis(age_ms))
        .unwrap_or_else(Instant::now)
}

/// Payload-panel history scrub (session-only; one topic at a time).
#[derive(Clone, Debug, Default)]
pub struct PayloadScrub {
    /// `(profile_id, topic)` currently scrubbing; cleared on topic switch → live.
    focus: Option<(String, String)>,
    /// Steps back from the live tip (`0` / None = live).
    steps_back: usize,
}

impl PayloadScrub {
    /// Ensure scrub state matches the selected topic (reset to live on switch).
    pub fn sync_topic(&mut self, profile_id: &str, topic: &str) {
        let key = (profile_id.to_string(), topic.to_string());
        if self.focus.as_ref() != Some(&key) {
            self.focus = Some(key);
            self.steps_back = 0;
        }
    }

    pub fn is_live(&self) -> bool {
        self.steps_back == 0
    }

    pub fn steps_back(&self) -> usize {
        self.steps_back
    }

    pub fn set_steps_back(&mut self, n: usize) {
        self.steps_back = n;
    }

    pub fn resume_live(&mut self) {
        self.steps_back = 0;
    }

    /// Index into `history` (oldest→newest) for the current scrub position.
    pub fn index_in(&self, len: usize) -> Option<usize> {
        if len == 0 {
            return None;
        }
        let tip = len - 1;
        let back = self.steps_back.min(tip);
        Some(tip - back)
    }
}
