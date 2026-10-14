//! Publish panel: rumqttc publish + Recent history restore.

use crate::mqtt::profile::ProfileStore;
use crate::mqtt::publish::{PublishDraft, PublishHistory, RecentPublish, Qos};
use crate::mqtt::session::{SessionManager, SessionState};
use crate::ui::hover_tip;
use crate::ui::icons::IconSet;
use crate::ui::json_view;
use eframe::egui::text::LayoutJob;
use eframe::egui::{self, Color32, Stroke, TextFormat};
use std::sync::Arc;

/// Warning chrome when intended JSON is invalid.
const WARN_BORDER: Color32 = Color32::from_rgb(210, 140, 50);
const WARN_BG: Color32 = Color32::from_rgb(55, 42, 28);

pub fn draw(
    ui: &mut egui::Ui,
    draft: &mut PublishDraft,
    history: &mut PublishHistory,
    sessions: &SessionManager,
    profiles: &ProfileStore,
    icons: &IconSet,
    status: &mut String,
) {
    ui.horizontal(|ui| {
        if let Some(tex) = icons.get("publish_message") {
            ui.add(
                egui::Image::new(tex)
                    .fit_to_exact_size(egui::vec2(20.0, 20.0))
                    .sense(egui::Sense::hover()),
            );
        }
        ui.heading("Publish");
    });
    ui.label("Send to a topic on a connected broker.");
    ui.weak("Double-click a topic in the tree to fill profile, topic, and payload.");
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

    if ui.button("Publish").clicked() {
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

    ui.add_space(10.0);
    ui.separator();
    ui.strong("Recent");
    ui.weak(format!(
        "Last {} publishes — click to restore form.",
        crate::mqtt::publish::RECENT_CAP
    ));

    if history.recent.is_empty() {
        ui.weak("No recent publishes yet.");
        return;
    }

    egui::ScrollArea::vertical()
        .id_salt("publish-recent")
        .max_height(160.0)
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
}

/// Editable multiline payload with JSON syntax highlight + invalid warning frame.
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

    frame.show(ui, |ui| {
        ui.add(
            egui::TextEdit::multiline(payload)
                .id_salt("publish_payload")
                .desired_width(ui.available_width())
                .desired_rows(8)
                .font(egui::TextStyle::Monospace)
                .layouter(&mut layouter),
        );
    });
}

/// Broker name in profile accent; rest of the row at normal body/monospace size.
fn recent_row_job(ui: &egui::Ui, entry: &RecentPublish, accent: Color32) -> LayoutJob {
    let broker = if entry.profile_name.is_empty() {
        "…"
    } else {
        entry.profile_name.as_str()
    };
    let body = ui.style().text_styles[&egui::TextStyle::Body].clone();
    let mono = ui.style().text_styles[&egui::TextStyle::Monospace].clone();
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
        &format!(
            " · {}  qos={}{}  {}",
            entry.topic,
            entry.qos.as_u8(),
            if entry.retain { " retain" } else { "" },
            entry.payload_preview()
        ),
        0.0,
        TextFormat {
            font_id: mono,
            color: ui.visuals().text_color(),
            ..Default::default()
        },
    );
    job
}
