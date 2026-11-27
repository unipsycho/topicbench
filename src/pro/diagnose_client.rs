//! Availability check for the Pro connection-diagnosis feature (sidecar `/diagnose`).
//!
//! The GUI shows the Diagnose entry point only when [`check_blocking`] returns
//! [`Availability::Ready`]: licensed, `topicbench-pro` answering on localhost, the
//! sidecar advertising the `diagnose` capability, and the shared `mcp-token`
//! accepted. Anything else leaves the feature invisible.
//!
//! Connecting to a sidecar that is not running can block on Windows, so callers
//! use [`spawn_check`] and read the result from the returned channel. The UI
//! thread never calls ureq.

use serde_json::Value;
use std::fs;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::Duration;

const DEFAULT_BASE: &str = "http://127.0.0.1:7853";
const PROBE_TIMEOUT: Duration = Duration::from_millis(800);

/// Result of probing the sidecar for diagnosis support.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Availability {
    /// Licensed, reachable, advertises `diagnose`, and token accepted.
    Ready { sidecar_version: String },
    /// This install has no Pro license; no probe was made.
    NotLicensed,
    /// Nothing answered on the sidecar port (not installed or not running).
    Unreachable,
    /// The sidecar runs but predates the `diagnose` capability.
    Unsupported { sidecar_version: String },
    /// The sidecar rejected the Bearer token (or it is missing).
    Unauthorized,
}

impl Availability {
    pub fn is_ready(&self) -> bool {
        matches!(self, Self::Ready { .. })
    }
}

/// Run [`check_blocking`] on a worker thread and return a one-shot receiver.
pub fn spawn_check() -> Receiver<Availability> {
    let (tx, rx) = mpsc::channel();
    let _ = thread::Builder::new()
        .name("tb-diagnose-probe".into())
        .spawn(move || {
            let _ = tx.send(check_blocking());
        });
    rx
}

/// Blocking probe: `/health` for version and capability, then `/diagnose` with the token.
pub fn check_blocking() -> Availability {
    if !crate::pro::is_licensed() {
        return Availability::NotLicensed;
    }
    let agent = ureq::AgentBuilder::new().timeout(PROBE_TIMEOUT).build();

    let health: Value = match agent.get(&format!("{DEFAULT_BASE}/health")).call() {
        Ok(resp) => match resp.into_json() {
            Ok(v) => v,
            Err(_) => return Availability::Unreachable,
        },
        Err(_) => return Availability::Unreachable,
    };
    let version = health
        .get("version")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    if !advertises_diagnose(&health) {
        return Availability::Unsupported {
            sidecar_version: version,
        };
    }

    let Some(token) = read_token() else {
        return Availability::Unauthorized;
    };
    match agent
        .get(&format!("{DEFAULT_BASE}/diagnose"))
        .set("Authorization", &format!("Bearer {token}"))
        .call()
    {
        Ok(_) => Availability::Ready {
            sidecar_version: version,
        },
        Err(ureq::Error::Status(401, _)) => Availability::Unauthorized,
        Err(_) => Availability::Unreachable,
    }
}

/// True when the `/health` body lists `diagnose` in `capabilities`.
pub fn advertises_diagnose(health: &Value) -> bool {
    health
        .get("capabilities")
        .and_then(Value::as_array)
        .is_some_and(|caps| caps.iter().any(|c| c.as_str() == Some("diagnose")))
}

fn read_token() -> Option<String> {
    let text = fs::read_to_string(token_path()).ok()?;
    let token = text.trim().to_string();
    (!token.is_empty()).then_some(token)
}

fn token_path() -> PathBuf {
    crate::mqtt::paths::config_file("mcp-token")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn advertises_diagnose_reads_capabilities() {
        assert!(advertises_diagnose(&json!({"capabilities": ["diagnose"]})));
        assert!(advertises_diagnose(
            &json!({"capabilities": ["sim", "diagnose"]})
        ));
        assert!(!advertises_diagnose(&json!({"capabilities": ["sim"]})));
        assert!(!advertises_diagnose(&json!({"version": "1.2.9"})));
    }

    #[test]
    fn ready_only_when_ready() {
        assert!(Availability::Ready {
            sidecar_version: "1.3.0".into()
        }
        .is_ready());
        assert!(!Availability::Unreachable.is_ready());
        assert!(!Availability::NotLicensed.is_ready());
    }
}

// ─── Phase B: run a diagnosis against one profile ─────────────────────────────

use eframe::egui::Context;
use serde::{Deserialize, Serialize};
use std::sync::mpsc::Receiver as RunReceiver;

const RUN_TIMEOUT: Duration = Duration::from_secs(120);

/// Body for `POST /diagnose`. Not `Debug`, so the password cannot be logged.
#[derive(Serialize)]
pub struct DiagnoseRequest {
    pub host: String,
    pub port: u16,
    /// `mqtt` or `websocket`.
    pub transport: String,
    pub use_tls: bool,
    pub ws_path: String,
    /// `3.1.1` or `5`.
    pub mqtt_version: String,
    /// Left empty on purpose: the sidecar makes a fresh client ID, so a probe never
    /// takes over the MQTT session of a live connection that uses the profile's ID.
    pub client_id: String,
    pub username: String,
    pub password: String,
    pub ca_pem: String,
    pub client_cert_pem: String,
    pub client_key_pem: String,
}

/// One stage of the report, as returned by the sidecar.
#[derive(Clone, Debug, Deserialize)]
pub struct StageResult {
    pub name: String,
    /// `pass`, `fail`, or `skipped`.
    pub status: String,
    pub ms: u64,
    pub detail: String,
    pub hint: String,
}

/// Full diagnosis report from the sidecar.
#[derive(Clone, Debug, Deserialize)]
pub struct DiagnoseReport {
    pub endpoint: String,
    pub ok: bool,
    pub stages: Vec<StageResult>,
}

/// Run the diagnosis on a worker thread. The result arrives on the returned channel;
/// the UI thread never blocks on the network.
pub fn run_async(
    req: DiagnoseRequest,
    ctx: Context,
) -> RunReceiver<Result<DiagnoseReport, String>> {
    let (tx, rx) = mpsc::channel();
    let _ = thread::Builder::new()
        .name("tb-diagnose-run".into())
        .spawn(move || {
            let _ = tx.send(run_blocking(&req));
            ctx.request_repaint();
        });
    rx
}

/// Blocking `POST /diagnose` with the shared Bearer token.
pub fn run_blocking(req: &DiagnoseRequest) -> Result<DiagnoseReport, String> {
    let token =
        read_token().ok_or_else(|| "Pro token missing: is TopicBench Pro running?".to_string())?;
    let body = serde_json::to_string(req).map_err(|e| format!("encode request: {e}"))?;
    let agent = ureq::AgentBuilder::new().timeout(RUN_TIMEOUT).build();
    match agent
        .post(&format!("{DEFAULT_BASE}/diagnose"))
        .set("Authorization", &format!("Bearer {token}"))
        .set("Content-Type", "application/json")
        .send_string(&body)
    {
        Ok(resp) => resp
            .into_json::<DiagnoseReport>()
            .map_err(|e| format!("read diagnosis report: {e}")),
        Err(ureq::Error::Status(401, _)) => Err("Pro sidecar rejected the token".into()),
        Err(ureq::Error::Status(code, resp)) => {
            let msg = resp.into_string().unwrap_or_default();
            Err(format!("Pro sidecar returned {code}: {msg}"))
        }
        Err(e) => Err(format!("Pro sidecar unreachable: {e}")),
    }
}

// ─── Phase C: auto-setup matrix ───────────────────────────────────────────────

use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};
use std::sync::Arc;

/// One transport / TLS / port / path / version combination.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Candidate {
    pub transport: String,
    pub use_tls: bool,
    pub port: u16,
    pub ws_path: String,
    pub mqtt_version: String,
}

/// Body for `POST /diagnose/matrix`: the diagnosis fields plus the opt-in and the current settings.
#[derive(Serialize)]
pub struct MatrixRequest {
    #[serde(flatten)]
    pub base: DiagnoseRequest,
    pub allow_plaintext_credentials: bool,
    pub current: Option<Candidate>,
}

/// Outcome for one candidate.
// Some fields are read when a working combination is applied to a profile (Phase D).
#[allow(dead_code)]
#[derive(Clone, Debug, Deserialize)]
pub struct CandidateResult {
    pub endpoint: String,
    pub transport: String,
    pub use_tls: bool,
    pub port: u16,
    pub ws_path: String,
    pub mqtt_version: String,
    pub ok: bool,
    pub stopped_at: Option<String>,
    pub detail: String,
    pub hint: String,
    pub ms: u64,
    pub credentials_sent: bool,
    pub note: String,
}

/// Job state as polled from the sidecar.
// Some fields are read when a working combination is applied to a profile (Phase D).
#[allow(dead_code)]
#[derive(Clone, Debug, Deserialize)]
pub struct MatrixView {
    pub job_id: String,
    /// `running`, `done`, or `cancelled`.
    pub status: String,
    pub total: usize,
    pub done: usize,
    pub winner: Option<usize>,
    pub results: Vec<CandidateResult>,
}

/// Messages from the matrix worker to the UI.
pub enum MatrixMsg {
    View(MatrixView),
    Failed(String),
}

/// Handle for a running matrix: receive progress, request cancellation.
pub struct MatrixRun {
    pub rx: RunReceiver<MatrixMsg>,
    cancel: Arc<AtomicBool>,
}

impl MatrixRun {
    pub fn cancel(&self) {
        self.cancel.store(true, AtomicOrdering::SeqCst);
    }
}

const MATRIX_POLL: Duration = Duration::from_millis(400);

/// Start a matrix job and poll it on a worker thread, sending each view to the UI.
pub fn run_matrix_async(req: MatrixRequest, ctx: Context) -> MatrixRun {
    let (tx, rx) = mpsc::channel();
    let cancel = Arc::new(AtomicBool::new(false));
    let cancel_w = cancel.clone();
    let _ = thread::Builder::new()
        .name("tb-diagnose-matrix".into())
        .spawn(move || {
            let outcome = matrix_worker(req, &cancel_w, &tx, &ctx);
            if let Err(e) = outcome {
                let _ = tx.send(MatrixMsg::Failed(e));
                ctx.request_repaint();
            }
        });
    MatrixRun { rx, cancel }
}

fn matrix_worker(
    req: MatrixRequest,
    cancel: &AtomicBool,
    tx: &mpsc::Sender<MatrixMsg>,
    ctx: &Context,
) -> Result<(), String> {
    let job_id = matrix_start_blocking(&req)?;
    let mut cancel_sent = false;
    loop {
        if cancel.load(AtomicOrdering::SeqCst) && !cancel_sent {
            matrix_cancel_blocking(&job_id)?;
            cancel_sent = true;
        }
        let view = matrix_poll_blocking(&job_id)?;
        let finished = view.status != "running";
        if tx.send(MatrixMsg::View(view)).is_err() {
            return Ok(());
        }
        ctx.request_repaint();
        if finished {
            return Ok(());
        }
        thread::sleep(MATRIX_POLL);
    }
}

fn token_or_err() -> Result<String, String> {
    read_token().ok_or_else(|| "Pro token missing: is TopicBench Pro running?".to_string())
}

fn matrix_agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(10))
        .build()
}

fn matrix_start_blocking(req: &MatrixRequest) -> Result<String, String> {
    let token = token_or_err()?;
    let body = serde_json::to_string(req).map_err(|e| format!("encode request: {e}"))?;
    let resp = matrix_agent()
        .post(&format!("{DEFAULT_BASE}/diagnose/matrix"))
        .set("Authorization", &format!("Bearer {token}"))
        .set("Content-Type", "application/json")
        .send_string(&body)
        .map_err(|e| format!("start auto-setup: {e}"))?;
    let v: Value = resp.into_json().map_err(|e| format!("read job id: {e}"))?;
    v.get("job_id")
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| "sidecar returned no job id".to_string())
}

fn matrix_poll_blocking(job_id: &str) -> Result<MatrixView, String> {
    let token = token_or_err()?;
    matrix_agent()
        .get(&format!("{DEFAULT_BASE}/diagnose/matrix/{job_id}"))
        .set("Authorization", &format!("Bearer {token}"))
        .call()
        .map_err(|e| format!("poll auto-setup: {e}"))?
        .into_json::<MatrixView>()
        .map_err(|e| format!("read auto-setup progress: {e}"))
}

fn matrix_cancel_blocking(job_id: &str) -> Result<(), String> {
    let token = token_or_err()?;
    matrix_agent()
        .post(&format!("{DEFAULT_BASE}/diagnose/matrix/{job_id}/cancel"))
        .set("Authorization", &format!("Bearer {token}"))
        .call()
        .map(|_| ())
        .map_err(|e| format!("cancel auto-setup: {e}"))
}

#[cfg(test)]
mod matrix_wire_tests {
    use super::*;

    #[test]
    fn matrix_request_flattens_diagnose_fields() {
        let req = MatrixRequest {
            base: DiagnoseRequest {
                host: "broker.example".into(),
                port: 1883,
                transport: "mqtt".into(),
                use_tls: false,
                ws_path: "/mqtt".into(),
                mqtt_version: "3.1.1".into(),
                client_id: String::new(),
                username: "u".into(),
                password: "p".into(),
                ca_pem: String::new(),
                client_cert_pem: String::new(),
                client_key_pem: String::new(),
            },
            allow_plaintext_credentials: false,
            current: Some(Candidate {
                transport: "mqtt".into(),
                use_tls: false,
                port: 1883,
                ws_path: "/mqtt".into(),
                mqtt_version: "3.1.1".into(),
            }),
        };
        let v: Value = serde_json::from_str(&serde_json::to_string(&req).unwrap()).unwrap();
        assert_eq!(v["host"], "broker.example");
        assert_eq!(v["password"], "p");
        assert_eq!(v["current"]["port"], 1883);
        assert_eq!(v["allow_plaintext_credentials"], false);
    }

    #[test]
    fn matrix_view_parses_sidecar_shape() {
        let sample = r#"{
            "job_id": "m0001",
            "status": "done",
            "total": 13,
            "done": 1,
            "winner": 0,
            "results": [{
                "endpoint": "mqtt://h:1883", "transport": "mqtt", "use_tls": false,
                "port": 1883, "ws_path": "/mqtt", "mqtt_version": "3.1.1",
                "ok": true, "stopped_at": null, "detail": "", "hint": "", "ms": 12,
                "credentials_sent": false, "note": "unencrypted transport accepted"
            }]
        }"#;
        let view: MatrixView = serde_json::from_str(sample).unwrap();
        assert_eq!(view.status, "done");
        assert_eq!(view.winner, Some(0));
        assert!(view.results[0].ok);
        assert!(view.results[0].stopped_at.is_none());
    }
}
