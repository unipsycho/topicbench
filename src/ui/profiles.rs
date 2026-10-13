//! Left panel: edit / save / connect profiles; All vs Connected filter.

use crate::mqtt::profile::ProfileStore;
use crate::mqtt::session::{SessionManager, SessionState};
use crate::mqtt::settings::AppSettings;
use crate::ui::flash;
use eframe::egui::{self, Color32, RichText};
use std::time::Instant;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ProfileListFilter {
    All,
    ConnectedOnly,
}

pub fn draw(
    ui: &mut egui::Ui,
    profiles: &mut ProfileStore,
    sessions: &mut SessionManager,
    settings: &mut AppSettings,
    list_filter: &mut ProfileListFilter,
    status: &mut String,
    ctx: &egui::Context,
) {
    ui.heading("Profiles");
    ui.label("Edit, save, and connect for real MQTT I/O.");
    ui.add_space(4.0);

    ui.horizontal(|ui| {
        ui.label("Show");
        ui.selectable_value(list_filter, ProfileListFilter::All, "All");
        ui.selectable_value(list_filter, ProfileListFilter::ConnectedOnly, "Connected");
    });

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

    let now = Instant::now();
    let ids: Vec<String> = profiles.profiles.keys().cloned().collect();
    for id in &ids {
        let Some(profile) = profiles.profiles.get(id) else {
            continue;
        };
        let session = sessions.open.get(id);
        let connected = session.map(|s| s.state == SessionState::Connected).unwrap_or(false);
        if *list_filter == ProfileListFilter::ConnectedOnly && !connected {
            continue;
        }

        let selected = profiles.selected_id.as_deref() == Some(id.as_str());
        let session_label = session.map(|s| s.state.label()).unwrap_or("closed");
        let msg_count = session.map(|s| s.message_count).unwrap_or(0);
        let flash_until = session.and_then(|s| s.flash_until);

        let fill = flash::flash_fill(flash_until, now).unwrap_or(Color32::TRANSPARENT);
        egui::Frame::NONE
            .fill(fill)
            .inner_margin(egui::Margin::symmetric(4, 2))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    if ui.selectable_label(selected, &profile.name).clicked() {
                        profiles.selected_id = Some(id.clone());
                    }
                    ui.label(format!("· {session_label}"));
                    if msg_count > 0 {
                        ui.label(RichText::new(format!("#{msg_count}")).strong());
                    }
                });
            });
    }

    ui.add_space(8.0);
    ui.separator();
    ui.add_space(4.0);

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
                ui.add(egui::DragValue::new(&mut profile.port).range(1..=65535));
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
                ui.add(egui::TextEdit::singleline(&mut profile.password).password(true));
            });
            ui.checkbox(&mut profile.use_tls, "Use TLS");

            sessions.sync_profile_name(&id, &profile.name);

            if let Some(session) = sessions.open.get(&id) {
                ui.weak(&session.detail);
            }

            ui.add_space(6.0);
            let label = match sessions.open.get(&id).map(|s| s.state) {
                Some(SessionState::Connected) | Some(SessionState::Connecting) => "Disconnect",
                _ => "Connect",
            };
            if ui.button(label).clicked() {
                let profile = profiles.profiles.get(&id).cloned();
                if let Some(profile) = profile {
                    let connecting = match sessions.open.get(&id).map(|s| s.state) {
                        Some(SessionState::Connected) | Some(SessionState::Connecting) => false,
                        _ => true,
                    };
                    match sessions.toggle(&profile, ctx) {
                        Ok(msg) => {
                            settings.remember_connected(&id, connecting);
                            *status = msg;
                        }
                        Err(e) => *status = e,
                    }
                }
            }
        }
    } else {
        ui.weak("No profile selected.");
    }
}
