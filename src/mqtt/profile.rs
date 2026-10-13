//! Connection profiles — editable in UI, persisted as JSON on disk.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

/// Saved broker connection settings + topic filters.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConnectionProfile {
    pub id: String,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub client_id: String,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub password: String,
    #[serde(default)]
    pub use_tls: bool,
    /// MQTT topic filters applied on connect (`+` / `#` OK).
    #[serde(default)]
    pub subscriptions: Vec<String>,
}

impl ConnectionProfile {
    pub fn new_blank(id: String) -> Self {
        Self {
            id: id.clone(),
            name: "New profile".into(),
            host: "127.0.0.1".into(),
            port: 1883,
            client_id: format!("mqtt-monitor-{id}"),
            username: String::new(),
            password: String::new(),
            use_tls: false,
            subscriptions: Vec::new(),
        }
    }

    pub fn sample_local() -> Self {
        Self {
            id: "local".into(),
            name: "Local broker".into(),
            host: "127.0.0.1".into(),
            port: 1883,
            client_id: "mqtt-monitor".into(),
            username: String::new(),
            password: String::new(),
            use_tls: false,
            subscriptions: vec!["sensors/#".into()],
        }
    }

    pub fn sample_lab() -> Self {
        Self {
            id: "lab".into(),
            name: "Lab devices".into(),
            host: "broker.lab.local".into(),
            port: 1883,
            client_id: "mqtt-monitor-lab".into(),
            username: String::new(),
            password: String::new(),
            use_tls: false,
            subscriptions: vec!["devices/+/telemetry".into()],
        }
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
    pub selected_id: Option<String>,
    /// Last save/load message for the status bar.
    pub last_io: String,
}

impl ProfileStore {
    /// Config path: `$XDG_CONFIG_HOME/mqtt-monitor/profiles.json` or `~/.config/...`.
    pub fn path() -> PathBuf {
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
            .unwrap_or_else(|| PathBuf::from("."));
        base.join("mqtt-monitor").join("profiles.json")
    }

    pub fn with_samples() -> Self {
        let mut store = Self::default();
        for p in [ConnectionProfile::sample_local(), ConnectionProfile::sample_lab()] {
            store.selected_id.get_or_insert_with(|| p.id.clone());
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
                    for p in file.profiles {
                        store.selected_id.get_or_insert_with(|| p.id.clone());
                        store.profiles.insert(p.id.clone(), p);
                    }
                    store.last_io =
                        format!("Loaded {} profile(s) from {}", store.profiles.len(), path.display());
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

    pub fn save(&mut self) -> Result<(), String> {
        self.save_to(&Self::path())
    }

    pub fn save_to(&mut self, path: &std::path::Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("mkdir {}: {e}", parent.display()))?;
        }
        let file = ProfilesFile {
            profiles: self.profiles.values().cloned().collect(),
        };
        let text = serde_json::to_string_pretty(&file).map_err(|e| e.to_string())?;
        fs::write(path, text).map_err(|e| format!("write {}: {e}", path.display()))?;
        self.last_io = format!("Saved {} profile(s) → {}", self.profiles.len(), path.display());
        Ok(())
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
        self.selected_id = Some(id.clone());
        id
    }

    pub fn delete_selected(&mut self) -> Option<String> {
        let id = self.selected_id.take()?;
        self.profiles.remove(&id);
        self.selected_id = self.profiles.keys().next().cloned();
        Some(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_and_load_roundtrip() {
        let dir = std::env::temp_dir().join(format!("mqtt-monitor-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let path = dir.join("profiles.json");

        let mut store = ProfileStore::with_samples();
        store.profiles.get_mut("local").unwrap().host = "10.0.0.5".into();
        store.save_to(&path).unwrap();

        let loaded = ProfileStore::load_from(&path);
        assert_eq!(loaded.profiles["local"].host, "10.0.0.5");
        assert_eq!(loaded.profiles.len(), 2);
        let _ = fs::remove_dir_all(&dir);
    }
}
