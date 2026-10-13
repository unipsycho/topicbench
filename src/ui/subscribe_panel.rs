//! Center panel: subscriptions, traffic, device picker.

use crate::mqtt::pins::PinBoard;
use crate::mqtt::session::{SessionManager, SessionState};
use crate::mqtt::subscribe;
use crate::ui::clipboard;
use crate::ui::search_bar::SearchFilter;
use eframe::egui::{self, RichText};

pub fn draw(
    ui: &mut egui::Ui,
    topic: &mut String,
    target_session_id: &mut Option<String>,
    sessions: &mut SessionManager,
    pins: &mut PinBoard,
    filter: &SearchFilter,
    status: &mut String,
) {
    ui.heading("Subscriptions");
    ui.label("Choose an open device/profile, then add or remove topic filters (`+` / `#` OK).");
    ui.add_space(6.0);

    if let Some(id) = target_session_id.as_ref() {
        if !sessions.open.contains_key(id) {
            *target_session_id = None;
        }
    }
    if target_session_id.is_none() {
        *target_session_id = sessions
            .connected_ids()
            .first()
            .map(|s| (*s).to_string())
            .or_else(|| sessions.open.keys().next().cloned());
    }

    ui.horizontal(|ui| {
        ui.label("Device / profile");
        let current_label = target_session_id
            .as_ref()
            .and_then(|id| sessions.open.get(id))
            .map(|s| format!("{} ({})", s.profile_name, s.state.label()))
            .unwrap_or_else(|| "(none open)".into());

        egui::ComboBox::from_id_salt("subscribe_target")
            .selected_text(current_label)
            .show_ui(ui, |ui| {
                if sessions.open.is_empty() {
                    ui.weak("Connect a profile on the left first.");
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
        ui.add(egui::TextEdit::singleline(topic).desired_width(280.0));
        if ui.button("Add").clicked() {
            if let Some(id) = target_session_id.clone() {
                let result = sessions
                    .open
                    .get_mut(&id)
                    .map(|s| {
                        let r = subscribe::add_subscription(&mut s.subscriptions, topic);
                        (r, s.profile_name.clone())
                    });
                match result {
                    Some((Ok(()), name)) => {
                        let filter_s = topic.trim().to_string();
                        match sessions.subscribe_live(&id, &filter_s) {
                            Ok(()) => *status = format!("Subscribed on {name}: {filter_s}"),
                            Err(e) => *status = format!("Saved filter; broker: {e}"),
                        }
                    }
                    Some((Err(e), _)) => *status = e,
                    None => *status = "Session gone".into(),
                }
            } else {
                *status = "Connect a profile before subscribing.".into();
            }
        }
        if ui.button("Sticky").clicked() {
            match target_session_id.as_deref().and_then(|id| sessions.open.get(id)) {
                Some(session) => {
                    pins.pin(&session.profile_id, &session.profile_name, topic);
                    *status = format!("Sticky {} from {}", topic, session.profile_name);
                }
                None => *status = "Open a profile to mark sticky topics.".into(),
            }
        }
    });

    ui.add_space(8.0);
    ui.strong("Active on selected device");
    let mut remove_filter: Option<String> = None;
    if let Some(id) = target_session_id.as_ref() {
        if let Some(session) = sessions.open.get(id) {
            if session.subscriptions.is_empty() {
                ui.weak("No subscriptions yet.");
            } else {
                for sub in &session.subscriptions {
                    ui.horizontal(|ui| {
                        clipboard::topic_label(ui, sub, status);
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
    if let (Some(id), Some(filter_s)) = (target_session_id.clone(), remove_filter) {
        let name = sessions
            .open
            .get_mut(&id)
            .and_then(|session| {
                if subscribe::remove_subscription(&mut session.subscriptions, &filter_s) {
                    Some(session.profile_name.clone())
                } else {
                    None
                }
            });
        if let Some(name) = name {
            let _ = sessions.unsubscribe_live(&id, &filter_s);
            *status = format!("Removed {filter_s} from {name}");
        }
    }

    ui.add_space(10.0);
    ui.separator();
    ui.add_space(4.0);
    ui.strong("Live traffic");
    if let Some(id) = target_session_id.as_ref() {
        if let Some(session) = sessions.open.get(id) {
            if session.traffic.is_empty() {
                ui.weak("Waiting for messages…");
            } else {
                egui::ScrollArea::vertical()
                    .id_salt("traffic_scroll")
                    .max_height(140.0)
                    .show(ui, |ui| {
                        for line in session.traffic.iter().take(40) {
                            if !filter.matches(&line.topic, Some(&line.payload)) {
                                continue;
                            }
                            ui.horizontal(|ui| {
                                clipboard::topic_label(ui, &line.topic, status);
                                clipboard::payload_with_copy(ui, &line.payload, 60, status);
                            });
                        }
                    });
            }
        }
    }
}
