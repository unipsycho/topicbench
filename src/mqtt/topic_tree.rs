//! Browsable topic hierarchy built from live subscribed traffic.

use crate::mqtt::publish::MqttMsgProps;
use std::collections::{BTreeMap, VecDeque};
use std::time::{Duration, Instant};

/// Update-flash duration: full-on border, then fade out (resets on each new value).
pub const FLASH_MS: u64 = 500;

/// Explorer-style per-leaf ring: drop oldest when either bound is exceeded.
const PAYLOAD_HISTORY_CAP: usize = 100;
/// Total payload UTF-8 bytes retained per leaf (MQTT Explorer ≈20 KB).
const PAYLOAD_HISTORY_BYTES: usize = 20 * 1024;

/// Trim oldest samples until under count and byte budgets (keep ≥1).
fn trim_payload_history(history: &mut VecDeque<(Instant, String)>) {
    while history.len() > PAYLOAD_HISTORY_CAP {
        history.pop_front();
    }
    while history.len() > 1 {
        let bytes: usize = history.iter().map(|(_, p)| p.len()).sum();
        if bytes <= PAYLOAD_HISTORY_BYTES {
            break;
        }
        history.pop_front();
    }
}

#[derive(Clone, Debug, Default)]
pub struct TopicNode {
    /// Child segment → node.
    pub children: BTreeMap<String, TopicNode>,
    /// Set when a full topic ending here has been seen.
    pub last_payload: Option<String>,
    /// MQTT RETAIN flag from the last message on this leaf.
    pub last_retain: bool,
    /// MQTT 5 properties from the last message (empty when none / MQTT 3.1.1).
    pub last_props: MqttMsgProps,
    pub message_count: u64,
    pub flash_until: Option<Instant>,
    /// Timed full payloads for Payload scrub (oldest → newest).
    pub payload_history: VecDeque<(Instant, String)>,
}

impl TopicNode {
    /// Walk `topic` path and return the node at that path, if it exists.
    pub fn node_at(&self, topic: &str) -> Option<&TopicNode> {
        let mut node = self;
        for part in topic.split('/').filter(|p| !p.is_empty()) {
            node = node.children.get(part)?;
        }
        Some(node)
    }

    /// Insert/update `topic` with `payload` and MQTT `retain` flag.
    pub fn observe(&mut self, topic: &str, payload: &str, retain: bool, now: Instant) {
        self.observe_n(topic, payload, retain, now, 1, None);
    }

    /// Like [`observe`], but bumps the leaf count by `n` (coalesced UI-frame batch).
    pub fn observe_n(
        &mut self,
        topic: &str,
        payload: &str,
        retain: bool,
        now: Instant,
        n: u64,
        props: Option<&MqttMsgProps>,
    ) {
        let flash = now + Duration::from_millis(FLASH_MS);
        let n = n.max(1);
        let parts: Vec<&str> = topic.split('/').collect();
        let mut node = self;
        for (i, part) in parts.iter().enumerate() {
            node = node.children.entry((*part).to_string()).or_default();
            if i + 1 == parts.len() {
                node.last_payload = Some(payload.to_string());
                node.last_retain = retain;
                node.last_props = props.cloned().unwrap_or_default();
                node.message_count = node.message_count.saturating_add(n);
                node.flash_until = Some(flash);
            }
        }
    }

    /// Replay timed samples into the leaf payload history (Payload scrub / Pro forward).
    pub fn push_payload_samples(&mut self, topic: &str, samples: &[(Instant, String)]) {
        if samples.is_empty() {
            return;
        }
        let parts: Vec<&str> = topic.split('/').collect();
        let mut node = self;
        for (i, part) in parts.iter().enumerate() {
            node = node.children.entry((*part).to_string()).or_default();
            if i + 1 == parts.len() {
                for (at, payload) in samples {
                    node.payload_history
                        .push_back((*at, payload.clone()));
                }
                trim_payload_history(&mut node.payload_history);
            }
        }
    }

    /// Total messages in this node and all descendants (keeps growing while collapsed).
    pub fn subtree_message_count(&self) -> u64 {
        let mut total = self.message_count;
        for child in self.children.values() {
            total = total.saturating_add(child.subtree_message_count());
        }
        total
    }

    /// Remove `topic` and its entire descendant hierarchy from the in-memory tree.
    /// Returns true if a node was removed. Empty / root path clears all children.
    pub fn remove_hierarchy(&mut self, topic: &str) -> bool {
        let parts: Vec<&str> = topic.split('/').filter(|p| !p.is_empty()).collect();
        if parts.is_empty() {
            let had = !self.children.is_empty();
            self.children.clear();
            self.last_payload = None;
            self.last_retain = false;
            self.last_props = MqttMsgProps::default();
            self.message_count = 0;
            self.flash_until = None;
            self.payload_history.clear();
            return had;
        }
        let mut node = self;
        for part in &parts[..parts.len() - 1] {
            match node.children.get_mut(*part) {
                Some(child) => node = child,
                None => return false,
            }
        }
        node.children.remove(parts[parts.len() - 1]).is_some()
    }
}

/// Root of the topic tree for one profile/session.
#[derive(Clone, Debug, Default)]
pub struct TopicTree {
    pub root: TopicNode,
}

impl TopicTree {
    pub fn observe(&mut self, topic: &str, payload: &str, retain: bool, now: Instant) {
        self.root.observe(topic, payload, retain, now);
    }

    pub fn observe_n(
        &mut self,
        topic: &str,
        payload: &str,
        retain: bool,
        now: Instant,
        n: u64,
        props: Option<&MqttMsgProps>,
    ) {
        self.root.observe_n(topic, payload, retain, now, n, props);
    }

    /// Drop a topic path and all descendants from the session display tree (not the broker).
    pub fn remove_hierarchy(&mut self, topic: &str) -> bool {
        self.root.remove_hierarchy(topic)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_hierarchy() {
        let mut tree = TopicTree::default();
        let now = Instant::now();
        tree.observe("sensors/lab/temp", "21.5", true, now);
        tree.observe("sensors/lab/humidity", "40", false, now);
        assert!(tree.root.children.contains_key("sensors"));
        let lab = &tree.root.children["sensors"].children["lab"];
        assert_eq!(lab.children["temp"].last_payload.as_deref(), Some("21.5"));
        assert!(lab.children["temp"].last_retain);
        assert!(!lab.children["humidity"].last_retain);
        assert_eq!(lab.children["humidity"].message_count, 1);
        assert_eq!(lab.subtree_message_count(), 2);
        assert_eq!(tree.root.children["sensors"].subtree_message_count(), 2);
        assert_eq!(
            tree.root.node_at("sensors/lab/temp").and_then(|n| n.last_payload.as_deref()),
            Some("21.5")
        );
        assert!(tree.root.node_at("missing").is_none());
    }

    #[test]
    fn observe_n_keeps_latest_payload_and_count() {
        let mut tree = TopicTree::default();
        let now = Instant::now();
        tree.observe_n("sensors/lab/temp", "21.5", false, now, 7, None);
        let leaf = tree.root.node_at("sensors/lab/temp").unwrap();
        assert_eq!(leaf.last_payload.as_deref(), Some("21.5"));
        assert_eq!(leaf.message_count, 7);
        tree.observe_n("sensors/lab/temp", "22.0", true, now, 3, None);
        let leaf = tree.root.node_at("sensors/lab/temp").unwrap();
        assert_eq!(leaf.last_payload.as_deref(), Some("22.0"));
        assert!(leaf.last_retain);
        assert_eq!(leaf.message_count, 10);
    }

    #[test]
    fn payload_history_caps_count_and_bytes() {
        let mut tree = TopicTree::default();
        let now = Instant::now();
        // Count cap: 100 messages, tiny payloads.
        let samples: Vec<_> = (0..150)
            .map(|i| (now, format!("v{i}")))
            .collect();
        tree.root.push_payload_samples("sensors/temp", &samples);
        let leaf = tree.root.node_at("sensors/temp").unwrap();
        assert_eq!(leaf.payload_history.len(), PAYLOAD_HISTORY_CAP);
        assert_eq!(leaf.payload_history.front().unwrap().1, "v50");

        // Byte cap: few oversized payloads shrink below count cap (keep ≥1).
        let mut tree = TopicTree::default();
        let big = "x".repeat(8 * 1024);
        let big_samples: Vec<_> = (0..5).map(|_| (now, big.clone())).collect();
        tree.root.push_payload_samples("sensors/blob", &big_samples);
        let leaf = tree.root.node_at("sensors/blob").unwrap();
        assert!(leaf.payload_history.len() <= 2);
        assert!(!leaf.payload_history.is_empty());
        let bytes: usize = leaf.payload_history.iter().map(|(_, p)| p.len()).sum();
        // May exceed budget with a single large sample; never with two oversize ones.
        if leaf.payload_history.len() > 1 {
            assert!(bytes <= PAYLOAD_HISTORY_BYTES);
        }
    }

    #[test]
    fn remove_hierarchy_drops_descendants() {
        let mut tree = TopicTree::default();
        let now = Instant::now();
        tree.observe("sensors/lab/temp", "21.5", true, now);
        tree.observe("sensors/lab/humidity", "40", false, now);
        tree.observe("sensors/yard/light", "1", false, now);
        assert!(tree.remove_hierarchy("sensors/lab"));
        assert!(tree.root.node_at("sensors/lab").is_none());
        assert!(tree.root.node_at("sensors/lab/temp").is_none());
        assert_eq!(
            tree.root
                .node_at("sensors/yard/light")
                .and_then(|n| n.last_payload.as_deref()),
            Some("1")
        );
        assert!(!tree.remove_hierarchy("sensors/lab"));
        assert!(tree.remove_hierarchy("sensors"));
        assert!(tree.root.children.is_empty());
    }
}
