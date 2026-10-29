//! Color Rules editor (Pro) — ordered fixed predicates; live tree paint.
//! Opens as a real OS viewport (Payload undock pattern).
//! Flat list rows (not boxed cards): column header once, drag grip to reorder
//! (first-match order), aligned Topic / Value / BGK chips.

use crate::pro::color_rules::{ColorRule, ColorRulesStore, MatchPred, RuleStyle};
use crate::ui::icons::{
    self, IconSet, CHROME_ACCENT_BORDER, CHROME_BG, CHROME_CAPTION, CHROME_HOVER, CHROME_MUTED,
    CHROME_ROW, CHROME_SEP,
};
use eframe::egui::{
    self, Color32, CursorIcon, Id, Pos2, RichText, Sense, TextStyle, ViewportBuilder, ViewportClass,
};

/// DnD payload — rule id; drop inserts before/after target by pointer Y.
#[derive(Clone)]
struct ColorRuleDnd(String);

/// Preset hues + greys. Alpha comes from click Y inside each gradient swatch.
const PRESETS: &[(&str, [u8; 3])] = &[
    ("Red", [220, 70, 70]),
    ("Orange", [230, 120, 50]),
    ("Amber", [220, 160, 50]),
    ("Yellow", [220, 200, 70]),
    ("Lime", [140, 200, 70]),
    ("Green", [70, 180, 100]),
    ("Teal", [50, 180, 160]),
    ("Cyan", [60, 180, 210]),
    ("Blue", [70, 140, 220]),
    ("Indigo", [110, 100, 220]),
    ("Magenta", [200, 80, 180]),
    ("Pink", [220, 100, 140]),
    ("White", [230, 232, 238]),
    ("Light grey", [170, 176, 186]),
    ("Mid grey", [120, 126, 138]),
    ("Dark grey", [70, 74, 84]),
    ("Near black", [40, 42, 48]),
];

/// List-row geometry — chips fixed right; Name/Topic/Match flex into leftover width.
const ROW_H: f32 = 30.0;
const HEADER_H: f32 = 22.0;
const GRIP_W: f32 = 14.0;
const COL_ENABLE: f32 = 22.0;
const COL_CHIP: f32 = 28.0;
const COL_DEL: f32 = 22.0;
/// Floor widths when the panel is narrow (extra space shared by weight).
const NAME_MIN: f32 = 72.0;
const TOPIC_MIN: f32 = 80.0;
const MATCH_MIN: f32 = 140.0;
const NAME_FLEX: f32 = 1.0;
const TOPIC_FLEX: f32 = 1.15;
const MATCH_FLEX: f32 = 2.4;
/// Extra air between Topic and Value chips (Value→BGK stays tighter).
const CHIP_GAP_TOPIC_VALUE: f32 = 12.0;
const CHIP_GAP_VALUE_BGK: f32 = 6.0;
/// Topic / Value / BGK chips + gaps — fixed slot before trash.
const COLORS_BLOCK_W: f32 = COL_CHIP * 3.0 + CHIP_GAP_TOPIC_VALUE + CHIP_GAP_VALUE_BGK;
/// Compact current-color chip on the rule row.
const CHIP_SIZE: f32 = 18.0;
/// Popover swatches — one horizontal row; tall for precise alpha clicks.
const SWATCH_W: f32 = 18.0;
const SWATCH_H: f32 = 54.0; // ~50% taller than prior 36px popover swatches
/// Floor alpha at bottom of swatch (near-transparent, still visible).
const ALPHA_FLOOR: u8 = 16;

pub fn draw_popup(
    ctx: &egui::Context,
    open: &mut bool,
    store: &mut ColorRulesStore,
    icons: &IconSet,
    status: &mut String,
    builder_seed: crate::mqtt::window_geom::WinGeom,
    saved_geom: &mut Option<crate::mqtt::window_geom::WinGeom>,
    geom_dirty: &mut bool,
) {
    if !*open {
        return;
    }
    let mut close = false;
    const MIN: [f32; 2] = [720.0, 280.0];
    // OS window — draggable outside the main TopicBench frame.
    ctx.show_viewport_immediate(
        // Id bump resets stale tiny OS window size from earlier sessions.
        egui::ViewportId::from_hash_of("topicbench_color_rules_vp_v3"),
        builder_seed
            .apply(ViewportBuilder::default().with_title("Color Rules"), MIN)
            .with_min_inner_size(MIN),
        |ctx, class| {
            let mut paint = |ui: &mut egui::Ui| {
                // Profiles-matching Body/Small + blue checkbox accent (also set globally).
                icons::apply_pro_panel_chrome(ui);
                // Header bar: icon + title + actions only (no subtitle here).
                icons::section_header_bar(ui, |ui| {
                    icons::section_icon(ui, icons, "color_rules");
                    icons::section_title(ui, "Color Rules");
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if icons
                            .tiny_header(ui, "close_red", false, "Close Color Rules")
                            .clicked()
                        {
                            close = true;
                        }
                        if icons
                            .tiny_header(ui, "add_blue", false, "Add rule")
                            .clicked()
                        {
                            match store.add_blank() {
                                Ok(()) => *status = "Added color rule".into(),
                                Err(e) => *status = e,
                            }
                        }
                    });
                });
                // Own line under the header — body font (same as panel text, not tiny/muted).
                ui.add_space(6.0);
                ui.label(
                    RichText::new(
                        "First match wins · drag ↕ to reorder · nested fields use dotted paths (meta.channel)",
                    )
                    .text_style(TextStyle::Body)
                    .color(CHROME_CAPTION),
                );
                ui.add_space(4.0);
                ui.separator();

                let mut remove: Option<String> = None;
                let mut dirty = false;
                let mut pending_reorder: Option<(String, String, bool)> = None;
                // Agent/screenshot helper: open first Topic chip popover once.
                let force_pop = std::env::var_os("TOPICBENCH_COLOR_RULES_OPEN_POPOVER").is_some();
                let n = store.rules.len();
                let row_w = ui.available_width();
                draw_column_header(ui, row_w);
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for i in 0..n {
                        draw_rule_row(
                            ui,
                            row_w,
                            &mut store.rules[i],
                            icons,
                            &mut remove,
                            &mut dirty,
                            &mut pending_reorder,
                            force_pop && i == 0,
                        );
                        // Profiles list breathing room between rounded strips.
                        ui.add_space(2.0);
                    }
                });
                if let Some((dragged, target, after)) = pending_reorder {
                    if store.reorder_ids(&dragged, &target, after) {
                        *status = "Reordered color rules".into();
                    }
                } else if let Some(id) = remove {
                    store.remove(&id);
                    *status = "Removed color rule".into();
                } else if dirty {
                    let _ = store.save();
                }
            };
            match class {
                ViewportClass::Embedded => {
                    egui::Window::new("Color Rules")
                        .id(egui::Id::new("topicbench_color_rules"))
                        .title_bar(false)
                        .collapsible(false)
                        .resizable(true)
                        .default_width(520.0)
                        .default_height(320.0)
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

/// Body width left of fixed Topic/Value/BGK + trash (matches `draw_rule_row` geometry).
fn body_width(row_w: f32) -> f32 {
    (row_w - GRIP_W - COLORS_BLOCK_W - COL_DEL - 12.0).max(80.0)
}

/// Share leftover body width across Name / Topic / Match (chips stay fixed right).
fn flex_text_cols(body_w: f32) -> (f32, f32, f32) {
    let gaps = 4.0 * 3.0; // enable↔name, name↔topic, topic↔match
    let avail = (body_w - COL_ENABLE - gaps).max(0.0);
    let mins = NAME_MIN + TOPIC_MIN + MATCH_MIN;
    if avail <= mins {
        let s = if mins > 0.0 { avail / mins } else { 0.0 };
        return (NAME_MIN * s, TOPIC_MIN * s, MATCH_MIN * s);
    }
    let extra = avail - mins;
    let tw = NAME_FLEX + TOPIC_FLEX + MATCH_FLEX;
    (
        NAME_MIN + extra * (NAME_FLEX / tw),
        TOPIC_MIN + extra * (TOPIC_FLEX / tw),
        MATCH_MIN + extra * (MATCH_FLEX / tw),
    )
}

/// Column names once — left cols from grip; Topic/Value/BGK right-anchored before trash.
fn draw_column_header(ui: &mut egui::Ui, row_w: f32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(row_w, HEADER_H), Sense::hover());
    ui.painter().hline(
        rect.x_range(),
        rect.bottom(),
        egui::Stroke::new(1.0_f32, CHROME_SEP),
    );
    let y = rect.center().y;
    let font = TextStyle::Body.resolve(ui.style());
    // Same right edge geometry as rows: delete, then fixed color block.
    let del_left = rect.right() - COL_DEL - 2.0;
    let colors_left = del_left - 2.0 - COLORS_BLOCK_W;
    let mut cx = colors_left;
    for (lab, gap_after) in [
        ("Topic", CHIP_GAP_TOPIC_VALUE),
        ("Value", CHIP_GAP_VALUE_BGK),
        ("BGK", 0.0),
    ] {
        ui.painter().text(
            egui::pos2(cx + COL_CHIP * 0.5, y),
            egui::Align2::CENTER_CENTER,
            lab,
            font.clone(),
            CHROME_CAPTION,
        );
        cx += COL_CHIP + gap_after;
    }
    // Left headers track flex Name / Topic / Match widths.
    let (name_w, topic_w, _) = flex_text_cols(body_width(row_w));
    let mut x = rect.left() + 2.0 + GRIP_W + 4.0 + COL_ENABLE + 4.0;
    let paint_h = |ui: &mut egui::Ui, label: &str, col_w: f32, x: &mut f32| {
        ui.painter().text(
            egui::pos2(*x, y),
            egui::Align2::LEFT_CENTER,
            label,
            font.clone(),
            CHROME_CAPTION,
        );
        *x += col_w + 4.0;
    };
    paint_h(ui, "Name", name_w, &mut x);
    paint_h(ui, "Topic", topic_w, &mut x);
    ui.painter().text(
        egui::pos2(x, y),
        egui::Align2::LEFT_CENTER,
        "Match",
        font,
        CHROME_CAPTION,
    );
}

fn draw_rule_row(
    ui: &mut egui::Ui,
    row_w: f32,
    rule: &mut ColorRule,
    icons: &IconSet,
    remove: &mut Option<String>,
    dirty: &mut bool,
    pending_reorder: &mut Option<(String, String, bool)>,
    force_open_topic_pop: bool,
) {
    let rule_id = rule.id.clone();
    // Profiles-style list strip — rounded CHROME_ROW, no hairline card border.
    let (row_rect, drop_response) =
        ui.allocate_exact_size(egui::vec2(row_w, ROW_H), Sense::hover());
    ui.painter().rect_filled(row_rect, 2.0, CHROME_ROW);

    let grip_rect = egui::Rect::from_min_size(
        egui::pos2(row_rect.left() + 2.0, row_rect.top()),
        egui::vec2(GRIP_W, ROW_H),
    );
    let del_rect = egui::Rect::from_min_size(
        egui::pos2(row_rect.right() - COL_DEL - 2.0, row_rect.top()),
        egui::vec2(COL_DEL, ROW_H),
    );
    // Fixed color slot immediately left of trash — never drifts with match widgets.
    let colors_rect = egui::Rect::from_min_size(
        egui::pos2(del_rect.left() - 2.0 - COLORS_BLOCK_W, row_rect.top()),
        egui::vec2(COLORS_BLOCK_W, ROW_H),
    );
    let body_rect = egui::Rect::from_min_max(
        egui::pos2(grip_rect.right() + 4.0, row_rect.top()),
        egui::pos2(colors_rect.left() - 2.0, row_rect.bottom()),
    );

    // Grip — Profiles / Bookmarks pattern: drag only from handle.
    icons::paint_drag_grip(ui, grip_rect);
    let grip_id = egui::Id::new(("cr_grip", rule_id.as_str()));
    let grip_response = ui
        .interact(grip_rect, grip_id, Sense::drag())
        .on_hover_cursor(CursorIcon::Grab)
        .on_hover_text("Drag to reorder (first match wins)");
    if grip_response.dragged() {
        grip_response.dnd_set_drag_payload(ColorRuleDnd(rule_id.clone()));
    }
    if ui.ctx().is_being_dragged(grip_id) {
        if let Some(pos) = ui.ctx().pointer_interact_pos() {
            let layer = egui::LayerId::new(egui::Order::Tooltip, grip_id);
            let preview = egui::Rect::from_center_size(pos, egui::vec2(GRIP_W, ROW_H));
            let painter = ui.ctx().layer_painter(layer);
            painter.rect_filled(preview, 2.0, CHROME_HOVER);
            // Grip dots on the floating preview (same geometry as paint_drag_grip).
            let color = CHROME_MUTED;
            let r = 1.2_f32;
            let cx = preview.center().x;
            let cy = preview.center().y;
            for col in [-1.0_f32, 1.0] {
                for row in [-1.0_f32, 0.0, 1.0] {
                    painter.circle_filled(egui::pos2(cx + col * 3.0, cy + row * 3.5), r, color);
                }
            }
        }
    }

    // Drop insert line before/after by pointer Y.
    if let Some(payload) = drop_response.dnd_hover_payload::<ColorRuleDnd>() {
        if payload.0.as_str() != rule_id.as_str() {
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
                egui::Stroke::new(2.0_f32, CHROME_ACCENT_BORDER),
            );
            ui.painter().rect_filled(
                drop_response.rect,
                0.0,
                Color32::from_rgba_unmultiplied(
                    CHROME_HOVER.r(),
                    CHROME_HOVER.g(),
                    CHROME_HOVER.b(),
                    80,
                ),
            );
        }
    }
    if let Some(payload) = drop_response.dnd_release_payload::<ColorRuleDnd>() {
        if payload.0.as_str() != rule_id.as_str() {
            let after = drop_response
                .interact_pointer_pos()
                .or_else(|| drop_response.hover_pos())
                .map(|p| p.y > drop_response.rect.center().y)
                .unwrap_or(false);
            *pending_reorder = Some((payload.0.clone(), rule_id.clone(), after));
        }
    }

    // Enable fixed; Name / Topic / Match share leftover — cannot push color chips.
    let (name_w, topic_w, match_w) = flex_text_cols(body_rect.width());
    ui.allocate_new_ui(egui::UiBuilder::new().max_rect(body_rect), |ui| {
        ui.set_min_size(body_rect.size());
        ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            ui.allocate_ui_with_layout(
                egui::vec2(COL_ENABLE, ROW_H),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    if ui.checkbox(&mut rule.enabled, "").changed() {
                        *dirty = true;
                    }
                },
            );
            ui.allocate_ui_with_layout(
                egui::vec2(name_w, ROW_H),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    if ui
                        .add(
                            egui::TextEdit::singleline(&mut rule.name)
                                .desired_width((name_w - 4.0).max(24.0))
                                .hint_text("Name"),
                        )
                        .changed()
                    {
                        *dirty = true;
                    }
                },
            );
            ui.allocate_ui_with_layout(
                egui::vec2(topic_w, ROW_H),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    if ui
                        .add(
                            egui::TextEdit::singleline(&mut rule.topic_glob)
                                .desired_width((topic_w - 4.0).max(24.0))
                                .hint_text("glob #"),
                        )
                        .changed()
                    {
                        *dirty = true;
                    }
                },
            );
            // Exact match slot — paint into child so overflow cannot grow the row.
            let match_w = match_w.max(40.0).min(ui.available_width().max(40.0));
            let (match_rect, _) =
                ui.allocate_exact_size(egui::vec2(match_w, ROW_H), Sense::hover());
            let mut match_ui = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(match_rect)
                    .layout(egui::Layout::left_to_right(egui::Align::Center)),
            );
            match_ui.set_clip_rect(match_ui.clip_rect().intersect(match_rect));
            match_ui.spacing_mut().item_spacing.x = 4.0;
            draw_match_controls(&mut match_ui, rule, dirty);
        });
    });

    // Color chips in the fixed right slot (aligned under header; uneven gaps).
    ui.allocate_new_ui(egui::UiBuilder::new().max_rect(colors_rect), |ui| {
        ui.set_min_size(colors_rect.size());
        ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            color_chip_cell(
                ui,
                "Topic",
                &rule.id,
                "topic",
                &mut rule.style.topic_color,
                dirty,
                force_open_topic_pop,
            );
            ui.add_space(CHIP_GAP_TOPIC_VALUE);
            color_chip_cell(
                ui,
                "Value",
                &rule.id,
                "value",
                &mut rule.style.value_color,
                dirty,
                false,
            );
            ui.add_space(CHIP_GAP_VALUE_BGK);
            color_chip_cell(
                ui,
                "Background",
                &rule.id,
                "bg",
                &mut rule.style.row_bg,
                dirty,
                false,
            );
        });
    });

    ui.allocate_new_ui(egui::UiBuilder::new().max_rect(del_rect), |ui| {
        ui.with_layout(
            egui::Layout::centered_and_justified(egui::Direction::LeftToRight),
            |ui| {
                if icons.tiny(ui, "delete_red", false, "Delete rule").clicked() {
                    *remove = Some(rule_id);
                }
            },
        );
    });
}

fn draw_match_controls(ui: &mut egui::Ui, rule: &mut ColorRule, dirty: &mut bool) {
    let mut kind = match_kind(&rule.match_rule);
    // Kind selector capped; value/path fields fill whatever remains.
    let kind_w = 132.0_f32.min(ui.available_width().max(72.0));
    egui::ComboBox::from_id_salt(("cr_kind", &rule.id))
        .selected_text(kind_label(kind))
        .width(kind_w)
        .show_ui(ui, |ui| {
            for (k, lab) in MATCH_KINDS {
                if ui.selectable_label(kind == *k, *lab).clicked() {
                    kind = k;
                    *dirty = true;
                }
            }
        });
    if kind != match_kind(&rule.match_rule) {
        rule.match_rule = default_pred(kind);
        *dirty = true;
    }
    match &mut rule.match_rule {
        MatchPred::TopicEquals { value } => {
            text_field(ui, value, "plant/line1/alarm", flex_w(ui), dirty);
        }
        MatchPred::TopicContains { value } => {
            text_field(ui, value, "alarm", flex_w(ui), dirty);
        }
        MatchPred::TopicMatches { pattern } => {
            if ui
                .add(
                    egui::TextEdit::singleline(pattern)
                        .desired_width(flex_w(ui))
                        .hint_text("*alarm* or plant/+/temp"),
                )
                .on_hover_text("Glob * / ? or MQTT + / #")
                .changed()
            {
                *dirty = true;
            }
        }
        MatchPred::PayloadEquals { value } => {
            text_field(ui, value, "ALARM", flex_w(ui), dirty);
        }
        MatchPred::PayloadContains { value } => {
            text_field(ui, value, "fault", flex_w(ui), dirty);
        }
        MatchPred::PayloadNumber { op, value } => {
            op_combo(ui, ("cr_payload_op", rule.id.as_str()), op, dirty);
            if ui.add(egui::DragValue::new(value).speed(0.5)).changed() {
                *dirty = true;
            }
        }
        MatchPred::JsonPathEquals { path, value } => {
            let (path_w, val_w) = split_pair(ui.available_width(), 0.55);
            path_field(ui, path, path_w, dirty);
            let mut s = value_as_edit(value);
            if ui
                .add(
                    egui::TextEdit::singleline(&mut s)
                        .desired_width(val_w)
                        .hint_text("Line-A"),
                )
                .on_hover_text("Exact value at that field (string / number / bool)")
                .changed()
            {
                *value = parse_value_edit(&s);
                *dirty = true;
            }
        }
        MatchPred::JsonPathContains { path, value } => {
            let (path_w, val_w) = split_pair(ui.available_width(), 0.55);
            path_field(ui, path, path_w, dirty);
            if ui
                .add(
                    egui::TextEdit::singleline(value)
                        .desired_width(val_w)
                        .hint_text("north"),
                )
                .on_hover_text("Case-insensitive substring of the nested field")
                .changed()
            {
                *dirty = true;
            }
        }
        MatchPred::JsonPathNumber { path, op, value } => {
            // Path takes leftover after the compact op + drag widgets.
            let path_w = (ui.available_width() - 48.0 - 56.0 - 8.0).max(40.0);
            path_field(ui, path, path_w, dirty);
            op_combo(ui, ("cr_op", rule.id.as_str()), op, dirty);
            if ui.add(egui::DragValue::new(value).speed(0.5)).changed() {
                *dirty = true;
            }
        }
        MatchPred::JsonPathTruthy { path } => {
            path_field(ui, path, flex_w(ui), dirty);
        }
    }
}

/// Fill the remaining clipped match slot (no preferred-width cap).
fn flex_w(ui: &egui::Ui) -> f32 {
    ui.available_width().max(24.0)
}

/// Split leftover width across path + value (fraction for path).
fn split_pair(avail: f32, path_frac: f32) -> (f32, f32) {
    let gap = 4.0;
    let usable = (avail - gap).max(48.0);
    let path_w = (usable * path_frac).max(32.0);
    let val_w = (usable - path_w).max(32.0);
    (path_w, val_w)
}

/// Nested field path — dotted keys, optional `$.`, array `foo[0]` / `foo.0`.
fn path_field(ui: &mut egui::Ui, path: &mut String, width: f32, dirty: &mut bool) {
    if ui
        .add(
            egui::TextEdit::singleline(path)
                .desired_width(width.max(24.0))
                .hint_text("meta.channel"),
        )
        .on_hover_text("Nested JSON field: meta.channel · channels[0].name · $.data.ok")
        .changed()
    {
        *dirty = true;
    }
}

/// Chip only (label lives in the column header); fixed width for alignment.
fn color_chip_cell(
    ui: &mut egui::Ui,
    label: &str,
    rule_id: &str,
    slot_key: &str,
    slot: &mut Option<[u8; 4]>,
    dirty: &mut bool,
    force_open: bool,
) {
    ui.allocate_ui_with_layout(
        egui::vec2(COL_CHIP, ROW_H),
        egui::Layout::centered_and_justified(egui::Direction::LeftToRight),
        |ui| {
            let popup_id = Id::new(("cr_color_pop", rule_id, slot_key));
            let tip = match slot {
                Some([r, g, b, a]) => {
                    format!("{label} color · rgba({r},{g},{b},{a}) — click to change")
                }
                None => format!("{label} color · unset — click to pick"),
            };
            let resp = paint_color_chip(ui, slot.as_ref()).on_hover_text(tip);
            if resp.clicked() {
                ui.memory_mut(|mem| mem.toggle_popup(popup_id));
            } else if force_open {
                ui.memory_mut(|mem| mem.open_popup(popup_id));
            }
            // CloseOnClickOutside: Esc / click-away dismiss; swatch/clear close explicitly.
            egui::popup::popup_below_widget(
                ui,
                popup_id,
                &resp,
                egui::popup::PopupCloseBehavior::CloseOnClickOutside,
                |ui| {
                    // Wide enough for all presets on one row (17 × ~22px).
                    ui.set_min_width((PRESETS.len() as f32) * (SWATCH_W + 4.0) + 16.0);
                    color_swatch_popover(ui, slot, dirty);
                },
            );
        },
    );
}

fn paint_color_chip(ui: &mut egui::Ui, color: Option<&[u8; 4]>) -> egui::Response {
    let (rect, resp) =
        ui.allocate_exact_size(egui::vec2(CHIP_SIZE, CHIP_SIZE), egui::Sense::click());
    let painter = ui.painter();
    paint_checker(painter, rect, 4.0);
    if let Some([r, g, b, a]) = color {
        painter.rect_filled(
            rect,
            2.0,
            Color32::from_rgba_unmultiplied(*r, *g, *b, *a),
        );
    }
    painter.rect_stroke(
        rect,
        2.0,
        egui::Stroke::new(1.0_f32, Color32::from_rgb(90, 96, 108)),
        egui::StrokeKind::Outside,
    );
    resp
}

fn paint_checker(painter: &egui::Painter, rect: egui::Rect, cell: f32) {
    let light = Color32::from_rgb(48, 52, 60);
    let dark = Color32::from_rgb(28, 30, 36);
    painter.rect_filled(rect, 2.0, dark);
    let mut y = rect.top();
    let mut row = 0i32;
    while y < rect.bottom() - 0.5 {
        let y1 = (y + cell).min(rect.bottom());
        let mut x = rect.left();
        let mut col = 0i32;
        while x < rect.right() - 0.5 {
            let x1 = (x + cell).min(rect.right());
            if (row + col) % 2 == 0 {
                painter.rect_filled(
                    egui::Rect::from_min_max(Pos2::new(x, y), Pos2::new(x1, y1)),
                    0.0,
                    light,
                );
            }
            x = x1;
            col += 1;
        }
        y = y1;
        row += 1;
    }
}

/// Popover body: hue/grey gradient swatches + clear for this slot.
fn color_swatch_popover(ui: &mut egui::Ui, slot: &mut Option<[u8; 4]>, dirty: &mut bool) {
    // Popup UI is a fresh tree — reapply Profiles Body 14 / Small 12 × zoom.
    icons::apply_pro_panel_chrome(ui);
    ui.horizontal(|ui| {
        ui.label(
            RichText::new("Pick color")
                .text_style(TextStyle::Body)
                .color(CHROME_MUTED),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .button(RichText::new("Clear").text_style(TextStyle::Body))
                .on_hover_text("Remove color for this slot")
                .clicked()
            {
                *slot = None;
                *dirty = true;
                ui.memory_mut(|mem| mem.close_popup());
            }
        });
    });
    ui.add_space(4.0);
    // Force one horizontal row — never wrap/stack swatches.
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        ui.set_min_width((PRESETS.len() as f32) * (SWATCH_W + 4.0));
        for (name, rgb) in PRESETS {
            let selected = slot
                .as_ref()
                .is_some_and(|c| c[0] == rgb[0] && c[1] == rgb[1] && c[2] == rgb[2]);
            let (rect, resp) =
                ui.allocate_exact_size(egui::vec2(SWATCH_W, SWATCH_H), egui::Sense::click());
            paint_gradient_swatch(ui, rect, *rgb, selected, slot.as_ref().filter(|_| selected));
            let tip = format!("{name} — click top=opaque, bottom=faded");
            let click_pos = resp.interact_pointer_pos();
            if resp.on_hover_text(tip).clicked() {
                if let Some(pos) = click_pos {
                    let alpha = alpha_from_y(rect, pos.y);
                    *slot = Some([rgb[0], rgb[1], rgb[2], alpha]);
                    *dirty = true;
                    ui.memory_mut(|mem| mem.close_popup());
                }
            }
        }
    });
}

/// Vertical gradient: top fully opaque → bottom near-transparent over dark base.
fn paint_gradient_swatch(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    rgb: [u8; 3],
    selected: bool,
    current: Option<&[u8; 4]>,
) {
    let painter = ui.painter();
    // Dark base so faded end of the gradient stays readable.
    painter.rect_filled(rect, 2.0, Color32::from_rgb(22, 24, 28));
    const STEPS: i32 = 24;
    for i in 0..STEPS {
        let t0 = i as f32 / STEPS as f32;
        let t1 = (i + 1) as f32 / STEPS as f32;
        let a0 = alpha_at_t(t0);
        let y0 = egui::lerp(rect.top()..=rect.bottom(), t0);
        let y1 = egui::lerp(rect.top()..=rect.bottom(), t1);
        let slice = egui::Rect::from_min_max(
            Pos2::new(rect.left(), y0),
            Pos2::new(rect.right(), y1),
        );
        painter.rect_filled(
            slice,
            0.0,
            Color32::from_rgba_unmultiplied(rgb[0], rgb[1], rgb[2], a0),
        );
    }
    painter.rect_stroke(
        rect,
        2.0,
        egui::Stroke::new(
            if selected { 1.5_f32 } else { 0.5_f32 },
            if selected {
                Color32::WHITE
            } else {
                Color32::from_rgb(70, 74, 84)
            },
        ),
        egui::StrokeKind::Outside,
    );
    // Tick at current alpha Y when this hue is selected.
    if let Some([_, _, _, a]) = current {
        let t = 1.0
            - ((*a as f32 - ALPHA_FLOOR as f32) / (255.0 - ALPHA_FLOOR as f32)).clamp(0.0, 1.0);
        let y = egui::lerp(rect.top()..=rect.bottom(), t);
        painter.hline(
            rect.left()..=rect.right(),
            y,
            egui::Stroke::new(1.0_f32, Color32::WHITE),
        );
    }
}

fn alpha_from_y(rect: egui::Rect, y: f32) -> u8 {
    let t = ((y - rect.top()) / rect.height()).clamp(0.0, 1.0);
    alpha_at_t(t)
}

fn alpha_at_t(t: f32) -> u8 {
    // t=0 top → opaque; t=1 bottom → ALPHA_FLOOR
    let a = 255.0 - t * (255.0 - ALPHA_FLOOR as f32);
    a.round().clamp(ALPHA_FLOOR as f32, 255.0) as u8
}

/// Editor combo entries (id, label) — topic · whole payload · nested field.
const MATCH_KINDS: &[(&str, &str)] = &[
    ("teq", "topic equals"),
    ("tcontains", "topic contains"),
    ("tmatch", "topic matches"),
    ("eq", "payload equals"),
    ("contains", "payload contains"),
    ("pnum", "payload number"),
    ("jseq", "field equals"),
    ("jscontains", "field contains"),
    ("jsnum", "field number"),
    ("jstruth", "field truthy"),
];

const NUM_OPS: &[&str] = &["<", "<=", ">", ">=", "=="];

fn match_kind(p: &MatchPred) -> &'static str {
    match p {
        MatchPred::TopicEquals { .. } => "teq",
        MatchPred::TopicContains { .. } => "tcontains",
        MatchPred::TopicMatches { .. } => "tmatch",
        MatchPred::PayloadEquals { .. } => "eq",
        MatchPred::PayloadContains { .. } => "contains",
        MatchPred::PayloadNumber { .. } => "pnum",
        MatchPred::JsonPathEquals { .. } => "jseq",
        MatchPred::JsonPathContains { .. } => "jscontains",
        MatchPred::JsonPathNumber { .. } => "jsnum",
        MatchPred::JsonPathTruthy { .. } => "jstruth",
    }
}

fn kind_label(k: &str) -> &'static str {
    MATCH_KINDS
        .iter()
        .find(|(id, _)| *id == k)
        .map(|(_, lab)| *lab)
        .unwrap_or("field truthy")
}

fn default_pred(k: &str) -> MatchPred {
    match k {
        "teq" => MatchPred::TopicEquals {
            value: String::new(),
        },
        "tcontains" => MatchPred::TopicContains {
            value: String::new(),
        },
        "tmatch" => MatchPred::TopicMatches {
            pattern: String::new(),
        },
        "eq" => MatchPred::PayloadEquals {
            value: String::new(),
        },
        "contains" => MatchPred::PayloadContains {
            value: String::new(),
        },
        "pnum" => MatchPred::PayloadNumber {
            op: ">".into(),
            value: 0.0,
        },
        "jseq" => MatchPred::JsonPathEquals {
            path: "meta.channel".into(),
            value: serde_json::Value::String(String::new()),
        },
        "jscontains" => MatchPred::JsonPathContains {
            path: "meta.channel".into(),
            value: String::new(),
        },
        "jsnum" => MatchPred::JsonPathNumber {
            path: String::new(),
            op: ">".into(),
            value: 0.0,
        },
        _ => MatchPred::JsonPathTruthy {
            path: String::new(),
        },
    }
}

fn text_field(ui: &mut egui::Ui, value: &mut String, hint: &str, width: f32, dirty: &mut bool) {
    if ui
        .add(
            egui::TextEdit::singleline(value)
                .desired_width(width)
                .hint_text(hint),
        )
        .changed()
    {
        *dirty = true;
    }
}

fn op_combo(ui: &mut egui::Ui, id: (&str, &str), op: &mut String, dirty: &mut bool) {
    egui::ComboBox::from_id_salt(id)
        .selected_text(op.as_str())
        .width(48.0)
        .show_ui(ui, |ui| {
            for o in NUM_OPS {
                if ui.selectable_label(op == *o, *o).clicked() {
                    *op = (*o).into();
                    *dirty = true;
                }
            }
        });
}

fn value_as_edit(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

fn parse_value_edit(s: &str) -> serde_json::Value {
    let t = s.trim();
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(t) {
        return v;
    }
    if let Ok(n) = t.parse::<f64>() {
        return serde_json::json!(n);
    }
    if t.eq_ignore_ascii_case("true") {
        return serde_json::json!(true);
    }
    if t.eq_ignore_ascii_case("false") {
        return serde_json::json!(false);
    }
    serde_json::Value::String(t.to_string())
}

#[allow(dead_code)]
fn _style(_: &RuleStyle) {}
