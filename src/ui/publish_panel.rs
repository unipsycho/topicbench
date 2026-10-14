//! Publish panel: rumqttc publish + Recent history restore + tree drop target.

use crate::mqtt::profile::ProfileStore;
use crate::mqtt::publish::{PublishDraft, PublishHistory, RecentPublish, TopicPublishDrop, Qos};
use crate::mqtt::session::{SessionManager, SessionState};
use crate::ui::hover_tip;
use crate::ui::icons::IconSet;
use eframe::egui::text::LayoutJob;
use eframe::egui::{self, Color32, TextFormat};

pub fn draw(
    ui: &mut egui::Ui,
    draft: &mut PublishDraft,
    history: &mut PublishHistory,
    sessions: &SessionManager,
    profiles: &ProfileStore,
    icons: &IconSet,
    status: &mut String,
) {
    // Whole panel accepts topic-tree drops (fill broker / topic / payload).
    let frame = egui::Frame::NONE.inner_margin(egui::Margin::ZERO);
    let (_, dropped) = ui.dnd_drop_zone::<TopicPublishDrop, _>(frame, |ui| {
        draw_inner(ui, draft, history, sessions, profiles, icons, status);
    });
    if let Some(drop) = dropped {
        draft.apply_tree_drop(&drop);
        *status = format!(
            "Publish filled from tree: {} · {}",
            drop.profile_name, drop.topic
        );
    }
}

fn draw_inner(
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
    ui.weak("Drop a topic from the tree here to fill profile, topic, and payload.");
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
        ui.text_edit_singleline(&mut draft.topic);
    });
    ui.label("Payload");
    ui.text_edit_multiline(&mut draft.payload);

    ui.horizontal(|ui| {
        ui.label("QoS");
        ui.selectable_value(&mut draft.qos, Qos::AtMostOnce, "0");
        ui.selectable_value(&mut draft.qos, Qos::AtLeastOnce, "1");
        ui.selectable_value(&mut draft.qos, Qos::ExactlyOnce, "2");
        ui.checkbox(&mut draft.retain, "Retain");
    });

    if ui.button("Publish").clicked() {
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
