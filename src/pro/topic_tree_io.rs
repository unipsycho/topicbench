//! Topic tree Export (Pro) — snapshot JSON. Import/republish is in the sidecar.

use crate::mqtt::session::SessionManager;
use crate::mqtt::topic_tree::TopicNode;
use serde_json::{json, Map, Value};
use std::time::{SystemTime, UNIX_EPOCH};

const FORMAT: &str = "topicbench.topic-tree";
const VERSION: u32 = 1;

/// Scope chosen in the export modal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportScope {
    Selection,
    Full,
}

/// Build the versioned export document from live session trees.
pub fn build_export(
    sessions: &SessionManager,
    scope: ExportScope,
    // (profile_id, topic_path) — topic empty = whole broker.
    selection: Option<(&str, &str)>,
) -> Result<Value, String> {
    let exported_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| {
            let secs = d.as_secs();
            // Simple RFC3339-ish UTC without chrono dep.
            let days = secs / 86400;
            let rem = secs % 86400;
            let h = rem / 3600;
            let m = (rem % 3600) / 60;
            let s = rem % 60;
            // 1970-01-01 + days — good enough for filename/timestamp field.
            let (y, mo, d) = civil_from_days(days as i64);
            format!("{y:04}-{mo:02}-{d:02}T{h:02}:{m:02}:{s:02}Z")
        })
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".into());

    let mut brokers: Vec<Value> = Vec::new();
    match scope {
        ExportScope::Full => {
            for (pid, sess) in &sessions.open {
                brokers.push(broker_entry(
                    pid,
                    &sess.profile_name,
                    &sess.topic_tree.root,
                    "",
                ));
            }
            if brokers.is_empty() {
                return Err("No connected brokers to export".into());
            }
        }
        ExportScope::Selection => {
            let (pid, topic) = selection.ok_or("Select a node in the tree")?;
            let sess = sessions
                .open
                .get(pid)
                .ok_or("Selected broker is not connected")?;
            brokers.push(broker_entry(
                pid,
                &sess.profile_name,
                &sess.topic_tree.root,
                topic,
            ));
        }
    }

    Ok(json!({
        "format": FORMAT,
        "version": VERSION,
        "exported_at": exported_at,
        "scope": match scope {
            ExportScope::Selection => "selection",
            ExportScope::Full => "full",
        },
        "brokers": brokers,
    }))
}

fn broker_entry(profile_id: &str, profile_name: &str, root: &TopicNode, branch: &str) -> Value {
    let topics = if branch.is_empty() {
        node_to_topics(root)
    } else if let Some(node) = root.node_at(branch) {
        // Wrap branch under its path segments so import rebuilds the same path.
        wrap_path(branch, node_to_topics(node), node)
    } else {
        Map::new()
    };
    json!({
        "profile_id": profile_id,
        "profile_name": profile_name,
        "topics": topics,
    })
}

/// Nest `inner` under path segments; attach leaf `_value` on the final segment when present.
fn wrap_path(path: &str, mut leaf_topics: Map<String, Value>, node: &TopicNode) -> Map<String, Value> {
    let parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
    if parts.is_empty() {
        return leaf_topics;
    }
    // If exporting a leaf that has a payload and no children, put value on the leaf object.
    if node.children.is_empty() {
        let mut leaf = Map::new();
        if let Some(v) = &node.last_payload {
            leaf.insert("_value".into(), Value::String(v.clone()));
            leaf.insert(
                "_meta".into(),
                json!({ "qos": 0, "retain": node.last_retain, "content_type": "text" }),
            );
        }
        // Merge any child topics (usually empty for pure leaf).
        for (k, v) in leaf_topics {
            leaf.insert(k, v);
        }
        leaf_topics = leaf;
    }
    let mut acc = leaf_topics;
    for seg in parts.iter().rev() {
        let mut wrap = Map::new();
        wrap.insert((*seg).to_string(), Value::Object(acc));
        acc = wrap;
    }
    acc
}

fn node_to_topics(node: &TopicNode) -> Map<String, Value> {
    let mut map = Map::new();
    for (seg, child) in &node.children {
        let mut obj = node_to_topics(child);
        if let Some(v) = &child.last_payload {
            obj.insert("_value".into(), Value::String(v.clone()));
            obj.insert(
                "_meta".into(),
                json!({
                    "qos": 0,
                    "retain": child.last_retain,
                    "content_type": "text",
                }),
            );
        }
        map.insert(seg.clone(), Value::Object(obj));
    }
    map
}

/// Default filename for the save dialog.
pub fn default_export_filename() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = secs / 86400;
    let rem = secs % 86400;
    let h = rem / 3600;
    let m = (rem % 3600) / 60;
    let s = rem % 60;
    let (y, mo, d) = civil_from_days(days as i64);
    format!("topicbench-export-{y:04}{mo:02}{d:02}-{h:02}{m:02}{s:02}.json")
}

// Import / remap / republish lives in the Pro sidecar (`/tree/preview`, `/tree/import`).
// OSS UI talks through `crate::pro::tree_client` — do not reimplement expand here.

/// Days since Unix epoch → (year, month, day) — Howard Hinnant algorithm.
fn civil_from_days(z: i64) -> (i32, u32, u32) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y as i32, m as u32, d as u32)
}
