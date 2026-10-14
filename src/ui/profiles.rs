//! Left panel: edit profiles (incl. subscriptions), connect, All vs Connected.

use crate::mqtt::pins::PinBoard;
use crate::mqtt::profile::ProfileStore;
use crate::mqtt::session::{SessionManager, SessionState};
use crate::mqtt::settings::AppSettings;
use crate::ui::hover_tip;
use crate::ui::shortcuts;
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

/// Id salt for the profile Name field (Ctrl+E / F2 focus).
pub const PROFILE_NAME_ID: &str = "profile_edit_name";

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
    request_editor_focus: &mut bool,
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
    ui.add_space(4.0);

    // Filter sits above the list, visually distinct (chrome strip, not a list row).
    egui::Frame::NONE
        .fill(Color32::from_rgb(48, 52, 60))
        .corner_radius(3.0)
        .inner_margin(egui::Margin::symmetric(8, 5))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Show").small().color(Color32::from_rgb(170, 176, 188)));
                ui.selectable_value(list_filter, ProfileListFilter::All, "All");
                ui.selectable_value(list_filter, ProfileListFilter::ConnectedOnly, "Connected");
            });
        });
    ui.add_space(6.0);

    // Selectable full-width list rows (name + status + count).
    let now = Instant::now();
    let ids: Vec<String> = profiles.profiles.keys().cloned().collect();
    let row_bg = Color32::from_rgb(36, 38, 44);
    let row_bg_sel = Color32::from_rgb(48, 78, 112);
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

        let fill = if selected { row_bg_sel } else { row_bg };
        // Full-width row: click selects profile; name/count text is selectable for copy.
        let row_h = 28.0;
        let (rect, row_response) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), row_h), egui::Sense::click());
        ui.painter().rect_filled(rect, 2.0, fill);
        // Update pulse: inset border only (no fill wash; layout size unchanged).
        flash::paint_inset(ui, rect, flash::flash_stroke(flash_until, now));
        let mut name_clicked = false;
        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(rect.shrink2(egui::vec2(8.0, 4.0))), |ui| {
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                // Selectable so Mike can drag-select / Ctrl+C the profile name.
                let name_resp = ui.add(
                    egui::Label::new(
                        RichText::new(&profile.name)
                            .strong()
                            .color(if selected {
                                Color32::from_rgb(210, 225, 255)
                            } else {
                                Color32::from_rgb(230, 232, 238)
                            }),
                    )
                    .selectable(true)
                    .sense(egui::Sense::click()),
                );
                if name_resp.clicked() {
                    name_clicked = true;
                }
                draw_session_status(ui, icons, state);
                if msg_count > 0 {
                    ui.add(
                        egui::Label::new(RichText::new(format!("#{msg_count}")).strong())
                            .selectable(true),
                    );
                }
            });
        });
        // Row click (or name click) selects the profile — text drag-select still works.
        if row_response.clicked() || name_clicked {
            profiles.selected_id = Some(id.clone());
        }
        ui.add_space(2.0);
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
                    hover_tip::attach(
                        ui.label("Name"),
                        shortcuts::tip("Focus profile editor", "Ctrl+E / F2"),
                    );
                    let resp = hover_tip::attach(
                        ui.add(
                            egui::TextEdit::singleline(&mut profile.name).id_salt(PROFILE_NAME_ID),
                        ),
                        shortcuts::tip("Focus profile editor", "Ctrl+E / F2"),
                    );
                    if *request_editor_focus {
                        resp.request_focus();
                        *request_editor_focus = false;
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("Color");
                    let mut rgb = profile.color_rgb;
                    if ui.color_edit_button_srgb(&mut rgb).changed() {
                        profile.color_rgb = rgb;
                    }
                    ui.weak("Topic tree + sticky labels");
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

            // Topic filters as a Profiles-style list (row strip + red X remove).
            let mut remove_at: Option<usize> = None;
            let sub_row_bg = Color32::from_rgb(36, 38, 44);
            if let Some(profile) = profiles.profiles.get(&id) {
                if profile.subscriptions.is_empty() {
                    ui.weak("No filters yet.");
                } else {
                    for (i, sub) in profile.subscriptions.iter().enumerate() {
                        let row_h = 28.0;
                        let (rect, _response) = ui.allocate_exact_size(
                            egui::vec2(ui.available_width(), row_h),
                            egui::Sense::hover(),
                        );
                        ui.painter().rect_filled(rect, 2.0, sub_row_bg);
                        ui.allocate_new_ui(
                            egui::UiBuilder::new().max_rect(rect.shrink2(egui::vec2(8.0, 4.0))),
                            |ui| {
                                ui.with_layout(
                                    egui::Layout::left_to_right(egui::Align::Center),
                                    |ui| {
                                        ui.spacing_mut().item_spacing.x = 6.0;
                                        clipboard::topic_label(ui, sub, status);
                                        ui.with_layout(
                                            egui::Layout::right_to_left(egui::Align::Center),
                                            |ui| {
                                                if icons
                                                    .tiny(ui, "remove_x", false, "Remove filter")
                                                    .clicked()
                                                {
                                                    remove_at = Some(i);
                                                }
                                            },
                                        );
                                    },
                                );
                            },
                        );
                        ui.add_space(2.0);
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
                ui.add(
                    egui::TextEdit::singleline(draft_filter)
                        .desired_width(160.0)
                        .hint_text("topic/#")
                        .font(egui::TextStyle::Monospace),
                );
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
            // Icon on the button: connected → disconnect; otherwise → connect.
            let live = sessions.open.get(&id).map(|s| s.state);
            let (label, icon_name) = match live {
                Some(SessionState::Connected) | Some(SessionState::Connecting) => {
                    ("Disconnect", "disconnect")
                }
                _ => ("Connect", "connect"),
            };
            let connect_tip = shortcuts::tip(
                if label == "Disconnect" {
                    "Disconnect selected profile"
                } else {
                    "Connect selected profile"
                },
                "Ctrl+Enter",
            );
            let connect_clicked = if let Some(tex) = icons.get(icon_name) {
                hover_tip::attach(
                    ui.add(
                        egui::Button::image_and_text(
                            egui::Image::new(tex).fit_to_exact_size(egui::vec2(18.0, 18.0)),
                            label,
                        )
                        .min_size(egui::vec2(120.0, 28.0)),
                    ),
                    connect_tip,
                )
                .clicked()
            } else {
                hover_tip::attach(
                    ui.add(egui::Button::new(label).min_size(egui::vec2(120.0, 28.0))),
                    connect_tip,
                )
                .clicked()
            };
            if connect_clicked {
                let profile = profiles.profiles.get(&id).cloned();
                if let Some(profile) = profile {
                    let connecting = match sessions.open.get(&id).map(|s| s.state) {
                        Some(SessionState::Connected) | Some(SessionState::Connecting) => false,
                        _ => true,
                    };
                    let max_pkt =
                        AppSettings::clamp_max_packet(settings.max_packet_bytes);
                    match sessions.toggle(&profile, max_pkt, ctx, pins) {
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
