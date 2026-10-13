//! Shared pin board: pinned topics from all open profiles, one place to compare.

use std::collections::BTreeMap;

/// A pinned topic tied to a profile/session for side-by-side monitoring.
#[derive(Clone, Debug)]
pub struct PinnedTopic {
    pub profile_id: String,
    pub profile_name: String,
    pub topic: String,
    /// Last payload placeholder — filled when MQTT I/O is wired.
    pub last_payload: String,
}

/// Pins from every open profile live here (shared board).
#[derive(Default)]
pub struct PinBoard {
    /// Key: `profile_id\0topic` keeps pins unique per broker+topic.
    pub items: BTreeMap<String, PinnedTopic>,
}

impl PinBoard {
    fn key(profile_id: &str, topic: &str) -> String {
        format!("{profile_id}\0{topic}")
    }

    pub fn pin(&mut self, profile_id: &str, profile_name: &str, topic: &str) {
        let topic = topic.trim();
        if topic.is_empty() {
            return;
        }
        let key = Self::key(profile_id, topic);
        self.items.entry(key).or_insert_with(|| PinnedTopic {
            profile_id: profile_id.to_string(),
            profile_name: profile_name.to_string(),
            topic: topic.to_string(),
            last_payload: "(no messages yet)".into(),
        });
    }

    pub fn unpin(&mut self, profile_id: &str, topic: &str) {
        self.items.remove(&Self::key(profile_id, topic));
    }

    pub fn clear(&mut self) {
        self.items.clear();
    }
}
