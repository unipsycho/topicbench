//! Subscribe (wildcards) + pin into the shared board.

use crate::mqtt::pins::PinBoard;
use crate::mqtt::session::{SessionManager, SessionState};
use crate::mqtt::subscribe;

pub fn draw(
    ui: &mut eframe::egui::Ui,
    topic: &mut String,
    sessions: &mut SessionManager,
    pins: &mut PinBoard,
    status: &mut String,
) {
    ui.heading("Subscribe");
    ui.label("Any topic filter, including MQTT wildcards `+` and `#`.");
    ui.add_space(4.0);

    ui.horizontal(|ui| {
        ui.label("Filter");
        ui.text_edit_singleline(topic);
    });

    // Target session: selected connected profile, else first connected.
    let target_id = sessions
        .connected_ids()
        .first()
        .map(|s| (*s).to_string());

    ui.horizontal(|ui| {
        if ui.button("Subscribe").clicked() {
            match target_id.as_deref() {
                Some(id) => {
                    if let Some(session) = sessions.open.get_mut(id) {
                        match subscribe::add_subscription(&mut session.subscriptions, topic) {
                            Ok(()) => *status = format!("Subscribed on {}: {topic}", session.profile_name),
                            Err(e) => *status = e,
                        }
                    }
                }
                None => *status = "Open/connect a profile before subscribing.".into(),
            }
        }
        if ui.button("Pin").clicked() {
            match target_id.as_deref().and_then(|id| sessions.open.get(id)) {
                Some(session) if session.state == SessionState::Connected => {
                    pins.pin(&session.profile_id, &session.profile_name, topic);
                    *status = format!("Pinned {} from {}", topic, session.profile_name);
                }
                _ => *status = "Connect a profile to pin topics.".into(),
            }
        }
    });

    ui.add_space(8.0);
    ui.collapsing("Subscriptions by open profile", |ui| {
        if sessions.open.is_empty() {
            ui.weak("No open sessions.");
            return;
        }
        for session in sessions.open.values() {
            ui.label(format!(
                "{} ({})",
                session.profile_name,
                session.state.label()
            ));
            if session.subscriptions.is_empty() {
                ui.weak("  (none)");
            } else {
                for sub in &session.subscriptions {
                    ui.monospace(format!("  {sub}"));
                }
            }
        }
    });
}
