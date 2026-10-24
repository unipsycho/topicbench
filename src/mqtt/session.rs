//! Multi-profile live sessions backed by rumqttc workers.

use crate::mqtt::live::{self, BrokerEvent};
use crate::mqtt::pins::PinBoard;
use crate::mqtt::profile::ConnectionProfile;
use crate::mqtt::subscribe;
use crate::mqtt::topic_tree::TopicTree;
use crate::mqtt::traffic_stats::TrafficStats;
use eframe::egui::Context;
use rumqttc::{Client, QoS};
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

pub use crate::mqtt::topic_tree::FLASH_MS;

/// While focused, wake egui at most this often from the inbox applier.
const FOCUSED_WAKE_MIN: Duration = Duration::from_millis(16);
/// Off-focus / off-workspace: keep Stats/counts alive without ANR present storms.
pub const BACKGROUND_UI_HZ_INTERVAL: Duration = Duration::from_secs(1);
/// Inbox applier flushes coalesced batches this often — independent of egui paint.
const INBOX_FLUSH_INTERVAL: Duration = Duration::from_millis(16);
/// Cap pending UI-apply batches if paint stalls (merge beyond this).
const PENDING_APPLY_CAP: usize = 90;
/// Per-topic timed samples retained while UI apply lags (≈10 min at 1 Hz).
const INBOX_SAMPLE_CAP: usize = 600;
/// Unfocused: floor sample spacing so Combined spans minutes under flood.
const INBOX_SAMPLE_GAP_BACKGROUND: Duration = Duration::from_millis(50);

/// One message applied to the topic tree this poll (for Pro MCP bridge fan-out).
#[derive(Clone, Debug)]
pub struct AppliedMessage {
    pub profile_id: String,
    pub profile_name: String,
    pub topic: String,
    pub payload: String,
    pub retain: bool,
}

/// Result of applying the background inbox on the UI thread.
pub struct PollOutcome {
    pub status: Option<String>,
    /// Messages were applied this drain (tree/traffic/pins updated).
    pub had_messages: bool,
    /// Inbox still has coalesced topics — need another focused frame.
    pub hit_cap: bool,
    /// Latest payload per topic applied this drain (coalesced — one entry per key).
    pub applied: Vec<AppliedMessage>,
}

impl PollOutcome {
    /// Focused UI should keep painting (flash / catch-up).
    pub fn need_repaint(&self) -> bool {
        self.hit_cap || self.had_messages
    }
}

/// Connection lifecycle for one profile.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionState {
    Disconnected,
    Connecting,
    Connected,
}

impl SessionState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Disconnected => "disconnected",
            Self::Connecting => "connecting…",
            Self::Connected => "connected",
        }
    }
}

#[derive(Clone, Debug)]
pub struct TrafficLine {
    pub topic: String,
    pub payload: String,
}

pub struct Session {
    pub profile_id: String,
    pub profile_name: String,
    pub state: SessionState,
    pub detail: String,
    pub subscriptions: Vec<String>,
    pub client: Option<Client>,
    /// Set on intentional Disconnect — worker must not reconnect.
    pub stop: Option<Arc<AtomicBool>>,
    pub user_stopped: bool,
    /// Bumped on each `connect()` — worker events must match to apply.
    pub generation: u64,
    pub message_count: u64,
    pub flash_until: Option<Instant>,
    pub traffic: VecDeque<TrafficLine>,
    pub topic_tree: TopicTree,
}

const TRAFFIC_CAP: usize = 80;

/// Latest payload for one topic (background inbox coalesces floods here).
struct CoalescedMsg {
    generation: u64,
    payload: String,
    retain: bool,
    count: u64,
    bytes: u64,
    /// Timed payloads retained for sticky/Combined history while UI apply lags.
    samples: VecDeque<(Instant, String)>,
}

struct PendingState {
    generation: u64,
    state: SessionState,
    detail: String,
}

#[derive(Default)]
struct InboxData {
    status: Option<String>,
    states: HashMap<String, PendingState>,
    /// `(profile_id, topic)` → latest payload + aggregated counts/bytes + samples.
    messages: HashMap<(String, String), CoalescedMsg>,
}

impl InboxData {
    fn is_empty(&self) -> bool {
        self.status.is_none() && self.states.is_empty() && self.messages.is_empty()
    }

    /// Merge another batch into self (used when pending-apply queue is capped).
    fn merge_from(&mut self, other: InboxData) {
        if other.status.is_some() {
            self.status = other.status;
        }
        for (id, pending) in other.states {
            let keep = self
                .states
                .get(&id)
                .map(|p| pending.generation >= p.generation)
                .unwrap_or(true);
            if keep {
                self.states.insert(id, pending);
            }
        }
        for (key, msg) in other.messages {
            match self.messages.get_mut(&key) {
                Some(entry) => {
                    if msg.generation < entry.generation {
                        continue;
                    }
                    if msg.generation != entry.generation {
                        entry.count = 0;
                        entry.bytes = 0;
                        entry.samples.clear();
                        entry.generation = msg.generation;
                    }
                    entry.payload = msg.payload;
                    entry.retain = msg.retain;
                    entry.count = entry.count.saturating_add(msg.count);
                    entry.bytes = entry.bytes.saturating_add(msg.bytes);
                    entry.samples.extend(msg.samples);
                    while entry.samples.len() > INBOX_SAMPLE_CAP {
                        entry.samples.pop_front();
                    }
                }
                None => {
                    self.messages.insert(key, msg);
                }
            }
        }
    }
}

/// Worker-thread inbox: drains MQTT events off the UI thread so workspace switches
/// never leave the egui loop stuck in apply/present (Hyprland `xdg_wm_base` ANR).
///
/// Ingest + sample rings run here at full rate; egui only paints (throttled when unfocused).
struct SharedInbox {
    data: Mutex<InboxData>,
    dirty: AtomicBool,
    /// Same flag workers / UI use — gates sample spacing, not ingest.
    ui_focused: Arc<AtomicBool>,
    /// Counted on every folded Message (never waits for egui apply).
    inbound_msgs: AtomicU64,
    inbound_bytes: AtomicU64,
}

impl SharedInbox {
    fn new(ui_focused: Arc<AtomicBool>) -> Self {
        Self {
            data: Mutex::new(InboxData::default()),
            dirty: AtomicBool::new(false),
            ui_focused,
            inbound_msgs: AtomicU64::new(0),
            inbound_bytes: AtomicU64::new(0),
        }
    }

    fn fold_event(&self, ev: BrokerEvent) {
        let mut g = self.data.lock().unwrap_or_else(|e| e.into_inner());
        match ev {
            BrokerEvent::State {
                profile_id,
                generation,
                state,
                detail,
            } => {
                // Newer (or same) generation wins; older reconnect chatter is dropped.
                let keep = g
                    .states
                    .get(&profile_id)
                    .map(|p| generation >= p.generation)
                    .unwrap_or(true);
                if keep {
                    g.status = Some(detail.clone());
                    g.states.insert(
                        profile_id,
                        PendingState {
                            generation,
                            state,
                            detail,
                        },
                    );
                }
            }
            BrokerEvent::Message {
                profile_id,
                generation,
                topic,
                payload,
                retain,
                payload_bytes,
            } => {
                let key = (profile_id, topic);
                if let Some(entry) = g.messages.get(&key) {
                    if generation < entry.generation {
                        return;
                    }
                }
                // Full ingest accounting — never gated on focus / paint.
                self.inbound_msgs.fetch_add(1, Ordering::Relaxed);
                self.inbound_bytes
                    .fetch_add(payload_bytes as u64, Ordering::Relaxed);

                let focused = self.ui_focused.load(Ordering::Relaxed);
                let now = Instant::now();
                let entry = g.messages.entry(key).or_insert(CoalescedMsg {
                    generation,
                    payload: String::new(),
                    retain: false,
                    count: 0,
                    bytes: 0,
                    samples: VecDeque::new(),
                });
                // Reset counts when a newer generation replaces a stale topic slot.
                if entry.generation != generation {
                    entry.count = 0;
                    entry.bytes = 0;
                    entry.samples.clear();
                    entry.generation = generation;
                }
                // History samples: live while focused; ≥20 Hz floor when unfocused so
                // Combined survives paint throttle without retaining every flood frame.
                let take_sample = focused
                    || entry
                        .samples
                        .back()
                        .map(|(t, _)| now.duration_since(*t) >= INBOX_SAMPLE_GAP_BACKGROUND)
                        .unwrap_or(true);
                if take_sample {
                    entry.samples.push_back((now, payload.clone()));
                    while entry.samples.len() > INBOX_SAMPLE_CAP {
                        entry.samples.pop_front();
                    }
                }
                entry.payload = payload;
                entry.retain = retain;
                entry.count = entry.count.saturating_add(1);
                entry.bytes = entry.bytes.saturating_add(payload_bytes as u64);
            }
            BrokerEvent::Notice {
                profile_id: _,
                generation: _,
                detail,
            } => {
                g.status = Some(detail);
            }
        }
        self.dirty.store(true, Ordering::Relaxed);
    }

    /// Drain every coalesced topic (no per-frame cap — that starved hot series at 1 Hz).
    fn take_all(&self) -> InboxData {
        let mut g = self.data.lock().unwrap_or_else(|e| e.into_inner());
        let out = InboxData {
            status: g.status.take(),
            states: std::mem::take(&mut g.states),
            messages: std::mem::take(&mut g.messages),
        };
        if g.status.is_none() && g.states.is_empty() && g.messages.is_empty() {
            self.dirty.store(false, Ordering::Relaxed);
        }
        out
    }

    fn is_dirty(&self) -> bool {
        self.dirty.load(Ordering::Relaxed)
    }

    fn inbound_totals(&self) -> (u64, u64) {
        (
            self.inbound_msgs.load(Ordering::Relaxed),
            self.inbound_bytes.load(Ordering::Relaxed),
        )
    }
}

pub struct SessionManager {
    pub open: BTreeMap<String, Session>,
    /// Process-lifetime MQTT payload byte counters (Stats ribbon).
    pub traffic_stats: TrafficStats,
    /// Shared with MQTT workers + inbox applier — gate egui wakeups.
    pub ui_focused: Arc<AtomicBool>,
    /// Hyprland (etc.): false while our window is off the active workspace.
    /// Cleared without waiting for egui `Focused(false)` so we stop wakes before present stalls.
    pub ui_visible: Arc<AtomicBool>,
    event_tx: Sender<BrokerEvent>,
    inbox: Arc<SharedInbox>,
    /// Coalesced batches flushed by the inbox thread (ingest ≠ paint).
    pending_apply: Arc<Mutex<VecDeque<InboxData>>>,
    /// Filled from `App::new` so the applier can wake egui only while focused.
    egui_ctx: Arc<Mutex<Option<Context>>>,
}

impl Default for SessionManager {
    fn default() -> Self {
        let (event_tx, event_rx) = mpsc::channel();
        let ui_focused = Arc::new(AtomicBool::new(true));
        let ui_visible = Arc::new(AtomicBool::new(true));
        let inbox = Arc::new(SharedInbox::new(ui_focused.clone()));
        let pending_apply = Arc::new(Mutex::new(VecDeque::new()));
        let egui_ctx = Arc::new(Mutex::new(None));
        spawn_inbox_applier(
            event_rx,
            inbox.clone(),
            pending_apply.clone(),
            ui_focused.clone(),
            egui_ctx.clone(),
        );
        crate::mqtt::visibility::spawn_hyprland_visibility_watcher(
            ui_visible.clone(),
            ui_focused.clone(),
        );
        Self {
            open: BTreeMap::new(),
            traffic_stats: TrafficStats::default(),
            ui_focused,
            ui_visible,
            event_tx,
            inbox,
            pending_apply,
            egui_ctx,
        }
    }
}

/// Drain broker events off the UI thread; coalesce; flush apply batches; wake egui (throttled).
fn spawn_inbox_applier(
    event_rx: Receiver<BrokerEvent>,
    inbox: Arc<SharedInbox>,
    pending_apply: Arc<Mutex<VecDeque<InboxData>>>,
    ui_focused: Arc<AtomicBool>,
    egui_ctx: Arc<Mutex<Option<Context>>>,
) {
    let _ = thread::Builder::new()
        .name("mqtt-inbox".into())
        .spawn(move || {
            let mut last_wake = Instant::now()
                .checked_sub(BACKGROUND_UI_HZ_INTERVAL)
                .unwrap_or_else(Instant::now);
            let mut last_flush = Instant::now()
                .checked_sub(INBOX_FLUSH_INTERVAL)
                .unwrap_or_else(Instant::now);
            while let Ok(ev) = event_rx.recv() {
                inbox.fold_event(ev);
                // Burst-drain the channel so fold keeps up with the broker.
                while let Ok(ev) = event_rx.try_recv() {
                    inbox.fold_event(ev);
                }
                // Flush coalesced topics on a fixed cadence — does not wait for egui paint.
                if last_flush.elapsed() >= INBOX_FLUSH_INTERVAL && inbox.is_dirty() {
                    let batch = inbox.take_all();
                    if !batch.is_empty() {
                        let mut q = pending_apply.lock().unwrap_or_else(|e| e.into_inner());
                        if q.len() >= PENDING_APPLY_CAP {
                            // Paint stalled: merge into the newest pending batch.
                            if let Some(back) = q.back_mut() {
                                back.merge_from(batch);
                            } else {
                                q.push_back(batch);
                            }
                        } else {
                            q.push_back(batch);
                        }
                    }
                    last_flush = Instant::now();
                }
                let focused = ui_focused.load(Ordering::Relaxed);
                let min_gap = if focused {
                    FOCUSED_WAKE_MIN
                } else {
                    // Background: one wake/sec so ribbon/Combined paint; not 60fps present.
                    BACKGROUND_UI_HZ_INTERVAL
                };
                if last_wake.elapsed() < min_gap {
                    continue;
                }
                let ctx = egui_ctx
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .clone();
                if let Some(ctx) = ctx {
                    last_wake = Instant::now();
                    ctx.request_repaint();
                }
            }
        });
}

impl SessionManager {
    /// Provide egui context so the inbox applier can wake the UI while focused.
    pub fn bind_egui_ctx(&self, ctx: &Context) {
        *self
            .egui_ctx
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = Some(ctx.clone());
    }

    pub fn connect(
        &mut self,
        profile: &ConnectionProfile,
        max_packet_bytes: usize,
        ctx: &Context,
    ) -> Result<(), String> {
        self.bind_egui_ctx(ctx);
        // Bump generation before stopping the old worker so late Disconnected events are stale.
        let generation = self
            .open
            .get(&profile.id)
            .map(|s| s.generation.wrapping_add(1))
            .unwrap_or(1);
        self.disconnect(&profile.id);

        let handle = live::spawn(
            profile,
            max_packet_bytes,
            generation,
            self.event_tx.clone(),
            ctx.clone(),
            self.ui_focused.clone(),
        )?;
        let mut subscriptions = if profile.subscriptions.is_empty() {
            self.open
                .get(&profile.id)
                .map(|s| s.subscriptions.clone())
                .unwrap_or_default()
        } else {
            profile.subscriptions.clone()
        };
        // Avoid broker double-delivery from overlapping filters (e.g. `#` + `sensors/#`).
        subscribe::prune_redundant(&mut subscriptions);

        self.open.insert(
            profile.id.clone(),
            Session {
                profile_id: profile.id.clone(),
                profile_name: profile.name.clone(),
                state: SessionState::Connecting,
                detail: format!("Connecting to {}…", profile.endpoint_label()),
                subscriptions,
                client: Some(handle.client),
                stop: Some(handle.stop),
                user_stopped: false,
                generation,
                message_count: 0,
                flash_until: None,
                traffic: VecDeque::new(),
                topic_tree: TopicTree::default(),
            },
        );
        Ok(())
    }

    pub fn apply_profile_subscriptions(
        &mut self,
        profile_id: &str,
        filters: &[String],
    ) -> Result<(), String> {
        let session = self
            .open
            .get_mut(profile_id)
            .ok_or_else(|| "No open session".to_string())?;
        let old = session.subscriptions.clone();
        let mut filters = filters.to_vec();
        subscribe::prune_redundant(&mut filters);
        session.subscriptions = filters.clone();
        if session.state != SessionState::Connected {
            return Ok(());
        }
        let client = session
            .client
            .as_ref()
            .ok_or_else(|| "No MQTT client".to_string())?;
        for f in &old {
            if !filters.iter().any(|n| n == f) {
                let _ = client.unsubscribe(f);
            }
        }
        for f in &filters {
            if !old.iter().any(|o| o == f) {
                client
                    .subscribe(f, QoS::AtLeastOnce)
                    .map_err(|e| format!("subscribe: {e}"))?;
            }
        }
        Ok(())
    }

    /// User Disconnect: stop worker reconnect loop; stay Disconnected until Connect.
    /// Clears that broker’s topic tree / traffic so it leaves the Topic tree panel.
    pub fn disconnect(&mut self, profile_id: &str) {
        if let Some(session) = self.open.get_mut(profile_id) {
            session.user_stopped = true;
            if let Some(stop) = session.stop.take() {
                stop.store(true, Ordering::SeqCst);
            }
            if let Some(client) = session.client.take() {
                let _ = client.disconnect();
            }
            session.state = SessionState::Disconnected;
            session.detail = "Disconnected".into();
            // Drop tree nodes + live traffic — disconnected brokers must leave the tree.
            session.topic_tree = TopicTree::default();
            session.traffic.clear();
            session.message_count = 0;
            session.flash_until = None;
        }
    }

    pub fn toggle(
        &mut self,
        profile: &ConnectionProfile,
        max_packet_bytes: usize,
        ctx: &Context,
        pins: &mut PinBoard,
    ) -> Result<String, String> {
        let id = profile.id.as_str();
        let should_connect = match self.open.get(id) {
            None => true,
            Some(s) => s.state == SessionState::Disconnected || s.client.is_none() || s.user_stopped,
        };
        if should_connect {
            self.connect(profile, max_packet_bytes, ctx)?;
            Ok(format!("Connecting {}", profile.name))
        } else {
            self.disconnect(id);
            // Payload/selection follow must not keep a dead broker focus.
            pins.clear_tree_focus_for(id);
            Ok(format!("Disconnected {}", profile.name))
        }
    }

    pub fn publish_live(
        &mut self,
        profile_id: &str,
        topic: &str,
        payload: &str,
        qos: crate::mqtt::publish::Qos,
        retain: bool,
    ) -> Result<(), String> {
        let bytes = payload.as_bytes();
        let n = bytes.len() as u64;
        {
            let session = self
                .open
                .get(profile_id)
                .ok_or_else(|| "No open session".to_string())?;
            if session.state != SessionState::Connected {
                return Err("Profile is not connected".into());
            }
            let client = session
                .client
                .as_ref()
                .ok_or_else(|| "No MQTT client".to_string())?;
            client
                .publish(topic, live::to_qos(qos), retain, bytes)
                .map_err(|e| format!("publish: {e}"))?;
        }
        // Count after a successful enqueue (session borrow released).
        self.traffic_stats.record_sent(n);
        Ok(())
    }

    /// Apply coalesced inbox updates flushed by the background applier thread.
    pub fn poll_events(&mut self, pins: &mut PinBoard) -> PollOutcome {
        let now = Instant::now();
        // Ingest totals are counted at fold time — sync before EMA/ribbon read.
        let (msgs, bytes) = self.inbox.inbound_totals();
        self.traffic_stats.sync_inbound(msgs, bytes);

        // Drain every flushed batch (and any leftover live inbox).
        let mut batches: Vec<InboxData> = {
            let mut q = self
                .pending_apply
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            q.drain(..).collect()
        };
        let live = self.inbox.take_all();
        if !live.is_empty() {
            batches.push(live);
        }

        let mut status: Option<String> = None;
        let mut resub: Vec<(String, Vec<String>)> = Vec::new();
        let mut had_msgs = false;
        let mut applied: Vec<AppliedMessage> = Vec::new();

        for batch in batches {
            if batch.status.is_some() {
                status = batch.status;
            }
            for (profile_id, pending) in batch.states {
                let Some(session) = self.open.get_mut(&profile_id) else {
                    continue;
                };
                // Stale worker from a prior reconnect — never apply.
                if pending.generation != session.generation {
                    continue;
                }
                // After intentional Disconnect, ignore reconnect chatter.
                if session.user_stopped {
                    if pending.state == SessionState::Disconnected {
                        session.state = SessionState::Disconnected;
                        session.detail = "Disconnected".into();
                        status = Some(format!("{}: Disconnected", session.profile_name));
                    }
                    continue;
                }
                let became_connected = pending.state == SessionState::Connected
                    && session.state != SessionState::Connected;
                let changed = session.state != pending.state || session.detail != pending.detail;
                session.state = pending.state;
                session.detail = pending.detail.clone();
                if became_connected {
                    resub.push((profile_id.clone(), session.subscriptions.clone()));
                }
                if changed {
                    status = Some(format!("{}: {}", session.profile_name, pending.detail));
                }
            }

            let flash = now + Duration::from_millis(FLASH_MS);
            for ((profile_id, topic), msg) in batch.messages {
                let Some(session) = self.open.get_mut(&profile_id) else {
                    continue;
                };
                if msg.generation != session.generation || session.user_stopped {
                    continue;
                }
                had_msgs = true;
                // Traffic bytes/msgs already counted in fold atomics — do not double-add.
                session.message_count = session.message_count.saturating_add(msg.count);
                session.flash_until = Some(flash);
                session
                    .topic_tree
                    .observe_n(&topic, &msg.payload, msg.retain, now, msg.count);
                session.traffic.push_front(TrafficLine {
                    topic: topic.clone(),
                    payload: msg.payload.clone(),
                });
                while session.traffic.len() > TRAFFIC_CAP {
                    session.traffic.pop_back();
                }
                let name = session.profile_name.clone();
                // Fan-out latest payload for Pro MCP bridge (one entry per coalesced key).
                applied.push(AppliedMessage {
                    profile_id: profile_id.clone(),
                    profile_name: name.clone(),
                    topic: topic.clone(),
                    payload: msg.payload.clone(),
                    retain: msg.retain,
                });
                // Replay timed samples into sticky/Combined + Payload history rings.
                let samples: Vec<(Instant, String)> = if msg.samples.is_empty() {
                    vec![(now, msg.payload)]
                } else {
                    msg.samples.into_iter().collect()
                };
                session
                    .topic_tree
                    .root
                    .push_payload_samples(&topic, &samples);
                // Pro deep-store forward (full payloads); drained on the UI thread.
                pins.enqueue_topic_hist(&profile_id, &topic, &samples);
                pins.on_message_samples(&profile_id, &name, &topic, &samples, msg.count);
            }
        }

        for (id, filters) in resub {
            if let Some(session) = self.open.get(&id) {
                if session.user_stopped {
                    continue;
                }
                if let Some(client) = session.client.as_ref() {
                    for filter in &filters {
                        let _ = client.subscribe(filter, QoS::AtLeastOnce);
                    }
                }
            }
        }

        PollOutcome {
            status,
            had_messages: had_msgs,
            // More work only if inbox received traffic since this drain.
            hit_cap: self.inbox.is_dirty()
                || !self
                    .pending_apply
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .is_empty(),
            applied,
        }
    }

    pub fn sync_profile_name(&mut self, profile_id: &str, profile_name: &str) {
        if let Some(session) = self.open.get_mut(profile_id) {
            session.profile_name = profile_name.to_string();
        }
    }

    pub fn close(&mut self, profile_id: &str) {
        self.disconnect(profile_id);
        self.open.remove(profile_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mqtt::pins::PinBoard;
    use crate::mqtt::profile::ConnectionProfile;
    use crate::mqtt::publish::Qos;
    use crate::mqtt::settings::MAX_PACKET_DEFAULT;
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    /// Public Mosquitto sandbox — anonymous MQTT on 1883.
    const MOSQUITTO_HOST: &str = "test.mosquitto.org";
    const MOSQUITTO_PORT: u16 = 1883;

    fn unique_id(prefix: &str) -> String {
        let ns = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        format!("{prefix}-{ns}")
    }

    fn mosquitto_profile(id: &str, subscriptions: Vec<String>) -> ConnectionProfile {
        let mut p = ConnectionProfile::new_blank(id.to_string());
        p.name = "Mosquitto session".into();
        p.host = MOSQUITTO_HOST.into();
        p.port = MOSQUITTO_PORT;
        p.client_id = unique_id("mqtt-master-sess");
        p.subscriptions = subscriptions;
        p
    }

    /// Poll inbox until `pred` or timeout.
    fn wait_until(
        sessions: &mut SessionManager,
        pins: &mut PinBoard,
        timeout: Duration,
        mut pred: impl FnMut(&SessionManager) -> bool,
    ) -> bool {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            sessions.poll_events(pins);
            if pred(sessions) {
                return true;
            }
            thread::sleep(Duration::from_millis(50));
        }
        false
    }

    #[test]
    fn connect_subscribe_hash_publish_roundtrip() {
        let ctx = Context::default();
        let mut sessions = SessionManager::default();
        sessions.bind_egui_ctx(&ctx);
        let mut pins = PinBoard::default();

        // Root `#` — public traffic piles in; we still publish our own topic for a firm assert.
        let profile = mosquitto_profile("sess-hash", vec!["#".into()]);
        sessions
            .connect(&profile, MAX_PACKET_DEFAULT, &ctx)
            .expect("connect");

        assert!(
            wait_until(&mut sessions, &mut pins, Duration::from_secs(20), |s| {
                s.open
                    .get(&profile.id)
                    .map(|sess| sess.state == SessionState::Connected)
                    .unwrap_or(false)
            }),
            "expected Connected via SessionManager"
        );

        let topic = format!("mqtt-master/session/{}", unique_id("rt"));
        let payload = format!("{{\"n\":{}}}", unique_id("v"));
        sessions
            .publish_live(&profile.id, &topic, &payload, Qos::AtLeastOnce, false)
            .expect("publish");

        let saw = wait_until(&mut sessions, &mut pins, Duration::from_secs(15), |s| {
            s.open
                .get(&profile.id)
                .map(|sess| {
                    sess.traffic
                        .iter()
                        .any(|line| line.topic == topic && line.payload == payload)
                        || sess.message_count > 0
                })
                .unwrap_or(false)
        });
        assert!(saw, "expected roundtrip or `#` traffic in session inbox");

        let count_before = sessions
            .open
            .get(&profile.id)
            .map(|s| s.message_count)
            .unwrap_or(0);
        assert!(count_before > 0, "message_count should advance under `#`");

        sessions.disconnect(&profile.id);
        assert!(
            wait_until(&mut sessions, &mut pins, Duration::from_secs(10), |s| {
                s.open
                    .get(&profile.id)
                    .map(|sess| sess.state == SessionState::Disconnected && sess.user_stopped)
                    .unwrap_or(false)
            }),
            "expected user-stopped Disconnected"
        );

        // Stay disconnected — no reconnect chatter applied after intentional stop.
        thread::sleep(Duration::from_secs(2));
        sessions.poll_events(&mut pins);
        let sess = sessions.open.get(&profile.id).expect("session retained");
        assert_eq!(sess.state, SessionState::Disconnected);
        assert!(sess.user_stopped);
        // disconnect() clears tree + traffic counters for the stopped session.
        assert_eq!(sess.message_count, 0);
        assert!(sess.traffic.is_empty());
    }

    #[test]
    fn toggle_connect_then_disconnect() {
        let ctx = Context::default();
        let mut sessions = SessionManager::default();
        sessions.bind_egui_ctx(&ctx);
        let mut pins = PinBoard::default();

        let profile = mosquitto_profile("sess-toggle", vec!["mqtt-master/toggle/#".into()]);
        let msg = sessions
            .toggle(&profile, MAX_PACKET_DEFAULT, &ctx, &mut pins)
            .expect("toggle connect");
        assert!(msg.contains("Connecting"));

        assert!(
            wait_until(&mut sessions, &mut pins, Duration::from_secs(20), |s| {
                s.open
                    .get(&profile.id)
                    .map(|sess| sess.state == SessionState::Connected)
                    .unwrap_or(false)
            }),
            "toggle should reach Connected"
        );

        let msg = sessions
            .toggle(&profile, MAX_PACKET_DEFAULT, &ctx, &mut pins)
            .expect("toggle disconnect");
        assert!(msg.contains("Disconnected"));
        assert_eq!(
            sessions.open.get(&profile.id).map(|s| s.state),
            Some(SessionState::Disconnected)
        );
    }
}
