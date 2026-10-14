//! Sticky strip — Raw / Value / Graph; rows or column grid from Settings.
//! Only stickies for currently **connected** profiles are shown.

use crate::mqtt::pins::{DetectedValue, PinBoard, StickyTopic, StickyViewMode};
use crate::mqtt::profile::ProfileStore;
use crate::mqtt::session::{SessionManager, SessionState};
use crate::mqtt::settings::{AppSettings, StickyLayoutMode};
use crate::ui::flash;
use crate::ui::icons::IconSet;
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
    settings: &AppSettings,
    icons: &IconSet,
    filter: &SearchFilter,
    status: &mut String,
) {
    ui.horizontal(|ui| {
        if let Some(tex) = icons.get("sticky_topic") {
            ui.add(
                egui::Image::new(tex)
                    .fit_to_exact_size(egui::vec2(20.0, 20.0))
                    .sense(egui::Sense::hover()),
            );
        }
        ui.heading("Sticky topics");
        ui.label(
            RichText::new(match settings.sticky_layout {
                StickyLayoutMode::Rows => "· rows".into(),
                StickyLayoutMode::Columns => {
                    format!("· {} columns", settings.sticky_column_count())
                }
            })
            .small()
            .weak(),
        );
    });
    ui.weak("Layout (rows / columns) is in Settings. Shown for connected profiles only.");
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
    let visible: Vec<String> = keys
        .into_iter()
        .filter(|key| {
            pins.items
                .get(key)
                .is_some_and(|p| filter.matches(&p.topic, Some(&p.last_payload)))
        })
        .collect();

    if visible.is_empty() {
        ui.weak("No stickies match the current search.");
        return;
    }

    match settings.sticky_layout {
        StickyLayoutMode::Rows => {
            for key in &visible {
                draw_card(ui, pins, profiles, key, icons, now, false, status);
                ui.add_space(4.0);
            }
        }
        StickyLayoutMode::Columns => {
            let cols = settings.sticky_column_count();
            ui.columns(cols, |columns| {
                for (i, key) in visible.iter().enumerate() {
                    let col = &mut columns[i % cols];
                    draw_card(col, pins, profiles, key, icons, now, true, status);
                    col.add_space(6.0);
                }
            });
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_card(
    ui: &mut egui::Ui,
    pins: &mut PinBoard,
    profiles: &mut ProfileStore,
    key: &str,
    icons: &IconSet,
    now: Instant,
    compact: bool,
    status: &mut String,
) {
    let Some(pin) = pins.items.get(key).cloned() else {
        return;
    };
    let fill = flash::flash_fill(pin.flash_until, now).unwrap_or(Color32::TRANSPARENT);
    egui::Frame::NONE
        .fill(fill)
        .stroke(Stroke::new(
            1.0_f32,
            ui.visuals().widgets.noninteractive.bg_stroke.color,
        ))
        .inner_margin(egui::Margin::same(6))
        .show(ui, |ui| {
            // Chrome: copy topic · identity · copy payload · unstick icon
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                if icons
                    .tiny(ui, "clipboard_checklist", false, "Copy topic")
                    .clicked()
                {
                    ui.ctx().copy_text(pin.topic.clone());
                    *status = format!("Copied topic: {}", pin.topic);
                }
                ui.strong(&pin.profile_name);
                ui.label(RichText::new(&pin.topic).monospace().small());
                if pin.message_count > 0 {
                    ui.label(RichText::new(format!("#{}", pin.message_count)).small().weak());
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if icons
                        .tiny(ui, "sticky_topic", true, "Unstick")
                        .clicked()
                    {
                        pins.unpin(&pin.profile_id, &pin.topic);
                        profiles.set_sticky_and_save(&pin.profile_id, &pin.topic, false);
                    }
                    if icons
                        .tiny(ui, "clipboard_checklist", false, "Copy payload")
                        .clicked()
                    {
                        let copy = json_view::try_pretty_json(&pin.last_payload)
                            .unwrap_or_else(|| pin.last_payload.clone());
                        ui.ctx().copy_text(copy);
                        *status = "Copied payload".into();
                    }
                });
            });

            // Mode dropdown (keep)
            ui.horizontal(|ui| {
                if let Some(live) = pins.items.get_mut(key) {
                    egui::ComboBox::from_id_salt(format!("sticky-view-{key}"))
                        .selected_text(live.view_mode.label())
                        .width(90.0)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut live.view_mode, StickyViewMode::Raw, "Raw");
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
            });

            ui.add_space(2.0);

            let mode = pins
                .items
                .get(key)
                .map(|p| p.view_mode)
                .unwrap_or(StickyViewMode::Raw);
            let is_json = matches!(mode, StickyViewMode::Value)
                && pins
                    .items
                    .get(key)
                    .is_some_and(|p| json_view::try_pretty_json(&p.last_payload).is_some());
            let height = if is_json {
                if compact {
                    180.0
                } else {
                    240.0
                }
            } else {
                72.0
            };

            ui.allocate_ui_with_layout(
                egui::vec2(ui.available_width().max(120.0), height),
                egui::Layout::top_down(egui::Align::LEFT),
                |ui| {
                    if let Some(p) = pins.items.get(key) {
                        match mode {
                            StickyViewMode::Raw => draw_raw(ui, p),
                            StickyViewMode::Value => draw_value(ui, p, key),
                            StickyViewMode::Graph => draw_graph(ui, p),
                        }
                    }
                },
            );
        });
}

fn draw_raw(ui: &mut egui::Ui, pin: &StickyTopic) {
    ui.label(RichText::new("Raw").small().weak());
    ui.label(RichText::new(&pin.last_payload).monospace().small());
}

fn draw_value(ui: &mut egui::Ui, pin: &StickyTopic, sticky_key: &str) {
    if let Some(pretty) = json_view::try_pretty_json(&pin.last_payload) {
        json_view::draw_pretty(ui, &pretty, ("sticky-json", sticky_key));
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
            ui.label(RichText::new(s).monospace().small());
        }
    }
}

fn draw_gauge(ui: &mut egui::Ui, value: f64) {
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

    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width().min(200.0).max(120.0), 64.0), egui::Sense::hover());
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
