//! TopicBench Pro — thin OSS capability probe / entitlement bridge.
//!
//! Public OSS must not depend on the private `topicbench-pro` crate. Minting and
//! paid engines live there; this module only probes entitlements.
//!
//! Probe order (`ProBridge::probe`):
//! 1. **DEV env** (`TOPICBENCH_PRO_DEV` / `TOPICBENCH_PRO_ENTITLEMENTS`) — UI preview; wins
//! 2. **Signed license file** under `~/.config/topicbench/license.json` (or `.lic`) → `Licensed`
//! 3. Else free / not installed
//!
//! Dev preview (not shown in Settings — do not treat as a license switch):
//! - `TOPICBENCH_PRO_DEV=1` — all entitlement flags true
//! - `TOPICBENCH_PRO_ENTITLEMENTS=multi_graph,mcp_server,…` — comma-separated flags
//!
//! Settings → TopicBench Pro (Activate / status) is intentionally omitted for now;
//! when it lands, it should write the same license file and re-probe (see `license_format`).

mod license_format;

use std::sync::OnceLock;

use crate::mqtt::paths;
use license_format::{load_license_path, LicensePayload, LICENSE_FILE_ALT, LICENSE_FILE_NAME};

/// Named paid capability flags (match [pro-feature-ideas] / open-core plan).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Entitlement {
    MultiGraph,
    McpServer,
    Webhooks,
    Simulation,
    ScenarioPlayer,
    RecordSimulate,
    TreeExport,
    HistoryScrubber,
    CsvExport,
    Bookmarks,
    ColorRules,
    AutoSuggestSims,
}

impl Entitlement {
    fn from_token(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "multi_graph" | "multigraph" | "combined_graph" => Some(Self::MultiGraph),
            "mcp_server" | "mcp" => Some(Self::McpServer),
            "webhooks" | "webhook" => Some(Self::Webhooks),
            "simulation" | "sim" => Some(Self::Simulation),
            "scenario_player" | "scenario" => Some(Self::ScenarioPlayer),
            "record_simulate" | "record_sim" => Some(Self::RecordSimulate),
            "tree_export" | "export" => Some(Self::TreeExport),
            "history_scrubber" | "scrubber" => Some(Self::HistoryScrubber),
            "csv_export" | "csv" | "parquet" => Some(Self::CsvExport),
            "bookmarks" | "bookmark" => Some(Self::Bookmarks),
            "color_rules" | "colors" => Some(Self::ColorRules),
            "auto_suggest_sims" | "auto_suggest" => Some(Self::AutoSuggestSims),
            _ => None,
        }
    }
}

/// Snapshot of which Pro capabilities are unlocked.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Entitlements {
    pub multi_graph: bool,
    pub mcp_server: bool,
    pub webhooks: bool,
    pub simulation: bool,
    pub scenario_player: bool,
    pub record_simulate: bool,
    pub tree_export: bool,
    pub history_scrubber: bool,
    pub csv_export: bool,
    pub bookmarks: bool,
    pub color_rules: bool,
    pub auto_suggest_sims: bool,
}

impl Entitlements {
    pub fn free() -> Self {
        Self::default()
    }

    pub fn all_enabled() -> Self {
        Self {
            multi_graph: true,
            mcp_server: true,
            webhooks: true,
            simulation: true,
            scenario_player: true,
            record_simulate: true,
            tree_export: true,
            history_scrubber: true,
            csv_export: true,
            bookmarks: true,
            color_rules: true,
            auto_suggest_sims: true,
        }
    }

    pub fn get(self, flag: Entitlement) -> bool {
        match flag {
            Entitlement::MultiGraph => self.multi_graph,
            Entitlement::McpServer => self.mcp_server,
            Entitlement::Webhooks => self.webhooks,
            Entitlement::Simulation => self.simulation,
            Entitlement::ScenarioPlayer => self.scenario_player,
            Entitlement::RecordSimulate => self.record_simulate,
            Entitlement::TreeExport => self.tree_export,
            Entitlement::HistoryScrubber => self.history_scrubber,
            Entitlement::CsvExport => self.csv_export,
            Entitlement::Bookmarks => self.bookmarks,
            Entitlement::ColorRules => self.color_rules,
            Entitlement::AutoSuggestSims => self.auto_suggest_sims,
        }
    }

    fn enable(&mut self, flag: Entitlement) {
        match flag {
            Entitlement::MultiGraph => self.multi_graph = true,
            Entitlement::McpServer => self.mcp_server = true,
            Entitlement::Webhooks => self.webhooks = true,
            Entitlement::Simulation => self.simulation = true,
            Entitlement::ScenarioPlayer => self.scenario_player = true,
            Entitlement::RecordSimulate => self.record_simulate = true,
            Entitlement::TreeExport => self.tree_export = true,
            Entitlement::HistoryScrubber => self.history_scrubber = true,
            Entitlement::CsvExport => self.csv_export = true,
            Entitlement::Bookmarks => self.bookmarks = true,
            Entitlement::ColorRules => self.color_rules = true,
            Entitlement::AutoSuggestSims => self.auto_suggest_sims = true,
        }
    }

    fn from_license_payload(payload: &LicensePayload) -> Self {
        let mut ents = Self::free();
        for token in &payload.entitlements {
            if let Some(flag) = Entitlement::from_token(token) {
                ents.enable(flag);
            }
        }
        ents
    }
}

/// Install / license state for Settings copy (never claims Licensed without a real key).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)] // Free used when sidecar handshake lands.
pub enum ProStatus {
    /// Pro companion not detected (default OSS scaffold).
    NotInstalled,
    /// Pro present but no valid license (future sidecar handshake).
    Free,
    /// Valid signed license file (or future sidecar).
    Licensed,
}

impl ProStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::NotInstalled => "Not installed",
            Self::Free => "Free",
            Self::Licensed => "Licensed",
        }
    }
}

/// Capability probe — ready for localhost sidecar IPC after file license.
#[allow(dead_code)] // Sidecar / plugin probes will implement this.
pub trait CapabilityProbe: Send + Sync {
    fn status(&self) -> ProStatus;
    fn entitlements(&self) -> Entitlements;
}

/// OSS-side bridge. Handshake / IPC TODO for TopicBench Pro sidecar.
pub struct ProBridge {
    /// Entitlements after DEV / license resolution (UI gates use these).
    entitlements: Entitlements,
    status: ProStatus,
}

impl ProBridge {
    /// Probe Pro. Order: DEV env → signed license file → free / not installed.
    pub fn probe() -> Self {
        // 1) DEV overrides win for local UI preview (status stays NotInstalled —
        //    Settings must never show “Licensed” from env alone).
        if let Some(ents) = try_dev_entitlements() {
            return Self {
                entitlements: ents,
                status: ProStatus::NotInstalled,
            };
        }

        // 2) Signed offline license under ~/.config/topicbench/
        // TODO(pro-ipc): after file license, optionally also handshake sidecar;
        // do not treat “Pro binary exists” as licensed without entitlements.
        if let Some(payload) = try_load_signed_license() {
            let entitlements = Entitlements::from_license_payload(&payload);
            // Optional: TOPICBENCH_PRO_DEBUG=1 → stderr probe line (no Settings UI yet).
            if env_truthy("TOPICBENCH_PRO_DEBUG") {
                eprintln!(
                    "topicbench pro: Licensed (file) multi_graph={} mcp_server={} simulation={}",
                    entitlements.multi_graph, entitlements.mcp_server, entitlements.simulation
                );
            }
            return Self {
                entitlements,
                status: ProStatus::Licensed,
            };
        }

        // 3) Free OSS
        Self {
            entitlements: Entitlements::free(),
            status: ProStatus::NotInstalled,
        }
    }

    pub fn status(&self) -> ProStatus {
        self.status
    }

    #[allow(dead_code)] // Settings / IPC callers will read the full snapshot.
    pub fn entitlements(&self) -> Entitlements {
        self.entitlements
    }

    pub fn has(&self, flag: Entitlement) -> bool {
        self.entitlements.get(flag)
    }
}

impl CapabilityProbe for ProBridge {
    fn status(&self) -> ProStatus {
        self.status
    }

    fn entitlements(&self) -> Entitlements {
        self.entitlements
    }
}

/// Process-wide bridge (re-probed once at first use; DEV / license read at that time).
/// Future Settings Activate should document restart, or clear this OnceLock / re-probe.
pub fn bridge() -> &'static ProBridge {
    static BRIDGE: OnceLock<ProBridge> = OnceLock::new();
    BRIDGE.get_or_init(ProBridge::probe)
}

/// UI helper: is this entitlement unlocked?
pub fn has(flag: Entitlement) -> bool {
    bridge().has(flag)
}

/// Settings / about label — install/license only (ignores DEV override).
/// Not shown in Settings until more Pro features ship; kept for future UI.
#[allow(dead_code)]
pub fn status_label() -> &'static str {
    bridge().status().label()
}

/// DEV-only entitlements when env is set; `None` → fall through to license file.
fn try_dev_entitlements() -> Option<Entitlements> {
    // TOPICBENCH_PRO_DEV=1|true|yes → all flags on (UI preview).
    if env_truthy("TOPICBENCH_PRO_DEV") {
        return Some(Entitlements::all_enabled());
    }
    // TOPICBENCH_PRO_ENTITLEMENTS=multi_graph,mcp_server
    let Ok(raw) = std::env::var("TOPICBENCH_PRO_ENTITLEMENTS") else {
        return None;
    };
    if raw.trim().is_empty() {
        return None;
    }
    let mut ents = Entitlements::free();
    for token in raw.split(',') {
        if let Some(flag) = Entitlement::from_token(token) {
            ents.enable(flag);
        }
    }
    Some(ents)
}

/// Try `license.json` then `license.lic`. Invalid/expired/missing → None (free).
fn try_load_signed_license() -> Option<LicensePayload> {
    for name in [LICENSE_FILE_NAME, LICENSE_FILE_ALT] {
        let path = paths::config_file(name);
        match load_license_path(&path) {
            Ok(Some(payload)) => return Some(payload),
            Ok(None) => continue,
            Err(_) => {
                // Bad signature / expired / corrupt — ignore and try next / fall free.
                // (No Settings UI yet to surface the error; keep honest-user quiet.)
                continue;
            }
        }
    }
    None
}

fn env_truthy(key: &str) -> bool {
    match std::env::var(key) {
        Ok(v) => {
            let v = v.trim();
            v == "1" || v.eq_ignore_ascii_case("true") || v.eq_ignore_ascii_case("yes")
        }
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn free_defaults_false() {
        let e = Entitlements::free();
        assert!(!e.multi_graph);
        assert!(!e.mcp_server);
        assert!(!e.simulation);
    }

    #[test]
    fn token_parse() {
        assert_eq!(
            Entitlement::from_token("multi_graph"),
            Some(Entitlement::MultiGraph)
        );
        assert_eq!(
            Entitlement::from_token("MCP"),
            Some(Entitlement::McpServer)
        );
    }

    #[test]
    fn license_payload_maps_flags() {
        let payload = LicensePayload {
            entitlements: vec!["multi_graph".into(), "simulation".into()],
            expires: None,
            licensee: None,
        };
        let e = Entitlements::from_license_payload(&payload);
        assert!(e.multi_graph);
        assert!(e.simulation);
        assert!(!e.mcp_server);
    }
}
