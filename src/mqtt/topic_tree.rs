//! Browsable topic hierarchy built from live subscribed traffic.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

pub const FLASH_MS: u64 = 400;

#[derive(Clone, Debug, Default)]
pub struct TopicNode {
    /// Child segment → node.
    pub children: BTreeMap<String, TopicNode>,
    /// Set when a full topic ending here has been seen.
    pub last_payload: Option<String>,
    pub message_count: u64,
    pub flash_until: Option<Instant>,
}

impl TopicNode {
    /// Insert/update `topic` with `payload`.
    pub fn observe(&mut self, topic: &str, payload: &str, now: Instant) {
        let flash = now + Duration::from_millis(FLASH_MS);
        let parts: Vec<&str> = topic.split('/').collect();
        let mut node = self;
        for (i, part) in parts.iter().enumerate() {
            node = node.children.entry((*part).to_string()).or_default();
            if i + 1 == parts.len() {
                node.last_payload = Some(payload.to_string());
                node.message_count = node.message_count.saturating_add(1);
                node.flash_until = Some(flash);
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
}

/// Root of the topic tree for one profile/session.
#[derive(Clone, Debug, Default)]
pub struct TopicTree {
    pub root: TopicNode,
}

impl TopicTree {
    pub fn observe(&mut self, topic: &str, payload: &str, now: Instant) {
        self.root.observe(topic, payload, now);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_hierarchy() {
        let mut tree = TopicTree::default();
        let now = Instant::now();
        tree.observe("sensors/lab/temp", "21.5", now);
        tree.observe("sensors/lab/humidity", "40", now);
        assert!(tree.root.children.contains_key("sensors"));
        let lab = &tree.root.children["sensors"].children["lab"];
        assert_eq!(lab.children["temp"].last_payload.as_deref(), Some("21.5"));
        assert_eq!(lab.children["humidity"].message_count, 1);
        assert_eq!(lab.subtree_message_count(), 2);
        assert_eq!(tree.root.children["sensors"].subtree_message_count(), 2);
    }
}
