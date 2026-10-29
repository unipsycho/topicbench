//! Connection profiles — editable in UI, persisted as JSON on disk.
//!
//! Passwords live in the OS keyring ([`crate::mqtt::secrets`]); `profiles.json`
//! never writes them (legacy plaintext is migrated on load).

use crate::mqtt::secrets;
use eframe::egui::Color32;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

/// Default accent (same green as Connected status).
fn default_profile_color() -> [u8; 3] {
    [90, 190, 120]
}

/// Default WebSocket path used by Mosquitto / HiveMQ / many brokers.
fn default_ws_path() -> String {
    "/mqtt".into()
}

/// TCP MQTT vs MQTT-over-WebSocket (TLS / WSS via [`ConnectionProfile::use_tls`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ProfileTransport {
    #[default]
    Mqtt,
    WebSocket,
}

impl ProfileTransport {
    pub fn label(self) -> &'static str {
        match self {
            Self::Mqtt => "MQTT",
            Self::WebSocket => "WebSocket",
        }
    }
}

/// MQTT protocol version for CONNECT (legacy profiles default to 3.1.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MqttVersion {
    #[default]
    V311,
    V5,
}

impl MqttVersion {
    pub fn label(self) -> &'static str {
        match self {
            Self::V311 => "3.1.1",
            Self::V5 => "5",
        }
    }

    pub fn is_v5(self) -> bool {
        matches!(self, Self::V5)
    }
}

/// How a sticky topic renders (persisted per sticky on the profile).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum StickyViewMode {
    #[default]
    Raw,
    Value,
    Graph,
}

impl StickyViewMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Raw => "Raw",
            Self::Value => "Value",
            Self::Graph => "Graph",
        }
    }
}

/// One sticky topic + its view mode (legacy profiles.json may store a bare string).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct StickyPref {
    pub topic: String,
    #[serde(default)]
    pub view_mode: StickyViewMode,
    /// Include this sticky’s series on the Combined multi-graph (default off).
    #[serde(default)]
    pub in_combined: bool,
}

impl StickyPref {
    pub fn new(topic: impl Into<String>, view_mode: StickyViewMode) -> Self {
        Self {
            topic: topic.into(),
            view_mode,
            in_combined: false,
        }
    }
}

impl<'de> Deserialize<'de> for StickyPref {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Helper {
            Topic(String),
            Full {
                topic: String,
                #[serde(default)]
                view_mode: StickyViewMode,
                #[serde(default)]
                in_combined: bool,
            },
        }
        match Helper::deserialize(deserializer)? {
            Helper::Topic(topic) => Ok(Self::new(topic, StickyViewMode::Raw)),
            Helper::Full {
                topic,
                view_mode,
                in_combined,
            } => Ok(Self {
                topic,
                view_mode,
                in_combined,
            }),
        }
    }
}

/// Saved broker connection settings + topic filters.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConnectionProfile {
    pub id: String,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub client_id: String,
    /// When true, each Connect uses a fresh UUID v4 as the MQTT client ID
    /// (static [`Self::client_id`] is kept for when this option is off).
    #[serde(default)]
    pub unique_client_id: bool,
    #[serde(default)]
    pub username: String,
    /// In-memory only — never written to profiles.json (OS keyring).
    #[serde(default, skip_serializing)]
    pub password: String,
    /// MQTT (TCP) or WebSocket; secure via [`Self::use_tls`] (TLS / WSS).
    #[serde(default)]
    pub transport: ProfileTransport,
    /// Path for WebSocket brokers (e.g. `/mqtt`). Ignored for MQTT TCP.
    #[serde(default = "default_ws_path")]
    pub ws_path: String,
    #[serde(default)]
    pub use_tls: bool,
    /// Optional folder with CA + client cert + private key (AWS IoT–style).
    #[serde(default)]
    pub certs_dir: String,
    /// MQTT topic filters applied on connect (`+` / `#` OK).
    #[serde(default)]
    pub subscriptions: Vec<String>,
    /// Sticky topics (+ view mode) restored on connect for this profile.
    #[serde(default)]
    pub stickies: Vec<StickyPref>,
    /// Accent RGB for profile name in tree headings / sticky labels.
    #[serde(default = "default_profile_color")]
    pub color_rgb: [u8; 3],
    /// MQTT 3.1.1 (default) or MQTT 5 — missing field on disk ⇒ 3.1.1.
    #[serde(default)]
    pub mqtt_version: MqttVersion,
    /// MQTT 5 session expiry interval in seconds (0 = expire on disconnect). Ignored for 3.1.1.
    #[serde(default)]
    pub session_expiry_secs: u32,
}

impl ConnectionProfile {
    pub fn color(&self) -> Color32 {
        Color32::from_rgb(self.color_rgb[0], self.color_rgb[1], self.color_rgb[2])
    }

    /// TLS for MQTT TCP, or WSS when transport is WebSocket.
    pub fn want_tls(&self) -> bool {
        self.use_tls || !self.certs_dir.trim().is_empty()
    }

    /// Short protocol badge: MQTT / MQTTS / WS / WSS.
    pub fn protocol_badge(&self) -> &'static str {
        match (self.transport, self.want_tls()) {
            (ProfileTransport::Mqtt, false) => "MQTT",
            (ProfileTransport::Mqtt, true) => "MQTTS",
            (ProfileTransport::WebSocket, false) => "WS",
            (ProfileTransport::WebSocket, true) => "WSS",
        }
    }

    /// Host:port for TCP, or `ws(s)://host:port/path` for WebSocket (rumqttc broker_addr).
    pub fn broker_addr(&self) -> String {
        match self.transport {
            ProfileTransport::Mqtt => self.host.clone(),
            ProfileTransport::WebSocket => {
                let scheme = if self.want_tls() { "wss" } else { "ws" };
                let path = normalize_ws_path(&self.ws_path);
                format!("{scheme}://{}:{}{path}", self.host.trim(), self.port)
            }
        }
    }

    /// Human-readable endpoint for status lines.
    pub fn endpoint_label(&self) -> String {
        match self.transport {
            ProfileTransport::Mqtt => format!("{}:{}", self.host.trim(), self.port),
            ProfileTransport::WebSocket => self.broker_addr(),
        }
    }

    /// Client ID for the MQTT CONNECT packet (fresh UUID when [`Self::unique_client_id`]).
    ///
    /// Regenerated on each user Connect (`live::spawn`), so multiple TopicBench
    /// instances sharing a profile do not collide. rumqttc auto-reconnect within
    /// one session keeps the same options (same UUID) until the user Disconnects.
    pub fn connect_client_id(&self) -> String {
        if self.unique_client_id {
            uuid::Uuid::new_v4().to_string()
        } else {
            self.client_id.clone()
        }
    }

    pub fn new_blank(id: String) -> Self {
        Self {
            id: id.clone(),
            name: "New profile".into(),
            host: "127.0.0.1".into(),
            port: 1883,
            client_id: format!("topicbench-{id}"),
            unique_client_id: false,
            username: String::new(),
            password: String::new(),
            transport: ProfileTransport::Mqtt,
            ws_path: default_ws_path(),
            use_tls: false,
            certs_dir: String::new(),
            // Default: subscribe to everything; users can narrow later in any order.
            subscriptions: vec!["#".into()],
            stickies: Vec::new(),
            color_rgb: default_profile_color(),
            mqtt_version: MqttVersion::V311,
            session_expiry_secs: 0,
        }
    }

    pub fn sample_local() -> Self {
        Self {
            id: "local".into(),
            name: "Local broker".into(),
            host: "127.0.0.1".into(),
            port: 1883,
            client_id: "topicbench".into(),
            unique_client_id: false,
            username: String::new(),
            password: String::new(),
            transport: ProfileTransport::Mqtt,
            ws_path: default_ws_path(),
            use_tls: false,
            certs_dir: String::new(),
            subscriptions: vec!["sensors/#".into()],
            stickies: Vec::new(),
            color_rgb: [90, 190, 120],
            mqtt_version: MqttVersion::V311,
            session_expiry_secs: 0,
        }
    }

    pub fn sample_lab() -> Self {
        Self {
            id: "lab".into(),
            name: "Lab devices".into(),
            host: "broker.lab.local".into(),
            port: 1883,
            client_id: "topicbench-lab".into(),
            unique_client_id: false,
            username: String::new(),
            password: String::new(),
            transport: ProfileTransport::Mqtt,
            ws_path: default_ws_path(),
            use_tls: false,
            certs_dir: String::new(),
            subscriptions: vec!["devices/+/telemetry".into()],
            stickies: Vec::new(),
            color_rgb: [200, 140, 80],
            mqtt_version: MqttVersion::V311,
            session_expiry_secs: 0,
        }
    }

    /// Add or remove a sticky topic on this profile (returns true if list changed).
    pub fn set_sticky(&mut self, topic: &str, on: bool) -> bool {
        let topic = topic.trim();
        if topic.is_empty() {
            return false;
        }
        let idx = self.stickies.iter().position(|s| s.topic == topic);
        match (on, idx) {
            (true, None) => {
                self.stickies
                    .push(StickyPref::new(topic, StickyViewMode::Raw));
                true
            }
            (false, Some(i)) => {
                self.stickies.remove(i);
                true
            }
            _ => false,
        }
    }

    /// Persist Raw/Value/Graph for an existing sticky (returns true if changed).
    pub fn set_sticky_view_mode(&mut self, topic: &str, mode: StickyViewMode) -> bool {
        let topic = topic.trim();
        if let Some(s) = self.stickies.iter_mut().find(|s| s.topic == topic) {
            if s.view_mode != mode {
                s.view_mode = mode;
                return true;
            }
        }
        false
    }

    /// Persist Combined-graph membership for an existing sticky (returns true if changed).
    pub fn set_sticky_in_combined(&mut self, topic: &str, on: bool) -> bool {
        let topic = topic.trim();
        if let Some(s) = self.stickies.iter_mut().find(|s| s.topic == topic) {
            if s.in_combined != on {
                s.in_combined = on;
                return true;
            }
        }
        false
    }

    /// Move sticky `dragged` before/after `target` in this profile’s list. Returns true if changed.
    pub fn reorder_sticky(&mut self, dragged: &str, target: &str, after: bool) -> bool {
        if dragged == target {
            return false;
        }
        let Some(from) = self.stickies.iter().position(|s| s.topic == dragged) else {
            return false;
        };
        let item = self.stickies.remove(from);
        let Some(to) = self.stickies.iter().position(|s| s.topic == target) else {
            self.stickies.push(item);
            return true;
        };
        let idx = if after { to + 1 } else { to };
        self.stickies.insert(idx, item);
        true
    }
}

#[derive(Serialize, Deserialize)]
struct ProfilesFile {
    profiles: Vec<ConnectionProfile>,
}

/// Profile store with JSON persistence under XDG config (or `~/.config`).
#[derive(Default)]
pub struct ProfileStore {
    pub profiles: BTreeMap<String, ConnectionProfile>,
    /// Display / save / topic-tree broker order (profile ids). IDs themselves never change.
    pub order: Vec<String>,
    pub selected_id: Option<String>,
    /// Last save/load message for the status bar.
    pub last_io: String,
}

impl ProfileStore {
    /// Config path: `$XDG_CONFIG_HOME/topicbench/profiles.json` or `~/.config/...`.
    pub fn path() -> PathBuf {
        super::paths::config_file("profiles.json")
    }

    pub fn with_samples() -> Self {
        let mut store = Self::default();
        for p in [ConnectionProfile::sample_local(), ConnectionProfile::sample_lab()] {
            store.selected_id.get_or_insert_with(|| p.id.clone());
            store.order.push(p.id.clone());
            store.profiles.insert(p.id.clone(), p);
        }
        store
    }

    /// Load from disk; if missing/invalid, seed samples (does not auto-write).
    pub fn load_or_sample() -> Self {
        Self::load_from(&Self::path())
    }

    pub fn load_from(path: &std::path::Path) -> Self {
        match fs::read_to_string(path) {
            Ok(text) => match serde_json::from_str::<ProfilesFile>(&text) {
                Ok(file) if !file.profiles.is_empty() => {
                    let mut store = Self::default();
                    // Array order in profiles.json is the user-facing list / topic-tree order.
                    for p in file.profiles {
                        if store.profiles.contains_key(&p.id) {
                            continue; // skip duplicate ids
                        }
                        store.selected_id.get_or_insert_with(|| p.id.clone());
                        store.order.push(p.id.clone());
                        store.profiles.insert(p.id.clone(), p);
                    }
                    let n = store.profiles.len();
                    let migrate_note = store.hydrate_and_migrate_passwords(path);
                    store.last_io = format!("Loaded {n} profile(s) from {}", path.display());
                    if let Some(note) = migrate_note {
                        store.last_io = format!("{} · {note}", store.last_io);
                    }
                    store
                }
                Ok(_) => {
                    let mut store = Self::with_samples();
                    store.last_io = format!("Empty file {}; using samples", path.display());
                    store
                }
                Err(e) => {
                    let mut store = Self::with_samples();
                    store.last_io = format!("Bad profiles.json ({e}); using samples");
                    store
                }
            },
            Err(_) => {
                let mut store = Self::with_samples();
                store.last_io = format!("No {}; using samples (Save to create)", path.display());
                store
            }
        }
    }

    /// Pull passwords from the OS keyring; migrate any legacy plaintext out of JSON.
    /// Returns an optional status note (migration / keyring warnings).
    fn hydrate_and_migrate_passwords(&mut self, path: &std::path::Path) -> Option<String> {
        let mut migrated = 0usize;
        let mut warnings = Vec::new();
        for p in self.profiles.values_mut() {
            if !p.password.is_empty() {
                // Legacy plaintext in profiles.json → keyring, then strip on resave.
                match secrets::store_password(&p.id, &p.password) {
                    Ok(()) => migrated += 1,
                    Err(e) => warnings.push(format!("{}: {e}", p.id)),
                }
            } else {
                match secrets::load_password(&p.id) {
                    Ok(Some(pw)) => p.password = pw,
                    Ok(None) => {}
                    Err(e) => warnings.push(format!("{}: {e}", p.id)),
                }
            }
        }
        let mut notes = Vec::new();
        if migrated > 0 {
            match self.save_to(path) {
                Ok(()) => notes.push(format!(
                    "moved {migrated} password(s) to OS keyring"
                )),
                Err(e) => notes.push(format!("keyring migrate save failed: {e}")),
            }
        }
        if !warnings.is_empty() {
            notes.push(format!("keyring: {}", warnings.join("; ")));
        }
        if notes.is_empty() {
            None
        } else {
            Some(notes.join(" · "))
        }
    }

    /// Sync in-memory passwords into the OS keyring (empty → delete entry).
    fn sync_passwords_to_keyring(&self) -> Vec<String> {
        let mut warnings = Vec::new();
        for p in self.profiles.values() {
            if let Err(e) = secrets::store_password(&p.id, &p.password) {
                if !p.password.is_empty() {
                    warnings.push(format!("{}: {e}", p.name));
                }
            }
        }
        warnings
    }

    pub fn save(&mut self) -> Result<(), String> {
        self.save_to(&Self::path())
    }

    pub fn save_to(&mut self, path: &std::path::Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("mkdir {}: {e}", parent.display()))?;
        }
        let keyring_warn = self.sync_passwords_to_keyring();
        self.sync_order();
        // `password` is skip_serializing — JSON never contains secrets.
        let file = ProfilesFile {
            profiles: self
                .order
                .iter()
                .filter_map(|id| self.profiles.get(id).cloned())
                .collect(),
        };
        let text = serde_json::to_string_pretty(&file).map_err(|e| e.to_string())?;
        fs::write(path, text).map_err(|e| format!("write {}: {e}", path.display()))?;
        restrict_config_permissions(path);
        self.last_io = format!("Saved {} profile(s) → {}", self.profiles.len(), path.display());
        if !keyring_warn.is_empty() {
            self.last_io = format!(
                "{} · password not in keyring ({}) — won’t survive restart",
                self.last_io,
                keyring_warn.join("; ")
            );
        }
        Ok(())
    }

    /// Keep `order` aligned with the map (drop missing, append orphans).
    pub fn sync_order(&mut self) {
        self.order.retain(|id| self.profiles.contains_key(id));
        for id in self.profiles.keys() {
            if !self.order.iter().any(|o| o == id) {
                self.order.push(id.clone());
            }
        }
    }

    /// Profile ids in display order (only those still in the map).
    pub fn ids_in_order(&self) -> Vec<String> {
        self.order
            .iter()
            .filter(|id| self.profiles.contains_key(id.as_str()))
            .cloned()
            .collect()
    }

    /// Move `dragged` before or after `target` in the display order. Returns true if changed.
    pub fn reorder(&mut self, dragged: &str, target: &str, after: bool) -> bool {
        if dragged == target || !self.profiles.contains_key(dragged) || !self.profiles.contains_key(target)
        {
            return false;
        }
        let Some(from) = self.order.iter().position(|x| x == dragged) else {
            return false;
        };
        self.order.remove(from);
        let Some(to) = self.order.iter().position(|x| x == target) else {
            self.order.push(dragged.to_string());
            return true;
        };
        let idx = if after { to + 1 } else { to };
        self.order.insert(idx, dragged.to_string());
        true
    }

    /// Insert a new blank profile and select it.
    pub fn add_new(&mut self) -> String {
        let mut n = 1u32;
        let id = loop {
            let candidate = format!("profile-{n}");
            if !self.profiles.contains_key(&candidate) {
                break candidate;
            }
            n += 1;
        };
        let profile = ConnectionProfile::new_blank(id.clone());
        self.profiles.insert(id.clone(), profile);
        self.order.push(id.clone());
        self.selected_id = Some(id.clone());
        id
    }

    pub fn delete_selected(&mut self) -> Option<String> {
        let id = self.selected_id.take()?;
        self.profiles.remove(&id);
        self.order.retain(|x| x != &id);
        secrets::delete_password(&id);
        self.selected_id = self.order.first().cloned();
        Some(id)
    }

    /// Persist a sticky topic change on a profile and save to disk.
    pub fn set_sticky_and_save(&mut self, profile_id: &str, topic: &str, on: bool) {
        let changed = self
            .profiles
            .get_mut(profile_id)
            .map(|p| p.set_sticky(topic, on))
            .unwrap_or(false);
        if changed {
            let _ = self.save();
        }
    }

    /// Persist a sticky’s Raw/Value/Graph mode and save to disk.
    pub fn set_sticky_view_mode_and_save(
        &mut self,
        profile_id: &str,
        topic: &str,
        mode: StickyViewMode,
    ) {
        let changed = self
            .profiles
            .get_mut(profile_id)
            .map(|p| p.set_sticky_view_mode(topic, mode))
            .unwrap_or(false);
        if changed {
            let _ = self.save();
        }
    }

    /// Persist Combined-graph membership and save to disk.
    pub fn set_sticky_in_combined_and_save(&mut self, profile_id: &str, topic: &str, on: bool) {
        let changed = self
            .profiles
            .get_mut(profile_id)
            .map(|p| p.set_sticky_in_combined(topic, on))
            .unwrap_or(false);
        if changed {
            let _ = self.save();
        }
    }

    /// Reorder stickies on a profile and save (same persistence as pin/unpin).
    pub fn reorder_sticky_and_save(
        &mut self,
        profile_id: &str,
        dragged: &str,
        target: &str,
        after: bool,
    ) -> bool {
        let changed = self
            .profiles
            .get_mut(profile_id)
            .map(|p| p.reorder_sticky(dragged, target, after))
            .unwrap_or(false);
        if changed {
            let _ = self.save();
        }
        changed
    }
}

/// Ensure WebSocket path starts with `/` (empty → `/mqtt`).
fn normalize_ws_path(path: &str) -> String {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return default_ws_path();
    }
    if trimmed.starts_with('/') {
        trimmed.to_string()
    } else {
        format!("/{trimmed}")
    }
}

/// Owner-only file mode so config isn’t world-readable (defense in depth).
fn restrict_config_permissions(path: &std::path::Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
    }
    let _ = path;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_never_written_to_json() {
        let dir = std::env::temp_dir().join(format!("topicbench-pw-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("profiles.json");

        let mut store = ProfileStore::with_samples();
        store.profiles.get_mut("local").unwrap().password = "s3cret".into();
        store.profiles.get_mut("local").unwrap().username = "alice".into();
        // Save strips password from JSON even if keyring sync fails in CI.
        let file = ProfilesFile {
            profiles: store.profiles.values().cloned().collect(),
        };
        let text = serde_json::to_string_pretty(&file).unwrap();
        assert!(!text.contains("s3cret"), "password must not serialize: {text}");
        assert!(!text.contains("\"password\""), "password field omitted: {text}");
        assert!(text.contains("alice"));
        let _ = path;
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_and_load_roundtrip() {
        let dir = std::env::temp_dir().join(format!("topicbench-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let path = dir.join("profiles.json");

        let mut store = ProfileStore::with_samples();
        store.profiles.get_mut("local").unwrap().host = "10.0.0.5".into();
        store.save_to(&path).unwrap();

        let loaded = ProfileStore::load_from(&path);
        assert_eq!(loaded.profiles["local"].host, "10.0.0.5");
        assert_eq!(loaded.profiles.len(), 2);
        assert_eq!(loaded.order, vec!["local".to_string(), "lab".to_string()]);
        assert!(loaded.profiles["local"].stickies.is_empty());

        store.profiles.get_mut("local").unwrap().set_sticky("sensors/temp", true);
        store
            .profiles
            .get_mut("local")
            .unwrap()
            .set_sticky_view_mode("sensors/temp", StickyViewMode::Value);
        store.save_to(&path).unwrap();
        let loaded2 = ProfileStore::load_from(&path);
        assert_eq!(
            loaded2.profiles["local"].stickies,
            vec![StickyPref::new("sensors/temp", StickyViewMode::Value)]
        );

        // Legacy bare-string stickies still load as Raw.
        let legacy = dir.join("legacy.json");
        fs::write(
            &legacy,
            r#"{"profiles":[{"id":"x","name":"X","host":"h","port":1883,"client_id":"c","stickies":["a/b"]}]}"#,
        )
        .unwrap();
        let legacy_store = ProfileStore::load_from(&legacy);
        assert_eq!(
            legacy_store.profiles["x"].stickies,
            vec![StickyPref::new("a/b", StickyViewMode::Raw)]
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn reorder_sticky_persists_topic_order() {
        let dir = std::env::temp_dir().join(format!("topicbench-sticky-order-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let path = dir.join("profiles.json");

        let mut store = ProfileStore::with_samples();
        let p = store.profiles.get_mut("local").unwrap();
        p.set_sticky("a/one", true);
        p.set_sticky("a/two", true);
        p.set_sticky("a/three", true);
        assert!(p.reorder_sticky("a/three", "a/one", false)); // three before one
        assert_eq!(
            p.stickies.iter().map(|s| s.topic.as_str()).collect::<Vec<_>>(),
            vec!["a/three", "a/one", "a/two"]
        );
        store.save_to(&path).unwrap();
        let loaded = ProfileStore::load_from(&path);
        assert_eq!(
            loaded.profiles["local"]
                .stickies
                .iter()
                .map(|s| s.topic.as_str())
                .collect::<Vec<_>>(),
            vec!["a/three", "a/one", "a/two"]
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn reorder_persists_array_order() {
        let dir = std::env::temp_dir().join(format!("topicbench-order-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let path = dir.join("profiles.json");

        let mut store = ProfileStore::with_samples();
        assert!(store.reorder("lab", "local", false)); // lab before local
        assert_eq!(store.order, vec!["lab".to_string(), "local".to_string()]);
        store.save_to(&path).unwrap();

        let loaded = ProfileStore::load_from(&path);
        assert_eq!(loaded.order, vec!["lab".to_string(), "local".to_string()]);
        // profiles.json array order matches
        let text = fs::read_to_string(&path).unwrap();
        let lab_pos = text.find("\"id\": \"lab\"").unwrap();
        let local_pos = text.find("\"id\": \"local\"").unwrap();
        assert!(lab_pos < local_pos);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn legacy_profiles_default_unique_client_id_off() {
        let json = r#"{"profiles":[{"id":"x","name":"X","host":"h","port":1883,"client_id":"c"}]}"#;
        let file: ProfilesFile = serde_json::from_str(json).unwrap();
        let p = &file.profiles[0];
        assert!(!p.unique_client_id);
        assert_eq!(p.connect_client_id(), "c");
        let mut with_uuid = p.clone();
        with_uuid.unique_client_id = true;
        let a = with_uuid.connect_client_id();
        let b = with_uuid.connect_client_id();
        assert_ne!(a, b);
        assert_eq!(a.len(), 36); // UUID v4 hyphenated
        assert_eq!(with_uuid.client_id, "c"); // static field unchanged
    }

    #[test]
    fn legacy_profiles_default_to_mqtt_transport() {
        let json = r#"{"profiles":[{"id":"x","name":"X","host":"h","port":1883,"client_id":"c"}]}"#;
        let file: ProfilesFile = serde_json::from_str(json).unwrap();
        let p = &file.profiles[0];
        assert_eq!(p.transport, ProfileTransport::Mqtt);
        assert_eq!(p.ws_path, "/mqtt");
        assert_eq!(p.broker_addr(), "h");
        assert_eq!(p.endpoint_label(), "h:1883");
        assert_eq!(p.protocol_badge(), "MQTT");
        assert_eq!(p.mqtt_version, MqttVersion::V311);
        assert_eq!(p.session_expiry_secs, 0);
    }

    #[test]
    fn websocket_broker_addr_builds_ws_and_wss_urls() {
        let mut p = ConnectionProfile::new_blank("ws".into());
        p.host = "broker.example".into();
        p.port = 8083;
        p.transport = ProfileTransport::WebSocket;
        p.ws_path = "mqtt".into(); // missing leading slash
        assert_eq!(p.broker_addr(), "ws://broker.example:8083/mqtt");
        assert_eq!(p.protocol_badge(), "WS");

        p.use_tls = true;
        p.port = 443;
        p.ws_path = "/mqtt".into();
        assert_eq!(p.broker_addr(), "wss://broker.example:443/mqtt");
        assert_eq!(p.protocol_badge(), "WSS");
        assert_eq!(p.endpoint_label(), p.broker_addr());
    }
}
