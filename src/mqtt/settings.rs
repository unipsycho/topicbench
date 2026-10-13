//! Persisted application settings (beside profiles.json).

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// How the topic tree opens by default.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TopicTreeExpandMode {
    /// Only top-level segments open.
    ExpandRoot,
    /// Every branch starts open.
    ExpandAll,
    /// Everything starts collapsed.
    CollapseAll,
}

impl TopicTreeExpandMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::ExpandRoot => "Expand root only",
            Self::ExpandAll => "Expand all",
            Self::CollapseAll => "Collapse all",
        }
    }
}

impl Default for TopicTreeExpandMode {
    fn default() -> Self {
        Self::ExpandRoot
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AppSettings {
    /// On startup, reconnect profiles listed in `last_connected`.
    #[serde(default)]
    pub auto_reconnect_on_reload: bool,
    #[serde(default)]
    pub topic_tree_expand: TopicTreeExpandMode,
    /// Profile ids that were connected when the app last ran / last connect change.
    #[serde(default)]
    pub last_connected: Vec<String>,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            auto_reconnect_on_reload: true,
            topic_tree_expand: TopicTreeExpandMode::ExpandRoot,
            last_connected: Vec::new(),
        }
    }
}

impl AppSettings {
    pub fn path() -> PathBuf {
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
            .unwrap_or_else(|| PathBuf::from("."));
        base.join("mqtt-monitor").join("settings.json")
    }

    pub fn load() -> Self {
        let path = Self::path();
        match fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_default(),
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

    pub fn remember_connected(&mut self, profile_id: &str, connected: bool) {
        if connected {
            if !self.last_connected.iter().any(|id| id == profile_id) {
                self.last_connected.push(profile_id.to_string());
            }
        } else {
            self.last_connected.retain(|id| id != profile_id);
        }
        let _ = self.save();
    }
}
