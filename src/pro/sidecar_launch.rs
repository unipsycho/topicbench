//! Auto-start `topicbench-pro` when licensed and the binary sits next to TopicBench.
//!
//! Free / unlicensed installs: no spawn. Already-running sidecar: leave it alone.
//! If we spawned the child this session, kill it when this handle is dropped (GUI quit).

use std::fs::OpenOptions;
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

const DEFAULT_PORT: u16 = 7853;
const PORT_PROBE: Duration = Duration::from_millis(150);

/// Outcome / status for Settings or diagnostics (best-effort).
#[derive(Clone, Debug, Default)]
pub enum LaunchStatus {
    #[default]
    Idle,
    /// Licensed but no `topicbench-pro` next to the GUI (or env override missing).
    NotInstalled,
    /// Port already accepting — we did not spawn.
    AlreadyRunning,
    /// We spawned the sidecar this session.
    Started,
    /// Spawn failed (permissions, AV, missing deps).
    Failed(String),
}

struct Inner {
    child: Option<Child>,
    status: LaunchStatus,
}

/// Owns an optional Pro child process. Drop kills only a child we started.
pub struct SidecarLauncher {
    inner: Arc<Mutex<Inner>>,
}

impl SidecarLauncher {
    /// Begin discovery/spawn on a background thread (never blocks the UI).
    /// Call only when [`crate::pro::is_licensed`] is true, or pass through
    /// [`Self::start_if_licensed`].
    pub fn start_if_licensed() -> Option<Self> {
        if !crate::pro::is_licensed() {
            return None;
        }
        Some(Self::start())
    }

    fn start() -> Self {
        let inner = Arc::new(Mutex::new(Inner {
            child: None,
            status: LaunchStatus::Idle,
        }));
        let slot = Arc::clone(&inner);
        let _ = thread::Builder::new()
            .name("tb-pro-launch".into())
            .spawn(move || launch_worker(slot));
        Self { inner }
    }

    /// For Settings / diagnostics (not painted yet).
    #[allow(dead_code)]
    pub fn status(&self) -> LaunchStatus {
        self.inner
            .lock()
            .map(|g| g.status.clone())
            .unwrap_or_default()
    }
}

impl Drop for SidecarLauncher {
    fn drop(&mut self) {
        let Ok(mut g) = self.inner.lock() else {
            return;
        };
        if let Some(mut child) = g.child.take() {
            let _ = child.kill();
            let _ = child.wait();
            eprintln!("topicbench: stopped Pro sidecar (we started it)");
        }
    }
}

fn launch_worker(slot: Arc<Mutex<Inner>>) {
    let Some(exe) = resolve_pro_binary() else {
        set_status(&slot, LaunchStatus::NotInstalled);
        eprintln!(
            "topicbench: Pro licensed but topicbench-pro not found next to the app \
             (set TOPICBENCH_PRO_PATH to override)"
        );
        return;
    };

    let port = sidecar_port();
    if port_open(port) {
        set_status(&slot, LaunchStatus::AlreadyRunning);
        eprintln!(
            "topicbench: Pro sidecar already on 127.0.0.1:{port} — not spawning"
        );
        return;
    }

    match spawn_pro(&exe) {
        Ok(child) => {
            if let Ok(mut g) = slot.lock() {
                g.child = Some(child);
                g.status = LaunchStatus::Started;
            }
            eprintln!(
                "topicbench: started Pro sidecar ({})",
                exe.display()
            );
        }
        Err(e) => {
            set_status(&slot, LaunchStatus::Failed(e.clone()));
            eprintln!("topicbench: failed to start Pro sidecar: {e}");
        }
    }
}

fn set_status(slot: &Arc<Mutex<Inner>>, status: LaunchStatus) {
    if let Ok(mut g) = slot.lock() {
        g.status = status;
    }
}

/// Resolve Pro binary: env override, then next to `current_exe`, then cwd.
pub fn resolve_pro_binary() -> Option<PathBuf> {
    for key in ["TOPICBENCH_PRO_PATH", "TOPICBENCH_PRO_EXE"] {
        if let Ok(p) = std::env::var(key) {
            let p = PathBuf::from(p.trim());
            if !p.as_os_str().is_empty() && p.is_file() {
                return Some(p);
            }
        }
    }

    let names = pro_binary_names();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            for name in &names {
                let cand = dir.join(name);
                if cand.is_file() {
                    return Some(cand);
                }
            }
        }
    }

    if let Ok(cwd) = std::env::current_dir() {
        for name in &names {
            let cand = cwd.join(name);
            if cand.is_file() {
                return Some(cand);
            }
        }
    }

    None
}

fn pro_binary_names() -> Vec<&'static str> {
    #[cfg(windows)]
    {
        vec!["topicbench-pro.exe", "TopicBench-Pro.exe"]
    }
    #[cfg(not(windows))]
    {
        vec!["topicbench-pro", "TopicBench-Pro"]
    }
}

fn sidecar_port() -> u16 {
    if let Ok(url) = std::env::var("TOPICBENCH_PRO_URL") {
        if let Some(port) = parse_port_from_url(&url) {
            return port;
        }
    }
    if let Ok(p) = std::env::var("TOPICBENCH_MCP_PORT") {
        if let Ok(n) = p.trim().parse() {
            return n;
        }
    }
    DEFAULT_PORT
}

fn parse_port_from_url(url: &str) -> Option<u16> {
    // http://127.0.0.1:7853 or http://127.0.0.1:7853/
    let rest = url.trim().trim_end_matches('/');
    let after_scheme = rest.split("://").nth(1)?;
    let hostport = after_scheme.split('/').next()?;
    let port_str = hostport.rsplit(':').next()?;
    port_str.parse().ok()
}

fn port_open(port: u16) -> bool {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    TcpStream::connect_timeout(&addr, PORT_PROBE).is_ok()
}

fn spawn_pro(exe: &Path) -> Result<Child, String> {
    let mut cmd = Command::new(exe);
    // Same cwd as GUI so Windows portable license paths resolve.
    if let Ok(cwd) = std::env::current_dir() {
        cmd.current_dir(cwd);
    }
    cmd.stdin(Stdio::null());

    // Append stderr to a small log; stdout discarded.
    let log_path = crate::mqtt::paths::config_file("pro-sidecar.log");
    if let Some(parent) = log_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    match OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
    {
        Ok(file) => {
            cmd.stdout(Stdio::null());
            cmd.stderr(Stdio::from(file));
        }
        Err(_) => {
            cmd.stdout(Stdio::null());
            cmd.stderr(Stdio::null());
        }
    }

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // Hide console window for the GUI-launched sidecar.
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    cmd.spawn()
        .map_err(|e| format!("{}: {e}", exe.display()))
}

#[cfg(test)]
mod tests {
    use super::parse_port_from_url;

    #[test]
    fn parses_pro_url_port() {
        assert_eq!(parse_port_from_url("http://127.0.0.1:7853"), Some(7853));
        assert_eq!(parse_port_from_url("http://127.0.0.1:7853/"), Some(7853));
        assert_eq!(parse_port_from_url("http://localhost:9000/mcp"), Some(9000));
    }
}
