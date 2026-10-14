//! Publish draft state + persisted Recent history (last 20).

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

pub const RECENT_CAP: usize = 20;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Qos {
    AtMostOnce = 0,
    AtLeastOnce = 1,
    ExactlyOnce = 2,
}

impl Qos {
    pub fn as_u8(self) -> u8 {
        self as u8
    }
}

#[derive(Clone, Debug)]
pub struct PublishDraft {
    pub profile_id: Option<String>,
    pub topic: String,
    pub payload: String,
    pub qos: Qos,
    pub retain: bool,
}

impl Default for PublishDraft {
    fn default() -> Self {
        Self {
            profile_id: None,
            topic: "sensors/demo".into(),
            payload: r#"{"ok":true}"#.into(),
            qos: Qos::AtMostOnce,
            retain: false,
        }
    }
}

impl PublishDraft {
    pub fn apply_recent(&mut self, entry: &RecentPublish) {
        self.topic = entry.topic.clone();
        self.payload = entry.payload.clone();
        self.qos = entry.qos;
        self.retain = entry.retain;
    }
}

/// One published message recalled into the form.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RecentPublish {
    pub topic: String,
    pub payload: String,
    pub qos: Qos,
    pub retain: bool,
}

impl RecentPublish {
    pub fn from_draft(draft: &PublishDraft) -> Self {
        Self {
            topic: draft.topic.clone(),
            payload: draft.payload.clone(),
            qos: draft.qos,
            retain: draft.retain,
        }
    }

    /// Short label for the Recent list.
    pub fn list_label(&self) -> String {
        let flat = self.payload.replace('\n', " ");
        let pay: String = if flat.chars().count() > 40 {
            format!("{}...", flat.chars().take(37).collect::<String>())
        } else {
            flat
        };
        format!(
            "{}  qos={}{}  {}",
            self.topic,
            self.qos.as_u8(),
            if self.retain { " retain" } else { "" },
            pay
        )
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PublishHistory {
    #[serde(default)]
    pub recent: Vec<RecentPublish>,
}

impl PublishHistory {
    pub fn path() -> PathBuf {
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
            .unwrap_or_else(|| PathBuf::from("."));
        base.join("mqtt-monitor").join("publish-recent.json")
    }

    pub fn load() -> Self {
        let path = Self::path();
        match fs::read_to_string(&path) {
            Ok(text) => {
                let mut h: Self = serde_json::from_str(&text).unwrap_or_default();
                h.recent.truncate(RECENT_CAP);
                h
            }
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self) -> Result<(), String> {
        let path = Self::path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("mkdir {}: {e}", parent.display()))?;
        }
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        fs::write(&path, text).map_err(|e| format!("write {}: {e}", path.display()))
    }

    /// Push newest first; drop duplicates of same topic+payload+qos+retain; cap at 20.
    pub fn push_and_save(&mut self, entry: RecentPublish) -> Result<(), String> {
        self.recent.retain(|e| {
            !(e.topic == entry.topic
                && e.payload == entry.payload
                && e.qos == entry.qos
                && e.retain == entry.retain)
        });
        self.recent.insert(0, entry);
        self.recent.truncate(RECENT_CAP);
        self.save()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_caps_newest_first() {
        let mut h = PublishHistory::default();
        for i in 0..25 {
            h.recent.insert(
                0,
                RecentPublish {
                    topic: format!("t{i}"),
                    payload: "x".into(),
                    qos: Qos::AtMostOnce,
                    retain: false,
                },
            );
            h.recent.truncate(RECENT_CAP);
        }
        assert_eq!(h.recent.len(), RECENT_CAP);
        assert_eq!(h.recent[0].topic, "t24");
    }
}
