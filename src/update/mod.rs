//! Check-for-updates: GitHub Releases, optional download/verify/apply.

mod github;
mod install;
pub mod notes_ui;

use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub use github::{open_url, ReleaseInfo};
pub use install::InstallOutcome;

/// How long a successful/failed check stays cached before a quiet startup recheck.
pub const CHECK_CACHE: Duration = Duration::from_secs(24 * 60 * 60);
/// "Remind me later" dismiss window (persisted).
pub const REMIND_LATER: Duration = Duration::from_secs(24 * 60 * 60);
/// Cap release notes shown in the update modal.
pub const NOTES_MAX_CHARS: usize = 2_000;

/// Result delivered from a background check or install thread.
#[derive(Debug, Clone)]
pub enum UpdateEvent {
    CheckOk {
        info: Option<ReleaseInfo>,
        /// True when latest tag is newer than the running app.
        update_available: bool,
    },
    CheckFailed(String),
    InstallDone(InstallOutcome),
}

/// UI-facing status for Settings → Updates.
#[derive(Debug, Clone, Default)]
pub enum UpdateStatus {
    #[default]
    Idle,
    Checking,
    UpToDate {
        latest: String,
    },
    Available(ReleaseInfo),
    /// Check failed — quiet on startup; Settings may show the message.
    Error(String),
    Installing,
    InstallFailed(String),
}

impl UpdateStatus {
    pub fn settings_line(&self) -> String {
        match self {
            Self::Idle => "Not checked yet.".into(),
            Self::Checking => "Checking…".into(),
            Self::UpToDate { latest } => format!("Up to date (latest {latest})."),
            Self::Available(info) => format!("Update available: {}.", info.tag),
            Self::Error(msg) => format!("Couldn't check: {msg}"),
            Self::Installing => "Installing update…".into(),
            Self::InstallFailed(msg) => format!("Install failed: {msg}"),
        }
    }
}

/// Background worker handle — poll `try_recv` from the UI thread.
pub struct UpdateService {
    tx: Sender<UpdateEvent>,
    rx: Receiver<UpdateEvent>,
}

impl UpdateService {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        Self { tx, rx }
    }

    pub fn try_recv(&self) -> Option<UpdateEvent> {
        self.rx.try_recv().ok()
    }

    /// Spawn a GitHub Releases check (never blocks the UI).
    pub fn start_check(&self) {
        let tx = self.tx.clone();
        let _ = thread::Builder::new()
            .name("update-check".into())
            .spawn(move || {
                let event = match github::fetch_latest() {
                    Ok(info) => {
                        let update_available = is_newer(&info.version, current_version());
                        UpdateEvent::CheckOk {
                            info: Some(info),
                            update_available,
                        }
                    }
                    Err(e) => UpdateEvent::CheckFailed(e),
                };
                let _ = tx.send(event);
            });
    }

    /// Download + verify + replace binary, or fall back to opening the release page.
    pub fn start_install(&self, info: ReleaseInfo) {
        let tx = self.tx.clone();
        let _ = thread::Builder::new()
            .name("update-install".into())
            .spawn(move || {
                let outcome = install::apply_update(&info);
                let _ = tx.send(UpdateEvent::InstallDone(outcome));
            });
    }
}

impl Default for UpdateService {
    fn default() -> Self {
        Self::new()
    }
}

/// Running app version from Cargo.toml.
pub fn current_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// True when `latest` is a valid semver strictly greater than `current`.
pub fn is_newer(latest: &str, current: &str) -> bool {
    let Ok(latest) = semver::Version::parse(strip_v(latest)) else {
        return false;
    };
    let Ok(current) = semver::Version::parse(strip_v(current)) else {
        return false;
    };
    latest > current
}

fn strip_v(s: &str) -> &str {
    s.strip_prefix('v').unwrap_or(s)
}

pub fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Whether a quiet startup check should run (24h cache).
pub fn should_auto_check(last_check_unix: Option<u64>) -> bool {
    match last_check_unix {
        None => true,
        Some(ts) => now_unix().saturating_sub(ts) >= CHECK_CACHE.as_secs(),
    }
}

/// True if "Remind me later" is still active.
pub fn remind_later_active(until_unix: Option<u64>) -> bool {
    match until_unix {
        Some(until) => now_unix() < until,
        None => false,
    }
}

/// Truncate release body for the modal.
pub fn truncate_notes(body: &str) -> String {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return "(No release notes.)".into();
    }
    if trimmed.chars().count() <= NOTES_MAX_CHARS {
        return trimmed.to_string();
    }
    let mut out: String = trimmed.chars().take(NOTES_MAX_CHARS).collect();
    out.push_str("…");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semver_compare_strips_v() {
        assert!(is_newer("v1.1.0", "1.0.0"));
        assert!(is_newer("1.0.1", "v1.0.0"));
        assert!(!is_newer("1.0.0", "1.0.0"));
        assert!(!is_newer("1.0.0", "1.1.0"));
    }

    #[test]
    fn notes_truncate() {
        let short = truncate_notes(" hello ");
        assert_eq!(short, "hello");
        let long = "x".repeat(NOTES_MAX_CHARS + 10);
        let t = truncate_notes(&long);
        assert!(t.ends_with('…'));
        assert_eq!(t.chars().count(), NOTES_MAX_CHARS + 1);
    }
}
