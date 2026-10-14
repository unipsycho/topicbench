//! Sticky strip — Raw / Value / Graph; presentation on the right.
//! Only stickies for currently **connected** profiles are shown.

use crate::mqtt::pins::{DetectedValue, PinBoard, StickyTopic, StickyViewMode};
use crate::mqtt::profile::ProfileStore;
use crate::mqtt::session::{SessionManager, SessionState};
use crate::ui::clipboard;
use crate::ui::flash;
use crate::ui::json_view;
use crate::ui::search_bar::SearchFilter;
use eframe::egui::{self, Color32, Pos2, RichText, Stroke};
use std::collections::HashSet;
use std::time::Instant;

pub fn draw(
    ui: &mut egui::Ui,
    pins: &mut PinBoard,
    profiles: &mut ProfileStore,
    sessions: &SessionManager,
    filter: &SearchFilter,
    status: &mut String,
) {
    ui.heading("Sticky topics");
    ui.label("Value/graph on the right — Raw · Value · Graph. Shown for connected profiles only.");
    ui.add_space(4.0);

    let connected: HashSet<&str> = sessions
        .open
        .iter()
        .filter(|(_, s)| s.state == SessionState::Connected)
        .map(|(id, _)| id.as_str())
        .collect();

    let keys: Vec<String> = pins
        .items
        .iter()
        .filter(|(_, p)| connected.contains(p.profile_id.as_str()))
        .map(|(k, _)| k.clone())
        .collect();

    if keys.is_empty() {
        ui.weak("No stickies for connected profiles — mark a topic sticky from the tree.");
        return;
    }

    let now = Instant::now();
    let mut any = false;
    for key in keys {
        let Some(pin) = pins.items.get(&key).cloned() else {
            continue;
        };
        if !filter.matches(&pin.topic, Some(&pin.last_payload)) {
            continue;
        }
        any = true;
        let fill = flash::flash_fill(pin.flash_until, now).unwrap_or(Color32::TRANSPARENT);
        egui::Frame::NONE
            .fill(fill)
            .stroke(Stroke::new(
                1.0_f32,
                ui.visuals().widgets.noninteractive.bg_stroke.color,
            ))
            .inner_margin(egui::Margin::same(6))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    // Left: identity + mode controls
                    ui.vertical(|ui| {
                        ui.set_min_width(200.0);
                        ui.horizontal(|ui| {
                            ui.strong(&pin.profile_name);
                            clipboard::topic_label(ui, &pin.topic, status);
                            if pin.message_count > 0 {
                                ui.label(RichText::new(format!("#{}", pin.message_count)).small());
                            }
                        });
                        ui.horizontal(|ui| {
                            if let Some(live) = pins.items.get_mut(&key) {
                                egui::ComboBox::from_id_salt(format!("sticky-view-{key}"))
                                    .selected_text(live.view_mode.label())
                                    .width(90.0)
                                    .show_ui(ui, |ui| {
                                        ui.selectable_value(
                                            &mut live.view_mode,
                                            StickyViewMode::Raw,
                                            "Raw",
                                        );
                                        ui.selectable_value(
                                            &mut live.view_mode,
                                            StickyViewMode::Value,
                                            "Value",
                                        );
                                        ui.selectable_value(
                                            &mut live.view_mode,
                                            StickyViewMode::Graph,
                                            "Graph",
                                        );
                                    });
                            }
                            if ui.small_button("Unstick").clicked() {
                                pins.unpin(&pin.profile_id, &pin.topic);
                                profiles.set_sticky_and_save(&pin.profile_id, &pin.topic, false);
                            }
                        });
                    });

                    ui.separator();

                    // Presentation pane — always left-aligned (never right_to_left).
                    let mode = pins
                        .items
                        .get(&key)
                        .map(|p| p.view_mode)
                        .unwrap_or(StickyViewMode::Raw);
                    let is_json = matches!(mode, StickyViewMode::Value)
                        && pins
                            .items
                            .get(&key)
                            .is_some_and(|p| json_view::try_pretty_json(&p.last_payload).is_some());
                    let height = if is_json { 240.0 } else { 72.0 };
                    ui.allocate_ui_with_layout(
                        egui::vec2(ui.available_width().max(180.0), height),
                        egui::Layout::top_down(egui::Align::LEFT),
                        |ui| {
                            if let Some(p) = pins.items.get(&key) {
                                match mode {
                                    StickyViewMode::Raw => draw_raw(ui, p, status),
                                    StickyViewMode::Value => draw_value(ui, p, &key, status),
                                    StickyViewMode::Graph => draw_graph(ui, p),
                                }
                            }
                        },
                    );
                });
            });
        ui.add_space(4.0);
    }
    if !any {
        ui.weak("No stickies match the current search.");
    }
}

fn draw_raw(ui: &mut egui::Ui, pin: &StickyTopic, status: &mut String) {
    ui.label(RichText::new("Raw").small().weak());
    clipboard::payload_with_copy(ui, &pin.last_payload, 96, status);
}

fn draw_value(ui: &mut egui::Ui, pin: &StickyTopic, sticky_key: &str, status: &mut String) {
    // Always beautify JSON objects/arrays in Value mode (no settings toggle yet).
    if let Some(pretty) = json_view::try_pretty_json(&pin.last_payload) {
        json_view::draw_pretty(ui, &pretty, ("sticky-json", sticky_key));
        ui.push_id(("copy-json", sticky_key), |ui| {
            if ui.small_button("Copy JSON").clicked() {
                ui.ctx().copy_text(pretty.clone());
                *status = "Copied pretty JSON".into();
            }
        });
        return;
    }

    match DetectedValue::from_payload(&pin.last_payload) {
        DetectedValue::Bool(on) => {
            ui.label(RichText::new("Boolean").small().weak());
            let label = if on { "ON" } else { "OFF" };
            let color = if on {
                Color32::from_rgb(70, 170, 100)
            } else {
                Color32::from_rgb(140, 140, 140)
            };
            ui.label(RichText::new(label).heading().color(color).strong());
        }
        DetectedValue::Number(n) => {
            ui.label(RichText::new("Number").small().weak());
            ui.label(RichText::new(format!("{n}")).heading().strong());
            draw_gauge(ui, n);
        }
        DetectedValue::Text(s) => {
            ui.label(RichText::new("String").small().weak());
            clipboard::payload_with_copy(ui, &s, 64, status);
        }
    }
}

fn draw_gauge(ui: &mut egui::Ui, value: f64) {
    // Simple meter: map value into 0..100-ish for display width; show bar relative to |value|.
    let (rect, _) = ui.allocate_exact_size(egui::vec2(160.0, 14.0), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 3.0, ui.visuals().extreme_bg_color);
    let frac = ((value.abs() % 100.0) / 100.0).clamp(0.05, 1.0) as f32;
    let mut fill = rect;
    fill.set_width(rect.width() * frac);
    painter.rect_filled(fill, 3.0, Color32::from_rgb(80, 160, 200));
}

fn draw_graph(ui: &mut egui::Ui, pin: &StickyTopic) {
    let samples = &pin.history;
    if samples.is_empty() {
        ui.weak("No history yet.");
        return;
    }

    let numeric: Vec<(f32, f64)> = samples
        .iter()
        .enumerate()
        .filter_map(|(i, s)| s.numeric.map(|v| (i as f32, v)))
        .collect();

    let (rect, _) = ui.allocate_exact_size(egui::vec2(200.0, 64.0), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 2.0, ui.visuals().extreme_bg_color);

    if !numeric.is_empty() {
        let min_v = numeric.iter().map(|(_, v)| *v).fold(f64::INFINITY, f64::min);
        let max_v = numeric
            .iter()
            .map(|(_, v)| *v)
            .fold(f64::NEG_INFINITY, f64::max);
        let span = (max_v - min_v).abs().max(1e-9);
        let n = samples.len().max(1) as f32;
        let mut points = Vec::with_capacity(numeric.len());
        for (i, v) in &numeric {
            let x = rect.left() + (*i / (n - 1.0).max(1.0)) * rect.width();
            let y = rect.bottom() - (((*v - min_v) / span) as f32) * rect.height();
            points.push(Pos2::new(x, y));
        }
        if points.len() >= 2 {
            painter.add(egui::Shape::line(
                points,
                Stroke::new(1.5_f32, Color32::from_rgb(80, 180, 120)),
            ));
        } else if let Some(p) = points.first() {
            painter.circle_filled(*p, 3.0, Color32::from_rgb(80, 180, 120));
        }
        ui.label(
            RichText::new(format!("min={min_v:.3} max={max_v:.3}"))
                .small()
                .weak(),
        );
    } else {
        let n = samples.len().max(1) as f32;
        for (i, _) in samples.iter().enumerate() {
            let x = rect.left() + (i as f32 / (n - 1.0).max(1.0)) * rect.width();
            let y = rect.center().y;
            painter.circle_filled(Pos2::new(x, y), 3.0, Color32::from_rgb(100, 160, 220));
        }
        ui.label(
            RichText::new(format!("{} arrivals", samples.len()))
                .small()
                .weak(),
        );
    }
}
