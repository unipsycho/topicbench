//! Multi-profile live sessions backed by rumqttc workers.

use crate::mqtt::live::{self, BrokerEvent};
use crate::mqtt::pins::PinBoard;
use crate::mqtt::profile::ConnectionProfile;
use crate::mqtt::subscribe;
use crate::mqtt::topic_tree::TopicTree;
use crate::mqtt::traffic_stats::TrafficStats;
use eframe::egui::Context;
use rumqttc::{Client, QoS};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub use crate::mqtt::topic_tree::FLASH_MS;

/// Max broker events applied on the UI thread per egui frame.
/// Unbounded drain + tree/traffic/pin work freezes the window under `#` floods.
const MAX_EVENTS_PER_FRAME: usize = 96;

/// Result of draining the broker event channel on the UI thread.
pub struct PollOutcome {
    pub status: Option<String>,
    /// Messages were applied this drain (tree/traffic/pins updated).
    pub had_messages: bool,
    /// Hit [`MAX_EVENTS_PER_FRAME`] — channel may still have events.
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

/// Latest payload for one topic within a single UI-frame batch.
struct CoalescedMsg {
    payload: String,
    retain: bool,
    count: u64,
}

pub struct SessionManager {
    pub open: BTreeMap<String, Session>,
    /// Process-lifetime MQTT payload byte counters (Stats ribbon).
    pub traffic_stats: TrafficStats,
    /// Shared with MQTT workers — slows `request_repaint` while on another workspace.
    pub ui_focused: Arc<AtomicBool>,
    event_tx: Sender<BrokerEvent>,
    event_rx: Receiver<BrokerEvent>,
}

impl Default for SessionManager {
    fn default() -> Self {
        let (event_tx, event_rx) = mpsc::channel();
        Self {
            open: BTreeMap::new(),
            traffic_stats: TrafficStats::default(),
            ui_focused: Arc::new(AtomicBool::new(true)),
            event_tx,
            event_rx,
        }
    }
}

impl SessionManager {
    pub fn connect(
        &mut self,
        profile: &ConnectionProfile,
        max_packet_bytes: usize,
        ctx: &Context,
    ) -> Result<(), String> {
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

    pub fn poll_events(&mut self, pins: &mut PinBoard) -> PollOutcome {
        let now = Instant::now();
        let mut status = None;
        let mut resub: Vec<(String, Vec<String>)> = Vec::new();
        // Same publish delivered once per overlapping subscription — count once.
        let mut seen_msgs: HashSet<(String, String, String)> = HashSet::new();
        // Latest display state per topic this frame; Stats still count every unique delivery.
        let mut coalesced: HashMap<(String, String), CoalescedMsg> = HashMap::new();
        let mut drained = 0usize;

        while drained < MAX_EVENTS_PER_FRAME {
            let Ok(ev) = self.event_rx.try_recv() else {
                break;
            };
            drained += 1;
            match ev {
                BrokerEvent::State {
                    profile_id,
                    state,
                    detail,
                } => {
                    if let Some(session) = self.open.get_mut(&profile_id) {
                        // After intentional Disconnect, ignore reconnect chatter.
                        if session.user_stopped {
                            if state == SessionState::Disconnected {
                                session.state = SessionState::Disconnected;
                                session.detail = "Disconnected".into();
                                status = Some(format!("{}: Disconnected", session.profile_name));
                            }
                            continue;
                        }

                        let became_connected =
                            state == SessionState::Connected && session.state != SessionState::Connected;
                        let changed = session.state != state || session.detail != detail;
                        session.state = state;
                        session.detail = detail.clone();
                        if became_connected {
                            resub.push((profile_id.clone(), session.subscriptions.clone()));
                        }
                        if changed {
                            status = Some(format!("{}: {detail}", session.profile_name));
                        }
                    }
                }
                BrokerEvent::Message {
                    profile_id,
                    topic,
                    payload,
                    retain,
                    payload_bytes,
                } => {
                    if !seen_msgs.insert((profile_id.clone(), topic.clone(), payload.clone())) {
                        continue;
                    }
                    if self
                        .open
                        .get(&profile_id)
                        .map(|s| s.user_stopped)
                        .unwrap_or(true)
                    {
                        continue;
                    }
                    // Deduped once — raw payload size (process-lifetime Stats).
                    self.traffic_stats.record_recv(payload_bytes as u64);
                    let entry = coalesced.entry((profile_id, topic)).or_insert(CoalescedMsg {
                        payload: String::new(),
                        retain: false,
                        count: 0,
                    });
                    entry.payload = payload;
                    entry.retain = retain;
                    entry.count = entry.count.saturating_add(1);
                }
                BrokerEvent::Notice { profile_id, detail } => {
                    if let Some(session) = self.open.get(&profile_id) {
                        if !session.user_stopped {
                            status = Some(format!("{}: {detail}", session.profile_name));
                        }
                    }
                }
            }
        }

        // One tree/traffic/pin apply per topic this frame (latest payload).
        let had_msgs = !coalesced.is_empty();
        let flash = now + Duration::from_millis(FLASH_MS);
        for ((profile_id, topic), msg) in coalesced {
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

        // Cap hit ⇒ backlog remains; keep frames flowing without draining forever.
        let hit_cap = drained >= MAX_EVENTS_PER_FRAME;
        PollOutcome {
            status,
            had_messages: had_msgs,
            hit_cap,
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
