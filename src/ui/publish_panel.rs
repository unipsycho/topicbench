//! Publish panel: real rumqttc publish to a connected profile.

use crate::mqtt::publish::{PublishDraft, Qos};
use crate::mqtt::session::{SessionManager, SessionState};
use eframe::egui;

pub fn draw(
    ui: &mut egui::Ui,
    draft: &mut PublishDraft,
    sessions: &SessionManager,
    status: &mut String,
) {
    ui.heading("Publish");
    ui.label("Send to a topic on a connected broker.");
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
            Some(id) => match sessions.publish_live(id, &draft.topic, &draft.payload, draft.qos, draft.retain)
            {
                Ok(()) => {
                    *status = format!(
                        "Published → {id} {} (qos={} retain={})",
                        draft.topic,
                        draft.qos.as_u8(),
                        draft.retain
                    );
                }
                Err(e) => *status = e,
            },
            None => *status = "Select a connected profile to publish.".into(),
        }
    }
}
