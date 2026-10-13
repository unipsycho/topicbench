//! Left panel: connection profiles + open/connect multiple at once.

use crate::mqtt::profile::ProfileStore;
use crate::mqtt::session::SessionManager;

pub fn draw(
    ui: &mut eframe::egui::Ui,
    profiles: &mut ProfileStore,
    sessions: &mut SessionManager,
    status: &mut String,
) {
    ui.heading("Profiles");
    ui.label("Multiple profiles can be open/connected at once.");
    ui.add_space(6.0);

    let ids: Vec<String> = profiles.profiles.keys().cloned().collect();
    for id in ids {
        let Some(profile) = profiles.profiles.get(&id).cloned() else {
            continue;
        };
        let selected = profiles.selected_id.as_deref() == Some(id.as_str());
        let session_label = sessions
            .open
            .get(&id)
            .map(|s| s.state.label())
            .unwrap_or("closed");

        ui.group(|ui| {
            ui.horizontal(|ui| {
                if ui.selectable_label(selected, &profile.name).clicked() {
                    profiles.selected_id = Some(id.clone());
                }
                ui.label(format!("· {session_label}"));
            });
            ui.label(format!("{}:{}", profile.host, profile.port));
            ui.horizontal(|ui| {
                let open_label = if sessions.open.contains_key(&id) {
                    "Toggle connect"
                } else {
                    "Open"
                };
                if ui.button(open_label).clicked() {
                    sessions.open_or_toggle(&id, &profile.name);
                    *status = format!("Session {} → {}", profile.name, sessions.open[&id].state.label());
                }
            });
        });
        ui.add_space(4.0);
    }

    ui.add_space(8.0);
    ui.weak("TODO: add/edit/delete profiles + persist to disk.");
}
