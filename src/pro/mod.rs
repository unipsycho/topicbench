//! Pro license probe for gated features (`is_licensed` / `has()`).
//!
//! One licensed bit unlocks **all** Pro features — no per-feature list in the
//! license file. Feature-named checks (`has(MultiGraph)`, `has(McpServer)`)
//! remain for UI clarity but all follow that single bit.
//!
//! Localhost IPC for Pro MCP lives in [`bridge`] — started only when licensed.

mod license_format;
pub mod machine_id;
pub mod bookmarks;
pub mod bridge;
pub mod bridge_pro_tools;
pub mod color_rules;
pub mod history_client;
pub mod recording;
pub mod sidecar_launch;
pub mod sim_client;
pub mod topic_tree_io;
pub mod tree_client;
pub mod update_client;

use std::sync::OnceLock;

use crate::mqtt::paths;
use license_format::{load_license_path, LICENSE_FILE_ALT, LICENSE_FILE_NAME};

pub use license_format::{
    write_license_request, LicenseRequestV1, LICENSE_REQUEST_FILE_NAME,
};

/// Named capability flags (UI / call sites). All true when Pro is licensed.
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
    /// Topic bookmarks / jump list (UI name TopicBookmarks).
    TopicBookmarks,
    ColorRules,
    AutoSuggestSims,
}

/// Snapshot of which capabilities are unlocked (all-or-nothing).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Entitlements {
    licensed: bool,
}

impl Entitlements {
    pub fn free() -> Self {
        Self { licensed: false }
    }

    /// Full Pro — every feature unlocked.
    pub fn all() -> Self {
        Self { licensed: true }
    }

    #[allow(dead_code)]
    pub fn all_enabled() -> Self {
        Self::all()
    }

    pub fn is_licensed(self) -> bool {
        self.licensed
    }

    pub fn get(self, _flag: Entitlement) -> bool {
        self.licensed
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
    /// Resolve Pro license for UI gates (all features or none).
    pub fn probe() -> Self {
        if try_dev_unlock() {
            return Self {
                entitlements: Entitlements::all(),
                status: ProStatus::NotInstalled,
            };
        }

        if try_load_signed_license() {
            if env_truthy("TOPICBENCH_PRO_DEBUG") {
                eprintln!("topicbench: Pro licensed (all features)");
            }
            return Self {
                entitlements: Entitlements::all(),
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

    pub fn is_licensed(&self) -> bool {
        self.entitlements.is_licensed()
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

/// True when Pro is licensed (all features available).
pub fn is_licensed() -> bool {
    bridge().is_licensed()
}

/// True when entitlement is unlocked (same as [`is_licensed`] for every flag).
pub fn has(flag: Entitlement) -> bool {
    bridge().has(flag)
}

#[allow(dead_code)]
pub fn status_label() -> &'static str {
    bridge().status().label()
}

/// DEV preview: any truthy `TOPICBENCH_PRO_DEV`, or non-empty `TOPICBENCH_PRO_ENTITLEMENTS`.
fn try_dev_unlock() -> bool {
    if env_truthy("TOPICBENCH_PRO_DEV") {
        return true;
    }
    match std::env::var("TOPICBENCH_PRO_ENTITLEMENTS") {
        Ok(raw) => !raw.trim().is_empty(),
        Err(_) => false,
    }
}

fn try_load_signed_license() -> bool {
    // Portable Windows: cwd .\topicbench\ first; then USERPROFILE\.config\topicbench.
    let mut tried = Vec::new();
    for dir in paths::license_search_dirs() {
        for name in [LICENSE_FILE_NAME, LICENSE_FILE_ALT] {
            let path = dir.join(name);
            tried.push(path.clone());
            match load_license_path(&path) {
                Ok(Some(_)) => return true,
                Ok(None) => continue,
                Err(e) => {
                    // Found but invalid — never collapse into silent "missing".
                    eprintln!(
                        "topicbench: Pro license invalid ({}) — {}",
                        e.customer_reason(),
                        paths::abs_display(&path)
                    );
                    return false;
                }
            }
        }
    }
    if env_truthy("TOPICBENCH_PRO_DEBUG") {
        let list = tried
            .iter()
            .map(|p| paths::abs_display(p))
            .collect::<Vec<_>>()
            .join(", ");
        eprintln!("topicbench: Pro license not found — looked for: {list}");
    }
    false
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
        assert!(!e.is_licensed());
        assert!(!e.get(Entitlement::MultiGraph));
        assert!(!e.get(Entitlement::McpServer));
    }

    #[test]
    fn all_unlocks_every_flag() {
        let e = Entitlements::all();
        assert!(e.is_licensed());
        assert!(e.get(Entitlement::MultiGraph));
        assert!(e.get(Entitlement::McpServer));
        assert!(e.get(Entitlement::Simulation));
        assert!(e.get(Entitlement::TreeExport));
    }
}
