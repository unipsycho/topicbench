//! Pro Simulation spreadsheet panel (floating or docked).
//!
//! One horizontal line per row; Start/Stop apply to **all** sims.
//! Talks HTTP to `topicbench-pro` `/sim` — no formula engine in OSS.

use crate::mqtt::profile::ProfileStore;
use crate::mqtt::session::SessionManager;
use crate::pro::sim_client::{self, SimClientState, SimRow};
use crate::ui::icons::{
    self, IconSet, CHROME_ACCENT_BORDER, CHROME_ACCENT_SOFT, CHROME_BG, CHROME_CAPTION,
    CHROME_MUTED, CHROME_ROW, CHROME_TEXT, TINY_ICON,
};
use eframe::egui::{self, Color32, Pos2, RichText, Vec2, ViewportBuilder, ViewportClass};
use serde_json::{json, Value};
use std::collections::HashSet;

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

// Spreadsheet geometry — Topic/Field/Value flex; Settings aligned slots; trash far-right.
const ROW_H: f32 = 22.0;
const ROW_GAP: f32 = 2.0; // between-row chrome — marquee drag target
const COL_GAP: f32 = 4.0;
/// Fixed label width inside Settings slots so values share one vertical column.
const SET_LABEL_W: f32 = 30.0;
/// Wide left gutter: circle select glyph + dedicated marquee drag strip (avoids fighting inputs).
const COL_GUTTER: f32 = 28.0;
const COL_PROFILE: f32 = 104.0;
const TOPIC_MIN: f32 = 120.0;
const FIELD_MIN: f32 = 96.0;
const VALUE_MIN: f32 = 72.0;
/// Cap so long ISO timestamps ellipsize inside the cell and never shove Generator/trash.
const VALUE_MAX: f32 = 120.0;
const TOPIC_FLEX: f32 = 1.55;
const FIELD_FLEX: f32 = 1.0;
const VALUE_FLEX: f32 = 0.75;
const COL_REFRESH: f32 = 148.0;
const COL_PREC: f32 = 40.0;
const COL_GEN: f32 = 110.0;
const COL_DELETE: f32 = 26.0;
/// Settings band: four vertical columns shared by every generator row.
const SET_GAP: f32 = 4.0;
const SET_A: f32 = 84.0; // min / val / start
const SET_B: f32 = 84.0; // max / ±% / step
const SET_C: f32 = 76.0; // T / dur / step / noise
const SET_D: f32 = 56.0; // loop / extras
const COL_SETTINGS: f32 = SET_A + SET_B + SET_C + SET_D + 3.0 * SET_GAP;
/// Available width below this → two-line stacked rows (less horizontal scroll).
const WRAP_WIDTH: f32 = 980.0;
/// Marquee drag must exceed this before box-select engages (preserves click / future reorder).
const MARQUEE_SLOP: f32 = 6.0;

/// Fixed columns in the wide single-line layout (everything except Topic/Field/Value).
fn wide_fixed_w() -> f32 {
    COL_GUTTER + COL_PROFILE + COL_REFRESH + COL_PREC + COL_GEN + COL_SETTINGS + COL_DELETE
}

fn wide_gap_w() -> f32 {
    9.0 * COL_GAP // ten cells → nine gaps
}

/// Fixed columns on wrap line 1 (everything except Topic/Field/Value).
fn wrap_fixed_w() -> f32 {
    COL_GUTTER + COL_PROFILE + COL_GEN + COL_DELETE
}

fn wrap_gap_w() -> f32 {
    6.0 * COL_GAP // seven cells → six gaps
}

/// Share leftover width across Topic / Field / Current value (value capped at [`VALUE_MAX`]).
fn flex_topic_field_value(pool: f32) -> (f32, f32, f32) {
    let mins = TOPIC_MIN + FIELD_MIN + VALUE_MIN;
    if pool <= mins {
        let s = if mins > 0.0 {
            (pool / mins).clamp(0.0, 1.0)
        } else {
            0.0
        };
        return (TOPIC_MIN * s, FIELD_MIN * s, VALUE_MIN * s);
    }
    let extra = pool - mins;
    let tw = TOPIC_FLEX + FIELD_FLEX + VALUE_FLEX;
    let mut topic = TOPIC_MIN + extra * (TOPIC_FLEX / tw);
    let mut field = FIELD_MIN + extra * (FIELD_FLEX / tw);
    let mut value = VALUE_MIN + extra * (VALUE_FLEX / tw);
    // Overflow past VALUE_MAX goes back to Topic/Field so timestamps never widen the column.
    if value > VALUE_MAX {
        let overflow = value - VALUE_MAX;
        value = VALUE_MAX;
        let tf = TOPIC_FLEX + FIELD_FLEX;
        topic += overflow * (TOPIC_FLEX / tf);
        field += overflow * (FIELD_FLEX / tf);
    }
    (topic, field, value)
}

/// Match Simulate toolbar **Start** (resume CSV tape, else start_all).
pub fn action_start(client: &mut SimClientState, status: &mut String) {
    if client.playback.active {
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

/// Match Simulate toolbar **Stop** (pause → stop tape → stop_all).
pub fn action_stop(client: &mut SimClientState, status: &mut String) {
    let pb_active = client.playback.active;
    let pb_paused = client.playback.paused;
    if pb_active && !pb_paused {
        match client.pause_playback() {
            Ok(()) => *status = "Playback paused".into(),
            Err(e) => *status = e,
        }
    } else if pb_active && pb_paused {
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

/// Consume Ctrl+Shift+G / H on this context (main shell or Simulation viewport).
pub fn consume_transport_keys(ctx: &egui::Context) -> Option<&'static str> {
    let mut out = None;
    ctx.input_mut(|i| {
        let mods = egui::Modifiers::COMMAND | egui::Modifiers::SHIFT;
        if i.consume_key(mods, egui::Key::G) {
            out = Some("start");
        } else if i.consume_key(mods, egui::Key::H) {
            out = Some("stop");
        }
    });
    out
}

/// Floating Simulation as a real OS viewport (`floating == true`).
/// Docked UI paints in the central column via [`draw_docked`].
#[allow(clippy::too_many_arguments)]
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
    builder_seed: crate::mqtt::window_geom::WinGeom,
    saved_geom: &mut Option<crate::mqtt::window_geom::WinGeom>,
    geom_dirty: &mut bool,
) {
    if !*open || !*floating {
        client.disarm_toolbar();
        return;
    }
    // Floating viewport restore can deliver a phantom click to the far-right
    // Open playback button — cool down before beginning native dialogs.
    client.arm_toolbar_after_open();

    // Refresh before paint so the title status matches sidecar state.
    if licensed {
        client.poll_if_due(false);
        ctx.request_repaint_after(std::time::Duration::from_millis(500));
    }

    let mut close = false;
    const MIN: [f32; 2] = [900.0, 220.0];
    // OS window (Payload undock pattern) — draggable outside the main TopicBench frame.
    // Seed is frozen for this open session so patch does not fight live resize.
    ctx.show_viewport_immediate(
        egui::ViewportId::from_hash_of("topicbench_simulation_vp"),
        builder_seed
            .apply(ViewportBuilder::default().with_title("Simulation"), MIN)
            .with_min_inner_size(MIN),
        |ctx, class| {
            // Same chords as global — viewport focus does not reach main handle_shortcuts.
            if licensed {
                match consume_transport_keys(ctx) {
                    Some("start") => action_start(client, status),
                    Some("stop") => action_stop(client, status),
                    _ => {}
                }
                // Delete/arrows while the floating Simulation window is focused.
                handle_sim_list_keys(ctx, client, status);
            }
            let mut paint = |ui: &mut egui::Ui| {
                // Profiles Body/Small fonts + blue checkbox/slider accents.
                icons::apply_pro_panel_chrome(ui);
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
            // Track live size/pos for next launch (does not update builder_seed).
            if let Some(g) = crate::mqtt::window_geom::capture(ctx) {
                if crate::mqtt::window_geom::sync_slot(saved_geom, g, MIN) {
                    *geom_dirty = true;
                }
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
    client.arm_toolbar_after_open();

    if licensed {
        client.poll_if_due(false);
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(500));
    }

    icons::apply_pro_panel_chrome(ui);
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
                    client.select_only(r.id.clone());
                    *status = format!("Added simulation {}", r.id);
                }
                Err(e) => *status = e,
            }
        }
        let sel_n = client.selected_ids.len();
        let del_enabled = sel_n > 0;
        let del_tip = if sel_n == 0 {
            crate::ui::shortcuts::tip("Select rows to delete", "Delete")
        } else if sel_n == 1 {
            crate::ui::shortcuts::tip("Delete selected simulation", "Delete")
        } else {
            crate::ui::shortcuts::tip(
                &format!("Delete {sel_n} selected simulations"),
                "Delete",
            )
        };
        if toolbar_btn(ui, icons, "delete_red", "Delete", del_enabled)
            .on_hover_text(del_tip)
            .clicked()
        {
            delete_selection(client, status);
        }
        let pb_active = client.playback.active;
        let pb_paused = client.playback.paused;
        let play_tip = if pb_active {
            if pb_paused {
                crate::ui::shortcuts::tip("Resume CSV playback", "Ctrl+Shift+G")
            } else {
                crate::ui::shortcuts::tip("Playback is already running", "Ctrl+Shift+G")
            }
        } else {
            crate::ui::shortcuts::tip("Start all simulations", "Ctrl+Shift+G")
        };
        let pause_tip = if pb_active {
            if pb_paused {
                crate::ui::shortcuts::tip("Playback is paused — Stop again to end tape", "Ctrl+Shift+H")
            } else {
                crate::ui::shortcuts::tip("Pause CSV playback", "Ctrl+Shift+H")
            }
        } else {
            crate::ui::shortcuts::tip(
                "Stop all simulations (also cancels playback)",
                "Ctrl+Shift+H",
            )
        };
        if toolbar_btn(ui, icons, "play", "Start", true)
            .on_hover_text(play_tip)
            .clicked()
        {
            action_start(client, status);
        }
        if toolbar_btn(ui, icons, "pause", "Stop", true)
            .on_hover_text(pause_tip)
            .clicked()
        {
            action_stop(client, status);
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
        if sel_n > 0 {
            ui.label(
                RichText::new(format!("{sel_n} selected"))
                    .small()
                    .color(CHROME_CAPTION),
            );
        }

        // Far right: Import / Export / Open playback (CSV tape).
        // Off-thread native dialogs — avoid Linux Wayland "app hung" ANR.
        let playback_dlg = egui::Id::new("dlg_sim_playback");
        let export_dlg = egui::Id::new("dlg_sim_export");
        let export_text_key = egui::Id::new("dlg_sim_export_text");
        let import_dlg = egui::Id::new("dlg_sim_import");
        let import_mode_dlg = egui::Id::new("dlg_sim_import_mode");
        let import_err_dlg = egui::Id::new("dlg_sim_import_err");
        let import_path_key = egui::Id::new("dlg_sim_import_path");
        let toolbar_ok = client.toolbar_armed();
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            if toolbar_btn(ui, icons, "playback", "Open playback", toolbar_ok)
                .on_hover_text("Open a topicbench-recording CSV — play back and/or suggest sims")
                .clicked()
                && toolbar_ok
            {
                crate::ui::native_dialog::begin_path(
                    ui.ctx(),
                    playback_dlg,
                    crate::ui::native_dialog::PathOp::PickFile,
                    |d| {
                        d.set_title("Open playback CSV")
                            .add_filter("CSV", &["csv"])
                    },
                );
            }
            if let Some(Some(path)) = crate::ui::native_dialog::poll_path(ui.ctx(), playback_dlg) {
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
            if toolbar_btn(ui, icons, "export", "Export", toolbar_ok)
                .on_hover_text("Export simulation definitions as a bare JSON array")
                .clicked()
                && toolbar_ok
            {
                match client.export_sims() {
                    Ok(json) => match serde_json::to_string_pretty(&json) {
                        Ok(text) => {
                            ui.ctx()
                                .data_mut(|d| d.insert_temp(export_text_key, text));
                            crate::ui::native_dialog::begin_path(
                                ui.ctx(),
                                export_dlg,
                                crate::ui::native_dialog::PathOp::SaveFile,
                                |d| {
                                    d.set_title("Export simulations")
                                        .add_filter("JSON", &["json"])
                                        .set_file_name("simulations.json")
                                },
                            );
                        }
                        Err(e) => *status = format!("json: {e}"),
                    },
                    Err(e) => *status = e,
                }
            }
            if let Some(path_opt) = crate::ui::native_dialog::poll_path(ui.ctx(), export_dlg) {
                if let Some(path) = path_opt {
                    if let Some(text) =
                        ui.ctx().data_mut(|d| d.remove_temp::<String>(export_text_key))
                    {
                        match std::fs::write(&path, format!("{text}\n")) {
                            Ok(()) => *status = format!("Exported {}", path.display()),
                            Err(e) => *status = format!("write: {e}"),
                        }
                    }
                } else {
                    let _ = ui.ctx().data_mut(|d| d.remove_temp::<String>(export_text_key));
                }
            }
            if toolbar_btn(ui, icons, "import", "Import", toolbar_ok)
                .on_hover_text(
                    "Import sims JSON ([…] or {simulations:[…]}) — Replace / Merge / Cancel",
                )
                .clicked()
                && toolbar_ok
            {
                crate::ui::native_dialog::begin_path(
                    ui.ctx(),
                    import_dlg,
                    crate::ui::native_dialog::PathOp::PickFile,
                    |d| {
                        d.set_title("Import simulations")
                            .add_filter("JSON", &["json"])
                    },
                );
            }
            if let Some(Some(path)) = crate::ui::native_dialog::poll_path(ui.ctx(), import_dlg) {
                if client.rows.is_empty() {
                    finish_sim_import(ui.ctx(), client, status, &path, "replace", import_err_dlg);
                } else {
                    ui.ctx().data_mut(|d| d.insert_temp(import_path_key, path));
                    crate::ui::native_dialog::begin_message(ui.ctx(), import_mode_dlg, |d| {
                        d.set_level(rfd::MessageLevel::Info)
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
                    });
                }
            }
            if let Some(result) =
                crate::ui::native_dialog::poll_message(ui.ctx(), import_mode_dlg)
            {
                if let Some(mode) = crate::ui::native_dialog::replace_merge_mode(result) {
                    if let Some(path) = ui
                        .ctx()
                        .data_mut(|d| d.remove_temp::<std::path::PathBuf>(import_path_key))
                    {
                        finish_sim_import(ui.ctx(), client, status, &path, mode, import_err_dlg);
                    }
                } else {
                    let _ = ui
                        .ctx()
                        .data_mut(|d| d.remove_temp::<std::path::PathBuf>(import_path_key));
                }
            }
            // Drain error acknowledgement so it never blocks the UI thread.
            let _ = crate::ui::native_dialog::poll_message(ui.ctx(), import_err_dlg);
        });
    });

    ui.add_space(4.0);

    // List keys (Delete / arrows) when not typing into a field.
    handle_sim_list_keys(ui.ctx(), client, status);

    let wrap = ui.available_width() < WRAP_WIDTH;

    let sheet = |ui: &mut egui::Ui| {
        ui.spacing_mut().item_spacing.y = ROW_GAP;
        // Match row frame horizontal margin so flex cols fill the real inner width.
        let inner_w = (ui.available_width() - 4.0).max(0.0);
        let (topic_w, field_w, value_w) = if wrap {
            flex_topic_field_value((inner_w - wrap_fixed_w() - wrap_gap_w()).max(0.0))
        } else {
            flex_topic_field_value((inner_w - wide_fixed_w() - wide_gap_w()).max(0.0))
        };
        draw_sheet_header(ui, wrap, topic_w, field_w, value_w);
        ui.add_space(1.0);

        let rows = client.rows.clone();
        let ids: Vec<String> = rows.iter().map(|r| r.id.clone()).collect();
        let mut row_rects: Vec<(String, egui::Rect)> = Vec::with_capacity(rows.len());
        let list_origin = ui.cursor().min;

        for row in &rows {
            let rect = draw_row(
                ui,
                row,
                &ids,
                wrap,
                topic_w,
                field_w,
                value_w,
                client,
                sessions,
                profiles,
                icons,
                status,
            );
            row_rects.push((row.id.clone(), rect));
        }

        let list_bottom = ui.cursor().min.y;
        let list_rect = egui::Rect::from_min_max(
            list_origin,
            Pos2::new(
                ui.max_rect().right(),
                list_bottom.max(list_origin.y + 4.0),
            ),
        );
        handle_marquee(ui, client, &ids, &row_rects, list_rect);
        client.prune_selection();
    };

    if scroll_both {
        egui::ScrollArea::both()
            .auto_shrink([false, false])
            .id_salt("sim_sheet_scroll")
            .show(ui, sheet);
    } else {
        // Docked: hug content vertically so Topic tree sits right under the list
        // (auto_shrink y=false left a tall empty band after the last row).
        egui::ScrollArea::horizontal()
            .auto_shrink([false, true])
            .id_salt("sim_sheet_scroll")
            .show(ui, sheet);
    }

    // Overlay last so it dims toolbar + sheet (and stays on top).
    draw_open_playback_modal(ui, icons, client, status);
}

fn delete_selection(client: &mut SimClientState, status: &mut String) {
    let n = client.selected_ids.len();
    if n == 0 {
        return;
    }
    match client.delete_selected() {
        Ok(deleted) => {
            *status = if deleted == 1 {
                "Deleted 1 simulation".into()
            } else {
                format!("Deleted {deleted} simulations")
            };
        }
        Err(e) => *status = e,
    }
}

fn handle_sim_list_keys(ctx: &egui::Context, client: &mut SimClientState, status: &mut String) {
    if ctx.wants_keyboard_input() || client.rows.is_empty() {
        return;
    }
    // Only when the sim list has a selection (avoids stealing profile Delete/arrows).
    if client.selected_ids.is_empty() && client.selection_focus.is_none() {
        return;
    }
    let shift = ctx.input(|i| i.modifiers.shift);
    let mut do_del = false;
    let mut move_by: Option<i32> = None;
    let mut clear = false;
    ctx.input_mut(|i| {
        if i.consume_key(egui::Modifiers::NONE, egui::Key::Delete)
            || i.consume_key(egui::Modifiers::COMMAND, egui::Key::Backspace)
        {
            if !client.selected_ids.is_empty() {
                do_del = true;
            }
        } else if i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp)
            || i.consume_key(egui::Modifiers::SHIFT, egui::Key::ArrowUp)
        {
            move_by = Some(-1);
        } else if i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown)
            || i.consume_key(egui::Modifiers::SHIFT, egui::Key::ArrowDown)
        {
            move_by = Some(1);
        } else if i.consume_key(egui::Modifiers::NONE, egui::Key::Escape) {
            clear = true;
        }
    });
    if do_del {
        delete_selection(client, status);
    } else if let Some(delta) = move_by {
        client.move_selection(delta, shift);
    } else if clear {
        client.clear_selection();
    }
}

fn apply_row_click(client: &mut SimClientState, ids: &[String], id: &str, mods: egui::Modifiers) {
    if mods.command || mods.ctrl {
        client.toggle_select(id.to_string());
    } else if mods.shift {
        client.select_range(ids, id);
    } else {
        client.select_only(id.to_string());
    }
}

#[derive(Clone)]
struct SimMarquee {
    origin: Pos2,
    /// Ctrl/Cmd held at press → add to selection instead of replace.
    additive: bool,
    /// Snapshot of selection when the drag began (additive mode).
    base: HashSet<String>,
}

impl Default for SimMarquee {
    fn default() -> Self {
        Self {
            origin: Pos2::ZERO,
            additive: false,
            base: HashSet::new(),
        }
    }
}

fn handle_marquee(
    ui: &mut egui::Ui,
    client: &mut SimClientState,
    ids: &[String],
    row_rects: &[(String, egui::Rect)],
    list_rect: egui::Rect,
) {
    let marquee_id = egui::Id::new("sim_list_marquee");
    let pointer = ui.input(|i| i.pointer.clone());
    let mods = ui.input(|i| i.modifiers);
    let mut marquee = ui.ctx().data_mut(|d| d.get_temp::<SimMarquee>(marquee_id));

    // Start on primary press over the list when not typing into a field.
    if pointer.primary_pressed()
        && !ui.ctx().wants_keyboard_input()
        && pointer
            .interact_pos()
            .is_some_and(|p| list_rect.contains(p))
    {
        let origin = pointer.interact_pos().unwrap_or(list_rect.min);
        // Wide left gutter — primary marquee hit strip (circle still click-selects).
        let in_gutter = row_rects.iter().any(|(_, r)| {
            let g = egui::Rect::from_min_size(r.min, Vec2::new(COL_GUTTER, r.height()));
            g.contains(origin)
        }) || row_rects.is_empty();
        let on_row_body = row_rects.iter().any(|(_, r)| r.contains(origin));
        // Between-row chrome (ROW_GAP) or empty list padding.
        let in_row_gap = list_rect.contains(origin) && !on_row_body;
        // Only block when a non-gutter widget already owns a drag (slider etc.).
        // Do NOT use is_using_pointer() — gutter click_and_drag would always trip it.
        let foreign_drag = ui.ctx().dragged_id().is_some() && !in_gutter;

        if !foreign_drag && (in_gutter || in_row_gap) {
            marquee = Some(SimMarquee {
                origin,
                additive: mods.command || mods.ctrl,
                base: if mods.command || mods.ctrl {
                    client.selected_ids.clone()
                } else {
                    HashSet::new()
                },
            });
        } else if !foreign_drag && !mods.shift && !(mods.command || mods.ctrl) && on_row_body {
            // Press on row chrome (not a text/slider steal) can still begin after slop.
            marquee = Some(SimMarquee {
                origin,
                additive: false,
                base: HashSet::new(),
            });
        }
    }

    if let Some(ref m) = marquee {
        if let Some(pos) = pointer.interact_pos() {
            let drag = (pos - m.origin).length();
            if pointer.primary_down() && drag >= MARQUEE_SLOP {
                let rect = egui::Rect::from_two_pos(m.origin, pos);
                ui.painter().rect(
                    rect,
                    2.0,
                    Color32::from_rgba_unmultiplied(80, 140, 200, 40),
                    egui::Stroke::new(1.0, CHROME_ACCENT_BORDER),
                    egui::StrokeKind::Outside,
                );
                let mut hit = m.base.clone();
                for (id, r) in row_rects {
                    if r.intersects(rect) {
                        hit.insert(id.clone());
                    }
                }
                client.selected_ids = hit;
                // Focus tip = last intersecting in list order.
                if let Some((id, _)) = row_rects.iter().rev().find(|(id, r)| {
                    r.intersects(rect) && client.selected_ids.contains(id)
                }) {
                    client.selection_focus = Some(id.clone());
                    if !m.additive {
                        client.selection_anchor = Some(id.clone());
                    }
                }
                ui.ctx().request_repaint();
            }
        }
        if pointer.primary_released() {
            // Click without drag on gutter already handled by row gutter response.
            marquee = None;
        }
    }

    if !pointer.primary_down() && !pointer.primary_pressed() {
        marquee = None;
    }
    ui.ctx().data_mut(|d| {
        if let Some(m) = marquee {
            d.insert_temp(marquee_id, m);
        } else {
            d.remove_temp::<SimMarquee>(marquee_id);
        }
    });
    let _ = ids;
}

fn draw_sheet_header(ui: &mut egui::Ui, wrap: bool, topic_w: f32, field_w: f32, value_w: f32) {
    let header_w = ui.available_width();
    ui.allocate_ui_with_layout(
        Vec2::new(header_w, 16.0),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.spacing_mut().item_spacing.x = COL_GAP;
            header_cell(ui, COL_GUTTER, "");
            header_cell(ui, COL_PROFILE, "Profile");
            header_cell(ui, topic_w, "Topic");
            header_cell(ui, field_w, "Field");
            if !wrap {
                header_cell(ui, COL_REFRESH, "Refresh");
                header_cell(ui, COL_PREC, "Prec");
            }
            header_cell(ui, value_w, "Current value");
            header_cell(ui, COL_GEN, "Generator");
            if !wrap {
                header_cell(ui, COL_SETTINGS, "Settings");
            }
            header_cell(ui, COL_DELETE, "");
        },
    );
}

fn header_cell(ui: &mut egui::Ui, width: f32, label: &str) {
    ui.allocate_ui_with_layout(
        Vec2::new(width, 16.0),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.set_min_width(width);
            if !label.is_empty() {
                ui.label(RichText::new(label).strong().color(CHROME_CAPTION));
            }
        },
    );
}

fn draw_row(
    ui: &mut egui::Ui,
    row: &SimRow,
    ids: &[String],
    wrap: bool,
    topic_w: f32,
    field_w: f32,
    value_w: f32,
    client: &mut SimClientState,
    sessions: &SessionManager,
    profiles: &ProfileStore,
    icons: &IconSet,
    status: &mut String,
) -> egui::Rect {
    let selected = client.is_selected(&row.id);
    let st = client.status_for(&row.id).cloned();
    let running = st.as_ref().map(|s| s.running).unwrap_or(false);
    let mut draft = row.clone();
    let mut dirty = false;
    let gen_type = sim_client::generator_label(&draft.generator);
    let mods = ui.input(|i| i.modifiers);

    let fill = if selected {
        CHROME_ACCENT_SOFT
    } else {
        CHROME_ROW
    };
    // No Frame stroke — a 1px selected stroke widens the row and walks trash right.

    // Full sheet width — Topic/Field/Value flex; trash lands on a shared right edge.
    let row_w = ui.available_width();
    let frame = egui::Frame::NONE
        .fill(fill)
        .stroke(egui::Stroke::NONE)
        .corner_radius(2.0)
        .inner_margin(egui::Margin::symmetric(2, 0));

    let inner = frame.show(ui, |ui| {
        ui.set_min_width((row_w - 4.0).max(200.0));
        ui.spacing_mut().item_spacing.x = COL_GAP;
        ui.spacing_mut().item_spacing.y = 0.0;
        if wrap {
            // Line 1: exact clipped width — value/settings never shove trash off the shared right edge.
            let line_w = ui.available_width();
            let (line_rect, _) =
                ui.allocate_exact_size(Vec2::new(line_w, ROW_H), egui::Sense::hover());
            ui.allocate_new_ui(egui::UiBuilder::new().max_rect(line_rect), |ui| {
                ui.set_clip_rect(line_rect);
                ui.spacing_mut().item_spacing.x = COL_GAP;
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    draw_gutter(ui, client, ids, &row.id, selected, mods);
                    draw_profile_cell(ui, &mut draft, &mut dirty, row, sessions, profiles);
                    draw_topic_cell(ui, &mut draft, &mut dirty, topic_w);
                    draw_field_cell(ui, &mut draft, &mut dirty, field_w);
                    draw_value_cell(
                        ui,
                        client,
                        ids,
                        &row.id,
                        &st,
                        running,
                        selected,
                        mods,
                        value_w,
                    );
                    draw_gen_cell(ui, &mut draft, &mut dirty, &gen_type, row);
                    pin_settings_and_trash(ui, icons, client, &row.id, status, None);
                });
            });
            // Line 2: refresh · prec · settings (aligned slots)
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = COL_GAP;
                ui.add_space(COL_GUTTER);
                draw_refresh_cell(ui, &mut draft, &mut dirty);
                draw_prec_cell(ui, &mut draft, &mut dirty);
                draw_settings_cell(ui, &row.id, &gen_type, &mut draft, client, &mut dirty);
            });
        } else {
            // Wide: exact clipped line — Constant ±% cannot grow the row or shove trash.
            let line_w = ui.available_width();
            let (line_rect, _) =
                ui.allocate_exact_size(Vec2::new(line_w, ROW_H), egui::Sense::hover());
            ui.allocate_new_ui(egui::UiBuilder::new().max_rect(line_rect), |ui| {
                ui.set_clip_rect(line_rect);
                ui.spacing_mut().item_spacing.x = COL_GAP;
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    draw_gutter(ui, client, ids, &row.id, selected, mods);
                    draw_profile_cell(ui, &mut draft, &mut dirty, row, sessions, profiles);
                    draw_topic_cell(ui, &mut draft, &mut dirty, topic_w);
                    draw_field_cell(ui, &mut draft, &mut dirty, field_w);
                    draw_refresh_cell(ui, &mut draft, &mut dirty);
                    draw_prec_cell(ui, &mut draft, &mut dirty);
                    draw_value_cell(
                        ui,
                        client,
                        ids,
                        &row.id,
                        &st,
                        running,
                        selected,
                        mods,
                        value_w,
                    );
                    draw_gen_cell(ui, &mut draft, &mut dirty, &gen_type, row);
                    // Trash pinned far-right; Settings clipped to COL_SETTINGS.
                    pin_settings_and_trash(
                        ui,
                        icons,
                        client,
                        &row.id,
                        status,
                        Some((&row.id, &gen_type, &mut draft, &mut dirty)),
                    );
                });
            });
        }
    });

    // Selection border painted after layout so it never changes row width.
    if selected {
        ui.painter().rect_stroke(
            inner.response.rect,
            2.0,
            egui::Stroke::new(1.0_f32, CHROME_ACCENT_BORDER),
            egui::StrokeKind::Inside,
        );
    }

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

    inner.response.rect
}

fn draw_gutter(
    ui: &mut egui::Ui,
    client: &mut SimClientState,
    ids: &[String],
    id: &str,
    selected: bool,
    mods: egui::Modifiers,
) {
    // Wide strip: click selects; drag from here starts marquee without hitting inputs.
    let (rect, resp) =
        ui.allocate_exact_size(Vec2::new(COL_GUTTER, ROW_H), egui::Sense::click_and_drag());
    // Quiet well so the marquee hit area reads as chrome, not empty void.
    let gut_fill = if selected {
        Color32::from_rgb(40, 64, 92)
    } else {
        Color32::from_rgb(40, 43, 50)
    };
    ui.painter().rect_filled(rect.shrink2(Vec2::new(1.0, 1.0)), 2.0, gut_fill);
    // Painter circles — ● often tofu/squares in egui's font; ○/● must stay circular.
    let center = rect.center();
    let radius = 4.0;
    if selected {
        ui.painter()
            .circle_filled(center, radius, CHROME_ACCENT_BORDER);
    } else {
        ui.painter().circle_stroke(
            center,
            radius,
            egui::Stroke::new(1.25_f32, CHROME_MUTED),
        );
    }
    if resp.clicked() {
        apply_row_click(client, ids, id, mods);
    }
    resp.on_hover_text("Click to select · Ctrl click toggle · Shift range · drag box-select");
}

fn draw_profile_cell(
    ui: &mut egui::Ui,
    draft: &mut SimRow,
    dirty: &mut bool,
    row: &SimRow,
    sessions: &SessionManager,
    profiles: &ProfileStore,
) {
    ui.allocate_ui_with_layout(
        Vec2::new(COL_PROFILE, ROW_H),
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
                            *dirty = true;
                        }
                    }
                });
        },
    );
}

fn draw_topic_cell(ui: &mut egui::Ui, draft: &mut SimRow, dirty: &mut bool, width: f32) {
    ui.allocate_ui_with_layout(
        Vec2::new(width, ROW_H),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.set_min_width(width);
            if ui
                .add(
                    egui::TextEdit::singleline(&mut draft.topic)
                        .desired_width((width - 6.0).max(40.0))
                        .hint_text("topic"),
                )
                .changed()
            {
                *dirty = true;
            }
        },
    );
}

fn draw_field_cell(ui: &mut egui::Ui, draft: &mut SimRow, dirty: &mut bool, width: f32) {
    ui.allocate_ui_with_layout(
        Vec2::new(width, ROW_H),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.set_min_width(width);
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
                *dirty = true;
            }
            if use_field {
                if ui
                    .add(
                        egui::TextEdit::singleline(&mut draft.field_path)
                            .desired_width((width - 28.0).max(32.0))
                            .hint_text("path"),
                    )
                    .on_hover_text("JSON dot path to patch (e.g. sensors.temp)")
                    .changed()
                {
                    *dirty = true;
                }
            }
        },
    );
}

fn draw_refresh_cell(ui: &mut egui::Ui, draft: &mut SimRow, dirty: &mut bool) {
    ui.allocate_ui_with_layout(
        Vec2::new(COL_REFRESH, ROW_H),
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
                        .trailing_fill(true)
                        .clamping(egui::SliderClamping::Always),
                )
                .on_hover_text("Publish interval (fine at low ms, ~1s steps above 1s)")
                .changed()
            {
                let i = idx.round().clamp(0.0, max_i) as usize;
                draft.interval_ms = INTERVAL_TICKS_MS[i];
                *dirty = true;
            }
            ui.add_sized(
                [48.0, 16.0],
                egui::Label::new(
                    RichText::new(format_interval_ms(draft.interval_ms))
                        .small()
                        .monospace()
                        .color(CHROME_TEXT),
                ),
            );
        },
    );
}

fn draw_prec_cell(ui: &mut egui::Ui, draft: &mut SimRow, dirty: &mut bool) {
    ui.allocate_ui_with_layout(
        Vec2::new(COL_PREC, ROW_H),
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
                *dirty = true;
            }
        },
    );
}

fn draw_value_cell(
    ui: &mut egui::Ui,
    client: &mut SimClientState,
    ids: &[String],
    id: &str,
    st: &Option<sim_client::SimStatus>,
    running: bool,
    selected: bool,
    mods: egui::Modifiers,
    width: f32,
) {
    let val = sim_client::value_label(st.as_ref());
    // Exact clipped width — long ISO strings ellipsize inside; never expand Gen/trash.
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, ROW_H), egui::Sense::click());
    if selected {
        ui.painter().rect_filled(
            rect.shrink2(Vec2::new(0.0, 1.0)),
            2.0,
            Color32::from_rgb(40, 64, 92),
        );
    }
    let color = if running {
        Color32::from_rgb(140, 210, 160)
    } else {
        CHROME_TEXT
    };
    ui.allocate_new_ui(egui::UiBuilder::new().max_rect(rect), |ui| {
        ui.set_clip_rect(rect);
        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
            let resp = ui.add(
                egui::Label::new(RichText::new(&val).monospace().color(color))
                    .truncate()
                    .sense(egui::Sense::click()),
            );
            if resp.clicked() {
                apply_row_click(client, ids, id, mods);
            }
            resp.on_hover_text(&val);
        });
    });
    // Click on empty padding in the value cell still selects.
    let resp = ui.interact(rect, ui.id().with(("sim_val", id)), egui::Sense::click());
    if resp.clicked() {
        apply_row_click(client, ids, id, mods);
    }
    resp.on_hover_text(&val);
}

fn draw_gen_cell(
    ui: &mut egui::Ui,
    draft: &mut SimRow,
    dirty: &mut bool,
    gen_type: &str,
    row: &SimRow,
) {
    ui.allocate_ui_with_layout(
        Vec2::new(COL_GEN, ROW_H),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.set_min_width(COL_GEN);
            let mut gt = gen_type.to_string();
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
                            *dirty = true;
                        }
                    }
                });
        },
    );
}

fn draw_settings_cell(
    ui: &mut egui::Ui,
    row_id: &str,
    gen_type: &str,
    draft: &mut SimRow,
    client: &mut SimClientState,
    dirty: &mut bool,
) {
    // Exact clipped band — Constant ±% (and friends) cannot grow past COL_SETTINGS.
    let (rect, _) = ui.allocate_exact_size(Vec2::new(COL_SETTINGS, ROW_H), egui::Sense::hover());
    ui.allocate_new_ui(egui::UiBuilder::new().max_rect(rect), |ui| {
        ui.set_clip_rect(rect);
        // Parent may be right_to_left (trash pin) — force LTR so slots stay A→D.
        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = SET_GAP;
            if edit_generator_inline(ui, row_id, gen_type, &mut draft.generator, client) {
                *dirty = true;
            }
        });
    });
}

/// Pin trash to the far right of the remaining row; Settings is a fixed clipped band beside it.
fn pin_settings_and_trash(
    ui: &mut egui::Ui,
    icons: &IconSet,
    client: &mut SimClientState,
    id: &str,
    status: &mut String,
    settings: Option<(&str, &str, &mut SimRow, &mut bool)>,
) {
    let rem = ui
        .available_width()
        .max(COL_SETTINGS + COL_DELETE + COL_GAP);
    // Exact rect so a fat Constant slider cannot expand past the row's right edge.
    let (rect, _) = ui.allocate_exact_size(Vec2::new(rem, ROW_H), egui::Sense::hover());
    ui.allocate_new_ui(egui::UiBuilder::new().max_rect(rect), |ui| {
        ui.set_clip_rect(rect);
        ui.spacing_mut().item_spacing.x = COL_GAP;
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            draw_delete_cell(ui, icons, client, id, status);
            if let Some((row_id, gen_type, draft, dirty)) = settings {
                draw_settings_cell(ui, row_id, gen_type, draft, client, dirty);
            }
        });
    });
}

/// One Settings column slot — exact clipped width so values share one vertical column.
fn settings_slot(ui: &mut egui::Ui, width: f32, add_contents: impl FnOnce(&mut egui::Ui)) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, ROW_H), egui::Sense::hover());
    ui.allocate_new_ui(egui::UiBuilder::new().max_rect(rect), |ui| {
        ui.set_clip_rect(rect);
        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 3.0;
            add_contents(ui);
        });
    });
}

/// Muted Settings label with fixed width so DragValues share a vertical column.
fn settings_label(ui: &mut egui::Ui, text: &str) {
    ui.add_sized(
        [SET_LABEL_W, ROW_H],
        egui::Label::new(RichText::new(text).small().color(CHROME_MUTED)),
    );
}

fn draw_delete_cell(
    ui: &mut egui::Ui,
    icons: &IconSet,
    client: &mut SimClientState,
    id: &str,
    status: &mut String,
) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(COL_DELETE, ROW_H), egui::Sense::hover());
    ui.allocate_new_ui(egui::UiBuilder::new().max_rect(rect), |ui| {
        if icons
            .tiny(ui, "delete_red", false, "Delete this simulation")
            .clicked()
        {
            match client.delete(id) {
                Ok(()) => *status = format!("Deleted {id}"),
                Err(e) => *status = e,
            }
        }
    });
}

/// Inline generator params in fixed Settings slots A–D; returns true if changed.
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
            settings_slot(ui, SET_A, |ui| {
                settings_label(ui, "val");
                if ui
                    .add(egui::TextEdit::singleline(&mut val).desired_width(44.0))
                    .changed()
                {
                    dirty = true;
                }
            });
            // ±% slider capped to B+C — never expands Settings / trash.
            settings_slot(ui, SET_B + SET_GAP + SET_C, |ui| {
                settings_label(ui, "±%");
                let sw = (ui.available_width() - 2.0).max(36.0);
                if ui
                    .add_sized(
                        [sw, ROW_H],
                        egui::Slider::new(&mut noise, 0.0..=50.0)
                            .show_value(false)
                            .trailing_fill(true)
                            .clamping(egui::SliderClamping::Always),
                    )
                    .on_hover_text(format!("Noise ±{noise:.0}%"))
                    .changed()
                {
                    dirty = true;
                }
            });
            settings_slot(ui, SET_D, |ui| {
                ui.label(
                    RichText::new(format!("{noise:.0}%"))
                        .small()
                        .monospace()
                        .color(CHROME_TEXT),
                );
            });
            if dirty {
                let value = serde_json::from_str::<Value>(&val).unwrap_or_else(|_| {
                    generator
                        .get("value")
                        .cloned()
                        .unwrap_or(Value::String(val))
                });
                *generator =
                    json!({"type": "constant", "value": value, "noise_pct": noise as f64});
            }
        }
        "counter" => {
            let mut start = f64_field(generator, "start", 0.0);
            let mut step = f64_field(generator, "step", 1.0);
            settings_slot(ui, SET_A, |ui| {
                settings_label(ui, "start");
                if ui.add(egui::DragValue::new(&mut start).speed(0.1)).changed() {
                    dirty = true;
                }
            });
            settings_slot(ui, SET_B, |ui| {
                settings_label(ui, "step");
                if ui.add(egui::DragValue::new(&mut step).speed(0.1)).changed() {
                    dirty = true;
                }
            });
            settings_slot(ui, SET_C, |_| {});
            settings_slot(ui, SET_D, |_| {});
            if dirty {
                *generator = json!({"type": "counter", "start": start, "step": step});
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
            dirty |= range_slots(ui, &mut min, &mut max);
            settings_slot(ui, SET_C, |ui| {
                settings_label(ui, "step");
                if ui.add(egui::DragValue::new(&mut step).speed(0.01)).changed() {
                    dirty = true;
                }
            });
            settings_slot(ui, SET_D, |_| {});
            if dirty {
                *generator = json!({"type": "random_walk", "min": min, "max": max, "step": step});
            }
        }
        "bool_toggle" => {
            let mut start = generator
                .get("start")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            settings_slot(ui, SET_A, |ui| {
                if ui.checkbox(&mut start, "start").changed() {
                    dirty = true;
                }
            });
            settings_slot(ui, SET_B, |_| {});
            settings_slot(ui, SET_C, |_| {});
            settings_slot(ui, SET_D, |_| {});
            if dirty {
                *generator = json!({"type": "bool_toggle", "start": start});
            }
        }
        "iso_timestamp" => {
            settings_slot(ui, SET_A, |ui| {
                ui.label(RichText::new("UTC").small().color(CHROME_MUTED));
            });
            settings_slot(ui, SET_B, |_| {});
            settings_slot(ui, SET_C, |_| {});
            settings_slot(ui, SET_D, |_| {});
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
            // Span A–D so the values field uses the full aligned Settings band.
            settings_slot(ui, COL_SETTINGS, |ui| {
                if ui
                    .add(
                        egui::TextEdit::singleline(&mut raw)
                            .desired_width((COL_SETTINGS - 8.0).max(80.0))
                            .hint_text("a,b,c"),
                    )
                    .changed()
                {
                    dirty = true;
                }
            });
            if dirty {
                let values: Vec<Value> = raw
                    .split(',')
                    .map(|s| s.trim())
                    .filter(|s| !s.is_empty())
                    .map(|s| {
                        serde_json::from_str::<Value>(s).unwrap_or(Value::String(s.to_string()))
                    })
                    .collect();
                *generator = json!({"type": "enum_cycle", "values": values});
            }
        }
        "formula" | "expr" => {
            let mut formula = generator
                .get("formula")
                .and_then(|v| v.as_str())
                .unwrap_or(r#"topic("lab/temp_c") * 1.8 + 32"#)
                .to_string();
            let mut resp_rect = egui::Rect::NOTHING;
            settings_slot(ui, COL_SETTINGS, |ui| {
                let resp = ui.add(
                    egui::TextEdit::singleline(&mut formula)
                        .desired_width((COL_SETTINGS - 8.0).max(80.0))
                        .hint_text(r#"topic("a/b") * 1.8 + 32"#),
                );
                resp_rect = resp.rect;
                if resp.changed() {
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
            });
            let _ = resp_rect;
            if dirty {
                *generator = json!({"type": "formula", "formula": formula});
            }
        }
        _ => {
            settings_slot(ui, SET_A, |_| {});
            settings_slot(ui, SET_B, |_| {});
            settings_slot(ui, SET_C, |_| {});
            settings_slot(ui, SET_D, |_| {});
        }
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
    let _ = row_id;
    let (mut min, mut max) = resolved_min_max(generator);
    let mut period = f64_field(generator, "period_s", 10.0);
    let mut dirty = range_slots(ui, &mut min, &mut max);
    if with_period {
        settings_slot(ui, SET_C, |ui| {
            settings_label(ui, "T");
            if ui
                .add(egui::DragValue::new(&mut period).range(0.1..=3600.0).speed(0.1))
                .changed()
            {
                dirty = true;
            }
        });
    } else {
        settings_slot(ui, SET_C, |_| {});
    }
    settings_slot(ui, SET_D, |_| {});
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
    let _ = row_id;
    let (mut min, mut max) = resolved_min_max(generator);
    // Support legacy start/end already mapped by resolved_min_max.
    let mut duration = f64_field(generator, "duration_s", 10.0);
    let mut loop_ = generator
        .get("loop")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    let mut dirty = range_slots(ui, &mut min, &mut max);
    settings_slot(ui, SET_C, |ui| {
        settings_label(ui, "dur");
        if ui
            .add(egui::DragValue::new(&mut duration).range(0.1..=3600.0).speed(0.1))
            .changed()
        {
            dirty = true;
        }
    });
    settings_slot(ui, SET_D, |ui| {
        if ui.checkbox(&mut loop_, "loop").changed() {
            dirty = true;
        }
    });
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
    let _ = row_id;
    let (mut min, mut max) = resolved_min_max(generator);
    let dirty = range_slots(ui, &mut min, &mut max);
    settings_slot(ui, SET_C, |_| {});
    settings_slot(ui, SET_D, |_| {});
    if dirty {
        *generator = json!({"type": gen_type, "min": min, "max": max});
    }
    dirty
}

/// Settings slots A+B for min/max — same x for every generator that uses a range.
fn range_slots(ui: &mut egui::Ui, min: &mut f64, max: &mut f64) -> bool {
    let mut dirty = false;
    settings_slot(ui, SET_A, |ui| {
        settings_label(ui, "min");
        if ui.add(egui::DragValue::new(min).speed(0.1)).changed() {
            dirty = true;
        }
    });
    settings_slot(ui, SET_B, |ui| {
        settings_label(ui, "max");
        if ui.add(egui::DragValue::new(max).speed(0.1)).changed() {
            dirty = true;
        }
    });
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
    icons: &IconSet,
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
                    ui.set_min_width(380.0);
                    ui.set_max_width(420.0);

                    // Same section chrome as Export topic tree / Settings panels.
                    icons::section_header_bar(ui, |ui| {
                        icons::section_icon(ui, icons, "playback");
                        icons::section_title(ui, "Open playback");
                    });
                    ui.add_space(6.0);
                    let bar_w = ui.available_width();
                    let (bar, _) =
                        ui.allocate_exact_size(egui::vec2(bar_w, 3.0), egui::Sense::hover());
                    ui.painter().rect_filled(bar, 1.5, CHROME_ACCENT_BORDER);
                    ui.add_space(12.0);

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
                    ui.separator();
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        ui.with_layout(
                            egui::Layout::right_to_left(egui::Align::Center),
                            |ui| {
                                ui.spacing_mut().item_spacing.x = 8.0;
                                let can = draft.play || draft.suggest;
                                // OK — accent fill + icon (mirrors Export / Settings Save).
                                let ok_btn = if let Some(tex) = icons.get("playback") {
                                    ui.add_enabled(
                                        can,
                                        egui::Button::image_and_text(
                                            egui::Image::new(tex).fit_to_exact_size(
                                                egui::vec2(TINY_ICON, TINY_ICON),
                                            ),
                                            RichText::new("OK").strong(),
                                        )
                                        .fill(CHROME_ACCENT_SOFT)
                                        .stroke(egui::Stroke::new(
                                            1.0_f32,
                                            CHROME_ACCENT_BORDER,
                                        ))
                                        .min_size(egui::vec2(120.0, 32.0)),
                                    )
                                } else {
                                    ui.add_enabled(
                                        can,
                                        egui::Button::new(RichText::new("OK").strong())
                                            .fill(CHROME_ACCENT_SOFT)
                                            .stroke(egui::Stroke::new(
                                                1.0_f32,
                                                CHROME_ACCENT_BORDER,
                                            ))
                                            .min_size(egui::vec2(120.0, 32.0)),
                                    )
                                };
                                if ok_btn.clicked() {
                                    confirm = true;
                                }
                                let cancel_btn = if let Some(tex) = icons.get("close_red") {
                                    ui.add(
                                        egui::Button::image_and_text(
                                            egui::Image::new(tex).fit_to_exact_size(
                                                egui::vec2(TINY_ICON, TINY_ICON),
                                            ),
                                            "Cancel",
                                        )
                                        .min_size(egui::vec2(104.0, 32.0)),
                                    )
                                } else {
                                    ui.add(
                                        egui::Button::new("Cancel")
                                            .min_size(egui::vec2(104.0, 32.0)),
                                    )
                                };
                                if cancel_btn.clicked() {
                                    close = true;
                                }
                            },
                        );
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

/// Apply import after path (+ optional Replace/Merge) without blocking the UI thread.
fn finish_sim_import(
    ctx: &egui::Context,
    client: &mut SimClientState,
    status: &mut String,
    path: &std::path::Path,
    mode: &str,
    err_dlg: egui::Id,
) {
    let fail = |ctx: &egui::Context, status: &mut String, msg: String, err_dlg: egui::Id| {
        *status = msg.clone();
        crate::ui::native_dialog::begin_message(ctx, err_dlg, move |d| {
            d.set_level(rfd::MessageLevel::Error)
                .set_title("Import failed")
                .set_description(msg)
                .set_buttons(rfd::MessageButtons::Ok)
        });
    };
    match std::fs::read_to_string(path) {
        Ok(raw) => match serde_json::from_str::<serde_json::Value>(&raw) {
            Ok(v) => match client.import_sims_mode(&v, mode) {
                Ok(n) => *status = format!("Imported {n} simulations ({mode})"),
                Err(e) => fail(ctx, status, format!("Import failed: {e}"), err_dlg),
            },
            Err(e) => fail(ctx, status, format!("Import failed: parse JSON: {e}"), err_dlg),
        },
        Err(e) => fail(
            ctx,
            status,
            format!("Import failed: read {}: {e}", path.display()),
            err_dlg,
        ),
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
