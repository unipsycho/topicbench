//! Color rules editor (Pro) — ordered fixed predicates; live tree paint.

use crate::pro::color_rules::{ColorRule, ColorRulesStore, MatchPred, RuleStyle};
use crate::ui::icons::{IconSet, CHROME_MUTED, CHROME_TEXT};
use eframe::egui::{self, Color32, RichText};

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
    egui::Window::new("Color rules")
        .id(egui::Id::new("topicbench_color_rules"))
        .collapsible(false)
        .resizable(true)
        .default_width(480.0)
        .default_height(360.0)
        .open(open)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Color rules").strong().color(CHROME_TEXT));
                ui.label(
                    RichText::new("First match wins · leaves with payloads")
                        .small()
                        .color(CHROME_MUTED),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
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
        });
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
            .show_ui(ui, |ui| {
                for (k, lab) in [
                    ("eq", "payload equals"),
                    ("contains", "payload contains"),
                    ("jseq", "json path equals"),
                    ("jsnum", "json path number"),
                    ("jstruth", "json path truthy"),
                ] {
                    if ui.selectable_label(kind == k, lab).clicked() {
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
            MatchPred::PayloadEquals { value }
            | MatchPred::PayloadContains { value } => {
                if ui
                    .add(egui::TextEdit::singleline(value).desired_width(120.0))
                    .changed()
                {
                    *dirty = true;
                }
            }
            MatchPred::JsonPathEquals { path, value } => {
                if ui
                    .add(
                        egui::TextEdit::singleline(path)
                            .desired_width(80.0)
                            .hint_text("path"),
                    )
                    .changed()
                {
                    *dirty = true;
                }
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
                if ui
                    .add(
                        egui::TextEdit::singleline(path)
                            .desired_width(70.0)
                            .hint_text("path"),
                    )
                    .changed()
                {
                    *dirty = true;
                }
                egui::ComboBox::from_id_salt(("cr_op", &rule.id))
                    .selected_text(op.as_str())
                    .width(48.0)
                    .show_ui(ui, |ui| {
                        for o in ["<", "<=", ">", ">=", "=="] {
                            if ui.selectable_label(op == o, o).clicked() {
                                *op = o.into();
                                *dirty = true;
                            }
                        }
                    });
                if ui
                    .add(egui::DragValue::new(value).speed(0.5))
                    .changed()
                {
                    *dirty = true;
                }
            }
            MatchPred::JsonPathTruthy { path } => {
                if ui
                    .add(
                        egui::TextEdit::singleline(path)
                            .desired_width(100.0)
                            .hint_text("path"),
                    )
                    .changed()
                {
                    *dirty = true;
                }
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

fn match_kind(p: &MatchPred) -> &'static str {
    match p {
        MatchPred::PayloadEquals { .. } => "eq",
        MatchPred::PayloadContains { .. } => "contains",
        MatchPred::JsonPathEquals { .. } => "jseq",
        MatchPred::JsonPathNumber { .. } => "jsnum",
        MatchPred::JsonPathTruthy { .. } => "jstruth",
    }
}

fn kind_label(k: &str) -> &'static str {
    match k {
        "eq" => "payload equals",
        "contains" => "payload contains",
        "jseq" => "json path equals",
        "jsnum" => "json path number",
        _ => "json path truthy",
    }
}

fn default_pred(k: &str) -> MatchPred {
    match k {
        "eq" => MatchPred::PayloadEquals {
            value: String::new(),
        },
        "contains" => MatchPred::PayloadContains {
            value: String::new(),
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
