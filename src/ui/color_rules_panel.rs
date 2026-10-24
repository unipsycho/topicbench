//! Color Rules editor (Pro) — ordered fixed predicates; live tree paint.
//! Opens as a real OS viewport (Payload undock pattern).
//! Each rule is a dense inline row with Topic / Value / Background color chips;
//! click a chip for a swatch popover (hue + alpha).

use crate::pro::color_rules::{ColorRule, ColorRulesStore, MatchPred, RuleStyle};
use crate::ui::icons::{self, IconSet, CHROME_BG, CHROME_MUTED};
use eframe::egui::{self, Color32, Id, Pos2, RichText, ViewportBuilder, ViewportClass};

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
) {
    if !*open {
        return;
    }
    let mut close = false;
    // OS window — draggable outside the main TopicBench frame.
    ctx.show_viewport_immediate(
        egui::ViewportId::from_hash_of("topicbench_color_rules_vp"),
        ViewportBuilder::default()
            .with_title("Color Rules")
            .with_inner_size([720.0, 420.0])
            .with_min_inner_size([520.0, 240.0]),
        |ctx, class| {
            let mut paint = |ui: &mut egui::Ui| {
                icons::section_header_bar(ui, |ui| {
                    icons::section_icon(ui, icons, "color_rules");
                    icons::section_title(ui, "Color Rules");
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if icons
                            .tiny(ui, "remove_x", false, "Close Color Rules")
                            .clicked()
                        {
                            close = true;
                        }
                        if ui.button("+ Add rule").clicked() {
                            match store.add_blank() {
                                Ok(()) => *status = "Added color rule".into(),
                                Err(e) => *status = e,
                            }
                        }
                    });
                });
                // Subtitle under header — body size, not cramped header chrome.
                ui.add_space(4.0);
                ui.label(
                    RichText::new("First match wins · leaves with payloads").color(CHROME_MUTED),
                );
                ui.add_space(2.0);
                ui.separator();

                let mut remove: Option<String> = None;
                let mut dirty = false;
                // Agent/screenshot helper: open first Topic chip popover once.
                let force_pop = std::env::var_os("TOPICBENCH_COLOR_RULES_OPEN_POPOVER").is_some();
                let n = store.rules.len();
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for i in 0..n {
                        ui.group(|ui| {
                            draw_rule_row(
                                ui,
                                &mut store.rules[i],
                                icons,
                                &mut remove,
                                &mut dirty,
                                force_pop && i == 0,
                            );
                        });
                        ui.add_space(2.0);
                    }
                });
                if let Some(id) = remove {
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
        },
    );
    if close {
        *open = false;
    }
}

fn draw_rule_row(
    ui: &mut egui::Ui,
    rule: &mut ColorRule,
    icons: &IconSet,
    remove: &mut Option<String>,
    dirty: &mut bool,
    force_open_topic_pop: bool,
) {
    // One dense row: enable · name · Topic scope · Match · Topic/Value/Background chips · trash
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        if ui.checkbox(&mut rule.enabled, "").changed() {
            *dirty = true;
        }
        if ui
            .add(
                egui::TextEdit::singleline(&mut rule.name)
                    .desired_width(100.0)
                    .hint_text("Name"),
            )
            .changed()
        {
            *dirty = true;
        }
        // Column labels use normal panel body size (not .small()).
        ui.label(RichText::new("Topic").color(CHROME_MUTED));
        if ui
            .add(
                egui::TextEdit::singleline(&mut rule.topic_glob)
                    .desired_width(110.0)
                    .hint_text("glob #"),
            )
            .changed()
        {
            *dirty = true;
        }

        ui.label(RichText::new("Match").color(CHROME_MUTED));
        draw_match_controls(ui, rule, dirty);

        ui.add_space(6.0);
        color_chip_column(
            ui,
            "Topic",
            &rule.id,
            "topic",
            &mut rule.style.topic_color,
            dirty,
            force_open_topic_pop,
        );
        color_chip_column(
            ui,
            "Value",
            &rule.id,
            "value",
            &mut rule.style.value_color,
            dirty,
            false,
        );
        color_chip_column(
            ui,
            "Background",
            &rule.id,
            "bg",
            &mut rule.style.row_bg,
            dirty,
            false,
        );

        ui.add_space(4.0);
        if icons.tiny(ui, "trash", false, "Delete rule").clicked() {
            *remove = Some(rule.id.clone());
        }
    });
}

fn draw_match_controls(ui: &mut egui::Ui, rule: &mut ColorRule, dirty: &mut bool) {
    let mut kind = match_kind(&rule.match_rule);
    egui::ComboBox::from_id_salt(("cr_kind", &rule.id))
        .selected_text(kind_label(kind))
        .width(148.0)
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
            text_field(ui, value, "plant/line1/alarm", 140.0, dirty);
        }
        MatchPred::TopicContains { value } => {
            text_field(ui, value, "alarm", 120.0, dirty);
        }
        MatchPred::TopicMatches { pattern } => {
            if ui
                .add(
                    egui::TextEdit::singleline(pattern)
                        .desired_width(150.0)
                        .hint_text("*alarm* or plant/+/temp"),
                )
                .on_hover_text("Glob * / ? or MQTT + / #")
                .changed()
            {
                *dirty = true;
            }
        }
        MatchPred::PayloadEquals { value } => {
            text_field(ui, value, "ALARM", 100.0, dirty);
        }
        MatchPred::PayloadContains { value } => {
            text_field(ui, value, "fault", 100.0, dirty);
        }
        MatchPred::PayloadNumber { op, value } => {
            op_combo(ui, ("cr_payload_op", rule.id.as_str()), op, dirty);
            if ui.add(egui::DragValue::new(value).speed(0.5)).changed() {
                *dirty = true;
            }
        }
        MatchPred::JsonPathEquals { path, value } => {
            text_field(ui, path, "path", 70.0, dirty);
            let mut s = value_as_edit(value);
            if ui
                .add(egui::TextEdit::singleline(&mut s).desired_width(70.0))
                .changed()
            {
                *value = parse_value_edit(&s);
                *dirty = true;
            }
        }
        MatchPred::JsonPathNumber { path, op, value } => {
            text_field(ui, path, "path", 60.0, dirty);
            op_combo(ui, ("cr_op", rule.id.as_str()), op, dirty);
            if ui.add(egui::DragValue::new(value).speed(0.5)).changed() {
                *dirty = true;
            }
        }
        MatchPred::JsonPathTruthy { path } => {
            text_field(ui, path, "path", 90.0, dirty);
        }
    }
}

/// Label + current-color chip; click opens the swatch popover for that slot.
fn color_chip_column(
    ui: &mut egui::Ui,
    label: &str,
    rule_id: &str,
    slot_key: &str,
    slot: &mut Option<[u8; 4]>,
    dirty: &mut bool,
    force_open: bool,
) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 3.0;
        ui.label(RichText::new(label).color(CHROME_MUTED));
        let popup_id = Id::new(("cr_color_pop", rule_id, slot_key));
        let tip = match slot {
            Some([r, g, b, a]) => format!("{label} color · rgba({r},{g},{b},{a}) — click to change"),
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
    });
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
    ui.horizontal(|ui| {
        ui.label(RichText::new("Pick color").small().color(CHROME_MUTED));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .small_button("Clear")
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

/// Editor combo entries (id, label) — topic · payload · JSON.
const MATCH_KINDS: &[(&str, &str)] = &[
    ("teq", "topic equals"),
    ("tcontains", "topic contains"),
    ("tmatch", "topic matches"),
    ("eq", "payload equals"),
    ("contains", "payload contains"),
    ("pnum", "payload number"),
    ("jseq", "json path equals"),
    ("jsnum", "json path number"),
    ("jstruth", "json path truthy"),
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
        MatchPred::JsonPathNumber { .. } => "jsnum",
        MatchPred::JsonPathTruthy { .. } => "jstruth",
    }
}

fn kind_label(k: &str) -> &'static str {
    MATCH_KINDS
        .iter()
        .find(|(id, _)| *id == k)
        .map(|(_, lab)| *lab)
        .unwrap_or("json path truthy")
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
            path: String::new(),
            value: serde_json::Value::String(String::new()),
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
