//! Pro Simulation spreadsheet panel (floating or docked).
//!
//! Talks HTTP to `topicbench-pro` `/sim`. No formula engine in OSS.

use crate::mqtt::profile::ProfileStore;
use crate::mqtt::session::SessionManager;
use crate::pro::sim_client::{self, SimClientState, SimRow};
use crate::ui::icons::{
    self, IconSet, CHROME_ACCENT_BORDER, CHROME_ACCENT_SOFT, CHROME_BG, CHROME_CAPTION,
    CHROME_MUTED, CHROME_TEXT,
};
use eframe::egui::{self, Color32, RichText};
use serde_json::json;

const GEN_TYPES: &[&str] = &[
    "constant",
    "counter",
    "sine",
    "triangle",
    "ramp",
    "uniform",
    "gaussian",
    "random_walk",
    "bool_toggle",
    "iso_timestamp",
    "enum_cycle",
    "expr",
];

/// Draw floating Window and/or docked SidePanel for Simulation.
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
    if !*open {
        return;
    }

    if *floating {
        let mut win_open = true;
        egui::Window::new("Simulation")
            .id(egui::Id::new("topicbench_simulation_win"))
            .default_size([920.0, 360.0])
            .min_width(640.0)
            .min_height(220.0)
            .frame(egui::Frame::window(&ctx.style()).fill(CHROME_BG))
            .open(&mut win_open)
            .show(ctx, |ui| {
                header_bar(ui, icons, floating, open);
                ui.separator();
                body(ui, licensed, client, sessions, profiles, status);
            });
        if !win_open {
            *open = false;
        }
    } else {
        egui::SidePanel::right("topicbench_simulation_dock")
            .default_width(420.0)
            .min_width(320.0)
            .frame(egui::Frame::NONE.fill(CHROME_BG).inner_margin(8.0))
            .show(ctx, |ui| {
                header_bar(ui, icons, floating, open);
                ui.separator();
                body(ui, licensed, client, sessions, profiles, status);
            });
    }

    // Keep ~2 Hz refresh while open + licensed.
    if licensed {
        client.poll_if_due(false);
        ctx.request_repaint_after(std::time::Duration::from_millis(500));
    }
}

fn header_bar(
    ui: &mut egui::Ui,
    icons: &IconSet,
    floating: &mut bool,
    open: &mut bool,
) {
    icons::section_header_bar(ui, |ui| {
        icons::section_icon(ui, icons, "data_table");
        icons::section_title(ui, "Simulation");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if icons
                .tiny(ui, "remove_x", false, "Close Simulation panel")
                .clicked()
            {
                *open = false;
            }
            let dock_tip = if *floating {
                "Dock to right side"
            } else {
                "Float as window"
            };
            let dock_icon = if *floating {
                "panel_docked"
            } else {
                "panel_undock"
            };
            if icons.tiny(ui, dock_icon, false, dock_tip).clicked() {
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
    status: &mut String,
) {
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
            RichText::new("Start the sidecar: cargo run --release --bin topicbench-pro")
                .small()
                .color(CHROME_MUTED),
        );
    } else if !client.sidecar_up && client.rows.is_empty() {
        ui.label(
            RichText::new("Connecting to topicbench-pro…")
                .color(CHROME_MUTED),
        );
    }

    // Toolbar
    ui.horizontal(|ui| {
        if ui
            .add(
                egui::Button::new(RichText::new("Add").strong())
                    .fill(CHROME_ACCENT_SOFT)
                    .stroke(egui::Stroke::new(1.0_f32, CHROME_ACCENT_BORDER)),
            )
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
        let sel = client.selected.clone();
        let start_en = sel.is_some();
        if ui
            .add_enabled(start_en, egui::Button::new("Start"))
            .on_hover_text("Start selected simulation")
            .clicked()
        {
            if let Some(id) = &sel {
                match client.start(id) {
                    Ok(()) => *status = format!("Started {id}"),
                    Err(e) => *status = e,
                }
            }
        }
        if ui
            .add_enabled(start_en, egui::Button::new("Stop"))
            .on_hover_text("Stop selected simulation")
            .clicked()
        {
            if let Some(id) = &sel {
                match client.stop(id) {
                    Ok(()) => *status = format!("Stopped {id}"),
                    Err(e) => *status = e,
                }
            }
        }
        if ui
            .button("Stop all")
            .on_hover_text("Stop every running simulation")
            .clicked()
        {
            match client.stop_all() {
                Ok(()) => *status = "Stopped all simulations".into(),
                Err(e) => *status = e,
            }
        }
        if ui
            .add_enabled(start_en, egui::Button::new("Delete"))
            .on_hover_text("Delete selected simulation")
            .clicked()
        {
            if let Some(id) = &sel {
                match client.delete(id) {
                    Ok(()) => *status = format!("Deleted {id}"),
                    Err(e) => *status = e,
                }
            }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let hint = if client.sidecar_up {
                "sidecar up"
            } else {
                "sidecar down"
            };
            ui.label(RichText::new(hint).small().color(CHROME_CAPTION));
        });
    });

    ui.add_space(4.0);

    // Spreadsheet header
    egui::Grid::new("sim_grid_hdr")
        .num_columns(5)
        .spacing([8.0, 4.0])
        .min_col_width(80.0)
        .show(ui, |ui| {
            for h in ["Topic / profile", "Generator", "Settings", "Current value", "Run"] {
                ui.label(RichText::new(h).strong().color(CHROME_CAPTION));
            }
            ui.end_row();
        });

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let rows = client.rows.clone();
            for row in rows {
                draw_row(ui, &row, client, sessions, profiles, status);
            }
        });
}

fn draw_row(
    ui: &mut egui::Ui,
    row: &SimRow,
    client: &mut SimClientState,
    sessions: &SessionManager,
    profiles: &ProfileStore,
    status: &mut String,
) {
    let selected = client.selected.as_deref() == Some(row.id.as_str());
    let st = client.status_for(&row.id).cloned();
    let running = st.as_ref().map(|s| s.running).unwrap_or(false);
    let mut draft = row.clone();
    let mut dirty = false;

    let frame = if selected {
        egui::Frame::NONE
            .fill(CHROME_ACCENT_SOFT)
            .stroke(egui::Stroke::new(1.0_f32, CHROME_ACCENT_BORDER))
            .inner_margin(4.0)
            .corner_radius(3.0)
    } else {
        egui::Frame::NONE.inner_margin(4.0)
    };

    frame.show(ui, |ui| {
        egui::Grid::new(format!("sim_row_{}", row.id))
            .num_columns(5)
            .spacing([8.0, 4.0])
            .min_col_width(80.0)
            .show(ui, |ui| {
                // Topic + profile
                ui.vertical(|ui| {
                    if ui
                        .add(
                            egui::TextEdit::singleline(&mut draft.topic)
                                .desired_width(200.0)
                                .hint_text("topic"),
                        )
                        .changed()
                    {
                        dirty = true;
                    }
                    egui::ComboBox::from_id_salt(format!("sim_prof_{}", row.id))
                        .selected_text(profile_label(profiles, &draft.profile))
                        .width(200.0)
                        .show_ui(ui, |ui| {
                            for p in profiles.profiles.values() {
                                let connected = sessions
                                    .open
                                    .get(&p.id)
                                    .map(|s| {
                                        matches!(
                                            s.state,
                                            crate::mqtt::session::SessionState::Connected
                                        )
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
                });

                // Generator type
                let mut gen_type = sim_client::generator_label(&draft.generator);
                egui::ComboBox::from_id_salt(format!("sim_gen_{}", row.id))
                    .selected_text(&gen_type)
                    .width(110.0)
                    .show_ui(ui, |ui| {
                        for t in GEN_TYPES {
                            if ui.selectable_label(gen_type == *t, *t).clicked() {
                                gen_type = (*t).to_string();
                                draft.generator = default_generator(t);
                                dirty = true;
                            }
                        }
                    });

                // Settings: interval + mode (+ field path / expr)
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("ms").small().color(CHROME_MUTED));
                        let mut ms = draft.interval_ms as i32;
                        if ui
                            .add(egui::DragValue::new(&mut ms).range(50..=600_000))
                            .changed()
                        {
                            draft.interval_ms = ms as u64;
                            dirty = true;
                        }
                    });
                    let modes = [("whole_payload", "whole"), ("field_patch", "field")];
                    egui::ComboBox::from_id_salt(format!("sim_mode_{}", row.id))
                        .selected_text(if draft.mode == "field_patch" {
                            "field"
                        } else {
                            "whole"
                        })
                        .width(80.0)
                        .show_ui(ui, |ui| {
                            for (val, lab) in modes {
                                if ui.selectable_label(draft.mode == val, lab).clicked() {
                                    draft.mode = val.to_string();
                                    dirty = true;
                                }
                            }
                        });
                    if draft.mode == "field_patch" {
                        if ui
                            .add(
                                egui::TextEdit::singleline(&mut draft.field_path)
                                    .desired_width(140.0)
                                    .hint_text("field.path"),
                            )
                            .changed()
                        {
                            dirty = true;
                        }
                    }
                    if gen_type == "expr" {
                        let mut formula = draft
                            .generator
                            .get("formula")
                            .and_then(|v| v.as_str())
                            .unwrap_or("sin(t)")
                            .to_string();
                        if ui
                            .add(
                                egui::TextEdit::singleline(&mut formula)
                                    .desired_width(140.0)
                                    .hint_text("expr"),
                            )
                            .changed()
                        {
                            draft.generator = json!({"type": "expr", "formula": formula});
                            dirty = true;
                        }
                    }
                    if gen_type == "constant" {
                        let mut c = draft
                            .generator
                            .get("value")
                            .map(|v| match v {
                                serde_json::Value::String(s) => s.clone(),
                                other => other.to_string(),
                            })
                            .unwrap_or_else(|| "0".into());
                        if ui
                            .add(
                                egui::TextEdit::singleline(&mut c)
                                    .desired_width(80.0)
                                    .hint_text("value"),
                            )
                            .changed()
                        {
                            let value = serde_json::from_str::<serde_json::Value>(&c)
                                .unwrap_or(serde_json::Value::String(c));
                            draft.generator = json!({"type": "constant", "value": value});
                            dirty = true;
                        }
                    }
                });

                // Current value
                ui.label(
                    RichText::new(sim_client::value_label(st.as_ref()))
                        .monospace()
                        .color(if running {
                            Color32::from_rgb(140, 210, 160)
                        } else {
                            CHROME_TEXT
                        }),
                );

                // Running + select
                ui.vertical(|ui| {
                    let run_lbl = if running { "▶ run" } else { "■ stop" };
                    let run_col = if running {
                        Color32::from_rgb(140, 210, 160)
                    } else {
                        CHROME_MUTED
                    };
                    ui.label(RichText::new(run_lbl).color(run_col));
                    if ui.selectable_label(selected, "select").clicked() {
                        client.selected = Some(row.id.clone());
                    }
                });

                ui.end_row();
            });
    });

    if dirty {
        match client.upsert(&draft) {
            Ok(_) => {}
            Err(e) => *status = e,
        }
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

fn default_generator(t: &str) -> serde_json::Value {
    match t {
        "constant" => json!({"type": "constant", "value": 0}),
        "counter" => json!({"type": "counter", "start": 0, "step": 1}),
        "sine" => json!({"type": "sine", "amplitude": 1.0, "period_s": 10.0, "offset": 0.0}),
        "triangle" => {
            json!({"type": "triangle", "amplitude": 1.0, "period_s": 10.0, "offset": 0.0})
        }
        "ramp" => {
            json!({"type": "ramp", "start": 0.0, "end": 1.0, "duration_s": 10.0, "loop": true})
        }
        "uniform" => json!({"type": "uniform", "min": 0.0, "max": 1.0}),
        "gaussian" => json!({"type": "gaussian", "mean": 0.0, "std_dev": 1.0}),
        "random_walk" => json!({"type": "random_walk", "start": 0.0, "step": 0.1}),
        "bool_toggle" => json!({"type": "bool_toggle", "start": false}),
        "iso_timestamp" => json!({"type": "iso_timestamp"}),
        "enum_cycle" => json!({"type": "enum_cycle", "values": ["a", "b", "c"]}),
        "expr" => json!({"type": "expr", "formula": "sin(t)"}),
        _ => json!({"type": "sine", "amplitude": 1.0, "period_s": 10.0, "offset": 0.0}),
    }
}
