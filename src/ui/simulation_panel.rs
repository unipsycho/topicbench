//! Pro Simulation spreadsheet panel (floating or docked).
//!
//! One horizontal line per row; Start/Stop apply to **all** sims.
//! Talks HTTP to `topicbench-pro` `/sim` — no formula engine in OSS.

use crate::mqtt::profile::ProfileStore;
use crate::mqtt::session::SessionManager;
use crate::pro::sim_client::{self, SimClientState, SimRow};
use crate::ui::icons::{
    self, IconSet, CHROME_BG, CHROME_CAPTION, CHROME_MUTED, CHROME_TEXT, TINY_ICON,
};
use eframe::egui::{self, Color32, RichText, Vec2, ViewportBuilder, ViewportClass};
use serde_json::{json, Value};

/// Wire ids ordered by display label (Bool Toggle … Random, Random Walk …).
const GEN_TYPES: &[&str] = &[
    "bool_toggle",
    "constant",
    "counter",
    "formula",
    "gaussian",
    "iso_timestamp",
    "enum_cycle",
    "ramp",
    "uniform", // display: Random
    "random_walk",
    "sine",
    "triangle",
];

/// Refresh-interval ticks (ms): fine near 50/100/250, coarser mid, ~1s above 1s.
const INTERVAL_TICKS_MS: &[u64] = &[
    50, 75, 100, 125, 150, 200, 250, 300, 400, 500, 750, 1000, 1500, 2000, 3000, 4000, 5000,
    6000, 7000, 8000, 9000, 10_000,
];

// Spreadsheet column widths (keep rows aligned).
// Refresh→Prec→Value→Generator are tight; Settings gets the reclaimed width.
const COL_PROFILE: f32 = 110.0;
const COL_TOPIC: f32 = 160.0;
const COL_FIELD: f32 = 130.0;
const COL_REFRESH: f32 = 158.0;
const COL_PREC: f32 = 44.0;
const COL_VALUE: f32 = 92.0;
const COL_GEN: f32 = 118.0;
const COL_SETTINGS: f32 = 400.0;
const COL_DELETE: f32 = 36.0;

/// Floating Simulation as a real OS viewport (`floating == true`).
/// Docked UI paints in the central column via [`draw_docked`].
pub fn draw(
    ctx: &egui::Context,
    open: &mut bool,
    floating: &mut bool,
    licensed: bool,
    client: &mut SimClientState,
    sessions: &SessionManager,
    profiles: &ProfileStore,
    icons: &IconSet,
    status: &mut String,
) {
    if !*open || !*floating {
        return;
    }

    // Refresh before paint so the title status matches sidecar state.
    if licensed {
        client.poll_if_due(false);
        ctx.request_repaint_after(std::time::Duration::from_millis(500));
    }

    let mut close = false;
    // OS window (Payload undock pattern) — draggable outside the main TopicBench frame.
    ctx.show_viewport_immediate(
        egui::ViewportId::from_hash_of("topicbench_simulation_vp"),
        ViewportBuilder::default()
            .with_title("Simulation")
            .with_inner_size([1100.0, 360.0])
            .with_min_inner_size([900.0, 220.0]),
        |ctx, class| {
            let mut paint = |ui: &mut egui::Ui| {
                header_bar(ui, icons, floating, open, client);
                ui.separator();
                // Viewport owns both axes — sheet scrolls inside.
                body(ui, licensed, client, sessions, profiles, icons, status, true);
            };
            match class {
                ViewportClass::Embedded => {
                    // Backend cannot spawn real windows — fall back to egui Window.
                    egui::Window::new("Simulation")
                        .id(egui::Id::new("topicbench_simulation_win"))
                        .title_bar(false)
                        .collapsible(false)
                        .default_size([1100.0, 360.0])
                        .min_width(900.0)
                        .min_height(220.0)
                        .frame(egui::Frame::window(&ctx.style()).fill(CHROME_BG))
                        .show(ctx, |ui| paint(ui));
                }
                _ => {
                    egui::CentralPanel::default()
                        .frame(
                            egui::Frame::central_panel(&ctx.style())
                                .fill(CHROME_BG)
                                .inner_margin(egui::Margin::same(8)),
                        )
                        .show(ctx, |ui| paint(ui));
                }
            }
            if ctx.input(|i| i.viewport().close_requested()) {
                close = true;
            }
        },
    );
    if close {
        *open = false;
    }
}

/// Docked Simulation section for the main middle column (same target as stickies / topic tree).
pub fn draw_docked(
    ui: &mut egui::Ui,
    open: &mut bool,
    floating: &mut bool,
    licensed: bool,
    client: &mut SimClientState,
    sessions: &SessionManager,
    profiles: &ProfileStore,
    icons: &IconSet,
    status: &mut String,
) {
    if !*open || *floating {
        return;
    }

    if licensed {
        client.poll_if_due(false);
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(500));
    }

    header_bar(ui, icons, floating, open, client);
    ui.separator();
    // Parent central ScrollArea owns vertical; sheet only scrolls horizontally.
    body(ui, licensed, client, sessions, profiles, icons, status, false);
}

fn header_bar(
    ui: &mut egui::Ui,
    icons: &IconSet,
    floating: &mut bool,
    open: &mut bool,
    client: &SimClientState,
) {
    icons::section_header_bar(ui, |ui| {
        icons::section_icon(ui, icons, "data_table");
        icons::section_title(ui, "Simulation");
        // Status pill — playback takes priority over generator Running/Stopped.
        ui.add_space(8.0);
        let pb = &client.playback;
        let any_running = client.statuses.iter().any(|s| s.running);
        let (label, tip, fg, bg, border) = if pb.active {
            if pb.paused {
                (
                    "Playback paused",
                    format!(
                        "CSV tape paused · loops completed: {} (0 = still on first pass)",
                        pb.loops_completed
                    ),
                    Color32::from_rgb(230, 200, 140),
                    Color32::from_rgb(52, 44, 28),
                    Color32::from_rgb(180, 140, 70),
                )
            } else {
                (
                    "Playback running",
                    format!(
                        "CSV tape playing · loops completed: {} (0 = still on first pass)",
                        pb.loops_completed
                    ),
                    Color32::from_rgb(170, 210, 240),
                    Color32::from_rgb(28, 44, 60),
                    Color32::from_rgb(80, 140, 190),
                )
            }
        } else if any_running {
            (
                "Running",
                "Generator simulations running".into(),
                Color32::from_rgb(170, 225, 180),
                Color32::from_rgb(32, 52, 40),
                Color32::from_rgb(80, 150, 100),
            )
        } else {
            (
                "Stopped",
                "No generators or playback active".into(),
                CHROME_MUTED,
                Color32::from_rgb(46, 48, 54),
                Color32::from_rgb(78, 82, 92),
            )
        };
        ui.scope(|ui| {
            ui.set_height(20.0);
            ui.set_max_height(20.0);
            egui::Frame::NONE
                .fill(bg)
                .stroke(egui::Stroke::new(1.0_f32, border))
                .corner_radius(8.0)
                .inner_margin(egui::Margin::symmetric(8, 2))
                .show(ui, |ui| {
                    ui.label(RichText::new(label).size(12.0).strong().color(fg))
                        .on_hover_text(tip);
                });
        });
        if pb.active {
            ui.add_space(6.0);
            ui.label(
                RichText::new(format!("Loops: {}", pb.loops_completed))
                    .size(12.0)
                    .color(CHROME_CAPTION),
            )
            .on_hover_text(
                "Loops completed — increments each time the CSV finishes and restarts. \
                 0 means still on the first pass.",
            );
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if icons
                .tiny_header(ui, "close_red", false, "Close Simulation panel")
                .clicked()
            {
                *open = false;
            }
            let dock_tip = if *floating {
                "Dock to main column"
            } else {
                "Float as window"
            };
            let dock_icon = if *floating {
                "panel_docked"
            } else {
                "panel_undock"
            };
            if icons
                .tiny_header(ui, dock_icon, false, dock_tip)
                .clicked()
            {
                *floating = !*floating;
            }
        });
    });
}

fn body(
    ui: &mut egui::Ui,
    licensed: bool,
    client: &mut SimClientState,
    sessions: &SessionManager,
    profiles: &ProfileStore,
    icons: &IconSet,
    status: &mut String,
    // True in the floating window (both-axis scroll); false when docked in central column.
    scroll_both: bool,
) {
    // Dev/screenshot: TOPICBENCH_OPEN_PLAYBACK_MODAL=/path/to.csv seeds the Open playback modal once.
    seed_open_playback_modal(client, sessions, profiles);

    if !licensed {
        ui.add_space(12.0);
        ui.label(
            RichText::new("TopicBench Pro required")
                .strong()
                .color(CHROME_TEXT),
        );
        ui.label(
            RichText::new(
                "Simulation publishes scripted payloads through your live sessions. \
                 Unlock Pro to add rows and start/stop generators.",
            )
            .color(CHROME_MUTED),
        );
        return;
    }

    if let Some(err) = &client.last_error {
        ui.colored_label(Color32::from_rgb(220, 140, 100), err);
        ui.label(
            RichText::new("TopicBench Pro is not running — start it and try again.")
                .small()
                .color(CHROME_MUTED),
        );
    } else if !client.sidecar_up && client.rows.is_empty() {
        ui.label(
            RichText::new("Connecting to TopicBench Pro…").color(CHROME_MUTED),
        );
    }

    // Toolbar — Start/Stop control the CSV tape while playback is active.
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        if toolbar_btn(ui, icons, "add_blue", "Add", true)
            .on_hover_text("Add a simulation row")
            .clicked()
        {
            let profile = default_profile(sessions, profiles);
            let row = sim_client::blank_row(&profile, "lab/sim/value");
            match client.upsert(&row) {
                Ok(r) => {
                    client.selected = Some(r.id.clone());
                    *status = format!("Added simulation {}", r.id);
                }
                Err(e) => *status = e,
            }
        }
        let pb_active = client.playback.active;
        let pb_paused = client.playback.paused;
        let play_tip = if pb_active {
            if pb_paused {
                "Resume CSV playback"
            } else {
                "Playback is already running"
            }
        } else {
            "Start all simulations"
        };
        let pause_tip = if pb_active {
            if pb_paused {
                "Playback is paused"
            } else {
                "Pause CSV playback"
            }
        } else {
            "Stop all simulations (also cancels playback)"
        };
        if toolbar_btn(ui, icons, "play", "Start", true)
            .on_hover_text(play_tip)
            .clicked()
        {
            if pb_active {
                match client.resume_playback() {
                    Ok(()) => *status = "Playback resumed".into(),
                    Err(e) => *status = e,
                }
            } else {
                match client.start_all() {
                    Ok(()) => *status = "Started all simulations".into(),
                    Err(e) => *status = e,
                }
            }
        }
        if toolbar_btn(ui, icons, "pause", "Stop", true)
            .on_hover_text(pause_tip)
            .clicked()
        {
            if pb_active && !pb_paused {
                match client.pause_playback() {
                    Ok(()) => *status = "Playback paused".into(),
                    Err(e) => *status = e,
                }
            } else if pb_active && pb_paused {
                // Second Stop while paused ends the tape.
                match client.stop_playback() {
                    Ok(()) => *status = "Playback stopped".into(),
                    Err(e) => *status = e,
                }
            } else {
                match client.stop_all() {
                    Ok(()) => *status = "Stopped all simulations".into(),
                    Err(e) => *status = e,
                }
            }
        }
        if pb_active {
            let mut loop_on = client.playback.loop_on;
            if ui
                .checkbox(&mut loop_on, "Loop")
                .on_hover_text("Restart the CSV when it ends and keep looping")
                .changed()
            {
                match client.set_playback_loop(loop_on) {
                    Ok(()) => {
                        *status = if loop_on {
                            "Playback loop on".into()
                        } else {
                            "Playback loop off".into()
                        };
                    }
                    Err(e) => *status = e,
                }
            }
        }

        // Far right: Import / Export / Open playback (CSV tape).
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            if toolbar_btn(ui, icons, "playback", "Open playback", true)
                .on_hover_text("Open a topicbench-recording CSV — play back and/or suggest sims")
                .clicked()
            {
                if let Some(path) = rfd::FileDialog::new()
                    .set_title("Open playback CSV")
                    .add_filter("CSV", &["csv"])
                    .pick_file()
                {
                    match std::fs::read_to_string(&path) {
                        Ok(csv) => {
                            client.pending_playback = Some(sim_client::PendingPlayback {
                                csv,
                                file_label: path
                                    .file_name()
                                    .and_then(|n| n.to_str())
                                    .unwrap_or("recording.csv")
                                    .to_string(),
                                profile: default_profile(sessions, profiles),
                                play: true,
                                suggest: true,
                                loop_on: false,
                            });
                        }
                        Err(e) => *status = format!("read {}: {e}", path.display()),
                    }
                }
            }
            if toolbar_btn(ui, icons, "export", "Export", true)
                .on_hover_text("Export simulation definitions as a bare JSON array")
                .clicked()
            {
                match client.export_sims() {
                    Ok(json) => {
                        if let Some(path) = rfd::FileDialog::new()
                            .set_title("Export simulations")
                            .add_filter("JSON", &["json"])
                            .set_file_name("simulations.json")
                            .save_file()
                        {
                            match serde_json::to_string_pretty(&json) {
                                Ok(text) => match std::fs::write(&path, format!("{text}\n")) {
                                    Ok(()) => {
                                        *status = format!("Exported {}", path.display());
                                    }
                                    Err(e) => *status = format!("write: {e}"),
                                },
                                Err(e) => *status = format!("json: {e}"),
                            }
                        }
                    }
                    Err(e) => *status = e,
                }
            }
            if toolbar_btn(ui, icons, "import", "Import", true)
                .on_hover_text(
                    "Import sims JSON ([…] or {simulations:[…]}) — Replace / Merge / Cancel",
                )
                .clicked()
            {
                if let Some(path) = rfd::FileDialog::new()
                    .set_title("Import simulations")
                    .add_filter("JSON", &["json"])
                    .pick_file()
                {
                    // Empty list: load without prompting. Else Replace / Merge / Cancel.
                    let mode = if client.rows.is_empty() {
                        Some("replace")
                    } else {
                        match rfd::MessageDialog::new()
                            .set_level(rfd::MessageLevel::Info)
                            .set_title("Import simulations")
                            .set_description(
                                "Replace the current list, or Merge (update matching \
                                 profile+topic+field rows and append the rest)?\n\
                                 Accepts bare [ … ] or { \"simulations\": [ … ] }.",
                            )
                            .set_buttons(rfd::MessageButtons::YesNoCancelCustom(
                                "Replace".into(),
                                "Merge".into(),
                                "Cancel".into(),
                            ))
                            .show()
                        {
                            rfd::MessageDialogResult::Yes => Some("replace"),
                            rfd::MessageDialogResult::No => Some("merge"),
                            rfd::MessageDialogResult::Custom(s)
                                if s.eq_ignore_ascii_case("replace") =>
                            {
                                Some("replace")
                            }
                            rfd::MessageDialogResult::Custom(s)
                                if s.eq_ignore_ascii_case("merge") =>
                            {
                                Some("merge")
                            }
                            _ => None,
                        }
                    };
                    if let Some(mode) = mode {
                        let fail = |status: &mut String, msg: String| {
                            *status = msg.clone();
                            // Modal so parse/shape errors aren't a silent no-op.
                            let _ = rfd::MessageDialog::new()
                                .set_level(rfd::MessageLevel::Error)
                                .set_title("Import failed")
                                .set_description(&msg)
                                .set_buttons(rfd::MessageButtons::Ok)
                                .show();
                        };
                        match std::fs::read_to_string(&path) {
                            Ok(raw) => match serde_json::from_str::<serde_json::Value>(&raw) {
                                Ok(v) => match client.import_sims_mode(&v, mode) {
                                    Ok(n) => {
                                        *status =
                                            format!("Imported {n} simulations ({mode})")
                                    }
                                    Err(e) => fail(status, format!("Import failed: {e}")),
                                },
                                Err(e) => {
                                    fail(status, format!("Import failed: parse JSON: {e}"))
                                }
                            },
                            Err(e) => fail(
                                status,
                                format!("Import failed: read {}: {e}", path.display()),
                            ),
                        }
                    }
                }
            }
        });
    });

    ui.add_space(4.0);

    let sheet = |ui: &mut egui::Ui| {
        egui::Grid::new("sim_sheet")
            .num_columns(9)
            .spacing([5.0, 6.0])
            .min_col_width(36.0)
            .striped(true)
            .show(ui, |ui| {
                // Profile → Topic → Field → Refresh → Prec → Value → Generator → Settings → Delete
                header_cell(ui, COL_PROFILE, "Profile");
                header_cell(ui, COL_TOPIC, "Topic");
                header_cell(ui, COL_FIELD, "Field");
                header_cell(ui, COL_REFRESH, "Refresh");
                header_cell(ui, COL_PREC, "Prec");
                header_cell(ui, COL_VALUE, "Current value");
                header_cell(ui, COL_GEN, "Generator");
                header_cell(ui, COL_SETTINGS, "Settings");
                header_cell(ui, COL_DELETE, "");
                ui.end_row();

                let rows = client.rows.clone();
                for row in rows {
                    draw_row(ui, &row, client, sessions, profiles, icons, status);
                    ui.end_row();
                }
            });
    };

    if scroll_both {
        egui::ScrollArea::both()
            .auto_shrink([false, false])
            .show(ui, sheet);
    } else {
        egui::ScrollArea::horizontal()
            .auto_shrink([false, false])
            .show(ui, sheet);
    }

    // Overlay last so it dims toolbar + sheet (and stays on top).
    draw_open_playback_modal(ui, client, status);
}

fn header_cell(ui: &mut egui::Ui, width: f32, label: &str) {
    ui.allocate_ui_with_layout(
        Vec2::new(width, 18.0),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.set_min_width(width);
            ui.label(RichText::new(label).strong().color(CHROME_CAPTION));
        },
    );
}

fn draw_row(
    ui: &mut egui::Ui,
    row: &SimRow,
    client: &mut SimClientState,
    sessions: &SessionManager,
    profiles: &ProfileStore,
    icons: &IconSet,
    status: &mut String,
) {
    let selected = client.selected.as_deref() == Some(row.id.as_str());
    let st = client.status_for(&row.id).cloned();
    let running = st.as_ref().map(|s| s.running).unwrap_or(false);
    let mut draft = row.clone();
    let mut dirty = false;
    let gen_type = sim_client::generator_label(&draft.generator);

    // Profile (broker) — first column
    ui.allocate_ui_with_layout(
        Vec2::new(COL_PROFILE, 28.0),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.set_min_width(COL_PROFILE);
            egui::ComboBox::from_id_salt(format!("sim_prof_{}", row.id))
                .selected_text(profile_label(profiles, &draft.profile))
                .width(COL_PROFILE - 8.0)
                .show_ui(ui, |ui| {
                    for p in profiles.profiles.values() {
                        let connected = sessions
                            .open
                            .get(&p.id)
                            .map(|s| {
                                matches!(s.state, crate::mqtt::session::SessionState::Connected)
                            })
                            .unwrap_or(false);
                        let label = if connected {
                            format!("● {}", p.name)
                        } else {
                            format!("○ {}", p.name)
                        };
                        if ui
                            .selectable_label(draft.profile == p.id, label)
                            .clicked()
                        {
                            draft.profile = p.id.clone();
                            dirty = true;
                        }
                    }
                });
        },
    );

    // Topic
    ui.allocate_ui_with_layout(
        Vec2::new(COL_TOPIC, 28.0),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.set_min_width(COL_TOPIC);
            if ui
                .add(
                    egui::TextEdit::singleline(&mut draft.topic)
                        .desired_width(COL_TOPIC - 8.0)
                        .hint_text("topic"),
                )
                .changed()
            {
                dirty = true;
            }
        },
    );

    // Field — compact checkbox; path only when checked (Field mode)
    ui.allocate_ui_with_layout(
        Vec2::new(COL_FIELD, 28.0),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.set_min_width(COL_FIELD);
            ui.spacing_mut().item_spacing.x = 4.0;
            let mut use_field = draft.mode == "field_patch";
            if ui
                .checkbox(&mut use_field, "")
                .on_hover_text("Apply generator to a JSON field path (unchecked = whole payload)")
                .changed()
            {
                if use_field {
                    draft.mode = "field_patch".into();
                    if draft.field_path.trim().is_empty() {
                        draft.field_path = "value".into();
                    }
                } else {
                    draft.mode = "whole_payload".into();
                }
                dirty = true;
            }
            if use_field {
                if ui
                    .add(
                        egui::TextEdit::singleline(&mut draft.field_path)
                            .desired_width(COL_FIELD - 28.0)
                            .hint_text("path"),
                    )
                    .on_hover_text("JSON dot path to patch (e.g. sensors.temp)")
                    .changed()
                {
                    dirty = true;
                }
            }
        },
    );

    // Refresh — stepped interval (fine at low ms, ~1s above 1s)
    ui.allocate_ui_with_layout(
        Vec2::new(COL_REFRESH, 28.0),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.set_min_width(COL_REFRESH);
            ui.spacing_mut().item_spacing.x = 4.0;
            let mut idx = interval_tick_index(draft.interval_ms) as f32;
            let max_i = (INTERVAL_TICKS_MS.len() - 1) as f32;
            if ui
                .add(
                    egui::Slider::new(&mut idx, 0.0..=max_i)
                        .integer()
                        .show_value(false)
                        .clamping(egui::SliderClamping::Always),
                )
                .on_hover_text("Publish interval (fine at low ms, ~1s steps above 1s)")
                .changed()
            {
                let i = idx.round().clamp(0.0, max_i) as usize;
                draft.interval_ms = INTERVAL_TICKS_MS[i];
                dirty = true;
            }
            // Fixed-ish label width so "100 ms" / "10 s" stay readable without padding the column.
            ui.add_sized(
                [52.0, 18.0],
                egui::Label::new(
                    RichText::new(format_interval_ms(draft.interval_ms))
                        .small()
                        .monospace()
                        .color(CHROME_TEXT),
                ),
            );
        },
    );

    // Prec — decimal digits for published numbers
    ui.allocate_ui_with_layout(
        Vec2::new(COL_PREC, 28.0),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.set_min_width(COL_PREC);
            let mut prec = draft.precision as i32;
            if ui
                .add(egui::DragValue::new(&mut prec).range(0..=12).speed(0.2))
                .on_hover_text("Digits after decimal when publishing numbers")
                .changed()
            {
                draft.precision = prec.clamp(0, 12) as u8;
                dirty = true;
            }
        },
    );

    // Current value — click selects row
    let val = sim_client::value_label(st.as_ref());
    ui.allocate_ui_with_layout(
        Vec2::new(COL_VALUE, 28.0),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.set_min_width(COL_VALUE);
            let resp = ui.selectable_label(
                selected,
                RichText::new(truncate(&val, 11))
                    .monospace()
                    .color(if running {
                        Color32::from_rgb(140, 210, 160)
                    } else {
                        CHROME_TEXT
                    }),
            );
            if resp.clicked() {
                client.selected = Some(row.id.clone());
            }
            resp.on_hover_text(&val);
        },
    );

    // Generator type
    ui.allocate_ui_with_layout(
        Vec2::new(COL_GEN, 28.0),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.set_min_width(COL_GEN);
            let mut gt = gen_type.clone();
            egui::ComboBox::from_id_salt(format!("sim_gen_{}", row.id))
                .selected_text(sim_client::generator_display_name(&gt))
                .width(COL_GEN - 8.0)
                .show_ui(ui, |ui| {
                    for t in GEN_TYPES {
                        if ui
                            .selectable_label(gt == *t, sim_client::generator_display_name(t))
                            .clicked()
                        {
                            gt = (*t).to_string();
                            draft.generator = default_generator(t);
                            dirty = true;
                        }
                    }
                });
        },
    );

    // Settings — generator-specific params only
    ui.allocate_ui_with_layout(
        Vec2::new(COL_SETTINGS, 28.0),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.set_min_width(COL_SETTINGS);
            ui.spacing_mut().item_spacing.x = 6.0;
            ui.set_max_height(28.0);
            if edit_generator_inline(ui, &row.id, &gen_type, &mut draft.generator, client) {
                dirty = true;
            }
        },
    );

    // Per-row delete
    ui.allocate_ui_with_layout(
        Vec2::new(COL_DELETE, 28.0),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.set_min_width(COL_DELETE);
            if icons
                .tiny(ui, "delete_red", false, "Delete this simulation")
                .clicked()
            {
                match client.delete(&row.id) {
                    Ok(()) => {
                        if client.selected.as_deref() == Some(row.id.as_str()) {
                            client.selected = None;
                        }
                        *status = format!("Deleted {}", row.id);
                    }
                    Err(e) => *status = e,
                }
            }
        },
    );

    if dirty {
        // Optimistic local patch so a 500ms list poll cannot flash the old mode.
        if let Some(slot) = client.rows.iter_mut().find(|r| r.id == draft.id) {
            *slot = draft.clone();
        }
        match client.upsert(&draft) {
            Ok(saved) => {
                if let Some(slot) = client.rows.iter_mut().find(|r| r.id == saved.id) {
                    *slot = saved;
                }
            }
            Err(e) => *status = e,
        }
    }
}

/// Inline generator params; returns true if changed.
fn edit_generator_inline(
    ui: &mut egui::Ui,
    row_id: &str,
    gen_type: &str,
    generator: &mut Value,
    client: &mut SimClientState,
) -> bool {
    let mut dirty = false;
    match gen_type {
        "constant" => {
            let mut val = num_or_str(generator.get("value"), "0");
            let mut noise = f64_field(generator, "noise_pct", 0.0) as f32;
            ui.label(RichText::new("val").small().color(CHROME_MUTED));
            if ui
                .add(egui::TextEdit::singleline(&mut val).desired_width(48.0))
                .changed()
            {
                let value = serde_json::from_str::<Value>(&val).unwrap_or(Value::String(val));
                *generator = json!({"type": "constant", "value": value, "noise_pct": noise as f64});
                dirty = true;
            }
            ui.label(RichText::new("±%").small().color(CHROME_MUTED));
            if ui
                .add(
                    egui::Slider::new(&mut noise, 0.0..=50.0)
                        .show_value(true)
                        .suffix("%")
                        .clamping(egui::SliderClamping::Always),
                )
                .changed()
            {
                let value = generator.get("value").cloned().unwrap_or(Value::from(0));
                *generator =
                    json!({"type": "constant", "value": value, "noise_pct": noise as f64});
                dirty = true;
            }
        }
        "counter" => {
            let mut start = f64_field(generator, "start", 0.0);
            let mut step = f64_field(generator, "step", 1.0);
            ui.label(RichText::new("start").small().color(CHROME_MUTED));
            if ui.add(egui::DragValue::new(&mut start).speed(0.1)).changed() {
                *generator = json!({"type": "counter", "start": start, "step": step});
                dirty = true;
            }
            ui.label(RichText::new("step").small().color(CHROME_MUTED));
            if ui.add(egui::DragValue::new(&mut step).speed(0.1)).changed() {
                *generator = json!({"type": "counter", "start": start, "step": step});
                dirty = true;
            }
        }
        "sine" | "triangle" => {
            dirty |= edit_min_max_period(ui, row_id, gen_type, generator, true);
        }
        "ramp" => {
            dirty |= edit_min_max_duration(ui, row_id, generator);
        }
        "uniform" | "gaussian" => {
            dirty |= edit_min_max_only(ui, row_id, gen_type, generator);
        }
        "random_walk" => {
            let mut min = f64_field(generator, "min", 0.0);
            let mut max = f64_field(generator, "max", 1.0);
            let mut step = f64_field(generator, "step", 0.1);
            dirty |= range_sliders(ui, row_id, &mut min, &mut max);
            ui.label(RichText::new("step").small().color(CHROME_MUTED));
            if ui.add(egui::DragValue::new(&mut step).speed(0.01)).changed() {
                dirty = true;
            }
            if dirty {
                *generator = json!({"type": "random_walk", "min": min, "max": max, "step": step});
            }
        }
        "bool_toggle" => {
            let mut start = generator
                .get("start")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            if ui.checkbox(&mut start, "start").changed() {
                *generator = json!({"type": "bool_toggle", "start": start});
                dirty = true;
            }
        }
        "iso_timestamp" => {
            ui.label(RichText::new("UTC").small().color(CHROME_MUTED));
        }
        "enum_cycle" => {
            let mut raw = generator
                .get("values")
                .map(|v| {
                    v.as_array()
                        .map(|a| {
                            a.iter()
                                .map(|x| match x {
                                    Value::String(s) => s.clone(),
                                    other => other.to_string(),
                                })
                                .collect::<Vec<_>>()
                                .join(",")
                        })
                        .unwrap_or_default()
                })
                .unwrap_or_else(|| "a,b,c".into());
            if ui
                .add(
                    egui::TextEdit::singleline(&mut raw)
                        .desired_width(120.0)
                        .hint_text("a,b,c"),
                )
                .changed()
            {
                let values: Vec<Value> = raw
                    .split(',')
                    .map(|s| s.trim())
                    .filter(|s| !s.is_empty())
                    .map(|s| {
                        serde_json::from_str::<Value>(s).unwrap_or(Value::String(s.to_string()))
                    })
                    .collect();
                *generator = json!({"type": "enum_cycle", "values": values});
                dirty = true;
            }
        }
        "formula" | "expr" => {
            let mut formula = generator
                .get("formula")
                .and_then(|v| v.as_str())
                .unwrap_or(r#"topic("lab/temp_c") * 1.8 + 32"#)
                .to_string();
            let resp = ui.add(
                egui::TextEdit::singleline(&mut formula)
                    .desired_width(168.0)
                    .hint_text(r#"topic("a/b") * 1.8 + 32"#),
            );
            if resp.changed() {
                *generator = json!({"type": "formula", "formula": formula.clone()});
                dirty = true;
            }
            // Live syntax check via sidecar; cache per row until formula changes.
            let cache_id = egui::Id::new(("sim_formula_ok", row_id));
            let need = ui.ctx().data(|d| {
                d.get_temp::<(String, bool)>(cache_id)
                    .map(|(f, _)| f != formula)
                    .unwrap_or(true)
            });
            if need {
                let ok = !formula.trim().is_empty()
                    && client.validate_formula(&formula).is_ok();
                ui.ctx()
                    .data_mut(|d| d.insert_temp(cache_id, (formula.clone(), ok)));
            }
            let ok = ui.ctx().data(|d| {
                d.get_temp::<(String, bool)>(cache_id)
                    .map(|(_, o)| o)
                    .unwrap_or(false)
            });
            let border = if ok {
                Color32::from_rgb(90, 180, 120)
            } else {
                Color32::from_rgb(200, 90, 90)
            };
            ui.painter().rect_stroke(
                resp.rect.expand(1.0),
                3.0,
                egui::Stroke::new(1.5, border),
                egui::StrokeKind::Outside,
            );
            if !ok {
                resp.on_hover_text(
                    "Invalid formula — use t, n, sin/cos/abs/min/max/rand/randn, \
                     topic(\"path\"), topic(\"path\", \"field\"), sim(\"id\")",
                );
            } else {
                resp.on_hover_text(
                    r#"Calculated channel: topic("lab/temp_c") * 1.8 + 32"#,
                );
            }
        }
        _ => {}
    }
    dirty
}

fn edit_min_max_period(
    ui: &mut egui::Ui,
    row_id: &str,
    gen_type: &str,
    generator: &mut Value,
    with_period: bool,
) -> bool {
    let (mut min, mut max) = resolved_min_max(generator);
    let mut period = f64_field(generator, "period_s", 10.0);
    let mut dirty = range_sliders(ui, row_id, &mut min, &mut max);
    if with_period {
        ui.label(RichText::new("T").small().color(CHROME_MUTED));
        if ui
            .add(egui::DragValue::new(&mut period).range(0.1..=3600.0).speed(0.1))
            .changed()
        {
            dirty = true;
        }
    }
    if dirty {
        *generator = json!({
            "type": gen_type,
            "min": min,
            "max": max,
            "period_s": period,
        });
    }
    dirty
}

fn edit_min_max_duration(ui: &mut egui::Ui, row_id: &str, generator: &mut Value) -> bool {
    let (mut min, mut max) = resolved_min_max(generator);
    // Support legacy start/end already mapped by resolved_min_max.
    let mut duration = f64_field(generator, "duration_s", 10.0);
    let mut loop_ = generator
        .get("loop")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    let mut dirty = range_sliders(ui, row_id, &mut min, &mut max);
    ui.label(RichText::new("dur").small().color(CHROME_MUTED));
    if ui
        .add(egui::DragValue::new(&mut duration).range(0.1..=3600.0).speed(0.1))
        .changed()
    {
        dirty = true;
    }
    if ui.checkbox(&mut loop_, "loop").changed() {
        dirty = true;
    }
    if dirty {
        *generator = json!({
            "type": "ramp",
            "min": min,
            "max": max,
            "duration_s": duration,
            "loop": loop_,
        });
    }
    dirty
}

fn edit_min_max_only(
    ui: &mut egui::Ui,
    row_id: &str,
    gen_type: &str,
    generator: &mut Value,
) -> bool {
    let (mut min, mut max) = resolved_min_max(generator);
    let dirty = range_sliders(ui, row_id, &mut min, &mut max);
    if dirty {
        *generator = json!({"type": gen_type, "min": min, "max": max});
    }
    dirty
}

fn range_sliders(ui: &mut egui::Ui, row_id: &str, min: &mut f64, max: &mut f64) -> bool {
    let _ = row_id;
    let mut dirty = false;
    ui.label(RichText::new("min").small().color(CHROME_MUTED));
    if ui.add(egui::DragValue::new(min).speed(0.1)).changed() {
        dirty = true;
    }
    ui.label(RichText::new("max").small().color(CHROME_MUTED));
    if ui.add(egui::DragValue::new(max).speed(0.1)).changed() {
        dirty = true;
    }
    if *min > *max {
        std::mem::swap(min, max);
    }
    dirty
}

/// Resolve min/max from new fields or legacy amplitude/offset / start/end / mean±std.
fn resolved_min_max(generator: &Value) -> (f64, f64) {
    if let (Some(mi), Some(ma)) = (
        generator.get("min").and_then(|v| v.as_f64()),
        generator.get("max").and_then(|v| v.as_f64()),
    ) {
        return if mi <= ma { (mi, ma) } else { (ma, mi) };
    }
    if let (Some(start), Some(end)) = (
        generator.get("start").and_then(|v| v.as_f64()),
        generator.get("end").and_then(|v| v.as_f64()),
    ) {
        return if start <= end {
            (start, end)
        } else {
            (end, start)
        };
    }
    if let Some(amp) = generator.get("amplitude").and_then(|v| v.as_f64()) {
        let o = generator
            .get("offset")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        return (o - amp, o + amp);
    }
    if let (Some(mean), Some(std)) = (
        generator.get("mean").and_then(|v| v.as_f64()),
        generator
            .get("std_dev")
            .or_else(|| generator.get("std"))
            .and_then(|v| v.as_f64()),
    ) {
        return (mean - 3.0 * std.abs(), mean + 3.0 * std.abs());
    }
    (0.0, 100.0)
}

fn f64_field(gen: &Value, key: &str, default: f64) -> f64 {
    gen.get(key).and_then(|v| v.as_f64()).unwrap_or(default)
}

fn num_or_str(v: Option<&Value>, default: &str) -> String {
    match v {
        Some(Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
        None => default.to_string(),
    }
}

fn truncate(s: &str, max_chars: usize) -> String {
    let mut it = s.chars();
    let head: String = it.by_ref().take(max_chars).collect();
    if it.next().is_some() {
        format!("{head}…")
    } else {
        head
    }
}


/// Icon + label button matching Profiles Connect / Publish chrome.

/// Nearest tick index for `interval_ms` (clamped to the tick table).
fn interval_tick_index(ms: u64) -> usize {
    let mut best = 0usize;
    let mut best_d = u64::MAX;
    for (i, &t) in INTERVAL_TICKS_MS.iter().enumerate() {
        let d = ms.abs_diff(t);
        if d < best_d {
            best_d = d;
            best = i;
        }
    }
    best
}

/// Compact interval label: `100 ms` or `2 s`.
fn format_interval_ms(ms: u64) -> String {
    if ms >= 1000 && ms % 1000 == 0 {
        format!("{} s", ms / 1000)
    } else if ms >= 1000 {
        format!("{:.1} s", ms as f64 / 1000.0)
    } else {
        format!("{ms} ms")
    }
}

fn toolbar_btn(
    ui: &mut egui::Ui,
    icons: &IconSet,
    icon: &str,
    label: &str,
    enabled: bool,
) -> egui::Response {
    let btn = if let Some(tex) = icons.get(icon) {
        egui::Button::image_and_text(
            egui::Image::new(tex).fit_to_exact_size(egui::vec2(TINY_ICON, TINY_ICON)),
            label,
        )
    } else {
        egui::Button::new(label)
    };
    let min_w = if label.len() > 10 { 128.0 } else { 88.0 };
    ui.add_enabled(enabled, btn.min_size(egui::vec2(min_w, 28.0)))
}

/// One-shot seed from `TOPICBENCH_OPEN_PLAYBACK_MODAL` (path to a recording CSV).
fn seed_open_playback_modal(
    client: &mut SimClientState,
    sessions: &SessionManager,
    profiles: &ProfileStore,
) {
    use std::sync::atomic::{AtomicBool, Ordering};
    static SEEDED: AtomicBool = AtomicBool::new(false);
    if client.pending_playback.is_some() || SEEDED.swap(true, Ordering::SeqCst) {
        return;
    }
    let Ok(path) = std::env::var("TOPICBENCH_OPEN_PLAYBACK_MODAL") else {
        return;
    };
    let path = path.trim();
    if path.is_empty() {
        return;
    }
    match std::fs::read_to_string(path) {
        Ok(csv) => {
            let label = std::path::Path::new(path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("recording.csv")
                .to_string();
            client.pending_playback = Some(sim_client::PendingPlayback {
                csv,
                file_label: label,
                profile: default_profile(sessions, profiles),
                play: true,
                suggest: true,
                loop_on: false,
            });
        }
        Err(e) => eprintln!("topicbench: TOPICBENCH_OPEN_PLAYBACK_MODAL read {path}: {e}"),
    }
}

/// Modal after picking a CSV: play back / suggest / loop (in-panel overlay — reliable in OS viewports).
fn draw_open_playback_modal(
    ui: &mut egui::Ui,
    client: &mut SimClientState,
    status: &mut String,
) {
    let Some(pending) = client.pending_playback.clone() else {
        return;
    };
    let mut draft = pending;
    let mut close = false;
    let mut confirm = false;

    let full = ui.max_rect();
    // Dim the sheet behind the modal.
    ui.painter()
        .rect_filled(full, 0.0, Color32::from_black_alpha(140));
    // Block clicks to the sheet while the modal is open.
    ui.interact(
        full,
        ui.id().with("open_playback_dim"),
        egui::Sense::click(),
    );

    let panel = egui::Area::new(egui::Id::new("open_playback_modal"))
        .order(egui::Order::Foreground)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .movable(false)
        .show(ui.ctx(), |ui| {
            egui::Frame::NONE
                .fill(Color32::from_rgb(36, 38, 44))
                .stroke(egui::Stroke::new(1.0, Color32::from_rgb(90, 96, 110)))
                .corner_radius(8.0)
                .inner_margin(egui::Margin::symmetric(16, 14))
                .show(ui, |ui| {
                    ui.set_min_width(360.0);
                    ui.label(RichText::new("Open playback").strong().size(15.0));
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new(format!("File: {}", draft.file_label))
                            .color(CHROME_CAPTION),
                    );
                    ui.add_space(10.0);
                    ui.checkbox(&mut draft.play, "Play it back")
                        .on_hover_text("Start CSV tape playback through the live session");
                    ui.checkbox(&mut draft.suggest, "Suggest simulation values")
                        .on_hover_text(
                            "Analyze the tape and merge stopped generator drafts into the list",
                        );
                    ui.add_enabled_ui(draft.play, |ui| {
                        ui.checkbox(&mut draft.loop_on, "Loop")
                            .on_hover_text("When playing, restart the CSV when it ends");
                    });
                    ui.add_space(14.0);
                    ui.horizontal(|ui| {
                        if ui.button("Cancel").clicked() {
                            close = true;
                        }
                        let can = draft.play || draft.suggest;
                        if ui
                            .add_enabled(
                                can,
                                egui::Button::new("OK").min_size(egui::vec2(72.0, 0.0)),
                            )
                            .clicked()
                        {
                            confirm = true;
                        }
                    });
                });
        });
    let _ = panel;

    if confirm {
        let mut notes = Vec::new();
        if draft.suggest {
            match client.suggest_from_recording(&draft.csv, &draft.profile) {
                Ok(rows) => {
                    if rows.is_empty() {
                        notes.push("no suggestions".into());
                    } else {
                        let arr: Vec<Value> = rows
                            .iter()
                            .filter_map(|r| serde_json::to_value(r).ok())
                            .collect();
                        match client.import_sims_mode(&Value::Array(arr), "merge") {
                            Ok(n) => notes.push(format!("suggested {n} sims")),
                            Err(e) => notes.push(format!("suggest merge: {e}")),
                        }
                    }
                }
                Err(e) => notes.push(format!("suggest: {e}")),
            }
        }
        if draft.play {
            match client.start_playback(&draft.csv, &draft.profile, draft.loop_on) {
                Ok(()) => notes.push("playback started".into()),
                Err(e) => notes.push(format!("playback: {e}")),
            }
        }
        *status = notes.join(" · ");
        client.pending_playback = None;
    } else if close {
        client.pending_playback = None;
    } else if let Some(slot) = client.pending_playback.as_mut() {
        *slot = draft;
    }
}

fn default_profile(sessions: &SessionManager, profiles: &ProfileStore) -> String {
    for p in profiles.profiles.values() {
        if sessions
            .open
            .get(&p.id)
            .map(|s| matches!(s.state, crate::mqtt::session::SessionState::Connected))
            .unwrap_or(false)
        {
            return p.id.clone();
        }
    }
    profiles
        .profiles
        .values()
        .next()
        .map(|p| p.id.clone())
        .unwrap_or_else(|| "profile-1".into())
}

fn profile_label(profiles: &ProfileStore, id: &str) -> String {
    profiles
        .profiles
        .get(id)
        .map(|p| p.name.clone())
        .unwrap_or_else(|| id.to_string())
}

fn default_generator(t: &str) -> Value {
    match t {
        "constant" => json!({"type": "constant", "value": 100, "noise_pct": 0}),
        "counter" => json!({"type": "counter", "start": 0, "step": 1}),
        "sine" => json!({"type": "sine", "min": 0.0, "max": 100.0, "period_s": 10.0}),
        "triangle" => json!({"type": "triangle", "min": 0.0, "max": 100.0, "period_s": 10.0}),
        "ramp" => {
            json!({"type": "ramp", "min": 0.0, "max": 100.0, "duration_s": 10.0, "loop": true})
        }
        "uniform" => json!({"type": "uniform", "min": 0.0, "max": 100.0}),
        "gaussian" => json!({"type": "gaussian", "min": 0.0, "max": 100.0}),
        "random_walk" => json!({"type": "random_walk", "min": 0.0, "max": 100.0, "step": 1.0}),
        "bool_toggle" => json!({"type": "bool_toggle", "start": false}),
        "iso_timestamp" => json!({"type": "iso_timestamp"}),
        "enum_cycle" => json!({"type": "enum_cycle", "values": ["a", "b", "c"]}),
        "formula" | "expr" => json!({"type": "formula", "formula": r#"topic("lab/temp_c") * 1.8 + 32"#}),
        _ => json!({"type": "sine", "min": 0.0, "max": 100.0, "period_s": 10.0}),
    }
}
