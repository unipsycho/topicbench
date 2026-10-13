//! Left panel: edit / save / open connection profiles.

use crate::mqtt::profile::ProfileStore;
use crate::mqtt::session::SessionManager;

pub fn draw(
    ui: &mut eframe::egui::Ui,
    profiles: &mut ProfileStore,
    sessions: &mut SessionManager,
    status: &mut String,
) {
    ui.heading("Profiles");
    ui.label("Edit and save to disk; open more than one at a time.");
    ui.add_space(6.0);

    ui.horizontal(|ui| {
        if ui.button("New").clicked() {
            let id = profiles.add_new();
            *status = format!("Created profile {id}");
        }
        if ui.button("Save").clicked() {
            match profiles.save() {
                Ok(()) => *status = profiles.last_io.clone(),
                Err(e) => *status = e,
            }
        }
        if ui.button("Delete").clicked() {
            if let Some(id) = profiles.delete_selected() {
                sessions.close(&id);
                *status = format!("Deleted profile {id}");
            }
        }
    });
    ui.weak(ProfileStore::path().display().to_string());
    ui.add_space(6.0);

    // Profile list
    let ids: Vec<String> = profiles.profiles.keys().cloned().collect();
    for id in &ids {
        let Some(profile) = profiles.profiles.get(id) else {
            continue;
        };
        let selected = profiles.selected_id.as_deref() == Some(id.as_str());
        let session_label = sessions
            .open
            .get(id)
            .map(|s| s.state.label())
            .unwrap_or("closed");

        ui.horizontal(|ui| {
            if ui.selectable_label(selected, &profile.name).clicked() {
                profiles.selected_id = Some(id.clone());
            }
            ui.label(format!("· {session_label}"));
        });
    }

    ui.add_space(8.0);
    ui.separator();
    ui.add_space(4.0);

    // Editor for selected profile
    let selected_id = profiles.selected_id.clone();
    if let Some(id) = selected_id {
        if let Some(profile) = profiles.profiles.get_mut(&id) {
            ui.heading("Edit");
            ui.horizontal(|ui| {
                ui.label("Name");
                ui.text_edit_singleline(&mut profile.name);
            });
            ui.horizontal(|ui| {
                ui.label("Host");
                ui.text_edit_singleline(&mut profile.host);
            });
            ui.horizontal(|ui| {
                ui.label("Port");
                ui.add(eframe::egui::DragValue::new(&mut profile.port).range(1..=65535));
            });
            ui.horizontal(|ui| {
                ui.label("Client ID");
                ui.text_edit_singleline(&mut profile.client_id);
            });
            ui.horizontal(|ui| {
                ui.label("Username");
                ui.text_edit_singleline(&mut profile.username);
            });
            ui.horizontal(|ui| {
                ui.label("Password");
                ui.add(eframe::egui::TextEdit::singleline(&mut profile.password).password(true));
            });
            ui.checkbox(&mut profile.use_tls, "Use TLS");

            sessions.sync_profile_name(&id, &profile.name);

            ui.add_space(6.0);
            let open_label = if sessions.open.contains_key(&id) {
                "Toggle connect"
            } else {
                "Open session"
            };
            if ui.button(open_label).clicked() {
                let name = profile.name.clone();
                sessions.open_or_toggle(&id, &name);
                *status = format!(
                    "Session {} → {}",
                    name,
                    sessions.open[&id].state.label()
                );
            }
        }
    } else {
        ui.weak("No profile selected.");
    }
}
