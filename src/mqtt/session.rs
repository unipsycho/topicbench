//! Multi-profile live sessions backed by rumqttc workers.

use crate::mqtt::live::{self, BrokerEvent};
use crate::mqtt::pins::PinBoard;
use crate::mqtt::profile::ConnectionProfile;
use crate::mqtt::subscribe;
use crate::mqtt::topic_tree::TopicTree;
use eframe::egui::Context;
use rumqttc::{Client, QoS};
use std::collections::{BTreeMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub use crate::mqtt::topic_tree::FLASH_MS;

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

pub struct SessionManager {
    pub open: BTreeMap<String, Session>,
    event_tx: Sender<BrokerEvent>,
    event_rx: Receiver<BrokerEvent>,
}

impl Default for SessionManager {
    fn default() -> Self {
        let (event_tx, event_rx) = mpsc::channel();
        Self {
            open: BTreeMap::new(),
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
        }
    }

    pub fn toggle(
        &mut self,
        profile: &ConnectionProfile,
        max_packet_bytes: usize,
        ctx: &Context,
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
            Ok(format!("Disconnected {}", profile.name))
        }
    }

    pub fn publish_live(
        &self,
        profile_id: &str,
        topic: &str,
        payload: &str,
        qos: crate::mqtt::publish::Qos,
        retain: bool,
    ) -> Result<(), String> {
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
            .publish(topic, live::to_qos(qos), retain, payload.as_bytes())
            .map_err(|e| format!("publish: {e}"))
    }

    pub fn poll_events(&mut self, pins: &mut PinBoard) -> Option<String> {
        let now = Instant::now();
        let mut status = None;
        let mut resub: Vec<(String, Vec<String>)> = Vec::new();
        // Same publish delivered once per overlapping subscription — count once.
        let mut seen_msgs: HashSet<(String, String, String)> = HashSet::new();

        while let Ok(ev) = self.event_rx.try_recv() {
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
                } => {
                    if !seen_msgs.insert((profile_id.clone(), topic.clone(), payload.clone())) {
                        continue;
                    }
                    if let Some(session) = self.open.get_mut(&profile_id) {
                        if session.user_stopped {
                            continue;
                        }
                        session.message_count = session.message_count.saturating_add(1);
                        session.flash_until = Some(now + Duration::from_millis(FLASH_MS));
                        session.topic_tree.observe(&topic, &payload, now);
                        session.traffic.push_front(TrafficLine {
                            topic: topic.clone(),
                            payload: payload.clone(),
                        });
                        while session.traffic.len() > TRAFFIC_CAP {
                            session.traffic.pop_back();
                        }
                        let name = session.profile_name.clone();
                        pins.on_message(&profile_id, &name, &topic, &payload, now);
                    }
                }
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

        status
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
