//! Left panel: edit profiles (incl. subscriptions), connect, All vs Connected.

use crate::mqtt::pins::PinBoard;
use crate::mqtt::profile::ProfileStore;
use crate::mqtt::session::{SessionManager, SessionState};
use crate::mqtt::settings::AppSettings;
use crate::mqtt::subscribe;
use crate::ui::clipboard;
use crate::ui::flash;
use crate::ui::icons::IconSet;
use eframe::egui::{self, Color32, RichText};
use std::time::Instant;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ProfileListFilter {
    All,
    ConnectedOnly,
}

/// Colored status chip + icon beside each profile name.
fn draw_session_status(ui: &mut egui::Ui, icons: &IconSet, state: Option<SessionState>) {
    let (label, color, icon) = match state {
        Some(SessionState::Connected) => (
            "Connected",
            Color32::from_rgb(90, 190, 120),
            "connect",
        ),
        Some(SessionState::Connecting) => (
            "Connecting",
            Color32::from_rgb(220, 180, 80),
            "connection_status",
        ),
        Some(SessionState::Disconnected) => (
            "Disconnected",
            Color32::from_rgb(180, 110, 110),
            "disconnect",
        ),
        None => (
            "Closed",
            Color32::from_rgb(140, 145, 155),
            "disconnect",
        ),
    };
    // Prefer specific icon; fall back to connection_status if missing.
    let icon_name = if icons.get(icon).is_some() {
        icon
    } else {
        "connection_status"
    };
    if let Some(tex) = icons.get(icon_name) {
        ui.add(
            egui::Image::new(tex)
                .fit_to_exact_size(egui::vec2(14.0, 14.0))
                .sense(egui::Sense::hover()),
        );
    }
    ui.label(RichText::new(label).small().strong().color(color));
}

pub fn draw(
    ui: &mut egui::Ui,
    profiles: &mut ProfileStore,
    sessions: &mut SessionManager,
    pins: &mut PinBoard,
    settings: &mut AppSettings,
    list_filter: &mut ProfileListFilter,
    icons: &IconSet,
    draft_filter: &mut String,
    status: &mut String,
    ctx: &egui::Context,
) {
    ui.horizontal(|ui| {
        if let Some(tex) = icons.get("profile") {
            ui.add(
                egui::Image::new(tex)
                    .fit_to_exact_size(egui::vec2(20.0, 20.0))
                    .sense(egui::Sense::hover()),
            );
        }
        ui.heading("Profiles");
    });
    ui.label("Edit fields here — New / Save / Delete are on the ribbon.");
    ui.add_space(4.0);

    ui.horizontal(|ui| {
        ui.label("Show");
        ui.selectable_value(list_filter, ProfileListFilter::All, "All");
        ui.selectable_value(list_filter, ProfileListFilter::ConnectedOnly, "Connected");
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
        let state = session.map(|s| s.state);
        let msg_count = session.map(|s| s.message_count).unwrap_or(0);
        let flash_until = session.and_then(|s| s.flash_until);

        let fill = flash::flash_fill(flash_until, now).unwrap_or(Color32::TRANSPARENT);
        egui::Frame::NONE
            .fill(fill)
            .inner_margin(egui::Margin::symmetric(4, 2))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    if ui.selectable_label(selected, &profile.name).clicked() {
                        profiles.selected_id = Some(id.clone());
                    }
                    draw_session_status(ui, icons, state);
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
        if profiles.profiles.contains_key(&id) {
            ui.heading("Edit");
            if let Some(profile) = profiles.profiles.get_mut(&id) {
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
            }

            if let Some(session) = sessions.open.get(&id) {
                ui.weak(&session.detail);
            }

            // Profile subscriptions (saved with profile; applied on connect)
            ui.add_space(8.0);
            ui.separator();
            ui.horizontal(|ui| {
                if let Some(tex) = icons.get("subscribe_topic") {
                    ui.add(
                        egui::Image::new(tex)
                            .fit_to_exact_size(egui::vec2(18.0, 18.0))
                            .sense(egui::Sense::hover()),
                    );
                }
                ui.strong("Subscriptions");
            });
            ui.weak("Topic filters for this profile (`+` / `#` OK). Saved with the profile.");

            let mut remove_at: Option<usize> = None;
            if let Some(profile) = profiles.profiles.get(&id) {
                if profile.subscriptions.is_empty() {
                    ui.weak("No filters yet.");
                } else {
                    for (i, sub) in profile.subscriptions.iter().enumerate() {
                        ui.horizontal(|ui| {
                            clipboard::topic_label(ui, sub, status);
                            if ui.small_button("Remove").clicked() {
                                remove_at = Some(i);
                            }
                        });
                    }
                }
            }
            if let Some(i) = remove_at {
                if let Some(profile) = profiles.profiles.get_mut(&id) {
                    if i < profile.subscriptions.len() {
                        let removed = profile.subscriptions.remove(i);
                        let filters = profile.subscriptions.clone();
                        let _ = sessions.apply_profile_subscriptions(&id, &filters);
                        *status = format!("Removed filter {removed}");
                    }
                }
            }

            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(draft_filter).desired_width(160.0).hint_text("topic/#"));
                if ui.button("Add").clicked() {
                    let filter = draft_filter.trim().to_string();
                    match profiles.profiles.get_mut(&id) {
                        Some(profile) => match subscribe::add_subscription(&mut profile.subscriptions, &filter)
                        {
                            Ok(()) => {
                                let filters = profile.subscriptions.clone();
                                let _ = sessions.apply_profile_subscriptions(&id, &filters);
                                draft_filter.clear();
                                *status = format!("Added filter {filter}");
                            }
                            Err(e) => *status = e,
                        },
                        None => {}
                    }
                }
            });

            ui.add_space(6.0);
            let label = match sessions.open.get(&id).map(|s| s.state) {
                Some(SessionState::Connected) | Some(SessionState::Connecting) => "Disconnect",
                _ => "Connect",
            };
            if icons.labeled_button(ui, "connect", label).clicked() {
                let profile = profiles.profiles.get(&id).cloned();
                if let Some(profile) = profile {
                    let connecting = match sessions.open.get(&id).map(|s| s.state) {
                        Some(SessionState::Connected) | Some(SessionState::Connecting) => false,
                        _ => true,
                    };
                    let max_pkt =
                        AppSettings::clamp_max_packet(settings.max_packet_bytes);
                    match sessions.toggle(&profile, max_pkt, ctx) {
                        Ok(msg) => {
                            settings.remember_connected(&id, connecting);
                            if connecting {
                                // Persist pruned filters (overlapping subs double-count messages).
                                let pruned = profiles
                                    .profiles
                                    .get_mut(&id)
                                    .map(|p| subscribe::prune_redundant(&mut p.subscriptions))
                                    .unwrap_or(false);
                                if pruned {
                                    let _ = profiles.save();
                                }
                                if let Some(p) = profiles.profiles.get(&id) {
                                    pins.restore_from_profile(p);
                                }
                            }
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
