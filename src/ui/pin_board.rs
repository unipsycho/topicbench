//! Sticky strip — Raw / Value / Graph; grouped per broker; rows or columns from Settings.
//! Only stickies for currently **connected** profiles are shown.
//! Drag-reorder from the left grip (same pattern as Profiles).

use crate::mqtt::pins::{DetectedValue, HistorySample, PinBoard, StickyTopic, StickyViewMode};
use crate::mqtt::profile::ProfileStore;
use crate::mqtt::session::{SessionManager, SessionState};
use crate::mqtt::settings::{AppSettings, StickyLayoutMode};
use crate::pro::{self, Entitlement};
use crate::ui::flash;
use crate::ui::hover_tip;
use crate::ui::icons::{
    self, IconSet, CHROME_ACCENT_BORDER, CHROME_HOVER, CHROME_MUTED, CHROME_TEXT,
};
use crate::ui::json_view;
use crate::ui::shortcuts;
use crate::ui::search_bar::SearchFilter;
use eframe::egui::{self, Align, Color32, CursorIcon, Pos2, RichText, Sense, Stroke};
use std::collections::{BTreeMap, HashSet};
use std::time::Instant;

/// DnD payload: sticky topic within one broker (profile_id + topic).
#[derive(Clone)]
struct StickyDnd {
    profile_id: String,
    topic: String,
}

/// Match Profiles grip width.
const GRIP_W: f32 = 16.0;

pub fn draw(
    ui: &mut egui::Ui,
    pins: &mut PinBoard,
    profiles: &mut ProfileStore,
    sessions: &SessionManager,
    settings: &AppSettings,
    icons: &IconSet,
    filter: &SearchFilter,
    status: &mut String,
    combined_graph_open: &mut bool,
) {
    icons::section_header_bar(ui, |ui| {
        icons::section_icon(ui, icons, "sticky_topic");
        icons::section_title(ui, "Sticky topics");
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
    ui.weak("Grouped by broker. Layout (rows / columns) is in Settings.");
    ui.add_space(4.0);

    let connected: HashSet<&str> = sessions
        .open
        .iter()
        .filter(|(_, s)| s.state == SessionState::Connected)
        .map(|(id, _)| id.as_str())
        .collect();

    // Pro combined multi-series graph (only when entitled — hide when free).
    if *combined_graph_open && pro::has(Entitlement::MultiGraph) {
        draw_combined_graph(ui, pins, &connected, filter, combined_graph_open);
        ui.add_space(8.0);
    }

    // Visible stickies grouped by profile_id; order follows profile.stickies.
    let mut by_broker: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (k, p) in &pins.items {
        if !connected.contains(p.profile_id.as_str()) {
            continue;
        }
        if filter.apply_to_stickies && !filter.matches(&p.topic, Some(&p.last_payload)) {
            continue;
        }
        by_broker
            .entry(p.profile_id.clone())
            .or_default()
            .push(k.clone());
    }
    for (profile_id, keys) in by_broker.iter_mut() {
        if let Some(prof) = profiles.profiles.get(profile_id) {
            keys.sort_by_key(|k| {
                let topic = pins
                    .items
                    .get(k)
                    .map(|s| s.topic.as_str())
                    .unwrap_or("");
                prof.stickies
                    .iter()
                    .position(|s| s.topic == topic)
                    .unwrap_or(usize::MAX)
            });
        }
    }

    if by_broker.is_empty() {
        let any_connected_sticky = pins
            .items
            .values()
            .any(|p| connected.contains(p.profile_id.as_str()));
        if any_connected_sticky && filter.apply_to_stickies && filter.is_active() {
            ui.weak("No stickies match the current search.");
        } else {
            ui.weak("No stickies for connected profiles — mark a topic sticky from the tree.");
        }
        return;
    }

    // Broker order follows Profiles list; unknown ids append after.
    let mut broker_ids: Vec<String> = profiles
        .ids_in_order()
        .into_iter()
        .filter(|id| by_broker.contains_key(id))
        .collect();
    for id in by_broker.keys() {
        if !broker_ids.iter().any(|x| x == id) {
            broker_ids.push(id.clone());
        }
    }

    let now = Instant::now();
    for profile_id in broker_ids {
        let Some(keys) = by_broker.remove(&profile_id) else {
            continue;
        };
        let (name, accent) = profiles
            .profiles
            .get(&profile_id)
            .map(|p| (p.name.clone(), p.color()))
            .or_else(|| {
                keys.first()
                    .and_then(|k| pins.items.get(k))
                    .map(|p| (p.profile_name.clone(), Color32::from_rgb(90, 190, 120)))
            })
            .unwrap_or_else(|| (profile_id.clone(), Color32::from_rgb(90, 190, 120)));

        draw_broker_heading(ui, &name, accent);
        ui.add_space(2.0);

        let mut pending_reorder: Option<(String, String, bool)> = None;
        match settings.sticky_layout {
            StickyLayoutMode::Rows => {
                ui.spacing_mut().item_spacing.y = 1.0;
                for key in &keys {
                    if let Some(r) = draw_sticky(
                        ui,
                        pins,
                        profiles,
                        key,
                        icons,
                        now,
                        false,
                        status,
                        combined_graph_open,
                    ) {
                        pending_reorder = Some(r);
                    }
                }
            }
            StickyLayoutMode::Columns => {
                let cols = settings.sticky_column_count();
                ui.columns(cols, |columns| {
                    for (i, key) in keys.iter().enumerate() {
                        let col = &mut columns[i % cols];
                        col.spacing_mut().item_spacing.y = 1.0;
                        if let Some(r) = draw_sticky(
                            col,
                            pins,
                            profiles,
                            key,
                            icons,
                            now,
                            true,
                            status,
                            combined_graph_open,
                        ) {

                            pending_reorder = Some(r);
                        }
                    }
                });
            }
        }
        if let Some((dragged, target, after)) = pending_reorder {
            if profiles.reorder_sticky_and_save(&profile_id, &dragged, &target, after) {
                *status = format!("Reordered stickies on {name}");
            }
        }
        ui.add_space(4.0);
    }
}

fn draw_broker_heading(ui: &mut egui::Ui, name: &str, accent: Color32) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(name).strong().color(accent));
        ui.add_space(6.0);
        let y = ui.cursor().top() + ui.text_style_height(&egui::TextStyle::Body) * 0.5;
        let (line, _) = ui.allocate_exact_size(
            egui::vec2(ui.available_width().max(8.0), 1.0),
            egui::Sense::hover(),
        );
        ui.painter().hline(
            line.x_range(),
            y,
            Stroke::new(1.0_f32, Color32::from_rgb(70, 74, 84)),
        );
    });
}

/// Returns pending reorder `(dragged_topic, target_topic, after)` when a drop lands on this row.
#[allow(clippy::too_many_arguments)]
fn draw_sticky(
    ui: &mut egui::Ui,
    pins: &mut PinBoard,
    profiles: &mut ProfileStore,
    key: &str,
    icons: &IconSet,
    now: Instant,
    compact: bool,
    status: &mut String,
    combined_graph_open: &mut bool,
) -> Option<(String, String, bool)> {
    let Some(pin) = pins.items.get(key).cloned() else {
        return None;
    };
    // Card chrome + flash are inset strokes (never grow layout when pulsing).
    let flash_stroke = flash::flash_stroke(pin.flash_until, now);
    let card_stroke = Stroke::new(
        1.0_f32,
        ui.visuals().widgets.noninteractive.bg_stroke.color,
    );

    let mut pending_reorder = None;
    let profile_id = pin.profile_id.clone();
    let topic = pin.topic.clone();

    // Grip left + card body (Profiles pattern: grip starts drag, body keeps clicks).
    let row = ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;

        let card = egui::Frame::NONE
            .inner_margin(egui::Margin {
                left: (6.0 + GRIP_W) as i8,
                right: 6,
                top: 2,
                bottom: 2,
            })
            .show(ui, |ui| {
                let mut mode_changed: Option<StickyViewMode> = None;
                let mut unstick = false;
                let mut copy_payload = false;
                let mut copy_topic = false;

                // Height is only topic row + icon row; value panel matches that strip.
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    let mode = pins
                        .items
                        .get(key)
                        .map(|p| p.view_mode)
                        .unwrap_or(StickyViewMode::Raw);

                    // Topic takes its natural width; only shrink so the value/chart keeps a floor.
                    let tiny_btn = icons::TINY_ICON + 4.0; // matches IconSet::tiny hit target
                    let gap = 4.0;
                    let sep_w = 1.0;
                    let min_value_w = match mode {
                        StickyViewMode::Graph => {
                            if compact {
                                100.0
                            } else {
                                140.0
                            }
                        }
                        _ => {
                            if compact {
                                80.0
                            } else {
                                110.0
                            }
                        }
                    };
                    let max_left = (ui.available_width() - min_value_w - sep_w - gap * 2.0)
                        .max(tiny_btn + 48.0);

                    let topic_font = egui::TextStyle::Monospace.resolve(ui.style());
                    let topic_galley = ui.fonts(|f| {
                        f.layout_no_wrap(pin.topic.clone(), topic_font, Color32::WHITE)
                    });
                    let topic_row_w = tiny_btn + gap + topic_galley.size().x;
                    // Mode trio + optional Pro combined + #n + copy + unstick.
                    let multi_graph = pro::has(Entitlement::MultiGraph);
                    let mode_w = 3.0 * tiny_btn
                        + if multi_graph && mode == StickyViewMode::Graph {
                            tiny_btn + 4.0
                        } else {
                            0.0
                        }
                        + 12.0;
                    let count_w = if pin.message_count > 0 { 32.0 } else { 0.0 };
                    let controls_w = mode_w + count_w + tiny_btn + tiny_btn + gap * 4.0;
                    let left_w = topic_row_w.max(controls_w).min(max_left).max(tiny_btn + 48.0);

                    // Size to content (not panel available_height) so the card stays chrome-tall.
                    let left = ui.vertical(|ui| {
                        ui.set_width(left_w);
                        ui.set_max_width(left_w);
                        ui.spacing_mut().item_spacing.y = 2.0;
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = gap;
                            if icons
                                .tiny(ui, "topic_tags", false, "Copy topic")
                                .clicked()
                            {
                                copy_topic = true;
                            }
                            ui.add(
                                egui::Label::new(json_view::topic_path_text(&pin.topic))
                                    .truncate()
                                    .selectable(false),
                            );
                        });

                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = gap;
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
                                            // Pro: Combined graph — only when entitled + Graph mode.
                                            if multi_graph
                                                && live.view_mode == StickyViewMode::Graph
                                                && icons
                                                    .tiny(
                                                        ui,
                                                        "node_graph",
                                                        *combined_graph_open,
                                                        "Combined graph",
                                                    )
                                                    .clicked()
                                            {
                                                *combined_graph_open = !*combined_graph_open;
                                            }
                                        });
                                    });
                                if live.view_mode != before {
                                    mode_changed = Some(live.view_mode);
                                }
                            }
                            if pin.message_count > 0 {
                                ui.label(
                                    RichText::new(format!("#{}", pin.message_count))
                                        .small()
                                        .color(CHROME_MUTED),
                                );
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
                        });
                    });
                    let height = left.response.rect.height().max(1.0);

                    let sep = ui
                        .allocate_exact_size(egui::vec2(sep_w, height), egui::Sense::hover())
                        .0;
                    ui.painter()
                        .rect_filled(sep, 0.0, Color32::from_rgb(55, 58, 66));

                    let right_w = ui.available_width().max(min_value_w * 0.5);
                    ui.allocate_ui_with_layout(
                        egui::vec2(right_w, height),
                        egui::Layout::top_down(Align::LEFT),
                        |ui| {
                            ui.set_max_height(height);
                            ui.set_clip_rect(ui.max_rect());
                            if let Some(p) = pins.items.get(key) {
                                match mode {
                                    StickyViewMode::Raw => draw_raw_strip(ui, p, key, height),
                                    StickyViewMode::Value => draw_value_strip(ui, p, key, height),
                                    StickyViewMode::Graph => draw_graph_strip(ui, p, height),
                                }
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
            });

        let row_rect = card.response.rect;
        flash::paint_inset_or(ui, row_rect, flash_stroke, card_stroke);

        // Grip in the left padding — drag only; buttons in the body stay clickable.
        let grip_rect = egui::Rect::from_min_size(
            egui::pos2(row_rect.left() + 2.0, row_rect.top()),
            egui::vec2(GRIP_W, row_rect.height()),
        );
        icons::paint_drag_grip(ui, grip_rect);
        let grip_id = egui::Id::new(("sticky_grip", key));
        let grip_response = hover_tip::attach(
            ui.interact(grip_rect, grip_id, Sense::drag())
                .on_hover_cursor(CursorIcon::Grab),
            "Drag to reorder",
        );
        if grip_response.dragged() {
            grip_response.dnd_set_drag_payload(StickyDnd {
                profile_id: profile_id.clone(),
                topic: topic.clone(),
            });
        }
        if ui.ctx().is_being_dragged(grip_id) {
            if let Some(pos) = ui.ctx().pointer_interact_pos() {
                let layer = egui::LayerId::new(egui::Order::Tooltip, grip_id);
                let preview =
                    egui::Rect::from_center_size(pos, egui::vec2(GRIP_W, GRIP_W + 8.0));
                let painter = ui.ctx().layer_painter(layer);
                painter.rect_filled(preview, 2.0, CHROME_HOVER);
                // Same 2×3 grip dots as Profiles floating preview.
                let color = CHROME_MUTED;
                let r = 1.2_f32;
                let cx = preview.center().x;
                let cy = preview.center().y;
                for col in [-1.0_f32, 1.0] {
                    for row in [-1.0_f32, 0.0, 1.0] {
                        painter.circle_filled(
                            egui::pos2(cx + col * 3.0, cy + row * 3.5),
                            r,
                            color,
                        );
                    }
                }
            }
        }

        // Whole card is the drop target (insert before/after by pointer Y).
        let drop_response = ui.interact(
            row_rect,
            egui::Id::new(("sticky_drop", key)),
            Sense::hover(),
        );
        if let Some(payload) = drop_response.dnd_hover_payload::<StickyDnd>() {
            if payload.profile_id == profile_id && payload.topic != topic {
                let after = drop_response
                    .hover_pos()
                    .map(|p| p.y > drop_response.rect.center().y)
                    .unwrap_or(false);
                let y = if after {
                    drop_response.rect.bottom()
                } else {
                    drop_response.rect.top()
                };
                ui.painter().hline(
                    drop_response.rect.x_range(),
                    y,
                    egui::Stroke::new(2.0, CHROME_ACCENT_BORDER),
                );
                ui.painter().rect_filled(
                    drop_response.rect,
                    2.0,
                    Color32::from_rgba_unmultiplied(
                        CHROME_HOVER.r(),
                        CHROME_HOVER.g(),
                        CHROME_HOVER.b(),
                        80,
                    ),
                );
            }
        }
        if let Some(payload) = drop_response.dnd_release_payload::<StickyDnd>() {
            if payload.profile_id == profile_id && payload.topic != topic {
                let after = drop_response
                    .interact_pointer_pos()
                    .or_else(|| drop_response.hover_pos())
                    .map(|p| p.y > drop_response.rect.center().y)
                    .unwrap_or(false);
                pending_reorder = Some((payload.topic.clone(), topic.clone(), after));
            }
        }
    });
    let _ = row;
    pending_reorder
}

/// Wrap text inside the chrome-height strip — never grows the sticky card.
fn draw_fixed_wrap(
    ui: &mut egui::Ui,
    text: RichText,
    height: f32,
    id_salt: impl std::hash::Hash,
) {
    egui::ScrollArea::vertical()
        .id_salt(id_salt)
        .max_height(height)
        .min_scrolled_height(height)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.set_max_width(ui.available_width());
            ui.add(egui::Label::new(text).wrap().selectable(false));
        });
}

/// Compact right-hand strip: raw payload, wrapped within chrome height.
fn draw_raw_strip(ui: &mut egui::Ui, pin: &StickyTopic, sticky_key: &str, height: f32) {
    // Same monospace body size as the topic (never .small()).
    let text = RichText::new(&pin.last_payload).monospace();
    draw_fixed_wrap(ui, text, height, ("sticky-raw", sticky_key));
}

/// Compact right-hand strip: scalars large; long text wraps at topic size.
fn draw_value_strip(ui: &mut egui::Ui, pin: &StickyTopic, sticky_key: &str, height: f32) {
    if let Some(pretty) = json_view::try_pretty_json(&pin.last_payload) {
        // Match topic monospace size for readability.
        let text = RichText::new(pretty).monospace();
        draw_fixed_wrap(ui, text, height, ("sticky-json", sticky_key));
        return;
    }

    match DetectedValue::from_payload(&pin.last_payload) {
        DetectedValue::Bool(on) => {
            let label = if on { "true" } else { "false" };
            let color = if on {
                Color32::from_rgb(70, 170, 100)
            } else {
                Color32::from_rgb(140, 140, 140)
            };
            draw_scalar_centered(
                ui,
                RichText::new(label).size(22.0).strong().color(color),
                height,
            );
        }
        DetectedValue::Number(n) => {
            draw_scalar_centered(
                ui,
                RichText::new(format!("{n}"))
                    .size(22.0)
                    .strong()
                    .monospace(),
                height,
            );
        }
        DetectedValue::Text(s) => {
            // Topic-sized monospace; wraps inside the strip when long.
            let text = RichText::new(s).monospace();
            draw_fixed_wrap(ui, text, height, ("sticky-text", sticky_key));
        }
    }
}

/// Large bool/number — centered in the value box.
fn draw_scalar_centered(ui: &mut egui::Ui, text: RichText, height: f32) {
    let w = ui.available_width().max(1.0);
    ui.allocate_ui_with_layout(
        egui::vec2(w, height),
        egui::Layout::centered_and_justified(egui::Direction::TopDown),
        |ui| {
            ui.label(text);
        },
    );
}

/// Series colors for the combined graph (cycle; style change after wrap is deferred).
const SERIES_PALETTE: [Color32; 8] = [
    Color32::from_rgb(80, 180, 120),
    Color32::from_rgb(100, 160, 220),
    Color32::from_rgb(220, 160, 80),
    Color32::from_rgb(200, 100, 160),
    Color32::from_rgb(120, 200, 200),
    Color32::from_rgb(180, 140, 220),
    Color32::from_rgb(220, 120, 100),
    Color32::from_rgb(160, 180, 100),
];

/// ~2.5× a single sticky graph strip — plot dominates the host card.
const COMBINED_PLOT_H: f32 = 120.0;

/// Last path segment for legend (topic leaf or field-path leaf).
fn sticky_short_label(topic: &str) -> String {
    let (base, path) = crate::mqtt::value_path::split_sticky_id(topic);
    if let Some(p) = path {
        p.rsplit('.').next().unwrap_or(p).to_string()
    } else {
        base.rsplit('/').next().unwrap_or(base).to_string()
    }
}

/// Pro: shared time-axis overlay of all Graph-mode stickies with numeric history.
fn draw_combined_graph(
    ui: &mut egui::Ui,
    pins: &PinBoard,
    connected: &HashSet<&str>,
    filter: &SearchFilter,
    open: &mut bool,
) {
    // v1: auto-include every connected Graph sticky that has numeric samples.
    let mut series: Vec<(&StickyTopic, Color32)> = Vec::new();
    for p in pins.items.values() {
        if !connected.contains(p.profile_id.as_str()) {
            continue;
        }
        if p.view_mode != StickyViewMode::Graph {
            continue;
        }
        if filter.apply_to_stickies && !filter.matches(&p.topic, Some(&p.last_payload)) {
            continue;
        }
        if !p.history.iter().any(|s| s.numeric.is_some()) {
            continue;
        }
        let color = SERIES_PALETTE[series.len() % SERIES_PALETTE.len()];
        series.push((p, color));
    }

    egui::Frame::NONE
        .fill(Color32::from_rgb(32, 34, 40))
        .stroke(Stroke::new(1.0_f32, CHROME_ACCENT_BORDER))
        .corner_radius(4.0)
        .inner_margin(egui::Margin::symmetric(10, 8))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Combined graph").strong().color(CHROME_TEXT));
                if !series.is_empty() {
                    ui.label(
                        RichText::new(format!("{} series", series.len()))
                            .small()
                            .color(CHROME_MUTED),
                    );
                }
                ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                    if ui.small_button("Dismiss").clicked() {
                        *open = false;
                    }
                });
            });

            if series.is_empty() {
                ui.label(
                    RichText::new("No Graph stickies with numeric history yet.")
                        .color(CHROME_MUTED),
                );
                ui.weak("Switch stickies to Graph mode — they appear here on one time axis.");
                return;
            }

            paint_multi_series_plot(ui, &series, COMBINED_PLOT_H);
            ui.add_space(6.0);
            draw_combined_legend(ui, &series);
        });
}

/// Shared X = Instant domain (union of histories); shared Y auto-scale; gap = omit (no hold-last).
fn paint_multi_series_plot(ui: &mut egui::Ui, series: &[(&StickyTopic, Color32)], height: f32) {
    let mut t0: Option<Instant> = None;
    let mut t1: Option<Instant> = None;
    let mut min_v = f64::INFINITY;
    let mut max_v = f64::NEG_INFINITY;
    for (pin, _) in series {
        for s in &pin.history {
            let Some(v) = s.numeric else { continue };
            if !v.is_finite() {
                continue;
            }
            min_v = min_v.min(v);
            max_v = max_v.max(v);
            t0 = Some(t0.map_or(s.at, |t| t.min(s.at)));
            t1 = Some(t1.map_or(s.at, |t| t.max(s.at)));
        }
    }
    if !min_v.is_finite() || !max_v.is_finite() {
        ui.weak("No numeric samples.");
        return;
    }
    let (Some(t0), Some(t1)) = (t0, t1) else {
        ui.weak("No numeric samples.");
        return;
    };
    let raw_span = (max_v - min_v).abs();
    // Flat history: pad so the line isn't glued to the midline.
    let (y_lo, y_hi) = if raw_span < 1e-12 {
        let pad = (min_v.abs() * 0.05).max(1.0);
        (min_v - pad, max_v + pad)
    } else {
        let pad = (raw_span * 0.04).max(raw_span * 1e-6);
        (min_v - pad, max_v + pad)
    };
    let y_span = (y_hi - y_lo).max(1e-9);
    let dt = t1.saturating_duration_since(t0).as_secs_f32().max(1e-3);

    let plot_w = ui.available_width().max(48.0);
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(plot_w, height), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 2.0, ui.visuals().extreme_bg_color);

    // Subtle midline for orientation on dark chrome.
    let mid_y = rect.center().y;
    painter.hline(
        rect.x_range(),
        mid_y,
        Stroke::new(1.0_f32, Color32::from_rgb(48, 52, 60)),
    );

    for (pin, color) in series {
        let mut points = Vec::new();
        for s in &pin.history {
            let Some(v) = s.numeric else { continue };
            if !v.is_finite() {
                continue;
            }
            let x_t = s.at.saturating_duration_since(t0).as_secs_f32() / dt;
            let x = rect.left() + x_t.clamp(0.0, 1.0) * rect.width();
            let y = rect.bottom() - (((v - y_lo) / y_span) as f32) * rect.height();
            points.push(Pos2::new(x, y));
        }
        if points.len() >= 2 {
            painter.add(egui::Shape::line(points, Stroke::new(1.6_f32, *color)));
        } else if let Some(p) = points.first() {
            painter.circle_filled(*p, 3.0, *color);
        }
    }

    response.on_hover_text(format!(
        "Y {:.3} … {:.3} · {} series · shared time axis",
        min_v,
        max_v,
        series.len()
    ));
}

/// Wrapping legend: color swatch + short name + current value; tooltip = full topic.
fn draw_combined_legend(ui: &mut egui::Ui, series: &[(&StickyTopic, Color32)]) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 12.0;
        ui.spacing_mut().item_spacing.y = 4.0;
        for (pin, color) in series {
            let short = sticky_short_label(&pin.topic);
            let cur = pin
                .history
                .iter()
                .rev()
                .find_map(|s| s.numeric)
                .map(|v| format!("{v}"))
                .unwrap_or_else(|| "—".into());
            let tip = if pin.profile_name.is_empty() {
                pin.topic.clone()
            } else {
                format!("{} · {}", pin.topic, pin.profile_name)
            };
            let r = ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                let (sw, _) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), Sense::hover());
                ui.painter().circle_filled(sw.center(), 3.5, *color);
                let name = if short.len() > 18 {
                    format!("{}…", &short[..17])
                } else {
                    short
                };
                ui.label(RichText::new(name).small().color(CHROME_TEXT));
                ui.label(RichText::new(cur).small().strong().monospace().color(*color));
            });
            r.response.on_hover_text(tip);
        }
    });
}

fn draw_graph_strip(ui: &mut egui::Ui, pin: &StickyTopic, height: f32) {
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

    let chart_h = (height - 2.0).max(16.0);
    // Value is content-width only; sparkline takes the remaining strip.
    ui.with_layout(egui::Layout::left_to_right(Align::Center), |ui| {
        ui.set_min_height(chart_h);
        ui.spacing_mut().item_spacing.x = 8.0;

        match DetectedValue::from_payload(&pin.last_payload) {
            DetectedValue::Bool(on) => {
                let label = if on { "true" } else { "false" };
                let color = if on {
                    Color32::from_rgb(70, 170, 100)
                } else {
                    Color32::from_rgb(140, 140, 140)
                };
                ui.label(RichText::new(label).size(22.0).strong().color(color));
            }
            DetectedValue::Number(n) => {
                ui.label(
                    RichText::new(format!("{n}"))
                        .size(22.0)
                        .strong()
                        .monospace(),
                );
            }
            DetectedValue::Text(_) => {
                if let Some((_, v)) = numeric.last() {
                    ui.label(
                        RichText::new(format!("{v}"))
                            .size(22.0)
                            .strong()
                            .monospace(),
                    );
                }
            }
        }

        let chart_w = ui.available_width().max(48.0);
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(chart_w, chart_h), egui::Sense::hover());
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
            response.on_hover_text(format!("min={min_v:.3}  max={max_v:.3}"));
        } else {
            let n = samples.len().max(1) as f32;
            for (i, _) in samples.iter().enumerate() {
                let x = rect.left() + (i as f32 / (n - 1.0).max(1.0)) * rect.width();
                let y = rect.center().y;
                painter.circle_filled(Pos2::new(x, y), 2.5, Color32::from_rgb(100, 160, 220));
            }
            response.on_hover_text(format!("{} arrivals", samples.len()));
        }
    });
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
            let label = if on { "true" } else { "false" };
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
