//! Fetch / cache Pro UI themes from the sidecar (`GET /themes`, `GET /themes/:id`).
//!
//! UI thread never blocks on the poller. [`ThemeClient::preview`] applies from cache
//! (or a short sync fetch) so Settings ComboBox changes show immediately.
//! Sidecar down → Classic.

use crate::ui::theme::{self, ChromeTokens, CLASSIC_ID};
use eframe::egui::Context;
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

const DEFAULT_BASE: &str = "http://127.0.0.1:7853";
const POLL_INTERVAL: Duration = Duration::from_secs(5);
const BACKOFF_START: Duration = Duration::from_secs(2);
const BACKOFF_MAX: Duration = Duration::from_secs(30);

/// Catalog row from `GET /themes`.
#[derive(Clone, Debug, Deserialize)]
pub struct ThemeInfo {
    pub id: String,
    pub name: String,
}

struct Shared {
    sidecar_up: bool,
    catalog: Vec<ThemeInfo>,
    /// Cached full tokens by id (Pro presets only).
    tokens: HashMap<String, ChromeTokens>,
    /// Last requested theme id (from settings / preview).
    wanted_id: String,
    /// Tokens currently applied (id + payload).
    applied_id: String,
    applied: ChromeTokens,
    /// Bumped when applied tokens change — UI re-calls apply_theme via [`ThemeClient::pump`].
    gen: u64,
}

impl Shared {
    fn new() -> Self {
        let classic = theme::classic();
        Self {
            sidecar_up: false,
            catalog: Vec::new(),
            tokens: HashMap::new(),
            wanted_id: CLASSIC_ID.to_string(),
            applied_id: CLASSIC_ID.to_string(),
            applied: classic,
            gen: 0,
        }
    }
}

enum Cmd {
    /// Refresh catalog + ensure wanted theme tokens.
    Tick,
    /// Settings / startup changed the preferred id.
    SetWanted(String),
}

/// Background theme catalog + token cache.
pub struct ThemeClient {
    shared: Arc<Mutex<Shared>>,
    tx: SyncSender<Cmd>,
    /// Last gen we pushed into egui.
    last_applied_gen: u64,
}

impl ThemeClient {
    pub fn new() -> Self {
        let shared = Arc::new(Mutex::new(Shared::new()));
        let (tx, rx) = mpsc::sync_channel::<Cmd>(32);
        let shared_bg = Arc::clone(&shared);
        thread::Builder::new()
            .name("tb-theme-poll".into())
            .spawn(move || poll_loop(shared_bg, rx))
            .ok();
        // Kick first fetch soon.
        let _ = tx.try_send(Cmd::Tick);
        Self {
            shared,
            tx,
            last_applied_gen: 0,
        }
    }

    /// Prefer this theme id (persisted settings). Classic when empty / unknown.
    pub fn set_wanted(&self, id: &str) {
        let id = normalize_id(id);
        if let Ok(mut g) = self.shared.lock() {
            if g.wanted_id == id {
                return;
            }
            g.wanted_id = id.clone();
        }
        // Wake poller; ignore full channel (preview already applied on UI thread).
        let _ = self.tx.try_send(Cmd::SetWanted(id));
    }

    /// Apply `id` immediately (Settings ComboBox preview / Save / Cancel revert).
    ///
    /// Classic is always available. Pro ids use cache, else a short sync GET; on
    /// failure falls back to Classic look while keeping `wanted_id` for retry.
    pub fn preview(&mut self, ctx: &Context, id: &str) {
        let id = normalize_id(id);
        if let Ok(mut g) = self.shared.lock() {
            g.wanted_id = id.clone();
        }
        let _ = self.tx.try_send(Cmd::SetWanted(id.clone()));

        let resolved = if id == CLASSIC_ID {
            Some((CLASSIC_ID.to_string(), theme::classic()))
        } else {
            let cached = self
                .shared
                .lock()
                .ok()
                .and_then(|g| g.tokens.get(&id).cloned());
            if let Some(t) = cached {
                Some((id.clone(), t))
            } else {
                // One-shot sync fetch so first ComboBox pick isn't delayed on poller.
                match fetch_theme_tokens(&id) {
                    Ok(t) => {
                        if let Ok(mut g) = self.shared.lock() {
                            g.tokens.insert(id.clone(), t);
                            g.sidecar_up = true;
                        }
                        Some((id.clone(), t))
                    }
                    Err(_) => None,
                }
            }
        };

        // Missing Pro tokens → Classic look (wanted_id still set for poller retry).
        let (applied_id, apply) =
            resolved.unwrap_or_else(|| (CLASSIC_ID.to_string(), theme::classic()));
        if let Ok(mut g) = self.shared.lock() {
            if g.applied_id != applied_id || g.applied != apply {
                g.applied_id = applied_id;
                g.applied = apply;
                g.gen = g.gen.wrapping_add(1);
            }
            self.last_applied_gen = g.gen;
        }
        theme::apply_theme(ctx, &apply);
        ctx.request_repaint();
    }

    /// Pro catalog when sidecar is up; empty otherwise.
    pub fn catalog(&self) -> Vec<ThemeInfo> {
        self.shared
            .lock()
            .map(|g| g.catalog.clone())
            .unwrap_or_default()
    }

    pub fn sidecar_up(&self) -> bool {
        self.shared
            .lock()
            .map(|g| g.sidecar_up)
            .unwrap_or(false)
    }

    /// Apply pending poller updates (e.g. prefetch finished after preview miss).
    pub fn pump(&mut self, ctx: &Context) {
        let (gen, tokens) = match self.shared.lock() {
            Ok(g) => (g.gen, g.applied),
            Err(_) => return,
        };
        if gen != self.last_applied_gen {
            theme::apply_theme(ctx, &tokens);
            self.last_applied_gen = gen;
            ctx.request_repaint();
        }
    }
}

fn normalize_id(id: &str) -> String {
    let id = id.trim();
    if id.is_empty() {
        CLASSIC_ID.to_string()
    } else {
        id.to_string()
    }
}

fn poll_loop(shared: Arc<Mutex<Shared>>, rx: Receiver<Cmd>) {
    let mut token: Option<String> = None;
    let mut backoff = BACKOFF_START;
    let mut next_ok_at = Instant::now();
    let base = sidecar_base();

    loop {
        // Block until a command or a short timeout so we don't need UI Tick spam.
        let mut force = false;
        let wait = next_ok_at.saturating_duration_since(Instant::now());
        match rx.recv_timeout(wait.max(Duration::from_millis(50))) {
            Ok(Cmd::Tick) => {}
            Ok(Cmd::SetWanted(id)) => {
                if let Ok(mut g) = shared.lock() {
                    g.wanted_id = id;
                }
                force = true;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
        }
        // Drain burst.
        while let Ok(cmd) = rx.try_recv() {
            match cmd {
                Cmd::Tick => {}
                Cmd::SetWanted(id) => {
                    if let Ok(mut g) = shared.lock() {
                        g.wanted_id = id;
                    }
                    force = true;
                }
            }
        }

        let now = Instant::now();
        if !force && now < next_ok_at {
            continue;
        }

        match refresh_once(&shared, &base, &mut token) {
            Ok(()) => {
                backoff = BACKOFF_START;
                next_ok_at = Instant::now() + POLL_INTERVAL;
            }
            Err(_) => {
                // Sidecar down → Classic look (keep wanted_id for retry).
                if let Ok(mut g) = shared.lock() {
                    if g.sidecar_up || g.applied_id != CLASSIC_ID {
                        g.sidecar_up = false;
                        g.catalog.clear();
                        apply_classic(&mut g);
                    }
                }
                next_ok_at = Instant::now() + backoff;
                backoff = (backoff * 2).min(BACKOFF_MAX);
            }
        }
    }
}

fn refresh_once(
    shared: &Arc<Mutex<Shared>>,
    base: &str,
    token: &mut Option<String>,
) -> Result<(), String> {
    let catalog_v = http_get(base, token, "/themes")?;
    let catalog: Vec<ThemeInfo> = serde_json::from_value(catalog_v)
        .map_err(|e| format!("themes catalog json: {e}"))?;

    // Prefetch every Pro palette so Settings ComboBox preview is instant.
    let mut fetched_all: HashMap<String, ChromeTokens> = HashMap::new();
    for info in &catalog {
        let path = format!("/themes/{}", info.id);
        match http_get(base, token, &path) {
            Ok(v) => {
                if let Ok(t) = serde_json::from_value::<ChromeTokens>(v) {
                    fetched_all.insert(info.id.clone(), t);
                }
            }
            Err(_) => continue,
        }
    }

    let wanted = shared
        .lock()
        .map(|g| g.wanted_id.clone())
        .unwrap_or_else(|_| CLASSIC_ID.to_string());

    if let Ok(mut g) = shared.lock() {
        g.sidecar_up = true;
        g.catalog = catalog;
        for (id, t) in fetched_all {
            g.tokens.insert(id, t);
        }
        if wanted == CLASSIC_ID {
            apply_classic(&mut g);
        } else if let Some(t) = g.tokens.get(&wanted).cloned() {
            if g.applied_id != wanted || g.applied != t {
                g.applied_id = wanted;
                g.applied = t;
                g.gen = g.gen.wrapping_add(1);
            }
        } else {
            // Wanted Pro id missing — keep Classic until it appears.
            apply_classic(&mut g);
        }
    }
    Ok(())
}

fn apply_classic(g: &mut Shared) {
    let classic = theme::classic();
    if g.applied_id != CLASSIC_ID || g.applied != classic {
        g.applied_id = CLASSIC_ID.to_string();
        g.applied = classic;
        g.gen = g.gen.wrapping_add(1);
    }
}

fn fetch_theme_tokens(id: &str) -> Result<ChromeTokens, String> {
    let base = sidecar_base();
    let mut token = None;
    let v = http_get(&base, &mut token, &format!("/themes/{id}"))?;
    serde_json::from_value(v).map_err(|e| format!("theme tokens json: {e}"))
}

fn sidecar_base() -> String {
    std::env::var("TOPICBENCH_MCP_URL")
        .ok()
        .and_then(|u| {
            let u = u.trim_end_matches('/').to_string();
            if let Some(stripped) = u.strip_suffix("/mcp") {
                Some(stripped.to_string())
            } else {
                Some(u)
            }
        })
        .unwrap_or_else(|| DEFAULT_BASE.to_string())
}

fn token_path() -> PathBuf {
    crate::mqtt::paths::config_file("mcp-token")
}

fn read_token_cached(token: &mut Option<String>) -> Result<String, String> {
    if let Some(t) = token.as_ref() {
        if !t.is_empty() {
            return Ok(t.clone());
        }
    }
    let path = token_path();
    let t = fs::read_to_string(&path)
        .map_err(|e| format!("cannot read {} ({e})", path.display()))?
        .trim()
        .to_string();
    if t.is_empty() {
        return Err("mcp-token empty".into());
    }
    *token = Some(t.clone());
    Ok(t)
}

fn http_get(base: &str, token: &mut Option<String>, path: &str) -> Result<serde_json::Value, String> {
    let tok = read_token_cached(token)?;
    let url = format!("{base}{path}");
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_millis(400))
        .timeout_read(Duration::from_secs(3))
        .build();
    let resp = agent
        .get(&url)
        .set("Authorization", &format!("Bearer {tok}"))
        .call();
    match resp {
        Ok(r) => {
            let text = r.into_string().map_err(|e| format!("read body: {e}"))?;
            serde_json::from_str(&text).map_err(|e| format!("json: {e}"))
        }
        Err(ureq::Error::Status(code, resp)) => {
            let text = resp.into_string().unwrap_or_default();
            Err(format!("HTTP {code}: {text}"))
        }
        Err(ureq::Error::Transport(t)) => Err(format!("unreachable ({t})")),
    }
}
