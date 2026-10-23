//! Pro Simulation spreadsheet panel (floating or docked).
//!
//! One horizontal line per row; Start/Stop apply to **all** sims.
//! Talks HTTP to `topicbench-pro` `/sim` — no formula engine in OSS.

use crate::mqtt::profile::ProfileStore;
use crate::mqtt::session::SessionManager;
use crate::pro::sim_client::{self, SimClientState, SimRow};
use crate::ui::icons::{
    self, IconSet, CHROME_ACCENT_BORDER, CHROME_ACCENT_SOFT, CHROME_BG, CHROME_CAPTION,
    CHROME_MUTED, CHROME_TEXT,
};
use eframe::egui::{self, Color32, RichText, Vec2};
use serde_json::{json, Value};

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
    "formula",
];

// Spreadsheet column widths (keep rows aligned).
const COL_TOPIC: f32 = 220.0;
const COL_GEN: f32 = 110.0;
const COL_SETTINGS: f32 = 520.0;
const COL_VALUE: f32 = 120.0;

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
            .default_size([980.0, 360.0])
            .min_width(780.0)
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
            .default_width(520.0)
            .min_width(420.0)
            .frame(egui::Frame::NONE.fill(CHROME_BG).inner_margin(8.0))
            .show(ctx, |ui| {
                header_bar(ui, icons, floating, open);
                ui.separator();
                body(ui, licensed, client, sessions, profiles, status);
            });
    }

    if licensed {
        client.poll_if_due(false);
        ctx.request_repaint_after(std::time::Duration::from_millis(500));
    }
}

fn header_bar(ui: &mut egui::Ui, icons: &IconSet, floating: &mut bool, open: &mut bool) {
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
            RichText::new("Connecting to topicbench-pro…").color(CHROME_MUTED),
        );
    }

    // Toolbar — Start/Stop affect every simulation.
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
        if ui
            .button("Start")
            .on_hover_text("Start all simulations")
            .clicked()
        {
            match client.start_all() {
                Ok(()) => *status = "Started all simulations".into(),
                Err(e) => *status = e,
            }
        }
        if ui
            .button("Stop")
            .on_hover_text("Stop all simulations")
            .clicked()
        {
            match client.stop_all() {
                Ok(()) => *status = "Stopped all simulations".into(),
                Err(e) => *status = e,
            }
        }
        let sel = client.selected.clone();
        if ui
            .add_enabled(sel.is_some(), egui::Button::new("Delete"))
            .on_hover_text("Delete selected row")
            .clicked()
        {
            if let Some(id) = &sel {
                match client.delete(id) {
                    Ok(()) => *status = format!("Deleted {id}"),
                    Err(e) => *status = e,
                }
            }
        }
    });

    ui.add_space(4.0);

    egui::ScrollArea::both()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            egui::Grid::new("sim_sheet")
                .num_columns(4)
                .spacing([10.0, 6.0])
                .min_col_width(40.0)
                .striped(true)
                .show(ui, |ui| {
                    // Header
                    header_cell(ui, COL_TOPIC, "Topic / profile");
                    header_cell(ui, COL_GEN, "Generator");
                    header_cell(ui, COL_SETTINGS, "Settings");
                    header_cell(ui, COL_VALUE, "Current value");
                    ui.end_row();

                    let rows = client.rows.clone();
                    for row in rows {
                        draw_row(ui, &row, client, sessions, profiles, status);
                        ui.end_row();
                    }
                });
        });
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
    status: &mut String,
) {
    let selected = client.selected.as_deref() == Some(row.id.as_str());
    let st = client.status_for(&row.id).cloned();
    let running = st.as_ref().map(|s| s.running).unwrap_or(false);
    let mut draft = row.clone();
    let mut dirty = false;
    let gen_type = sim_client::generator_label(&draft.generator);

    // Topic / profile — single line
    ui.allocate_ui_with_layout(
        Vec2::new(COL_TOPIC, 28.0),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.set_min_width(COL_TOPIC);
            ui.spacing_mut().item_spacing.x = 4.0;
            if ui
                .add(
                    egui::TextEdit::singleline(&mut draft.topic)
                        .desired_width(118.0)
                        .hint_text("topic"),
                )
                .changed()
            {
                dirty = true;
            }
            egui::ComboBox::from_id_salt(format!("sim_prof_{}", row.id))
                .selected_text(profile_label(profiles, &draft.profile))
                .width(90.0)
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

    // Generator type
    ui.allocate_ui_with_layout(
        Vec2::new(COL_GEN, 28.0),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.set_min_width(COL_GEN);
            let mut gt = gen_type.clone();
            egui::ComboBox::from_id_salt(format!("sim_gen_{}", row.id))
                .selected_text(&gt)
                .width(COL_GEN - 8.0)
                .show_ui(ui, |ui| {
                    for t in GEN_TYPES {
                        if ui.selectable_label(gt == *t, *t).clicked() {
                            gt = (*t).to_string();
                            draft.generator = default_generator(t);
                            dirty = true;
                        }
                    }
                });
        },
    );

    // Settings — all inline
    ui.allocate_ui_with_layout(
        Vec2::new(COL_SETTINGS, 28.0),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.set_min_width(COL_SETTINGS);
            ui.spacing_mut().item_spacing.x = 6.0;
            ui.set_max_height(28.0);

            // Interval
            ui.label(RichText::new("ms").small().color(CHROME_MUTED));
            let mut ms = draft.interval_ms as f32;
            if ui
                .add(
                    egui::Slider::new(&mut ms, 50.0..=10_000.0)
                        .show_value(false)
                        .clamping(egui::SliderClamping::Always),
                )
                .on_hover_text(format!("{} ms", draft.interval_ms))
                .changed()
            {
                draft.interval_ms = ms.round() as u64;
                dirty = true;
            }
            let mut ms_i = draft.interval_ms as i32;
            if ui
                .add(egui::DragValue::new(&mut ms_i).range(50..=600_000).speed(10))
                .changed()
            {
                draft.interval_ms = ms_i as u64;
                dirty = true;
            }

            // Mode
            egui::ComboBox::from_id_salt(format!("sim_mode_{}", row.id))
                .selected_text(if draft.mode == "field_patch" {
                    "field"
                } else {
                    "whole"
                })
                .width(64.0)
                .show_ui(ui, |ui| {
                    for (val, lab) in [("whole_payload", "whole"), ("field_patch", "field")] {
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
                            .desired_width(72.0)
                            .hint_text("path"),
                    )
                    .changed()
                {
                    dirty = true;
                }
            }

            // Decimal precision for published numbers
            ui.label(RichText::new("prec").small().color(CHROME_MUTED));
            let mut prec = draft.precision as i32;
            if ui
                .add(
                    egui::DragValue::new(&mut prec)
                        .range(0..=12)
                        .speed(0.2)
                        .prefix(""),
                )
                .on_hover_text("Digits after decimal when publishing numbers")
                .changed()
            {
                draft.precision = prec.clamp(0, 12) as u8;
                dirty = true;
            }

            // Generator-specific inline controls
            if edit_generator_inline(ui, &row.id, &gen_type, &mut draft.generator, client) {
                dirty = true;
            }
        },
    );

    // Current value — click selects row for Delete
    let val = sim_client::value_label(st.as_ref());
    ui.allocate_ui_with_layout(
        Vec2::new(COL_VALUE, 28.0),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.set_min_width(COL_VALUE);
            let resp = ui.selectable_label(
                selected,
                RichText::new(truncate(&val, 14))
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

    if dirty {
        match client.upsert(&draft) {
            Ok(_) => {}
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
