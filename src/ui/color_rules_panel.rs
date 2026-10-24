//! Color rules editor (Pro) — ordered fixed predicates; live tree paint.
//! Opens as a real OS viewport (Payload undock pattern).

use crate::pro::color_rules::{ColorRule, ColorRulesStore, MatchPred, RuleStyle};
use crate::ui::icons::{self, IconSet, CHROME_BG, CHROME_MUTED};
use eframe::egui::{self, Color32, RichText, ViewportBuilder, ViewportClass};

const PRESETS: &[(&str, [u8; 3])] = &[
    ("Red", [220, 70, 70]),
    ("Amber", [220, 160, 50]),
    ("Green", [70, 180, 100]),
    ("Blue", [70, 140, 220]),
    ("Magenta", [200, 80, 180]),
];

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
            .with_inner_size([580.0, 420.0])
            .with_min_inner_size([400.0, 260.0]),
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
    ui.horizontal(|ui| {
        ui.label(RichText::new("colors").small().color(CHROME_MUTED));
        color_swatches(ui, "topic", &mut rule.style.topic_color, dirty);
        color_swatches(ui, "value", &mut rule.style.value_color, dirty);
        color_swatches(ui, "bg", &mut rule.style.row_bg, dirty);
    });
}

fn color_swatches(
    ui: &mut egui::Ui,
    label: &str,
    slot: &mut Option<[u8; 3]>,
    dirty: &mut bool,
) {
    ui.label(RichText::new(label).small());
    for (name, rgb) in PRESETS {
        let selected = slot.as_ref() == Some(rgb);
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::click());
        ui.painter()
            .rect_filled(rect, 2.0, Color32::from_rgb(rgb[0], rgb[1], rgb[2]));
        if selected {
            ui.painter().rect_stroke(
                rect,
                2.0,
                egui::Stroke::new(1.5_f32, Color32::WHITE),
                egui::StrokeKind::Outside,
            );
        }
        if resp.on_hover_text(*name).clicked() {
            *slot = Some(*rgb);
            *dirty = true;
        }
    }
    if ui.small_button("×").on_hover_text("Clear").clicked() {
        *slot = None;
        *dirty = true;
    }
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
