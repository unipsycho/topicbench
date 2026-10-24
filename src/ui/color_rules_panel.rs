//! Color rules editor (Pro) — ordered fixed predicates; live tree paint.
//! Opens as a real OS viewport (Payload undock pattern).

use crate::pro::color_rules::{ColorRule, ColorRulesStore, MatchPred, RuleStyle};
use crate::ui::icons::{self, IconSet, CHROME_BG, CHROME_MUTED};
use eframe::egui::{self, Color32, Pos2, RichText, ViewportBuilder, ViewportClass};

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

/// Swatch size: tall enough for a readable opaque→transparent gradient.
const SWATCH_W: f32 = 14.0;
const SWATCH_H: f32 = 28.0;
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
            .with_title("Color rules")
            .with_inner_size([640.0, 460.0])
            .with_min_inner_size([480.0, 280.0]),
        |ctx, class| {
            let mut paint = |ui: &mut egui::Ui| {
                icons::section_header_bar(ui, |ui| {
                    icons::section_icon(ui, icons, "alerts");
                    icons::section_title(ui, "Color rules");
                    ui.add_space(8.0);
                    ui.label(
                        RichText::new("First match wins · leaves with payloads")
                            .small()
                            .color(CHROME_MUTED),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if icons
                            .tiny(ui, "remove_x", false, "Close Color rules")
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
                ui.separator();

                let mut remove: Option<String> = None;
                let mut dirty = false;
                let n = store.rules.len();
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for i in 0..n {
                        ui.group(|ui| {
                            draw_rule_row(ui, &mut store.rules[i], icons, &mut remove, &mut dirty);
                        });
                        ui.add_space(4.0);
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
                    egui::Window::new("Color rules")
                        .id(egui::Id::new("topicbench_color_rules"))
                        .title_bar(false)
                        .collapsible(false)
                        .resizable(true)
                        .default_width(480.0)
                        .default_height(360.0)
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
) {
    ui.horizontal(|ui| {
        if ui.checkbox(&mut rule.enabled, "").changed() {
            *dirty = true;
        }
        if ui
            .add(
                egui::TextEdit::singleline(&mut rule.name)
                    .desired_width(120.0)
                    .hint_text("Name"),
            )
            .changed()
        {
            *dirty = true;
        }
        ui.label(RichText::new("topic").small().color(CHROME_MUTED));
        if ui
            .add(
                egui::TextEdit::singleline(&mut rule.topic_glob)
                    .desired_width(140.0)
                    .hint_text("glob #"),
            )
            .changed()
        {
            *dirty = true;
        }
        if icons.tiny(ui, "trash", false, "Delete rule").clicked() {
            *remove = Some(rule.id.clone());
        }
    });
    ui.horizontal(|ui| {
        ui.label(RichText::new("match").small().color(CHROME_MUTED));
        let mut kind = match_kind(&rule.match_rule);
        egui::ComboBox::from_id_salt(("cr_kind", &rule.id))
            .selected_text(kind_label(kind))
            .width(168.0)
            .show_ui(ui, |ui| {
                // Topic · Payload text · Payload number · JSON path
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
                text_field(ui, value, "plant/line1/alarm", 160.0, dirty);
            }
            MatchPred::TopicContains { value } => {
                text_field(ui, value, "alarm", 140.0, dirty);
            }
            MatchPred::TopicMatches { pattern } => {
                if ui
                    .add(
                        egui::TextEdit::singleline(pattern)
                            .desired_width(180.0)
                            .hint_text("*alarm* or plant/+/temp"),
                    )
                    .on_hover_text("Glob * / ? or MQTT + / #")
                    .changed()
                {
                    *dirty = true;
                }
            }
            MatchPred::PayloadEquals { value } => {
                text_field(ui, value, "ALARM", 120.0, dirty);
            }
            MatchPred::PayloadContains { value } => {
                text_field(ui, value, "fault", 120.0, dirty);
            }
            MatchPred::PayloadNumber { op, value } => {
                op_combo(ui, ("cr_payload_op", rule.id.as_str()), op, dirty);
                if ui.add(egui::DragValue::new(value).speed(0.5)).changed() {
                    *dirty = true;
                }
            }
            MatchPred::JsonPathEquals { path, value } => {
                text_field(ui, path, "path", 80.0, dirty);
                let mut s = value_as_edit(value);
                if ui
                    .add(egui::TextEdit::singleline(&mut s).desired_width(80.0))
                    .changed()
                {
                    *value = parse_value_edit(&s);
                    *dirty = true;
                }
            }
            MatchPred::JsonPathNumber { path, op, value } => {
                text_field(ui, path, "path", 70.0, dirty);
                op_combo(ui, ("cr_op", rule.id.as_str()), op, dirty);
                if ui.add(egui::DragValue::new(value).speed(0.5)).changed() {
                    *dirty = true;
                }
            }
            MatchPred::JsonPathTruthy { path } => {
                text_field(ui, path, "path", 100.0, dirty);
            }
        }
    });
    // One horizontal strip per slot; wrap only when the window is too narrow.
    ui.label(RichText::new("colors").small().color(CHROME_MUTED));
    color_swatches(ui, "topic", &mut rule.style.topic_color, dirty);
    color_swatches(ui, "value", &mut rule.style.value_color, dirty);
    color_swatches(ui, "bg", &mut rule.style.row_bg, dirty);
}

fn color_swatches(
    ui: &mut egui::Ui,
    label: &str,
    slot: &mut Option<[u8; 4]>,
    dirty: &mut bool,
) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 3.0;
        ui.label(RichText::new(label).small());
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
                }
            }
        }
        if ui.small_button("×").on_hover_text("Clear").clicked() {
            *slot = None;
            *dirty = true;
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
    const STEPS: i32 = 20;
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
        let t = 1.0 - ((*a as f32 - ALPHA_FLOOR as f32) / (255.0 - ALPHA_FLOOR as f32)).clamp(0.0, 1.0);
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
