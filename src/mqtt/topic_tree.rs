//! Browsable topic hierarchy built from live subscribed traffic.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

/// Update-flash duration: full-on border, then fade out (resets on each new value).
pub const FLASH_MS: u64 = 500;

#[derive(Clone, Debug, Default)]
pub struct TopicNode {
    /// Child segment → node.
    pub children: BTreeMap<String, TopicNode>,
    /// Set when a full topic ending here has been seen.
    pub last_payload: Option<String>,
    /// MQTT RETAIN flag from the last message on this leaf.
    pub last_retain: bool,
    pub message_count: u64,
    pub flash_until: Option<Instant>,
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
        let flash = now + Duration::from_millis(FLASH_MS);
        let parts: Vec<&str> = topic.split('/').collect();
        let mut node = self;
        for (i, part) in parts.iter().enumerate() {
            node = node.children.entry((*part).to_string()).or_default();
            if i + 1 == parts.len() {
                node.last_payload = Some(payload.to_string());
                node.last_retain = retain;
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

    /// Remove `topic` and its entire descendant hierarchy from the in-memory tree.
    /// Returns true if a node was removed. Empty / root path clears all children.
    pub fn remove_hierarchy(&mut self, topic: &str) -> bool {
        let parts: Vec<&str> = topic.split('/').filter(|p| !p.is_empty()).collect();
        if parts.is_empty() {
            let had = !self.children.is_empty();
            self.children.clear();
            self.last_payload = None;
            self.last_retain = false;
            self.message_count = 0;
            self.flash_until = None;
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
