//! Pro connection diagnosis: the Diagnose button under Connect, the stage window,
//! the auto-setup matrix, and the one-click fix that applies a working combination.
//!
//! Everything here is inert unless Pro is licensed and the sidecar answers
//! [`Availability::Ready`]. Free installs never probe, never show the button, and
//! never open the window.
//!
//! The profile editor only knows two things: whether to show the button and which
//! profile was requested. Those travel through egui's temp data, so the profile
//! panel's signature stays as it was.

use crate::mqtt::certs;
use crate::mqtt::profile::{ConnectionProfile, MqttVersion, ProfileStore, ProfileTransport};
use crate::mqtt::secrets;
use crate::mqtt::session::{SessionManager, SessionState};
use crate::pro::diagnose_client::{
    self, Availability, Candidate, DiagnoseReport, DiagnoseRequest, MatrixMsg, MatrixRequest,
    MatrixRun, MatrixView, StageResult,
};
use crate::ui::hover_tip;
use crate::ui::panel_window as pw;
use crate::ui::icons::IconSet;
use eframe::egui::{self, Color32, RichText};
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::{Duration, Instant};

/// One auto-setup run: its worker handle, the latest view, and whether it has ended.
struct MatrixState {
    /// `None` when the run could not start (for example the profile could not be read).
    run: Option<MatrixRun>,
    view: Option<MatrixView>,
    error: Option<String>,
    finished: bool,
}

/// The connection settings the one-click fix changes. Nothing else on a profile is touched.
#[derive(Clone)]
struct Settings {
    transport: ProfileTransport,
    use_tls: bool,
    port: u16,
    ws_path: String,
    mqtt_version: MqttVersion,
}

/// A change waiting for the user's confirmation.
#[derive(Clone)]
struct PendingApply {
    profile_id: String,
    settings: Settings,
    changes: Vec<String>,
}

/// The last applied change, kept so it can be reverted once.
#[derive(Clone)]
struct UndoRecord {
    profile_id: String,
    previous: Settings,
}

/// Something the user asked for in the window, handled after the windows are drawn.
enum Action {
    StartApply(usize),
    ConfirmApply,
    CancelApply,
    Undo,
}

/// Temp-data key: `bool`, true when the sidecar is ready for diagnosis.
const READY_KEY: &str = "tb-diagnose-ready";
/// Temp-data key: `String` profile id requested by the Diagnose button.
const REQUEST_KEY: &str = "tb-diagnose-request";
const RECHECK_EVERY: Duration = Duration::from_secs(10);
const PASS: Color32 = Color32::from_rgb(90, 190, 120);
const FAIL: Color32 = Color32::from_rgb(220, 90, 90);
const WARN: Color32 = Color32::from_rgb(220, 180, 80);
const SKIP: Color32 = Color32::from_rgb(140, 140, 150);

pub struct DiagnosePanel {
    availability: Option<Availability>,
    probe_rx: Option<Receiver<Availability>>,
    last_probe: Option<Instant>,
    profile_id: Option<String>,
    run_rx: Option<Receiver<Result<DiagnoseReport, String>>>,
    report: Option<DiagnoseReport>,
    error: Option<String>,
    open: bool,
    matrix: Option<MatrixState>,
    /// Opt-in: also send the password to candidates without TLS. Off by default.
    matrix_plaintext: bool,
    pending: Option<PendingApply>,
    undo: Option<UndoRecord>,
    notice: Option<String>,
}

impl Default for DiagnosePanel {
    fn default() -> Self {
        Self::new()
    }
}

impl DiagnosePanel {
    pub fn new() -> Self {
        Self {
            availability: None,
            probe_rx: None,
            last_probe: None,
            profile_id: None,
            run_rx: None,
            report: None,
            error: None,
            open: false,
            matrix: None,
            matrix_plaintext: false,
            pending: None,
            undo: None,
            notice: None,
        }
    }

    fn ready(&self) -> bool {
        self.availability
            .as_ref()
            .is_some_and(Availability::is_ready)
    }

    /// Call once per frame, before the profile panel draws.
    pub fn update(
        &mut self,
        ctx: &egui::Context,
        profiles: &mut ProfileStore,
        sessions: &SessionManager,
        icons: &IconSet,
    ) {
        self.poll_probe();
        self.maybe_probe(ctx);
        ctx.data_mut(|d| d.insert_temp(egui::Id::new(READY_KEY), self.ready()));

        if let Some(id) = ctx.data_mut(|d| d.remove_temp::<String>(egui::Id::new(REQUEST_KEY))) {
            self.start(ctx, profiles, &id);
        }
        self.poll_run(ctx);
        self.poll_matrix(ctx);
        if let Some(action) = self.show(ctx, profiles, sessions, icons) {
            self.handle(ctx, profiles, sessions, action);
        }
    }

    fn maybe_probe(&mut self, ctx: &egui::Context) {
        // Free installs stop here: no thread, no network, no button.
        if !crate::pro::has(crate::pro::Entitlement::Diagnose) || self.probe_rx.is_some() {
            if self.probe_rx.is_some() {
                ctx.request_repaint_after(Duration::from_millis(150));
            }
            return;
        }
        if matches!(self.availability, Some(Availability::NotLicensed)) {
            return;
        }
        // Ready sidecars are not re-probed. Otherwise retry on an interval, so a sidecar
        // started after the GUI is picked up without a restart.
        if self.ready() {
            return;
        }
        let due = self
            .last_probe
            .map_or(true, |t| t.elapsed() >= RECHECK_EVERY);
        if due {
            self.probe_rx = Some(diagnose_client::spawn_check());
            self.last_probe = Some(Instant::now());
            ctx.request_repaint_after(Duration::from_millis(150));
        }
    }

    fn poll_probe(&mut self) {
        let Some(rx) = self.probe_rx.as_ref() else {
            return;
        };
        match rx.try_recv() {
            Ok(avail) => {
                self.availability = Some(avail);
                self.probe_rx = None;
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => self.probe_rx = None,
        }
    }

    fn start(&mut self, ctx: &egui::Context, profiles: &ProfileStore, id: &str) {
        self.profile_id = Some(id.to_string());
        self.open = true;
        self.report = None;
        self.error = None;
        self.matrix = None;
        self.pending = None;
        let Some(profile) = profiles.profiles.get(id) else {
            self.error = Some("This profile no longer exists.".into());
            self.run_rx = None;
            return;
        };
        match build_request(profile) {
            Ok(req) => self.run_rx = Some(diagnose_client::run_async(req, ctx.clone())),
            Err(e) => {
                self.error = Some(e);
                self.run_rx = None;
            }
        }
    }

    fn poll_run(&mut self, ctx: &egui::Context) {
        let Some(rx) = self.run_rx.as_ref() else {
            return;
        };
        match rx.try_recv() {
            Ok(Ok(report)) => {
                self.report = Some(report);
                self.run_rx = None;
            }
            Ok(Err(e)) => {
                self.error = Some(e);
                self.run_rx = None;
            }
            Err(TryRecvError::Empty) => ctx.request_repaint_after(Duration::from_millis(150)),
            Err(TryRecvError::Disconnected) => {
                self.error = Some("The diagnosis worker stopped unexpectedly.".into());
                self.run_rx = None;
            }
        }
    }

    /// Draw the diagnosis window and the confirmation window. Returns the user's request, if any.
    fn show(
        &mut self,
        ctx: &egui::Context,
        profiles: &ProfileStore,
        sessions: &SessionManager,
        icons: &IconSet,
    ) -> Option<Action> {
        // Never draw outside a licensed, reachable sidecar (e.g. it stopped while the window was open).
        if !self.open || !self.ready() {
            return None;
        }
        let profile = self
            .profile_id
            .as_deref()
            .and_then(|id| profiles.profiles.get(id));
        let title = profile
            .map(|p| format!("Diagnose: {}", p.name))
            .unwrap_or_else(|| "Diagnose".to_string());
        // A profile with a certificate folder always uses TLS, so plaintext results cannot be applied.
        let cert_forced = profile.is_some_and(|p| !p.certs_dir.trim().is_empty());
        let live = self
            .profile_id
            .as_deref()
            .is_some_and(|id| is_live(sessions, id));

        let running = self.run_rx.is_some();
        let report = self.report.clone();
        let error = self.error.clone();
        let notice = self.notice.clone();
        let matrix_running = self.matrix.as_ref().is_some_and(|m| !m.finished);
        let matrix_view = self.matrix.as_ref().and_then(|m| m.view.clone());
        let matrix_error = self.matrix.as_ref().and_then(|m| m.error.clone());
        let undo_available = self
            .undo
            .as_ref()
            .is_some_and(|u| Some(&u.profile_id) == self.profile_id.as_ref());
        let pending = self.pending.clone();

        let mut action: Option<Action> = None;
        let mut rerun = false;
        let mut start_matrix_req = false;
        let mut cancel_matrix_req = false;
        let tex = icons.get("stethoscope").cloned();
        let subtitle = report
            .as_ref()
            .map(|r| r.endpoint.clone())
            .or_else(|| profile.map(|p| format!("{}:{}", p.host, p.port)))
            .unwrap_or_default();
        let pill = status_pill(running, report.as_ref());
        let mut close = false;
        // Tall results (auto-setup table) scroll instead of running off the screen.
        let max_body_h = (ctx.screen_rect().height() * 0.85 - 110.0).max(240.0);

        egui::Window::new(title.clone())
            .order(egui::Order::Foreground)
            .title_bar(false)
            .collapsible(false)
            .resizable(true)
            .default_width(600.0)
            .frame(pw::window_frame(ctx))
            .show(ctx, |ui| {
                close = pw::header_bar(ui, tex.as_ref(), icons, &title, &subtitle, pill, None).close;
                egui::Frame::NONE
                    .inner_margin(egui::Margin::symmetric(18, 14))
                    .show(ui, |ui| {
                      egui::ScrollArea::vertical()
                        .max_height(max_body_h)
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 4.0;
                        if let Some(n) = &notice {
                            banner(ui, WARN, n);
                        }
                        // Only relevant once there is something to apply or undo.
                        if live && (matrix_view.is_some() || undo_available) {
                            ui.weak("Disconnect this profile to apply a change.");
                        }
                        if running {
                            card(ui, Color32::from_rgba_unmultiplied(120, 150, 200, 22), None, |ui| {
                                ui.horizontal(|ui| {
                                    ui.spinner();
                                    ui.label("Running checks… slow or distant brokers can take 10–15 seconds.");
                                });
                            });
                        }
                        if let Some(e) = &error {
                            banner(ui, FAIL, e);
                        }
                        if let Some(r) = &report {
                            summary(ui, r);
                            for s in &r.stages {
                                stage_row(ui, s);
                            }
                        }
                        ui.horizontal(|ui| {
                            if ui
                                .add_enabled(
                                    !running,
                                    egui::Button::new(RichText::new("Run again").strong())
                                        .fill(pw::accent())
                                        .min_size(egui::vec2(96.0, 28.0)),
                                )
                                .clicked()
                            {
                                rerun = true;
                            }
                            if ui
                                .add_enabled(
                                    undo_available && !live,
                                    egui::Button::new("Undo last change").min_size(egui::vec2(0.0, 28.0)),
                                )
                                .clicked()
                            {
                                action = Some(Action::Undo);
                            }
                        });

                        ui.add_space(4.0);
                        card(ui, Color32::from_rgba_unmultiplied(120, 150, 200, 14), Some(pw::card_edge()), |ui| {
                            ui.label(RichText::new("Auto-setup").strong().size(15.0).color(pw::accent_text()));
                            ui.weak(
                                "Try other transports, TLS, ports, paths, and MQTT versions on this host. \
                                 Nothing changes until you choose Apply.",
                            );
                            ui.add_space(2.0);
                            ui.horizontal(|ui| {
                                if matrix_running {
                                    if ui.button("Cancel").clicked() {
                                        cancel_matrix_req = true;
                                    }
                                    ui.spinner();
                                } else if ui
                                    .add_enabled(
                                        !running,
                                        egui::Button::new("Try other combinations")
                                            .min_size(egui::vec2(0.0, 28.0)),
                                    )
                                    .clicked()
                                {
                                    start_matrix_req = true;
                                }
                            });
                            ui.checkbox(
                                &mut self.matrix_plaintext,
                                "Also send the password on ports without TLS (not recommended)",
                            );
                            if let Some(e) = &matrix_error {
                                ui.label(RichText::new(e).color(FAIL));
                            }
                            if let Some(v) = &matrix_view {
                                ui.add_space(2.0);
                                if let Some(i) = matrix_table(ui, v, cert_forced, live) {
                                    action = Some(Action::StartApply(i));
                                }
                            }
                        });
                        ui.weak(
                            "Checks run from this computer. They may not match the network a device uses, \
                             and they do not change the broker.",
                        );
                        });
                    });
            });
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) && pending.is_none() {
            close = true;
        }
        let open = !close;

        if let Some(p) = pending {
            egui::Window::new("Apply to profile")
                .order(egui::Order::Foreground)
                .title_bar(false)
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .frame(pw::window_frame(ctx))
                .show(ctx, |ui| {
                    pw::header_bar(
                        ui,
                        tex.as_ref(),
                        icons,
                        "Apply to profile",
                        "Review the change",
                        None,
                        None,
                    );
                    egui::Frame::NONE
                        .inner_margin(egui::Margin::symmetric(18, 14))
                        .show(ui, |ui| {
                            ui.spacing_mut().item_spacing.y = 8.0;
                            ui.label(RichText::new("Change this profile to:").strong());
                            card(
                                ui,
                                Color32::from_rgba_unmultiplied(90, 190, 120, 24),
                                Some(PASS),
                                |ui| {
                                    for c in &p.changes {
                                        ui.label(format!("•  {c}"));
                                    }
                                },
                            );
                            ui.weak(
                                "Passwords, subscriptions, and other settings stay as they are. \
                                 The diagnosis runs again afterwards. Undo is available once.",
                            );
                            ui.horizontal(|ui| {
                                if ui
                                    .add(
                                        egui::Button::new(RichText::new("Apply").strong())
                                            .fill(pw::accent())
                                            .min_size(egui::vec2(96.0, 28.0)),
                                    )
                                    .clicked()
                                {
                                    action = Some(Action::ConfirmApply);
                                }
                                if ui
                                    .add(
                                        egui::Button::new("Cancel").min_size(egui::vec2(0.0, 28.0)),
                                    )
                                    .clicked()
                                {
                                    action = Some(Action::CancelApply);
                                }
                            });
                        });
                });
        }

        self.open = open;
        if rerun {
            if let Some(id) = self.profile_id.clone() {
                self.start(ctx, profiles, &id);
            }
        }
        if cancel_matrix_req {
            if let Some(run) = self.matrix.as_ref().and_then(|m| m.run.as_ref()) {
                run.cancel();
            }
        }
        if start_matrix_req {
            self.start_matrix(ctx, profiles);
        }
        action
    }

    /// Carry out a request from the window. Changes go to the profile store and are saved at once.
    fn handle(
        &mut self,
        ctx: &egui::Context,
        profiles: &mut ProfileStore,
        sessions: &SessionManager,
        action: Action,
    ) {
        match action {
            Action::StartApply(i) => {
                let Some(id) = self.profile_id.clone() else {
                    return;
                };
                let Some(row) = self
                    .matrix
                    .as_ref()
                    .and_then(|m| m.view.as_ref())
                    .and_then(|v| v.results.get(i))
                    .cloned()
                else {
                    return;
                };
                let Some(profile) = profiles.profiles.get(&id) else {
                    return;
                };
                if is_live(sessions, &id) {
                    self.notice = Some("Disconnect this profile before applying a change.".into());
                    return;
                }
                let settings = Settings {
                    transport: if row.transport == "websocket" {
                        ProfileTransport::WebSocket
                    } else {
                        ProfileTransport::Mqtt
                    },
                    use_tls: row.use_tls,
                    port: row.port,
                    ws_path: row.ws_path.clone(),
                    mqtt_version: if row.mqtt_version == "5" {
                        MqttVersion::V5
                    } else {
                        MqttVersion::V311
                    },
                };
                let changes = changes_between(&settings_of(profile), &settings);
                if changes.is_empty() {
                    self.notice = Some("The profile already uses these settings.".into());
                    return;
                }
                self.notice = None;
                self.pending = Some(PendingApply {
                    profile_id: id,
                    settings,
                    changes,
                });
            }
            Action::CancelApply => self.pending = None,
            Action::ConfirmApply => {
                let Some(p) = self.pending.take() else {
                    return;
                };
                if is_live(sessions, &p.profile_id) {
                    self.notice = Some("Disconnect this profile before applying a change.".into());
                    return;
                }
                let Some(profile) = profiles.profiles.get_mut(&p.profile_id) else {
                    self.notice = Some("This profile no longer exists.".into());
                    return;
                };
                let previous = settings_of(profile);
                apply_settings(profile, &p.settings);
                match profiles.save() {
                    Ok(()) => {
                        self.undo = Some(UndoRecord {
                            profile_id: p.profile_id.clone(),
                            previous,
                        });
                        self.start(ctx, profiles, &p.profile_id);
                        self.notice = Some("Applied. The diagnosis ran again to confirm.".into());
                    }
                    Err(e) => {
                        self.notice = Some(format!("Could not save the profile: {e}"));
                    }
                }
            }
            Action::Undo => {
                let Some(u) = self.undo.take() else {
                    return;
                };
                if is_live(sessions, &u.profile_id) {
                    self.notice = Some("Disconnect this profile before undoing a change.".into());
                    self.undo = Some(u);
                    return;
                }
                let Some(profile) = profiles.profiles.get_mut(&u.profile_id) else {
                    self.notice = Some("This profile no longer exists.".into());
                    return;
                };
                apply_settings(profile, &u.previous);
                self.notice = Some(match profiles.save() {
                    Ok(()) => "Undone. The previous settings are back.".into(),
                    Err(e) => format!("Could not save the profile: {e}"),
                });
            }
        }
    }

    fn start_matrix(&mut self, ctx: &egui::Context, profiles: &ProfileStore) {
        let Some(id) = self.profile_id.clone() else {
            return;
        };
        let Some(profile) = profiles.profiles.get(&id) else {
            return;
        };
        let base = match build_request(profile) {
            Ok(b) => b,
            Err(e) => {
                self.matrix = Some(MatrixState {
                    run: None,
                    view: None,
                    error: Some(e),
                    finished: true,
                });
                return;
            }
        };
        let current = Candidate {
            transport: match profile.transport {
                ProfileTransport::Mqtt => "mqtt",
                ProfileTransport::WebSocket => "websocket",
            }
            .into(),
            use_tls: profile.want_tls(),
            port: profile.port,
            ws_path: profile.ws_path.clone(),
            mqtt_version: match profile.mqtt_version {
                MqttVersion::V311 => "3.1.1",
                MqttVersion::V5 => "5",
            }
            .into(),
        };
        let req = MatrixRequest {
            base,
            allow_plaintext_credentials: self.matrix_plaintext,
            current: Some(current),
        };
        self.matrix = Some(MatrixState {
            run: Some(diagnose_client::run_matrix_async(req, ctx.clone())),
            view: None,
            error: None,
            finished: false,
        });
    }

    fn poll_matrix(&mut self, ctx: &egui::Context) {
        let Some(m) = self.matrix.as_mut() else {
            return;
        };
        if m.finished {
            return;
        }
        // Drain the channel first, then apply, so the receiver borrow ends before state changes.
        let mut msgs = Vec::new();
        let mut disconnected = false;
        if let Some(run) = m.run.as_ref() {
            loop {
                match run.rx.try_recv() {
                    Ok(msg) => msgs.push(msg),
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        disconnected = true;
                        break;
                    }
                }
            }
        }
        for msg in msgs {
            match msg {
                MatrixMsg::View(v) => {
                    if v.status != "running" {
                        m.finished = true;
                    }
                    m.view = Some(v);
                }
                MatrixMsg::Failed(e) => {
                    m.error = Some(e);
                    m.finished = true;
                }
            }
        }
        if disconnected {
            m.finished = true;
        }
        if !m.finished {
            ctx.request_repaint_after(Duration::from_millis(150));
        }
    }
}

/// True while the profile has a connection open or opening.
fn is_live(sessions: &SessionManager, id: &str) -> bool {
    sessions
        .open
        .get(id)
        .is_some_and(|s| matches!(s.state, SessionState::Connected | SessionState::Connecting))
}

fn settings_of(p: &ConnectionProfile) -> Settings {
    Settings {
        transport: p.transport,
        use_tls: p.use_tls,
        port: p.port,
        ws_path: p.ws_path.clone(),
        mqtt_version: p.mqtt_version,
    }
}

fn apply_settings(p: &mut ConnectionProfile, s: &Settings) {
    p.transport = s.transport;
    p.use_tls = s.use_tls;
    p.port = s.port;
    p.ws_path = s.ws_path.clone();
    p.mqtt_version = s.mqtt_version;
}

fn transport_label(t: ProfileTransport) -> &'static str {
    match t {
        ProfileTransport::Mqtt => "MQTT (TCP)",
        ProfileTransport::WebSocket => "WebSocket",
    }
}

fn version_label(v: MqttVersion) -> &'static str {
    match v {
        MqttVersion::V311 => "3.1.1",
        MqttVersion::V5 => "5",
    }
}

/// Human-readable list of the fields that would change. Empty when nothing differs.
fn changes_between(cur: &Settings, next: &Settings) -> Vec<String> {
    let mut out = Vec::new();
    if cur.transport != next.transport {
        out.push(format!(
            "Transport: {} → {}",
            transport_label(cur.transport),
            transport_label(next.transport)
        ));
    }
    if cur.use_tls != next.use_tls {
        let label = |on: bool| if on { "on" } else { "off" };
        out.push(format!(
            "TLS: {} → {}",
            label(cur.use_tls),
            label(next.use_tls)
        ));
    }
    if cur.port != next.port {
        out.push(format!("Port: {} → {}", cur.port, next.port));
    }
    if next.transport == ProfileTransport::WebSocket && cur.ws_path != next.ws_path {
        out.push(format!(
            "WebSocket path: {} → {}",
            cur.ws_path, next.ws_path
        ));
    }
    if cur.mqtt_version != next.mqtt_version {
        out.push(format!(
            "MQTT version: {} → {}",
            version_label(cur.mqtt_version),
            version_label(next.mqtt_version)
        ));
    }
    out
}

/// Button under Connect in the profile editor. Shows only when the sidecar is ready.
/// True when the Diagnose button should be drawn (licensed, sidecar up, capability present).
pub fn button_visible(ctx: &egui::Context) -> bool {
    ctx.data(|d| {
        d.get_temp::<bool>(egui::Id::new(READY_KEY))
            .unwrap_or(false)
    })
}

/// Square stethoscope button, drawn in the same row as Connect / Disconnect.
pub fn draw_button(
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    icons: &IconSet,
    profile_id: &str,
    height: f32,
) {
    if !button_visible(ctx) {
        return;
    }
    let size = egui::vec2(height, height);
    let resp = match icons.get("stethoscope") {
        Some(tex) => ui.add(
            egui::Button::image(
                egui::Image::new(tex).fit_to_exact_size(egui::vec2(height - 10.0, height - 10.0)),
            )
            .min_size(size),
        ),
        None => ui.add(egui::Button::new("Dx").min_size(size)),
    };
    let clicked = hover_tip::attach(
        resp,
        "Diagnose connection: step through DNS, TCP, TLS, WebSocket, and MQTT CONNECT to find where it fails (Pro)",
    )
    .clicked();
    if clicked {
        ctx.data_mut(|d| d.insert_temp(egui::Id::new(REQUEST_KEY), profile_id.to_string()));
    }
}

/// Progress line plus a results table, one row per candidate tried.
/// Returns the index of a row whose Apply button was clicked.
fn matrix_table(ui: &mut egui::Ui, v: &MatrixView, cert_forced: bool, live: bool) -> Option<usize> {
    let mut clicked = None;
    let working = v.results.iter().filter(|r| r.ok).count();
    let summary = match (v.status.as_str(), v.winner) {
        ("running", _) => format!("Checked {} of {}…", v.done, v.total),
        ("cancelled", _) => format!("Stopped after {} of {}.", v.done, v.total),
        (_, Some(i)) => format!(
            "{} working combination(s). First: {}",
            working, v.results[i].endpoint
        ),
        _ => "No working combination found.".to_string(),
    };
    ui.label(RichText::new(summary).strong());
    // One compact line per combination; the full hint is in the hover text.
    for (i, r) in v.results.iter().enumerate() {
        let (color, fill) = if r.ok {
            (PASS, Color32::from_rgba_unmultiplied(90, 190, 120, 18))
        } else {
            (FAIL, Color32::from_rgba_unmultiplied(220, 90, 90, 10))
        };
        let row = egui::Frame::NONE
            .fill(fill)
            .corner_radius(6.0)
            .inner_margin(egui::Margin::symmetric(8, 3))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    status_dot(ui, if r.ok { "pass" } else { "fail" }, color);
                    ui.label(RichText::new(&r.endpoint).monospace().size(12.5));
                    ui.weak(format!("MQTT {}", r.mqtt_version));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        // Plaintext results cannot be applied when a certificate folder forces TLS.
                        let applicable = r.ok && (r.use_tls || !cert_forced);
                        if r.ok {
                            ui.add_enabled_ui(applicable && !live, |ui| {
                                if ui.button("Apply").clicked() {
                                    clicked = Some(i);
                                }
                            });
                            ui.label(RichText::new(format!("{} ms", r.ms)).color(PASS));
                        } else {
                            let stage = r.stopped_at.as_deref().unwrap_or("unknown");
                            ui.label(RichText::new(format!("failed at {}", stage_title(stage))).color(FAIL));
                        }
                    });
                });
            });
        let mut tip = String::new();
        if r.credentials_sent {
            tip.push_str("Password sent. ");
        }
        let detail = if r.ok { &r.note } else { &r.hint };
        tip.push_str(detail);
        if !tip.trim().is_empty() {
            row.response.on_hover_text(tip);
        }
    }
    // The same hint usually applies to every failed row, so show it once.
    if v.winner.is_none() && v.status != "running" {
        if let Some(h) = v.results.iter().find(|r| !r.hint.is_empty()).map(|r| r.hint.clone()) {
            ui.label(RichText::new(h).color(WARN));
        }
    }
    clicked
}

fn summary(ui: &mut egui::Ui, r: &DiagnoseReport) {
    if r.ok {
        banner(ui, PASS, &format!("All checks passed: {}", r.endpoint));
    } else {
        let failed = r
            .stages
            .iter()
            .find(|s| s.status == "fail")
            .map(|s| stage_title(&s.name))
            .unwrap_or("unknown");
        banner(ui, FAIL, &format!("Stopped at {failed}: {}", r.endpoint));
    }
}

fn stage_title(name: &str) -> &str {
    match name {
        "dns" => "DNS lookup",
        "tcp" => "TCP connect",
        "tls" => "TLS handshake",
        "ws_upgrade" => "WebSocket upgrade",
        "connect" => "MQTT CONNECT",
        other => other,
    }
}

fn stage_row(ui: &mut egui::Ui, s: &StageResult) {
    let (color, fill) = match s.status.as_str() {
        "pass" => (PASS, Color32::from_rgba_unmultiplied(90, 190, 120, 14)),
        "fail" => (FAIL, Color32::from_rgba_unmultiplied(220, 90, 90, 30)),
        _ => (SKIP, Color32::from_rgba_unmultiplied(150, 150, 150, 8)),
    };
    let edge = (s.status == "fail").then_some(FAIL);
    card(ui, fill, edge, |ui| {
        ui.horizontal(|ui| {
            status_dot(ui, &s.status, color);
            ui.label(RichText::new(stage_title(&s.name)).strong().color(
                if s.status == "skipped" {
                    SKIP
                } else {
                    ui.visuals().text_color()
                },
            ));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if s.status != "skipped" {
                    ui.weak(format!("{} ms", s.ms));
                }
            });
        });
        if !s.detail.is_empty() {
            ui.horizontal(|ui| {
                ui.add_space(26.0);
                ui.add(egui::Label::new(RichText::new(&s.detail).weak()).wrap());
            });
        }
        if !s.hint.is_empty() {
            let c = if s.status == "fail" { FAIL } else { WARN };
            ui.horizontal(|ui| {
                ui.add_space(26.0);
                ui.add(egui::Label::new(RichText::new(&s.hint).color(c)).wrap());
            });
        }
    });
}

/// Round status marker with a hand-drawn tick, cross, or dash (no font glyph needed).
fn status_dot(ui: &mut egui::Ui, status: &str, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::hover());
    let p = ui.painter();
    let c = rect.center();
    p.circle_filled(c, 9.0, color.gamma_multiply(0.28));
    p.circle_stroke(c, 9.0, egui::Stroke::new(1.5, color));
    let st = egui::Stroke::new(2.0, color);
    match status {
        "pass" => {
            p.line_segment([c + egui::vec2(-4.0, 0.5), c + egui::vec2(-1.0, 3.5)], st);
            p.line_segment([c + egui::vec2(-1.0, 3.5), c + egui::vec2(4.5, -3.0)], st);
        }
        "fail" => {
            p.line_segment([c + egui::vec2(-3.5, -3.5), c + egui::vec2(3.5, 3.5)], st);
            p.line_segment([c + egui::vec2(-3.5, 3.5), c + egui::vec2(3.5, -3.5)], st);
        }
        _ => {
            p.line_segment([c + egui::vec2(-4.0, 0.0), c + egui::vec2(4.0, 0.0)], st);
        }
    }
}

/// Rounded, tinted block used for stages, banners, and the auto-setup area.
fn card(ui: &mut egui::Ui, fill: Color32, edge: Option<Color32>, add: impl FnOnce(&mut egui::Ui)) {
    let mut frame = egui::Frame::NONE
        .fill(fill)
        .corner_radius(8.0)
        .inner_margin(egui::Margin::symmetric(12, 5));
    if let Some(e) = edge {
        frame = frame.stroke(egui::Stroke::new(1.0, e.gamma_multiply(0.6)));
    }
    frame.show(ui, |ui| {
        ui.set_width(ui.available_width());
        add(ui);
    });
}

fn banner(ui: &mut egui::Ui, color: Color32, text: &str) {
    card(ui, color.gamma_multiply(0.18), Some(color), |ui| {
        ui.label(RichText::new(text).strong().color(color));
    });
}


/// Status chip for the header: what the diagnosis is doing or concluded.
fn status_pill(running: bool, report: Option<&DiagnoseReport>) -> Option<(&'static str, Color32)> {
    if running {
        return Some(("Running", pw::accent_text()));
    }
    report.map(|r| {
        if r.ok {
            ("Healthy", PASS)
        } else {
            ("Problem found", FAIL)
        }
    })
}


/// Build the sidecar request from a profile, using the same settings the live connection uses.
fn build_request(profile: &ConnectionProfile) -> Result<DiagnoseRequest, String> {
    let password = if profile.password.is_empty() {
        secrets::load_password(&profile.id)
            .ok()
            .flatten()
            .unwrap_or_default()
    } else {
        profile.password.clone()
    };
    let (ca_pem, client_cert_pem, client_key_pem) =
        match certs::discover_optional(&profile.certs_dir)? {
            Some(resolved) => {
                let (ca, cert, key) = resolved.load_pem()?;
                (
                    String::from_utf8_lossy(&ca).into_owned(),
                    String::from_utf8_lossy(&cert).into_owned(),
                    String::from_utf8_lossy(&key).into_owned(),
                )
            }
            None => (String::new(), String::new(), String::new()),
        };
    Ok(DiagnoseRequest {
        host: profile.host.clone(),
        port: profile.port,
        transport: match profile.transport {
            ProfileTransport::Mqtt => "mqtt",
            ProfileTransport::WebSocket => "websocket",
        }
        .into(),
        use_tls: profile.want_tls(),
        ws_path: profile.ws_path.clone(),
        mqtt_version: match profile.mqtt_version {
            MqttVersion::V311 => "3.1.1",
            MqttVersion::V5 => "5",
        }
        .into(),
        client_id: String::new(),
        username: profile.username.clone(),
        password,
        ca_pem,
        client_cert_pem,
        client_key_pem,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(
        transport: ProfileTransport,
        tls: bool,
        port: u16,
        version: MqttVersion,
    ) -> Settings {
        Settings {
            transport,
            use_tls: tls,
            port,
            ws_path: "/mqtt".into(),
            mqtt_version: version,
        }
    }

    #[test]
    fn build_request_maps_profile_settings() {
        let mut p = ConnectionProfile::new_blank("id-1".to_string());
        p.host = "broker.example".into();
        p.port = 8084;
        p.transport = ProfileTransport::WebSocket;
        p.ws_path = "/mqtt".into();
        p.use_tls = true;
        p.mqtt_version = MqttVersion::V5;
        p.username = "dev".into();
        p.password = "s3cret".into();
        p.client_id = "live-session-id".into();
        let req = build_request(&p).expect("request");
        assert_eq!(req.host, "broker.example");
        assert_eq!(req.port, 8084);
        assert_eq!(req.transport, "websocket");
        assert!(req.use_tls);
        assert_eq!(req.mqtt_version, "5");
        assert_eq!(req.password, "s3cret");
        // Never reuse the live session's client ID: a probe would take the session over.
        assert!(req.client_id.is_empty());
        assert!(req.ca_pem.is_empty());
    }

    #[test]
    fn changes_list_only_what_differs() {
        let cur = settings(ProfileTransport::Mqtt, false, 1883, MqttVersion::V311);
        let next = settings(ProfileTransport::Mqtt, true, 8883, MqttVersion::V5);
        let changes = changes_between(&cur, &next);
        assert_eq!(
            changes,
            vec![
                "TLS: off → on",
                "Port: 1883 → 8883",
                "MQTT version: 3.1.1 → 5"
            ]
        );
        assert!(changes_between(&cur, &cur).is_empty());
    }

    #[test]
    fn apply_then_undo_restores_exact_settings() {
        let mut p = ConnectionProfile::new_blank("id-2".to_string());
        p.port = 1883;
        p.username = "keep-me".into();
        let before = settings_of(&p);
        let target = settings(ProfileTransport::WebSocket, true, 8084, MqttVersion::V5);
        apply_settings(&mut p, &target);
        assert_eq!(p.port, 8084);
        assert_eq!(p.transport, ProfileTransport::WebSocket);
        assert_eq!(
            p.username, "keep-me",
            "apply must not touch unrelated fields"
        );
        apply_settings(&mut p, &before);
        assert_eq!(p.port, 1883);
        assert_eq!(p.transport, ProfileTransport::Mqtt);
        assert!(!p.use_tls);
        assert_eq!(p.mqtt_version, MqttVersion::V311);
    }
}
