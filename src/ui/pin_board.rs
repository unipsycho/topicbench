//! Shared sticky strip — Normal / Graph / Historical views + clipboard.

use crate::mqtt::pins::{PinBoard, StickyTopic, StickyViewMode};
use crate::ui::clipboard;
use crate::ui::flash;
use crate::ui::search_bar::SearchFilter;
use eframe::egui::{self, Color32, Pos2, RichText, Stroke};
use std::time::Instant;

pub fn draw(ui: &mut egui::Ui, pins: &mut PinBoard, filter: &SearchFilter, status: &mut String) {
    ui.heading("Sticky topics");
    ui.label("Marked topics stay here with live data while you browse the tree.");
    ui.add_space(4.0);

    if pins.items.is_empty() {
        ui.weak("No stickies yet — mark a topic sticky from the tree or Sticky.");
        return;
    }

    let now = Instant::now();
    let keys: Vec<String> = pins.items.keys().cloned().collect();
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
                    ui.strong(&pin.profile_name);
                    clipboard::topic_label(ui, &pin.topic, status);
                    if pin.message_count > 0 {
                        ui.label(RichText::new(format!("#{}", pin.message_count)).small());
                    }
                    // Per-sticky view mode
                    if let Some(live) = pins.items.get_mut(&key) {
                        egui::ComboBox::from_id_salt(format!("sticky-view-{key}"))
                            .selected_text(live.view_mode.label())
                            .width(100.0)
                            .show_ui(ui, |ui| {
                                ui.selectable_value(
                                    &mut live.view_mode,
                                    StickyViewMode::Normal,
                                    "Normal",
                                );
                                ui.selectable_value(
                                    &mut live.view_mode,
                                    StickyViewMode::Graph,
                                    "Graph",
                                );
                                ui.selectable_value(
                                    &mut live.view_mode,
                                    StickyViewMode::Historical,
                                    "Historical",
                                );
                            });
                    }
                    if ui.small_button("Unstick").clicked() {
                        pins.unpin(&pin.profile_id, &pin.topic);
                    }
                });

                let mode = pins
                    .items
                    .get(&key)
                    .map(|p| p.view_mode)
                    .unwrap_or(StickyViewMode::Normal);
                let pin_ref = pins.items.get(&key);
                match (mode, pin_ref) {
                    (StickyViewMode::Normal, Some(p)) => {
                        ui.horizontal(|ui| {
                            clipboard::payload_with_copy(ui, &p.last_payload, 80, status);
                        });
                    }
                    (StickyViewMode::Graph, Some(p)) => draw_graph(ui, p, false),
                    (StickyViewMode::Historical, Some(p)) => draw_graph(ui, p, true),
                    _ => {}
                }
            });
        ui.add_space(4.0);
    }
    if !any {
        ui.weak("No stickies match the current search.");
    }
}

/// Graph = live sparkline / arrival dots; Historical = same chart emphasizing recent history window.
fn draw_graph(ui: &mut egui::Ui, pin: &StickyTopic, historical: bool) {
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

    let height = if historical { 72.0 } else { 56.0 };
    let (rect, _resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), height), egui::Sense::hover());
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
        ui.weak(format!(
            "{}  min={min_v:.3} max={max_v:.3} ({} pts)",
            if historical { "History" } else { "Live" },
            numeric.len()
        ));
    } else {
        // String payloads: arrival dots on a timeline
        let n = samples.len().max(1) as f32;
        for (i, _) in samples.iter().enumerate() {
            let x = rect.left() + (i as f32 / (n - 1.0).max(1.0)) * rect.width();
            let y = rect.center().y;
            painter.circle_filled(Pos2::new(x, y), 3.0, Color32::from_rgb(100, 160, 220));
        }
        ui.weak(format!(
            "{} arrivals (non-numeric) — {} marks",
            if historical { "History" } else { "Timeline" },
            samples.len()
        ));
    }
}
