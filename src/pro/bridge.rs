//! Localhost IPC bridge for TopicBench Pro (MCP / future engines).
//!
//! **v1 transport (Unix):** domain socket at `~/.config/topicbench/bridge.sock`
//! with one JSON object per line (request → response). MQTT truth stays in
//! SessionHub; this only serializes tool calls onto the same UI paths.
//!
//! Started only when Pro is licensed (`pro::has(Entitlement::McpServer)` /
//! `pro::is_licensed()`). Without a license the socket is never bound.
//!
//! On Windows, UDS is unavailable — `BridgeHandle::start` returns an error and
//! the app continues without a local MCP bridge (HTTP sim/history clients still work).

#[cfg(unix)]
#[path = "bridge_unix.rs"]
mod imp;

#[cfg(not(unix))]
mod imp {
    use crate::mqtt::session::SessionManager;
    use crate::mqtt::profile::ProfileStore;
    use eframe::egui::Context;
    use std::path::PathBuf;

    use crate::mqtt::paths;

    /// Socket filename under the TopicBench config dir (path reserved for future).
    pub const BRIDGE_SOCK_NAME: &str = "bridge.sock";

    pub fn socket_path() -> PathBuf {
        paths::config_file(BRIDGE_SOCK_NAME)
    }

    /// Stub handle — Pro MCP bridge needs Unix domain sockets.
    pub struct BridgeHandle {
        pub listening: bool,
        pub socket_path: PathBuf,
    }

    impl BridgeHandle {
        pub fn start(_ctx: &Context) -> Result<Self, String> {
            Err(
                "Pro MCP bridge uses Unix domain sockets (Linux); not available on this platform"
                    .into(),
            )
        }

        pub fn bind_egui_ctx(&self, _ctx: &Context) {}

        pub fn on_message(
            &self,
            _profile_id: &str,
            _profile_name: &str,
            _topic: &str,
            _payload: &str,
            _retain: bool,
        ) {
        }

        pub fn tick_waiters(&self) {}

        pub fn poll(
            &self,
            _profiles: &ProfileStore,
            _sessions: &mut SessionManager,
            _max_packet_bytes: usize,
            _ctx: &Context,
            _status_out: &mut String,
        ) {
        }
    }
}

pub use imp::*;
