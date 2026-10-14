//! Sticky strip — Raw / Value / Graph; rows or column grid from Settings.
//! Only stickies for currently **connected** profiles are shown.

use crate::mqtt::pins::{DetectedValue, HistorySample, PinBoard, StickyTopic, StickyViewMode};
use crate::mqtt::profile::ProfileStore;
use crate::mqtt::session::{SessionManager, SessionState};
use crate::mqtt::settings::{AppSettings, StickyLayoutMode};
use crate::ui::flash;
use crate::ui::icons::IconSet;
use crate::ui::json_view;
use crate::ui::shortcuts;
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
    // Card chrome + flash are inset strokes (never grow layout when pulsing).
    let flash_stroke = flash::flash_stroke(pin.flash_until, now);
    let card_stroke = Stroke::new(
        1.0_f32,
        ui.visuals().widgets.noninteractive.bg_stroke.color,
    );
    let card = egui::Frame::NONE
        .inner_margin(egui::Margin::same(6))
        .show(ui, |ui| {
            // Identity on first row when possible; icon chrome wraps as one group.
            let mut mode_changed: Option<StickyViewMode> = None;
            let mut unstick = false;
            let mut copy_payload = false;
            let mut copy_topic = false;
            // Tiny icons ~14px + gaps + Raw/Value/Graph frame padding.
            const ICON_GROUP_W: f32 = 118.0;
            let row_h = ui.spacing().interact_size.y.max(18.0);

            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                if icons
                    .tiny(ui, "topic_tags", false, "Copy topic")
                    .clicked()
                {
                    copy_topic = true;
                }
                let accent = profiles
                    .profiles
                    .get(&pin.profile_id)
                    .map(|p| p.color())
                    .unwrap_or(Color32::from_rgb(90, 190, 120));
                ui.label(RichText::new(&pin.profile_name).color(accent));
                ui.label(json_view::topic_path_text(&pin.topic));
                if pin.message_count > 0 {
                    ui.label(RichText::new(format!("#{}", pin.message_count)).small().weak());
                }

                // Whole icon bar is one wrap atom — snaps below when it won't fit.
                ui.allocate_ui_with_layout(
                    egui::vec2(ICON_GROUP_W, row_h),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        if let Some(live) = pins.items.get_mut(key) {
                            let before = live.view_mode;
                            egui::Frame::NONE
                                .fill(Color32::from_rgb(32, 34, 40))
                                .corner_radius(3.0)
                                .inner_margin(egui::Margin::symmetric(3, 2))
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        ui.spacing_mut().item_spacing.x = 2.0;
                                        for (mode, icon, tip) in [
                                            (StickyViewMode::Raw, "terminal", "Raw"),
                                            (StickyViewMode::Value, "json_payload", "Value"),
                                            (StickyViewMode::Graph, "line_graph", "Graph"),
                                        ] {
                                            if icons
                                                .tiny(ui, icon, live.view_mode == mode, tip)
                                                .clicked()
                                            {
                                                live.view_mode = mode;
                                            }
                                        }
                                    });
                                });
                            if live.view_mode != before {
                                mode_changed = Some(live.view_mode);
                            }
                        }
                        if icons
                            .tiny(ui, "clipboard_checklist", false, "Copy payload")
                            .clicked()
                        {
                            copy_payload = true;
                        }
                        if icons
                            .tiny(
                                ui,
                                "sticky_topic",
                                true,
                                &shortcuts::tip("Unstick", "Ctrl+Shift+S"),
                            )
                            .clicked()
                        {
                            unstick = true;
                        }
                    },
                );
            });
            if copy_topic {
                ui.ctx().copy_text(pin.topic.clone());
                *status = format!("Copied topic: {}", pin.topic);
            }
            if copy_payload {
                let copy = json_view::try_pretty_json(&pin.last_payload)
                    .unwrap_or_else(|| pin.last_payload.clone());
                ui.ctx().copy_text(copy);
                *status = "Copied payload".into();
            }
            if unstick {
                pins.unpin(&pin.profile_id, &pin.topic);
                profiles.set_sticky_and_save(&pin.profile_id, &pin.topic, false);
            }
            if let Some(mode) = mode_changed {
                profiles.set_sticky_view_mode_and_save(&pin.profile_id, &pin.topic, mode);
            }

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
            // Raw uses the same max height as Value/JSON so long payloads scroll.
            let tall = is_json || matches!(mode, StickyViewMode::Raw);
            let height = if tall {
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
                            StickyViewMode::Raw => draw_raw(ui, p, key),
                            StickyViewMode::Value => draw_value(ui, p, key),
                            StickyViewMode::Graph => draw_graph(ui, p),
                        }
                    }
                },
            );
        });
    flash::paint_inset_or(ui, card.response.rect, flash_stroke, card_stroke);
}

/// Max content height for Raw — matches Value JSON ScrollArea.
const RAW_MAX_HEIGHT: f32 = 220.0;

fn draw_raw(ui: &mut egui::Ui, pin: &StickyTopic, sticky_key: &str) {
    ui.label(RichText::new("Raw").small().weak());
    // Match Value/body font size; cap height like JSON Value mode.
    egui::ScrollArea::vertical()
        .id_salt(("sticky-raw", sticky_key))
        .max_height(RAW_MAX_HEIGHT)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.set_max_width(ui.available_width());
            ui.label(RichText::new(&pin.last_payload).monospace());
        });
}

fn draw_value(ui: &mut egui::Ui, pin: &StickyTopic, sticky_key: &str) {
    // Sticky cards cap JSON height; Payload panel passes None for full height.
    draw_value_payload(
        ui,
        &pin.last_payload,
        Some(&pin.history),
        ("sticky-json", sticky_key),
        Some(220.0),
    );
}

/// Shared sticky **Value** view (pretty JSON / bool / number / string).
/// `history`: numeric samples for Value gauge auto-range (sticky cards).
/// `max_json_height`: `None` fills remaining panel height.
pub fn draw_value_payload(
    ui: &mut egui::Ui,
    payload: &str,
    history: Option<&std::collections::VecDeque<HistorySample>>,
    id_salt: impl std::hash::Hash,
    max_json_height: Option<f32>,
) {
    if let Some(pretty) = json_view::try_pretty_json(payload) {
        json_view::draw_pretty(ui, &pretty, id_salt, max_json_height);
        return;
    }

    match DetectedValue::from_payload(payload) {
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
            draw_gauge(ui, n, history);
        }
        DetectedValue::Text(s) => {
            ui.label(RichText::new("String").small().weak());
            ui.label(RichText::new(s).monospace().small());
        }
    }
}

/// Auto-range from sticky history min/max; pad flat/single-sample so the thumb moves.
fn gauge_range(value: f64, history: Option<&std::collections::VecDeque<HistorySample>>) -> (f64, f64) {
    let mut low = value;
    let mut high = value;
    if let Some(hist) = history {
        for s in hist {
            if let Some(v) = s.numeric {
                if v.is_finite() {
                    low = low.min(v);
                    high = high.max(v);
                }
            }
        }
    }
    if !value.is_finite() {
        return (-1.0, 1.0);
    }
    let span = high - low;
    if !span.is_finite() || span < 1e-12 {
        // Single sample or flat history — small pad so the thumb isn't stuck mid-track.
        let pad = (value.abs() * 0.05).max(1.0);
        (value - pad, value + pad)
    } else {
        // Tiny outer pad so extreme samples aren't flush with the track ends.
        let pad = (span * 0.02).max(span * 1e-6);
        (low - pad, high + pad)
    }
}

fn fmt_scale(v: f64) -> String {
    if !v.is_finite() {
        return "—".into();
    }
    let a = v.abs();
    if a >= 1000.0 || (v.fract().abs() < 1e-9 && a < 1e12) {
        format!("{v:.0}")
    } else if a >= 10.0 {
        format!("{v:.1}")
    } else if a >= 1.0 {
        format!("{v:.2}")
    } else {
        format!("{v:.3}")
    }
}

/// Read-only numeric meter: low/high labels + thumb from history auto-range.
fn draw_gauge(
    ui: &mut egui::Ui,
    value: f64,
    history: Option<&std::collections::VecDeque<HistorySample>>,
) {
    let (low, high) = gauge_range(value, history);
    let span = (high - low).abs().max(1e-12);
    let frac = ((value - low) / span).clamp(0.0, 1.0) as f32;

    let width = ui.available_width().min(220.0).max(140.0);
    let track_h = 10.0_f32;
    let label_h = ui.text_style_height(&egui::TextStyle::Small);
    let total_h = track_h + 4.0 + label_h;

    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, total_h), egui::Sense::hover());
    let track = egui::Rect::from_min_size(
        egui::pos2(rect.left(), rect.top() + 2.0),
        egui::vec2(rect.width(), track_h),
    );
    let painter = ui.painter_at(rect);
    painter.rect_filled(track, 3.0, ui.visuals().extreme_bg_color);

    // Fill up to value.
    let mut fill = track;
    fill.set_width((track.width() * frac).max(2.0));
    painter.rect_filled(fill, 3.0, Color32::from_rgb(80, 160, 200));

    // Thumb (read-only).
    let cx = track.left() + track.width() * frac;
    let cy = track.center().y;
    painter.circle_filled(egui::pos2(cx, cy), 5.0, Color32::from_rgb(200, 220, 240));
    painter.circle_stroke(
        egui::pos2(cx, cy),
        5.0,
        Stroke::new(1.0_f32, Color32::from_rgb(60, 120, 160)),
    );

    // Low / high scale labels under the track.
    let low_s = fmt_scale(low);
    let high_s = fmt_scale(high);
    let galley_l = painter.layout_no_wrap(
        low_s,
        egui::FontId::proportional(label_h * 0.85),
        ui.visuals().weak_text_color(),
    );
    let galley_h = painter.layout_no_wrap(
        high_s,
        egui::FontId::proportional(label_h * 0.85),
        ui.visuals().weak_text_color(),
    );
    let label_y = track.bottom() + 2.0;
    painter.galley(
        egui::pos2(track.left(), label_y),
        galley_l,
        ui.visuals().weak_text_color(),
    );
    painter.galley(
        egui::pos2(track.right() - galley_h.size().x, label_y),
        galley_h,
        ui.visuals().weak_text_color(),
    );
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::time::Instant;

    fn sample(n: f64) -> HistorySample {
        HistorySample {
            at: Instant::now(),
            payload: n.to_string(),
            numeric: Some(n),
        }
    }

    #[test]
    fn gauge_pads_flat_history() {
        let mut h = VecDeque::new();
        h.push_back(sample(42.0));
        h.push_back(sample(42.0));
        let (low, high) = gauge_range(42.0, Some(&h));
        assert!(low < 42.0 && high > 42.0);
        assert!((high - low) >= 2.0 - 1e-9);
    }

    #[test]
    fn gauge_uses_history_minmax() {
        let mut h = VecDeque::new();
        h.push_back(sample(10.0));
        h.push_back(sample(30.0));
        h.push_back(sample(20.0));
        let (low, high) = gauge_range(20.0, Some(&h));
        assert!(low < 10.0);
        assert!(high > 30.0);
    }

    #[test]
    fn gauge_single_sample_no_history() {
        let (low, high) = gauge_range(5.0, None);
        assert!(low < 5.0 && high > 5.0);
    }
}
