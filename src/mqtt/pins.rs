//! Shared sticky topics: live payloads, history, Raw/Value/Graph modes.

use crate::mqtt::profile::ConnectionProfile;
use crate::mqtt::topic_tree::FLASH_MS;
use std::collections::{BTreeMap, VecDeque};
use std::time::{Duration, Instant};

pub use crate::mqtt::profile::StickyViewMode;

const HISTORY_CAP: usize = 120;

/// Detected payload kind for Value mode.
#[derive(Clone, Debug)]
pub enum DetectedValue {
    Bool(bool),
    Number(f64),
    Text(String),
}

impl DetectedValue {
    pub fn from_payload(payload: &str) -> Self {
        let t = payload.trim();
        if t.eq_ignore_ascii_case("true") || t == "1" || t.eq_ignore_ascii_case("on") {
            return Self::Bool(true);
        }
        if t.eq_ignore_ascii_case("false") || t == "0" || t.eq_ignore_ascii_case("off") {
            return Self::Bool(false);
        }
        if let Ok(n) = t.parse::<f64>() {
            return Self::Number(n);
        }
        Self::Text(payload.to_string())
    }
}

#[derive(Clone, Debug)]
pub struct HistorySample {
    #[allow(dead_code)]
    pub at: Instant,
    #[allow(dead_code)]
    pub payload: String,
    pub numeric: Option<f64>,
}

/// A sticky/pinned topic tied to a profile for side-by-side monitoring.
#[derive(Clone, Debug)]
pub struct StickyTopic {
    pub profile_id: String,
    pub profile_name: String,
    pub topic: String,
    pub last_payload: String,
    pub message_count: u64,
    pub flash_until: Option<Instant>,
    pub view_mode: StickyViewMode,
    pub history: VecDeque<HistorySample>,
}

/// Stickies from every open profile live here (shared board at top).
#[derive(Default)]
pub struct PinBoard {
    /// Key: `profile_id\0topic`.
    pub items: BTreeMap<String, StickyTopic>,
    /// Selected tree topic: `(profile_id, profile_name, topic path)`.
    /// Set on row click; used by Ctrl+Shift+S and the Payload panel.
    pub last_tree_focus: Option<(String, String, String)>,
}

impl PinBoard {
    pub fn key(profile_id: &str, topic: &str) -> String {
        format!("{profile_id}\0{topic}")
    }

    /// Drop Payload/selection follow when that broker disconnects.
    pub fn clear_tree_focus_for(&mut self, profile_id: &str) {
        if self
            .last_tree_focus
            .as_ref()
            .is_some_and(|(id, _, _)| id == profile_id)
        {
            self.last_tree_focus = None;
        }
    }

    /// Pin a topic if missing (keeps existing view mode).
    pub fn pin(&mut self, profile_id: &str, profile_name: &str, topic: &str) {
        let topic = topic.trim();
        if topic.is_empty() {
            return;
        }
        let key = Self::key(profile_id, topic);
        self.items.entry(key).or_insert_with(|| StickyTopic {
            profile_id: profile_id.to_string(),
            profile_name: profile_name.to_string(),
            topic: topic.to_string(),
            last_payload: "(waiting…)".into(),
            message_count: 0,
            flash_until: None,
            view_mode: StickyViewMode::Raw,
            history: VecDeque::new(),
        });
    }

    /// Pin or update view mode (used when restoring from profile).
    pub fn pin_with_mode(
        &mut self,
        profile_id: &str,
        profile_name: &str,
        topic: &str,
        view_mode: StickyViewMode,
    ) {
        let topic = topic.trim();
        if topic.is_empty() {
            return;
        }
        let key = Self::key(profile_id, topic);
        match self.items.entry(key) {
            std::collections::btree_map::Entry::Occupied(mut e) => {
                e.get_mut().view_mode = view_mode;
                e.get_mut().profile_name = profile_name.to_string();
            }
            std::collections::btree_map::Entry::Vacant(v) => {
                v.insert(StickyTopic {
                    profile_id: profile_id.to_string(),
                    profile_name: profile_name.to_string(),
                    topic: topic.to_string(),
                    last_payload: "(waiting…)".into(),
                    message_count: 0,
                    flash_until: None,
                    view_mode,
                    history: VecDeque::new(),
                });
            }
        }
    }

    pub fn unpin(&mut self, profile_id: &str, topic: &str) {
        self.items.remove(&Self::key(profile_id, topic));
    }

    pub fn is_sticky(&self, profile_id: &str, topic: &str) -> bool {
        self.items.contains_key(&Self::key(profile_id, topic))
    }

    /// Restore stickies saved on a profile (idempotent; reapplies view mode).
    pub fn restore_from_profile(&mut self, profile: &ConnectionProfile) {
        for sticky in &profile.stickies {
            self.pin_with_mode(
                &profile.id,
                &profile.name,
                &sticky.topic,
                sticky.view_mode,
            );
        }
    }

    pub fn on_message(
        &mut self,
        profile_id: &str,
        profile_name: &str,
        topic: &str,
        payload: &str,
        now: Instant,
    ) {
        let flash = now + Duration::from_millis(FLASH_MS);
        for sticky in self.items.values_mut() {
            if sticky.profile_id != profile_id {
                continue;
            }
            let (base, path) = crate::mqtt::value_path::split_sticky_id(&sticky.topic);
            let matches = base == topic
                || crate::mqtt::subscribe::topic_matches_filter(topic, base);
            if !matches {
                continue;
            }
            // Field-path stickies store the extracted scalar so Graph can plot it.
            let extracted = match path {
                Some(p) => crate::mqtt::value_path::extract_path_value(payload, p)
                    .unwrap_or_else(|| payload.to_string()),
                None => payload.to_string(),
            };
            let numeric = crate::mqtt::value_path::sample_numeric(&extracted);
            sticky.profile_name = profile_name.to_string();
            sticky.last_payload = extracted.clone();
            sticky.message_count = sticky.message_count.saturating_add(1);
            sticky.flash_until = Some(flash);
            sticky.history.push_back(HistorySample {
                at: now,
                payload: extracted,
                numeric,
            });
            while sticky.history.len() > HISTORY_CAP {
                sticky.history.pop_front();
            }
        }
    }

    /// Pin (or switch) a topic/field sticky into Graph view and seed current sample.
    pub fn pin_graph(
        &mut self,
        profile_id: &str,
        profile_name: &str,
        sticky_topic: &str,
        current_value: &str,
        now: Instant,
    ) {
        self.pin_with_mode(
            profile_id,
            profile_name,
            sticky_topic,
            StickyViewMode::Graph,
        );
        let key = Self::key(profile_id, sticky_topic);
        if let Some(sticky) = self.items.get_mut(&key) {
            let numeric = crate::mqtt::value_path::sample_numeric(current_value);
            sticky.last_payload = current_value.to_string();
            if sticky.history.is_empty() {
                sticky.history.push_back(HistorySample {
                    at: now,
                    payload: current_value.to_string(),
                    numeric,
                });
                sticky.message_count = sticky.message_count.max(1);
            }
        }
    }
}
