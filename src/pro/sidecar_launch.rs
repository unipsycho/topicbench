//! Keep `topicbench-pro` running while TopicBench is open and Pro is licensed.
//!
//! A small supervisor thread checks every few seconds: if nothing answers on the sidecar
//! port it finds the binary and starts it, and if our child dies it starts it again (with
//! backoff). Free / unlicensed installs never spawn. A sidecar someone else started is left
//! alone. A child we spawned is killed when this handle is dropped (GUI quit).

use std::fs::OpenOptions;
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

const DEFAULT_PORT: u16 = 7853;
const PORT_PROBE: Duration = Duration::from_millis(150);
/// How often the supervisor looks at the port.
const CHECK_EVERY: Duration = Duration::from_secs(4);
/// Pause before respawning a child that keeps dying (doubles up to the cap).
const BACKOFF_START: Duration = Duration::from_secs(4);
const BACKOFF_MAX: Duration = Duration::from_secs(60);

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
    stop: Arc<AtomicBool>,
}

impl SidecarLauncher {
    /// Begin discovery/spawn on a background thread (never blocks the UI).
    /// Call only when [`crate::pro::is_licensed`] is true, or pass through
    /// [`Self::start_if_licensed`].
    pub fn start_if_licensed() -> Option<Self> {
        // The supervisor re-checks the licence on every pass, so this is always started:
        // a licence activated (or a sidecar installed) after launch is picked up too.
        Some(Self::start())
    }

    fn start() -> Self {
        let inner = Arc::new(Mutex::new(Inner {
            child: None,
            status: LaunchStatus::Idle,
        }));
        let stop = Arc::new(AtomicBool::new(false));
        let slot = Arc::clone(&inner);
        let flag = Arc::clone(&stop);
        let _ = thread::Builder::new()
            .name("tb-pro-launch".into())
            .spawn(move || supervise(slot, flag));
        Self { inner, stop }
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
        self.stop.store(true, Ordering::SeqCst);
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

fn supervise(slot: Arc<Mutex<Inner>>, stop: Arc<AtomicBool>) {
    let mut backoff = BACKOFF_START;
    let mut next_spawn = Instant::now();
    let mut warned_missing = false;
    while !stop.load(Ordering::SeqCst) {
        tick(&slot, &mut backoff, &mut next_spawn, &mut warned_missing);
        // Sleep in short slices so quitting the GUI is not held up.
        let until = Instant::now() + CHECK_EVERY;
        while Instant::now() < until && !stop.load(Ordering::SeqCst) {
            thread::sleep(Duration::from_millis(250));
        }
    }
}

fn tick(
    slot: &Arc<Mutex<Inner>>,
    backoff: &mut Duration,
    next_spawn: &mut Instant,
    warned_missing: &mut bool,
) {
    if !crate::pro::is_licensed() {
        return;
    }
    let port = sidecar_port();

    // Reap our child if it exited, so a crash is noticed.
    let mut child_alive = false;
    if let Ok(mut g) = slot.lock() {
        if let Some(child) = g.child.as_mut() {
            match child.try_wait() {
                Ok(None) => child_alive = true,
                Ok(Some(status)) => {
                    eprintln!("topicbench: Pro sidecar exited ({status}); will restart");
                    g.child = None;
                    g.status = LaunchStatus::Failed(format!("sidecar exited ({status})"));
                }
                Err(_) => child_alive = true,
            }
        }
    }

    if port_open(port) {
        let ours = child_alive;
        if let Ok(mut g) = slot.lock() {
            g.status = if ours {
                LaunchStatus::Started
            } else {
                LaunchStatus::AlreadyRunning
            };
        }
        if ours {
            *backoff = BACKOFF_START; // stayed up: reset
        }
        return;
    }
    // Our own child is still starting up (port not open yet): give it time.
    if child_alive || Instant::now() < *next_spawn {
        return;
    }

    let Some(exe) = resolve_pro_binary() else {
        set_status(slot, LaunchStatus::NotInstalled);
        if !*warned_missing {
            *warned_missing = true;
            eprintln!(
                "topicbench: Pro licensed but topicbench-pro not found \
                 (set TOPICBENCH_PRO_PATH to override); will keep looking"
            );
        }
        return;
    };
    *warned_missing = false;

    match spawn_pro(&exe) {
        Ok(child) => {
            if let Ok(mut g) = slot.lock() {
                g.child = Some(child);
                g.status = LaunchStatus::Started;
            }
            eprintln!("topicbench: started Pro sidecar ({})", exe.display());
            // If it dies right away, wait longer before the next attempt.
            *next_spawn = Instant::now() + *backoff;
            *backoff = (*backoff * 2).min(BACKOFF_MAX);
        }
        Err(e) => {
            set_status(slot, LaunchStatus::Failed(e.clone()));
            eprintln!("topicbench: failed to start Pro sidecar: {e}");
            *next_spawn = Instant::now() + *backoff;
            *backoff = (*backoff * 2).min(BACKOFF_MAX);
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

    // Common install spots, so a sidecar installed beside neither the GUI nor the cwd is found.
    for dir in extra_search_dirs() {
        for name in &names {
            let cand = dir.join(name);
            if cand.is_file() {
                return Some(cand);
            }
        }
    }

    None
}

fn extra_search_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(cfg) = crate::mqtt::paths::config_file("pro").parent() {
        dirs.push(cfg.join("pro"));
    }
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        dirs.push(home.join("Applications/TopicBench"));
        dirs.push(home.join(".local/bin"));
    }
    if let Some(local) = std::env::var_os("LOCALAPPDATA").map(PathBuf::from) {
        dirs.push(local.join("TopicBench"));
        dirs.push(local.join("Programs").join("TopicBench"));
    }
    if let Some(path) = std::env::var_os("PATH") {
        dirs.extend(std::env::split_paths(&path));
    }
    dirs
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
