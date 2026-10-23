//! Entitlement probe for gated features (verify + `has()`).
//!
//! Localhost IPC for Pro MCP lives in [`bridge`] — started only when
//! [`Entitlement::McpServer`] is unlocked (no Settings Pro panel yet).

mod license_format;
pub mod bridge;

use std::sync::OnceLock;

use crate::mqtt::paths;
use license_format::{load_license_path, LicensePayload, LICENSE_FILE_ALT, LICENSE_FILE_NAME};

/// Named capability flags.
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

/// Snapshot of which capabilities are unlocked.
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

/// Install / license state label (for future UI).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum ProStatus {
    NotInstalled,
    Free,
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

#[allow(dead_code)]
pub trait CapabilityProbe: Send + Sync {
    fn status(&self) -> ProStatus;
    fn entitlements(&self) -> Entitlements;
}

/// Process entitlement bridge (file verify + optional local overrides).
pub struct ProBridge {
    entitlements: Entitlements,
    status: ProStatus,
}

impl ProBridge {
    /// Resolve entitlements for UI gates.
    pub fn probe() -> Self {
        if let Some(ents) = try_dev_entitlements() {
            return Self {
                entitlements: ents,
                status: ProStatus::NotInstalled,
            };
        }

        if let Some(payload) = try_load_signed_license() {
            let entitlements = Entitlements::from_license_payload(&payload);
            if env_truthy("TOPICBENCH_PRO_DEBUG") {
                eprintln!(
                    "topicbench: licensed multi_graph={} mcp_server={} simulation={}",
                    entitlements.multi_graph, entitlements.mcp_server, entitlements.simulation
                );
            }
            return Self {
                entitlements,
                status: ProStatus::Licensed,
            };
        }

        Self {
            entitlements: Entitlements::free(),
            status: ProStatus::NotInstalled,
        }
    }

    pub fn status(&self) -> ProStatus {
        self.status
    }

    #[allow(dead_code)]
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

/// Process-wide bridge (probed once at first use).
pub fn bridge() -> &'static ProBridge {
    static BRIDGE: OnceLock<ProBridge> = OnceLock::new();
    BRIDGE.get_or_init(ProBridge::probe)
}

/// True when entitlement is unlocked.
pub fn has(flag: Entitlement) -> bool {
    bridge().has(flag)
}

#[allow(dead_code)]
pub fn status_label() -> &'static str {
    bridge().status().label()
}

fn try_dev_entitlements() -> Option<Entitlements> {
    if env_truthy("TOPICBENCH_PRO_DEV") {
        return Some(Entitlements::all_enabled());
    }
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

fn try_load_signed_license() -> Option<LicensePayload> {
    for name in [LICENSE_FILE_NAME, LICENSE_FILE_ALT] {
        let path = paths::config_file(name);
        match load_license_path(&path) {
            Ok(Some(payload)) => return Some(payload),
            Ok(None) => continue,
            Err(_) => continue,
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
