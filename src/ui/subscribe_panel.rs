//! Center panel: subscriptions on a chosen open device/profile.

use crate::mqtt::pins::PinBoard;
use crate::mqtt::session::{SessionManager, SessionState};
use crate::mqtt::subscribe;

pub fn draw(
    ui: &mut eframe::egui::Ui,
    topic: &mut String,
    target_session_id: &mut Option<String>,
    sessions: &mut SessionManager,
    pins: &mut PinBoard,
    status: &mut String,
) {
    ui.heading("Subscriptions");
    ui.label("Choose an open device/profile, then add or remove topic filters (`+` / `#` OK).");
    ui.add_space(6.0);

    // Drop stale target if session closed
    if let Some(id) = target_session_id.as_ref() {
        if !sessions.open.contains_key(id) {
            *target_session_id = None;
        }
    }
    // Default to first connected (or first open) session
    if target_session_id.is_none() {
        *target_session_id = sessions
            .connected_ids()
            .first()
            .map(|s| (*s).to_string())
            .or_else(|| sessions.open.keys().next().cloned());
    }

    // Device / profile picker
    ui.horizontal(|ui| {
        ui.label("Device / profile");
        let current_label = target_session_id
            .as_ref()
            .and_then(|id| sessions.open.get(id))
            .map(|s| format!("{} ({})", s.profile_name, s.state.label()))
            .unwrap_or_else(|| "(none open)".into());

        eframe::egui::ComboBox::from_id_salt("subscribe_target")
            .selected_text(current_label)
            .show_ui(ui, |ui| {
                if sessions.open.is_empty() {
                    ui.weak("Open a profile on the left first.");
                    return;
                }
                let choices: Vec<(String, String, SessionState)> = sessions
                    .open
                    .values()
                    .map(|s| (s.profile_id.clone(), s.profile_name.clone(), s.state))
                    .collect();
                for (id, name, state) in choices {
                    let label = format!("{name} ({})", state.label());
                    if ui
                        .selectable_label(target_session_id.as_deref() == Some(id.as_str()), label)
                        .clicked()
                    {
                        *target_session_id = Some(id);
                    }
                }
            });
    });

    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.label("Topic filter");
        ui.add(eframe::egui::TextEdit::singleline(topic).desired_width(280.0));
        if ui.button("Add").clicked() {
            match target_session_id.as_deref() {
                Some(id) => {
                    if let Some(session) = sessions.open.get_mut(id) {
                        match subscribe::add_subscription(&mut session.subscriptions, topic) {
                            Ok(()) => {
                                *status = format!("Subscribed on {}: {topic}", session.profile_name)
                            }
                            Err(e) => *status = e,
                        }
                    }
                }
                None => *status = "Open a profile before subscribing.".into(),
            }
        }
        if ui.button("Pin").clicked() {
            match target_session_id.as_deref().and_then(|id| sessions.open.get(id)) {
                Some(session) if session.state == SessionState::Connected => {
                    pins.pin(&session.profile_id, &session.profile_name, topic);
                    *status = format!("Pinned {} from {}", topic, session.profile_name);
                }
                Some(_) => *status = "Connect the selected profile to pin.".into(),
                None => *status = "Open a profile to pin topics.".into(),
            }
        }
    });

    ui.add_space(10.0);
    ui.strong("Active on selected device");
    let mut remove_filter: Option<String> = None;
    if let Some(id) = target_session_id.as_ref() {
        if let Some(session) = sessions.open.get(id) {
            if session.subscriptions.is_empty() {
                ui.weak("No subscriptions yet.");
            } else {
                for sub in &session.subscriptions {
                    ui.horizontal(|ui| {
                        ui.monospace(sub);
                        if ui.small_button("Remove").clicked() {
                            remove_filter = Some(sub.clone());
                        }
                    });
                }
            }
        }
    } else {
        ui.weak("No open session selected.");
    }
    if let (Some(id), Some(filter)) = (target_session_id.clone(), remove_filter) {
        if let Some(session) = sessions.open.get_mut(&id) {
            if subscribe::remove_subscription(&mut session.subscriptions, &filter) {
                *status = format!("Removed {filter} from {}", session.profile_name);
            }
        }
    }

    ui.add_space(10.0);
    ui.separator();
    ui.add_space(4.0);
    ui.strong("All subscriptions (every open device)");
    if sessions.open.is_empty() {
        ui.weak("No open sessions.");
    } else {
        for session in sessions.open.values() {
            ui.label(format!(
                "{} — {}",
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
    }
}
