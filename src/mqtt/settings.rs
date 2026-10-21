//! Persisted application settings (beside profiles.json).

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// Soft UI payload floor (2 KB) — Settings slider minimum.
pub const MAX_PACKET_MIN: usize = 2 * 1024;
/// Soft UI payload ceiling (1 MB) — Settings slider maximum.
pub const MAX_PACKET_MAX: usize = 1024 * 1024;
/// Default soft UI payload limit (256 KB).
pub const MAX_PACKET_DEFAULT: usize = 256 * 1024;
/// rumqttc wire decode ceiling — high enough that oversize is a soft skip, not a disconnect.
pub const WIRE_MAX_PACKET: usize = 16 * 1024 * 1024;

/// Font zoom: 4 steps down, default, 5 steps up.
pub const FONT_ZOOM_MIN: i8 = -4;
pub const FONT_ZOOM_MAX: i8 = 5;
/// Relative size change per zoom step (~10%).
pub const FONT_ZOOM_STEP_FACTOR: f32 = 0.10;

/// How deep the topic tree opens on connect / first show.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TopicTreeExpandMode {
    /// Open the first level of segments (was `expand_root`).
    #[serde(alias = "expand_root")]
    Expand1,
    /// Open two levels deep.
    Expand2,
    /// Open three levels deep.
    Expand3,
    /// Every branch starts open.
    ExpandAll,
}

impl TopicTreeExpandMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Expand1 => "Expand 1 level",
            Self::Expand2 => "Expand 2 levels",
            Self::Expand3 => "Expand 3 levels",
            Self::ExpandAll => "Expand all",
        }
    }

    /// Nodes with `depth < levels()` start open (`ExpandAll` = unlimited).
    pub fn levels(self) -> Option<usize> {
        match self {
            Self::Expand1 => Some(1),
            Self::Expand2 => Some(2),
            Self::Expand3 => Some(3),
            Self::ExpandAll => None,
        }
    }
}

impl Default for TopicTreeExpandMode {
    fn default() -> Self {
        Self::Expand1
    }
}

/// Search bar scope — persisted in settings.json.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SearchScope {
    #[default]
    All,
    Topics,
    Payloads,
}

impl SearchScope {
    pub fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::Topics => "Topics",
            Self::Payloads => "Payloads",
        }
    }
}

/// How sticky topic cards are arranged.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum StickyLayoutMode {
    #[default]
    Rows,
    Columns,
}

impl StickyLayoutMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Rows => "Rows",
            Self::Columns => "Columns",
        }
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
    /// Max MQTT packet size in bytes (incoming + outgoing). Clamped 2KB–256KB.
    #[serde(default = "default_max_packet_bytes")]
    pub max_packet_bytes: usize,
    /// Sticky board: stack as rows or as a column grid.
    #[serde(default)]
    pub sticky_layout: StickyLayoutMode,
    /// Column count when `sticky_layout` is Columns (clamped 2–4).
    #[serde(default = "default_sticky_columns")]
    pub sticky_columns: u8,
    /// App-wide font zoom step (−4…+5). 0 = default egui sizes.
    #[serde(default)]
    pub font_zoom_step: i8,
    /// Ribbon panel open/closed — restored on reload (same for Payload as Publish/Stickies/…).
    #[serde(default = "default_true")]
    pub show_profiles: bool,
    #[serde(default)]
    pub show_live_traffic: bool,
    #[serde(default)]
    pub show_publish: bool,
    #[serde(default)]
    pub show_payload: bool,
    #[serde(default = "default_true")]
    pub show_stickies: bool,
    #[serde(default = "default_true")]
    pub show_topic_tree: bool,
    /// Search bar text (supports `|` OR terms) — restored on reload.
    #[serde(default)]
    pub search_query: String,
    /// Search scope dropdown — restored on reload.
    #[serde(default)]
    pub search_scope: SearchScope,
    /// When true, the search filter also hides non-matching sticky topics.
    #[serde(default = "default_true")]
    pub search_apply_to_stickies: bool,
    /// Unix seconds of last update check (startup cache / Settings “last checked”).
    #[serde(default)]
    pub last_update_check: Option<u64>,
    /// Skipped release tag — don’t prompt again until a newer tag appears.
    #[serde(default)]
    pub skipped_update_version: Option<String>,
    /// Unix seconds until “Remind me later” expires (~24h).
    #[serde(default)]
    pub remind_later_until: Option<u64>,
}

fn default_true() -> bool {
    true
}

fn default_max_packet_bytes() -> usize {
    MAX_PACKET_DEFAULT
}

fn default_sticky_columns() -> u8 {
    2
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            auto_reconnect_on_reload: true,
            topic_tree_expand: TopicTreeExpandMode::Expand1,
            last_connected: Vec::new(),
            max_packet_bytes: MAX_PACKET_DEFAULT,
            sticky_layout: StickyLayoutMode::Rows,
            sticky_columns: 2,
            font_zoom_step: 0,
            show_profiles: true,
            show_live_traffic: false,
            show_publish: false,
            show_payload: false,
            show_stickies: true,
            show_topic_tree: true,
            search_query: String::new(),
            search_scope: SearchScope::All,
            search_apply_to_stickies: true,
            last_update_check: None,
            skipped_update_version: None,
            remind_later_until: None,
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

    /// Clamp packet size into the supported Settings range.
    pub fn clamp_max_packet(bytes: usize) -> usize {
        bytes.clamp(MAX_PACKET_MIN, MAX_PACKET_MAX)
    }

    pub fn max_packet_kb(&self) -> usize {
        Self::clamp_max_packet(self.max_packet_bytes) / 1024
    }

    pub fn set_max_packet_kb(&mut self, kb: usize) {
        self.max_packet_bytes = Self::clamp_max_packet(kb.saturating_mul(1024));
    }

    pub fn clamp_sticky_columns(n: u8) -> u8 {
        n.clamp(2, 4)
    }

    pub fn sticky_column_count(&self) -> usize {
        Self::clamp_sticky_columns(self.sticky_columns) as usize
    }

    pub fn clamp_font_zoom(step: i8) -> i8 {
        step.clamp(FONT_ZOOM_MIN, FONT_ZOOM_MAX)
    }

    pub fn font_zoom_factor(&self) -> f32 {
        1.0 + Self::clamp_font_zoom(self.font_zoom_step) as f32 * FONT_ZOOM_STEP_FACTOR
    }

    pub fn zoom_font_in(&mut self) -> bool {
        let next = Self::clamp_font_zoom(self.font_zoom_step.saturating_add(1));
        if next == self.font_zoom_step {
            return false;
        }
        self.font_zoom_step = next;
        true
    }

    pub fn zoom_font_out(&mut self) -> bool {
        let next = Self::clamp_font_zoom(self.font_zoom_step.saturating_sub(1));
        if next == self.font_zoom_step {
            return false;
        }
        self.font_zoom_step = next;
        true
    }

    /// Reset font zoom to default (step 0).
    pub fn zoom_font_reset(&mut self) -> bool {
        if self.font_zoom_step == 0 {
            return false;
        }
        self.font_zoom_step = 0;
        true
    }

    pub fn load() -> Self {
        let path = Self::path();
        let mut s = match fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_default(),
            Err(_) => Self::default(),
        };
        // Soft-migrate the old 64 KB default — `#` retained dumps on AMI brokers exceed it.
        const OLD_DEFAULT: usize = 64 * 1024;
        if s.max_packet_bytes == OLD_DEFAULT {
            s.max_packet_bytes = MAX_PACKET_DEFAULT;
        }
        s.max_packet_bytes = Self::clamp_max_packet(s.max_packet_bytes);
        s.sticky_columns = Self::clamp_sticky_columns(s.sticky_columns);
        s.font_zoom_step = Self::clamp_font_zoom(s.font_zoom_step);
        s
    }

    pub fn save(&self) -> Result<(), String> {
        let path = Self::path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("mkdir {}: {e}", parent.display()))?;
        }
        let mut clone = self.clone();
        clone.max_packet_bytes = Self::clamp_max_packet(clone.max_packet_bytes);
        clone.sticky_columns = Self::clamp_sticky_columns(clone.sticky_columns);
        clone.font_zoom_step = Self::clamp_font_zoom(clone.font_zoom_step);
        let text = serde_json::to_string_pretty(&clone).map_err(|e| e.to_string())?;
        fs::write(&path, text).map_err(|e| format!("write {}: {e}", path.display()))
    }

    /// Snapshot ribbon panel visibility into settings (call before save).
    pub fn set_panels(
        &mut self,
        profiles: bool,
        live_traffic: bool,
        publish: bool,
        payload: bool,
        stickies: bool,
        topic_tree: bool,
    ) {
        self.show_profiles = profiles;
        self.show_live_traffic = live_traffic;
        self.show_publish = publish;
        self.show_payload = payload;
        self.show_stickies = stickies;
        self.show_topic_tree = topic_tree;
    }

    /// Snapshot search bar query + scope + stickies apply (call before save).
    pub fn set_search(&mut self, query: &str, scope: SearchScope, apply_to_stickies: bool) {
        self.search_query = query.to_string();
        self.search_scope = scope;
        self.search_apply_to_stickies = apply_to_stickies;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamp_max_packet_range() {
        assert_eq!(AppSettings::clamp_max_packet(0), MAX_PACKET_MIN);
        assert_eq!(AppSettings::clamp_max_packet(1024), MAX_PACKET_MIN);
        assert_eq!(AppSettings::clamp_max_packet(64 * 1024), 64 * 1024);
        assert_eq!(AppSettings::clamp_max_packet(256 * 1024), 256 * 1024);
        assert_eq!(AppSettings::clamp_max_packet(2 * 1024 * 1024), MAX_PACKET_MAX);
    }

    #[test]
    fn font_zoom_steps() {
        let mut s = AppSettings::default();
        assert_eq!(s.font_zoom_step, 0);
        assert!((s.font_zoom_factor() - 1.0).abs() < f32::EPSILON);
        for _ in 0..5 {
            s.zoom_font_in();
        }
        assert_eq!(s.font_zoom_step, FONT_ZOOM_MAX);
        for _ in 0..10 {
            s.zoom_font_out();
        }
        assert_eq!(s.font_zoom_step, FONT_ZOOM_MIN);
    }
}
