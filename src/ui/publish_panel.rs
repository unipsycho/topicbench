//! Publish panel: topic, payload, QoS, retain — stub until rumqttc publish.

use crate::mqtt::publish::{publish_placeholder, PublishDraft, Qos};
use crate::mqtt::session::{SessionManager, SessionState};

pub fn draw(
    ui: &mut eframe::egui::Ui,
    draft: &mut PublishDraft,
    sessions: &SessionManager,
    status: &mut String,
) {
    ui.heading("Publish");
    ui.label("Send to a topic on an open/connected profile.");
    ui.add_space(4.0);

    // Profile picker among connected sessions
    let connected: Vec<(String, String)> = sessions
        .open
        .values()
        .filter(|s| s.state == SessionState::Connected)
        .map(|s| (s.profile_id.clone(), s.profile_name.clone()))
        .collect();

    if connected.is_empty() {
        ui.weak("No connected profiles.");
    } else {
        eframe::egui::ComboBox::from_label("Profile")
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
        *status = publish_placeholder(draft);
    }

    ui.add_space(8.0);
    ui.weak("TODO: rumqttc Client::try_publish on the session thread.");
}
