//! Multi-profile sessions: several brokers open/connected at once.

use std::collections::BTreeMap;

/// Connection lifecycle for one profile (placeholder until rumqttc threads exist).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionState {
    Disconnected,
    Connecting,
    Connected,
}

impl SessionState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Disconnected => "disconnected",
            Self::Connecting => "connecting…",
            Self::Connected => "connected",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Session {
    pub profile_id: String,
    pub profile_name: String,
    pub state: SessionState,
    /// Active subscriptions for this session (supports `+` / `#`).
    pub subscriptions: Vec<String>,
}

/// Open sessions keyed by profile id — multiple may be Connected simultaneously.
#[derive(Default)]
pub struct SessionManager {
    pub open: BTreeMap<String, Session>,
}

impl SessionManager {
    pub fn open_or_toggle(&mut self, profile_id: &str, profile_name: &str) {
        if let Some(session) = self.open.get_mut(profile_id) {
            session.state = match session.state {
                SessionState::Disconnected => SessionState::Connected, // TODO: rumqttc connect
                SessionState::Connecting => SessionState::Disconnected,
                SessionState::Connected => SessionState::Disconnected, // TODO: disconnect
            };
            return;
        }

        self.open.insert(
            profile_id.to_string(),
            Session {
                profile_id: profile_id.to_string(),
                profile_name: profile_name.to_string(),
                state: SessionState::Connected, // placeholder: mark open without broker I/O
                subscriptions: Vec::new(),
            },
        );
    }

    pub fn connected_ids(&self) -> Vec<&str> {
        self.open
            .iter()
            .filter(|(_, s)| s.state == SessionState::Connected)
            .map(|(id, _)| id.as_str())
            .collect()
    }
}
