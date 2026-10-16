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
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

pub use crate::mqtt::topic_tree::FLASH_MS;

/// Max coalesced topics applied on the UI thread per focused frame.
const MAX_TOPICS_APPLY_PER_FRAME: usize = 128;
/// While focused, wake egui at most this often from the inbox applier.
const FOCUSED_WAKE_MIN: Duration = Duration::from_millis(16);

/// Result of applying the background inbox on the UI thread.
pub struct PollOutcome {
    pub status: Option<String>,
    /// Messages were applied this drain (tree/traffic/pins updated).
    pub had_messages: bool,
    /// Inbox still has coalesced topics — need another focused frame.
    pub hit_cap: bool,
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
    pub message_count: u64,
    pub flash_until: Option<Instant>,
    pub traffic: VecDeque<TrafficLine>,
    pub topic_tree: TopicTree,
}

const TRAFFIC_CAP: usize = 80;

/// Latest payload for one topic (background inbox coalesces floods here).
struct CoalescedMsg {
    payload: String,
    retain: bool,
    count: u64,
    bytes: u64,
}

struct PendingState {
    state: SessionState,
    detail: String,
}

#[derive(Default)]
struct InboxData {
    status: Option<String>,
    states: HashMap<String, PendingState>,
    /// `(profile_id, topic)` → latest payload + aggregated counts/bytes.
    messages: HashMap<(String, String), CoalescedMsg>,
}

/// Worker-thread inbox: drains MQTT events off the UI thread so workspace switches
/// never leave the egui loop stuck in apply/present (Hyprland `xdg_wm_base` ANR).
struct SharedInbox {
    data: Mutex<InboxData>,
    dirty: AtomicBool,
}

impl Default for SharedInbox {
    fn default() -> Self {
        Self {
            data: Mutex::new(InboxData::default()),
            dirty: AtomicBool::new(false),
        }
    }
}

impl SharedInbox {
    fn fold_event(&self, ev: BrokerEvent) {
        let mut g = self.data.lock().unwrap_or_else(|e| e.into_inner());
        match ev {
            BrokerEvent::State {
                profile_id,
                state,
                detail,
            } => {
                g.status = Some(detail.clone());
                g.states.insert(profile_id, PendingState { state, detail });
            }
            BrokerEvent::Message {
                profile_id,
                topic,
                payload,
                retain,
                payload_bytes,
            } => {
                let entry = g
                    .messages
                    .entry((profile_id, topic))
                    .or_insert(CoalescedMsg {
                        payload: String::new(),
                        retain: false,
                        count: 0,
                        bytes: 0,
                    });
                entry.payload = payload;
                entry.retain = retain;
                entry.count = entry.count.saturating_add(1);
                entry.bytes = entry.bytes.saturating_add(payload_bytes as u64);
            }
            BrokerEvent::Notice { profile_id: _, detail } => {
                g.status = Some(detail);
            }
        }
        self.dirty.store(true, Ordering::Relaxed);
    }

    fn take_batch(&self, max_topics: usize) -> (InboxData, bool) {
        let mut g = self.data.lock().unwrap_or_else(|e| e.into_inner());
        let status = g.status.take();
        let states = std::mem::take(&mut g.states);
        let mut messages = HashMap::new();
        if g.messages.len() <= max_topics {
            messages = std::mem::take(&mut g.messages);
        } else {
            let keys: Vec<_> = g.messages.keys().take(max_topics).cloned().collect();
            for k in keys {
                if let Some(v) = g.messages.remove(&k) {
                    messages.insert(k, v);
                }
            }
        }
        let more = !g.messages.is_empty();
        if !more && g.states.is_empty() && g.status.is_none() {
            self.dirty.store(false, Ordering::Relaxed);
        }
        (
            InboxData {
                status,
                states,
                messages,
            },
            more,
        )
    }

    fn is_dirty(&self) -> bool {
        self.dirty.load(Ordering::Relaxed)
    }
}

pub struct SessionManager {
    pub open: BTreeMap<String, Session>,
    /// Process-lifetime MQTT payload byte counters (Stats ribbon).
    pub traffic_stats: TrafficStats,
    /// Shared with MQTT workers + inbox applier — gate egui wakeups.
    pub ui_focused: Arc<AtomicBool>,
    event_tx: Sender<BrokerEvent>,
    inbox: Arc<SharedInbox>,
    /// Filled from `App::new` so the applier can wake egui only while focused.
    egui_ctx: Arc<Mutex<Option<Context>>>,
}

impl Default for SessionManager {
    fn default() -> Self {
        let (event_tx, event_rx) = mpsc::channel();
        let inbox = Arc::new(SharedInbox::default());
        let ui_focused = Arc::new(AtomicBool::new(true));
        let egui_ctx = Arc::new(Mutex::new(None));
        spawn_inbox_applier(
            event_rx,
            inbox.clone(),
            ui_focused.clone(),
            egui_ctx.clone(),
        );
        Self {
            open: BTreeMap::new(),
            traffic_stats: TrafficStats::default(),
            ui_focused,
            event_tx,
            inbox,
            egui_ctx,
        }
    }
}

/// Drain broker events off the UI thread; coalesce; wake egui only when focused.
fn spawn_inbox_applier(
    event_rx: Receiver<BrokerEvent>,
    inbox: Arc<SharedInbox>,
    ui_focused: Arc<AtomicBool>,
    egui_ctx: Arc<Mutex<Option<Context>>>,
) {
    let _ = thread::Builder::new()
        .name("mqtt-inbox".into())
        .spawn(move || {
            let mut last_wake = Instant::now()
                .checked_sub(FOCUSED_WAKE_MIN)
                .unwrap_or_else(Instant::now);
            while let Ok(ev) = event_rx.recv() {
                inbox.fold_event(ev);
                // Unfocused / other workspace: never request_repaint — present/swap
                // must not block Hyprland xdg_wm_base ping → pong.
                if !ui_focused.load(Ordering::Relaxed) {
                    continue;
                }
                if last_wake.elapsed() < FOCUSED_WAKE_MIN {
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
        self.disconnect(&profile.id);

        let handle = live::spawn(
            profile,
            max_packet_bytes,
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
                detail: format!("Connecting to {}:{}…", profile.host, profile.port),
                subscriptions,
                client: Some(handle.client),
                stop: Some(handle.stop),
                user_stopped: false,
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

    /// Apply coalesced inbox updates produced by the background applier thread.
    pub fn poll_events(&mut self, pins: &mut PinBoard) -> PollOutcome {
        let now = Instant::now();
        let (batch, more) = self.inbox.take_batch(MAX_TOPICS_APPLY_PER_FRAME);
        let mut status = batch.status;
        let mut resub: Vec<(String, Vec<String>)> = Vec::new();

        for (profile_id, pending) in batch.states {
            let Some(session) = self.open.get_mut(&profile_id) else {
                continue;
            };
            // After intentional Disconnect, ignore reconnect chatter.
            if session.user_stopped {
                if pending.state == SessionState::Disconnected {
                    session.state = SessionState::Disconnected;
                    session.detail = "Disconnected".into();
                    status = Some(format!("{}: Disconnected", session.profile_name));
                }
                continue;
            }
            let became_connected =
                pending.state == SessionState::Connected && session.state != SessionState::Connected;
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

        let had_msgs = !batch.messages.is_empty();
        let flash = now + Duration::from_millis(FLASH_MS);
        for ((profile_id, topic), msg) in batch.messages {
            self.traffic_stats.record_recv(msg.bytes);
            let Some(session) = self.open.get_mut(&profile_id) else {
                continue;
            };
            if session.user_stopped {
                continue;
            }
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
            pins.on_message_n(&profile_id, &name, &topic, &msg.payload, now, msg.count);
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
            hit_cap: more || self.inbox.is_dirty(),
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
