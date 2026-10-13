//! Connection profiles (broker host, port, credentials, TLS, client id).

use std::collections::BTreeMap;

/// Saved broker connection settings (not yet wired to rumqttc).
#[derive(Clone, Debug)]
pub struct ConnectionProfile {
    pub id: String,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub client_id: String,
    pub username: String,
    pub password: String,
    pub use_tls: bool,
}

impl ConnectionProfile {
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
        }
    }
}

/// In-memory profile store — swap for disk/JSON later.
#[derive(Default)]
pub struct ProfileStore {
    pub profiles: BTreeMap<String, ConnectionProfile>,
    pub selected_id: Option<String>,
}

impl ProfileStore {
    pub fn with_sample() -> Self {
        let mut store = Self::default();
        for p in [ConnectionProfile::sample_local(), ConnectionProfile::sample_lab()] {
            store.selected_id.get_or_insert_with(|| p.id.clone());
            store.profiles.insert(p.id.clone(), p);
        }
        store
    }

    pub fn selected(&self) -> Option<&ConnectionProfile> {
        self.selected_id.as_ref().and_then(|id| self.profiles.get(id))
    }
}
