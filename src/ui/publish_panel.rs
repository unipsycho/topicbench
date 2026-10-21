//! Publish panel: rumqttc publish + Recent history restore.

use crate::mqtt::profile::ProfileStore;
use crate::mqtt::publish::{PublishDraft, PublishHistory, RecentPublish, Qos};
use crate::mqtt::session::{SessionManager, SessionState};
use crate::ui::hover_tip;
use crate::ui::icons::{self, IconSet, TINY_ICON};
use crate::ui::json_view;
use eframe::egui::text::LayoutJob;
use eframe::egui::{self, Color32, Sense, Stroke, TextFormat, UiBuilder};
use std::sync::Arc;

/// Warning chrome when intended JSON is invalid.
const WARN_BORDER: Color32 = Color32::from_rgb(210, 140, 50);
const WARN_BG: Color32 = Color32::from_rgb(55, 42, 28);

pub fn draw(
    ui: &mut egui::Ui,
    draft: &mut PublishDraft,
    history: &mut PublishHistory,
    sessions: &mut SessionManager,
    profiles: &ProfileStore,
    icons: &IconSet,
    status: &mut String,
) {
    icons::section_header_bar(ui, |ui| {
        icons::section_icon(ui, icons, "publish_message");
        icons::section_title(ui, "Publish");
    });
    ui.label("Send to a topic on a connected broker. Double-click to populate.");
    ui.add_space(4.0);

    let connected: Vec<(String, String)> = sessions
        .open
        .values()
        .filter(|s| s.state == SessionState::Connected)
        .map(|s| (s.profile_id.clone(), s.profile_name.clone()))
        .collect();

    if connected.is_empty() {
        ui.weak("No connected profiles.");
    } else {
        egui::ComboBox::from_label("Profile")
            .selected_text(
                draft
                    .profile_id
                    .as_ref()
                    .and_then(|id| connected.iter().find(|(pid, _)| pid == id))
                    .map(|(_, name)| name.as_str())
                    .unwrap_or("Select…"),
            )
            .show_ui(ui, |ui| {
                for (id, name) in &connected {
                    ui.selectable_value(&mut draft.profile_id, Some(id.clone()), name);
                }
            });
    }

    ui.horizontal(|ui| {
        ui.label("Topic");
        // Match topic-tree leaf typography (monospace, normal weight).
        ui.add(
            egui::TextEdit::singleline(&mut draft.topic)
                .font(egui::TextStyle::Monospace)
                .desired_width(ui.available_width().max(120.0)),
        );
    });

    // Payload: beautified + syntax-highlighted when JSON; warn if intended JSON is broken.
    ui.label("Payload");
    let json_invalid = json_view::json_edit_invalid(&draft.payload);
    draw_payload_editor(ui, &mut draft.payload, json_invalid);
    if json_invalid {
        ui.colored_label(
            WARN_BORDER,
            "Invalid JSON — fix before publishing (plain text is fine).",
        );
    }

    ui.horizontal(|ui| {
        ui.label("QoS");
        ui.selectable_value(&mut draft.qos, Qos::AtMostOnce, "0");
        ui.selectable_value(&mut draft.qos, Qos::AtLeastOnce, "1");
        ui.selectable_value(&mut draft.qos, Qos::ExactlyOnce, "2");
        ui.checkbox(&mut draft.retain, "Retain");
    });

    // Same chrome as Profiles Connect/Disconnect — icon + label.
    let publish_clicked = if let Some(tex) = icons.get("publish_message") {
        ui.add(
            egui::Button::image_and_text(
                egui::Image::new(tex).fit_to_exact_size(egui::vec2(TINY_ICON, TINY_ICON)),
                "Publish",
            )
            .min_size(egui::vec2(120.0, 28.0)),
        )
        .clicked()
    } else {
        ui.add(egui::Button::new("Publish").min_size(egui::vec2(120.0, 28.0)))
            .clicked()
    };
    if publish_clicked {
        if json_view::json_edit_invalid(&draft.payload) {
            *status = "Publish blocked: payload looks like JSON but is invalid — fix or clear braces.".into();
        } else {
            match draft.profile_id.as_deref() {
                Some(id) => {
                    match sessions.publish_live(id, &draft.topic, &draft.payload, draft.qos, draft.retain)
                    {
                        Ok(()) => {
                            let broker = connected
                                .iter()
                                .find(|(pid, _)| pid == id)
                                .map(|(_, name)| name.as_str())
                                .unwrap_or(id);
                            let entry = RecentPublish::from_draft(draft, broker);
                            match history.push_and_save(entry) {
                                Ok(()) => {
                                    *status = format!(
                                        "Published → {broker} {} (qos={} retain={})",
                                        draft.topic,
                                        draft.qos.as_u8(),
                                        draft.retain
                                    );
                                }
                                Err(e) => {
                                    *status = format!(
                                        "Published → {broker} {} (history save failed: {e})",
                                        draft.topic
                                    );
                                }
                            }
                        }
                        Err(e) => *status = e,
                    }
                }
                None => *status = "Select a connected profile to publish.".into(),
            }
        }
    }

    ui.add_space(8.0);
    ui.separator();
    ui.add_space(4.0);
    // Match Profiles “Edit” subsection chrome.
    icons::section_header_bar(ui, |ui| {
        icons::section_title(ui, "Recent");
    });
    ui.weak(format!(
        "Last {} publishes — click to restore form.",
        crate::mqtt::publish::RECENT_CAP
    ));

    if history.recent.is_empty() {
        ui.weak("No recent publishes yet.");
        return;
    }

    // Claim leftover SidePanel height permanently (allocate_exact_size), then fill it.
    // Prior attempt only set ScrollArea::max_height(available_height) — that still left a
    // short list when tip binaries kept max_height(160), and max_height alone does not
    // reserve space in the parent placer the way allocate_exact_size does.
    let remaining = ui.available_size();
    let (rect, _) = ui.allocate_exact_size(remaining, Sense::hover());
    ui.allocate_new_ui(UiBuilder::new().max_rect(rect), |ui| {
        egui::ScrollArea::vertical()
            .id_salt("publish-recent")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for (i, entry) in history.recent.iter().enumerate() {
                    let accent = profiles
                        .profiles
                        .get(&entry.profile_id)
                        .map(|p| p.color())
                        .unwrap_or(Color32::from_rgb(90, 190, 120));
                    let job = recent_row_job(ui, entry, accent);
                    if hover_tip::attach(
                        ui.selectable_label(false, job),
                        format!(
                            "broker={} topic={} qos={} retain={}\n{}",
                            entry.profile_name,
                            entry.topic,
                            entry.qos.as_u8(),
                            entry.retain,
                            entry.payload
                        ),
                    )
                    .clicked()
                    {
                        draft.apply_recent(entry);
                        *status = format!(
                            "Restored recent #{}: {} · {}",
                            i + 1,
                            entry.profile_name,
                            entry.topic
                        );
                    }
                }
            });
    });
}

/// Leave room below the editor for QoS, Publish, separator, Recent header, and a usable list.
const RESERVE_BELOW_PAYLOAD: f32 = 240.0;
/// Floor so a short payload still shows ~8 monospace rows.
const PAYLOAD_MIN_H: f32 = 120.0;

/// Editable multiline payload with JSON syntax highlight + invalid warning frame.
/// Cap height to leftover panel space (minus reserve for controls/Recent) and scroll inside.
fn draw_payload_editor(ui: &mut egui::Ui, payload: &mut String, json_invalid: bool) {
    let font_size = ui.text_style_height(&egui::TextStyle::Monospace);
    let mut layouter = |ui: &egui::Ui, text: &str, wrap_width: f32| -> Arc<egui::Galley> {
        let mut job = if json_view::looks_like_json(text) {
            json_view::highlight_job(text, font_size)
        } else {
            json_view::plain_mono_job(text, font_size)
        };
        job.wrap.max_width = wrap_width;
        ui.fonts(|f| f.layout_job(job))
    };

    let frame = if json_invalid {
        egui::Frame::NONE
            .fill(WARN_BG)
            .stroke(Stroke::new(1.5_f32, WARN_BORDER))
            .inner_margin(egui::Margin::same(4))
            .corner_radius(4.0)
    } else {
        egui::Frame::NONE
            .inner_margin(egui::Margin::same(2))
            .corner_radius(4.0)
    };

    // TextEdit grows with galley height; ScrollArea max_height keeps it in-viewport.
    // Recent still claims whatever is left after QoS/Publish via allocate_exact_size.
    let max_h = (ui.available_height() - RESERVE_BELOW_PAYLOAD).max(PAYLOAD_MIN_H);

    frame.show(ui, |ui| {
        egui::ScrollArea::vertical()
            .id_salt("publish_payload_scroll")
            .max_height(max_h)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                ui.add(
                    egui::TextEdit::multiline(payload)
                        .id_salt("publish_payload")
                        .desired_width(ui.available_width())
                        .desired_rows(8)
                        .font(egui::TextStyle::Monospace)
                        .layouter(&mut layouter),
                );
            });
    });
}

/// Broker in profile accent; topic bright (primary scan); qos/payload muted.
fn recent_row_job(ui: &egui::Ui, entry: &RecentPublish, accent: Color32) -> LayoutJob {
    let broker = if entry.profile_name.is_empty() {
        "…"
    } else {
        entry.profile_name.as_str()
    };
    let body = ui.style().text_styles[&egui::TextStyle::Body].clone();
    let mono = ui.style().text_styles[&egui::TextStyle::Monospace].clone();
    let muted = ui.visuals().weak_text_color();
    // Bright topic — stands out vs muted qos/payload (tree C_TOPIC is too quiet here).
    const TOPIC: Color32 = Color32::from_rgb(210, 225, 255);
    let mut job = LayoutJob::default();
    job.append(
        broker,
        0.0,
        TextFormat {
            font_id: body,
            color: accent,
            ..Default::default()
        },
    );
    job.append(
        " · ",
        0.0,
        TextFormat {
            font_id: mono.clone(),
            color: muted,
            ..Default::default()
        },
    );
    job.append(
        &entry.topic,
        0.0,
        TextFormat {
            font_id: mono.clone(),
            color: TOPIC,
            ..Default::default()
        },
    );
    job.append(
        &format!(
            "  qos={}{}  {}",
            entry.qos.as_u8(),
            if entry.retain { " retain" } else { "" },
            entry.payload_preview()
        ),
        0.0,
        TextFormat {
            font_id: mono,
            color: muted,
            ..Default::default()
        },
    );
    job
}
