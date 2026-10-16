//! Left panel: edit profiles (incl. subscriptions), connect, All vs Connected.

use crate::mqtt::certs;
use crate::mqtt::pins::PinBoard;
use crate::mqtt::profile::ProfileStore;
use crate::mqtt::session::{SessionManager, SessionState};
use crate::mqtt::settings::AppSettings;
use crate::ui::hover_tip;
use crate::ui::shortcuts;
use crate::mqtt::subscribe;
use crate::ui::clipboard;
use crate::ui::flash;
use crate::ui::icons::{
    self, IconSet, CHROME_ACCENT_BORDER, CHROME_ACCENT_SOFT, CHROME_CAPTION, CHROME_HOVER,
    CHROME_ROW, CHROME_SECTION,
};
use eframe::egui::{self, Color32, CursorIcon, RichText};
use std::path::Path;
use std::time::Instant;

/// DnD payload for reordering the Profiles list (profile id only — never mutates ids).
#[derive(Clone)]
struct ProfileDnd(String);

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
    show_password: &mut bool,
) {
    icons::section_header_bar(ui, |ui| {
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
        .fill(CHROME_SECTION)
        .corner_radius(3.0)
        .inner_margin(egui::Margin::symmetric(8, 5))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Show").small().color(CHROME_CAPTION));
                ui.selectable_value(list_filter, ProfileListFilter::All, "All");
                ui.selectable_value(list_filter, ProfileListFilter::ConnectedOnly, "Connected");
            });
        });
    ui.add_space(6.0);

    // Selectable full-width list rows (name + status + count); drag to reorder.
    let now = Instant::now();
    let ids: Vec<String> = profiles.ids_in_order();
    let row_bg = CHROME_ROW;
    let row_bg_sel = CHROME_ACCENT_SOFT;
    let mut pending_reorder: Option<(String, String, bool)> = None;
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
        let name = profile.name.clone();

        let fill = if selected { row_bg_sel } else { row_bg };
        let row_h = 28.0;
        let dnd_id = egui::Id::new(("profile_dnd", id.as_str()));
        // Drag source paints the row; short click still selects.
        let row_response = ui
            .dnd_drag_source(dnd_id, ProfileDnd(id.clone()), |ui| {
                let (rect, _) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), row_h),
                    egui::Sense::hover(),
                );
                ui.painter().rect_filled(rect, 2.0, fill);
                flash::paint_inset(ui, rect, flash::flash_stroke(flash_until, now));
                ui.allocate_new_ui(
                    egui::UiBuilder::new().max_rect(rect.shrink2(egui::vec2(8.0, 4.0))),
                    |ui| {
                        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                            ui.spacing_mut().item_spacing.x = 6.0;
                            ui.add(
                                egui::Label::new(
                                    RichText::new(&name)
                                        .strong()
                                        .color(if selected {
                                            Color32::from_rgb(210, 225, 255)
                                        } else {
                                            Color32::from_rgb(230, 232, 238)
                                        }),
                                )
                                .selectable(false),
                            );
                            draw_session_status(ui, icons, state);
                            if msg_count > 0 {
                                ui.label(RichText::new(format!("#{msg_count}")).strong());
                            }
                        });
                    },
                );
            })
            .response
            .on_hover_cursor(CursorIcon::Grab);

        // Drop target: insert before/after based on pointer Y within this row.
        if let Some(payload) = row_response.dnd_hover_payload::<ProfileDnd>() {
            if payload.0.as_str() != id.as_str() {
                let after = row_response
                    .hover_pos()
                    .map(|p| p.y > row_response.rect.center().y)
                    .unwrap_or(false);
                let y = if after {
                    row_response.rect.bottom()
                } else {
                    row_response.rect.top()
                };
                ui.painter().hline(
                    row_response.rect.x_range(),
                    y,
                    egui::Stroke::new(2.0, CHROME_ACCENT_BORDER),
                );
                // Quiet hover wash so the drop target reads clearly.
                ui.painter().rect_filled(
                    row_response.rect,
                    2.0,
                    Color32::from_rgba_unmultiplied(
                        CHROME_HOVER.r(),
                        CHROME_HOVER.g(),
                        CHROME_HOVER.b(),
                        80,
                    ),
                );
            }
        }
        if let Some(payload) = row_response.dnd_release_payload::<ProfileDnd>() {
            if payload.0.as_str() != id.as_str() {
                let after = row_response
                    .interact_pointer_pos()
                    .or_else(|| row_response.hover_pos())
                    .map(|p| p.y > row_response.rect.center().y)
                    .unwrap_or(false);
                pending_reorder = Some((payload.0.clone(), id.clone(), after));
            }
        }

        if row_response.clicked() {
            if profiles.selected_id.as_deref() != Some(id.as_str()) {
                *show_password = false;
            }
            profiles.selected_id = Some(id.clone());
        }
        ui.add_space(2.0);
    }
    if let Some((dragged, target, after)) = pending_reorder {
        if profiles.reorder(&dragged, &target, after) {
            match profiles.save() {
                Ok(()) => *status = format!("Reordered profiles ({})", profiles.last_io),
                Err(e) => *status = format!("Reorder saved in memory; disk: {e}"),
            }
        }
    }

    ui.add_space(8.0);
    ui.separator();
    ui.add_space(4.0);

    let selected_id = profiles.selected_id.clone();
    if let Some(id) = selected_id {
        if profiles.profiles.contains_key(&id) {
            icons::section_header_bar(ui, |ui| {
                ui.heading("Edit");
            });
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
                    let color_resp = hover_tip::attach(
                        ui.color_edit_button_srgb(&mut rgb),
                        "Topic tree + sticky labels",
                    );
                    if color_resp.changed() {
                        profile.color_rgb = rgb;
                    }
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
                // Password + eyeball toggle (masked by default; revealed text is selectable).
                ui.horizontal(|ui| {
                    ui.label("Password");
                    ui.add(
                        egui::TextEdit::singleline(&mut profile.password)
                            .password(!*show_password)
                            .desired_width(160.0),
                    );
                    let (eye_icon, eye_tip) = if *show_password {
                        ("eye_off", "Hide password")
                    } else {
                        ("eye", "Show password")
                    };
                    if icons.tiny(ui, eye_icon, *show_password, eye_tip).clicked() {
                        *show_password = !*show_password;
                    }
                });
                ui.checkbox(&mut profile.use_tls, "Use TLS");
                // Optional certs folder for AWS IoT–style CA + client cert + key.
                ui.horizontal(|ui| {
                    if let Some(tex) = icons.get("tls_security") {
                        ui.add(
                            egui::Image::new(tex)
                                .fit_to_exact_size(egui::vec2(14.0, 14.0))
                                .sense(egui::Sense::hover()),
                        );
                    }
                    ui.label("Certs folder");
                    ui.add(
                        egui::TextEdit::singleline(&mut profile.certs_dir)
                            .desired_width(140.0)
                            .hint_text("/path/to/certs"),
                    );
                    if icons
                        .tiny(ui, "folder_open", false, "Choose certs folder")
                        .clicked()
                    {
                        if let Some(path) = rfd::FileDialog::new()
                            .set_title("MQTT TLS certs folder")
                            .pick_folder()
                        {
                            profile.certs_dir = path.display().to_string();
                            profile.use_tls = true;
                        }
                    }
                    if !profile.certs_dir.trim().is_empty()
                        && icons
                            .tiny(ui, "remove_x", false, "Clear certs folder")
                            .clicked()
                    {
                        profile.certs_dir.clear();
                    }
                });
                // Show which three files were auto-detected (or the error).
                let certs_trim = profile.certs_dir.trim().to_string();
                if !certs_trim.is_empty() {
                    match certs::discover(Path::new(&certs_trim)) {
                        Ok(resolved) => {
                            ui.weak(format!(
                                "CA: {}  ·  Cert: {}  ·  Key: {}",
                                resolved.ca_name(),
                                resolved.cert_name(),
                                resolved.key_name()
                            ));
                            if profile.port == 1883 {
                                ui.weak("Tip: AWS IoT typically uses port 8883 with TLS.");
                            }
                        }
                        Err(e) => {
                            ui.colored_label(Color32::from_rgb(220, 140, 100), e);
                        }
                    }
                }
                sessions.sync_profile_name(&id, &profile.name);
            }

            if let Some(session) = sessions.open.get(&id) {
                ui.weak(&session.detail);
            }

            // Profile subscriptions (saved with profile; applied on connect)
            ui.add_space(8.0);
            ui.separator();
            icons::section_header_bar(ui, |ui| {
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
            let sub_row_bg = CHROME_ROW;
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
