//! Settings + keyboard-shortcuts + update-available overlay modals.

use super::MqttMonitorApp;
use crate::mqtt::settings::AppSettings;
use crate::ui::icons::{
    CHROME_ACCENT_BORDER, CHROME_ACCENT_SOFT, CHROME_MUTED, CHROME_SEP, CHROME_TEXT,
};
use crate::ui::{settings_panel, shortcuts};
use crate::update::{self, InstallOutcome, UpdateEvent, UpdateStatus};
use eframe::egui::{self, RichText};

impl MqttMonitorApp {
    /// Open Settings with a draft snapshot of current settings.
    pub(super) fn open_settings(&mut self) {
        self.settings_draft = Some(self.settings.clone());
        self.settings_open = true;
    }

    /// Commit draft to live settings + disk, then close.
    fn save_settings_modal(&mut self) {
        if let Some(draft) = self.settings_draft.take() {
            // Keep update prefs from live settings (edited outside the draft).
            let last_update_check = self.settings.last_update_check;
            let skipped_update_version = self.settings.skipped_update_version.clone();
            let remind_later_until = self.settings.remind_later_until;
            self.settings = draft;
            self.settings.last_update_check = last_update_check;
            self.settings.skipped_update_version = skipped_update_version;
            self.settings.remind_later_until = remind_later_until;
            match self.settings.save() {
                Ok(()) => {
                    self.status = format!("Saved {}", AppSettings::path().display());
                }
                Err(e) => self.status = e,
            }
        }
        self.settings_open = false;
    }

    /// Discard draft and close without applying.
    fn cancel_settings_modal(&mut self) {
        self.settings_draft = None;
        self.settings_open = false;
    }

    /// Kick a manual check from Settings → Check now.
    pub(super) fn request_update_check(&mut self, quiet: bool) {
        if matches!(
            self.update_status,
            UpdateStatus::Checking | UpdateStatus::Installing
        ) {
            return;
        }
        self.update_check_quiet = quiet;
        self.update_status = UpdateStatus::Checking;
        self.update_service.start_check();
    }

    /// Apply a background update event on the UI thread.
    pub(super) fn handle_update_event(&mut self, ev: UpdateEvent) {
        match ev {
            UpdateEvent::CheckOk {
                info,
                update_available,
            } => {
                self.settings.last_update_check = Some(update::now_unix());
                let _ = self.settings.save();
                match info {
                    Some(info) if update_available => {
                        let skipped = self
                            .settings
                            .skipped_update_version
                            .as_deref()
                            .map(|s| s == info.tag || s == info.version)
                            .unwrap_or(false);
                        let remind = update::remind_later_active(self.settings.remind_later_until)
                            || self.update_remind_session;
                        self.update_status = UpdateStatus::Available(info.clone());
                        if !skipped && !remind {
                            self.update_modal_open = true;
                            self.update_offer = Some(info);
                        } else {
                            self.update_offer = Some(info);
                        }
                    }
                    Some(info) => {
                        self.update_status = UpdateStatus::UpToDate {
                            latest: info.tag.clone(),
                        };
                        self.update_offer = None;
                        self.update_modal_open = false;
                    }
                    None => {
                        self.update_status = UpdateStatus::UpToDate {
                            latest: update::current_version().into(),
                        };
                    }
                }
            }
            UpdateEvent::CheckFailed(msg) => {
                self.settings.last_update_check = Some(update::now_unix());
                let _ = self.settings.save();
                self.update_status = UpdateStatus::Error(msg);
                // Quiet on startup — Settings still shows “Couldn't check”.
                if !self.update_check_quiet {
                    // Manual Check now already surfaces via status line.
                }
            }
            UpdateEvent::InstallDone(outcome) => match outcome {
                InstallOutcome::ReplacedAndRelaunched => {
                    // install.rs exits the process after relaunch; defensive fallback:
                    self.update_status = UpdateStatus::Installing;
                    self.update_modal_open = false;
                }
                InstallOutcome::OpenedBrowser { reason } => {
                    self.update_status = UpdateStatus::InstallFailed(format!(
                        "Opened release page ({reason})"
                    ));
                    self.update_modal_fallback_note = Some(reason);
                }
            },
        }
    }

    /// Overlays last so they own pointer/hover above Payload and other panels.
    pub(super) fn draw_overlays(&mut self, ctx: &egui::Context) {
        if self.shortcuts_open {
            let mut close_shortcuts = false;
            let modal = egui::Modal::new(egui::Id::new("mqtt_master_shortcuts")).show(ctx, |ui| {
                ui.set_min_width(480.0);
                ui.set_max_width(560.0);
                egui::ScrollArea::vertical()
                    .max_height(520.0)
                    .show(ui, |ui| {
                        shortcuts::draw_help(ui);
                    });
                ui.add_space(8.0);
                ui.separator();
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Close").clicked() {
                            close_shortcuts = true;
                        }
                    });
                });
            });
            if close_shortcuts || modal.should_close() {
                self.shortcuts_open = false;
            }
        }

        // Update available — ask before install (no auto-install toggle in v1).
        if self.update_modal_open {
            self.draw_update_modal(ctx);
        }

        // Settings — centered modal (Save commits; Cancel / Escape / outside discards).
        if self.settings_open {
            if self.settings_draft.is_none() {
                self.settings_draft = Some(self.settings.clone());
            }
            let mut open_shortcuts = false;
            let mut save_clicked = false;
            let mut cancel_clicked = false;
            let mut check_now = false;
            let checking = matches!(self.update_status, UpdateStatus::Checking);
            let last_check = self.settings.last_update_check;
            let status_snapshot = self.update_status.clone();
            let modal = egui::Modal::new(egui::Id::new("mqtt_master_settings")).show(ctx, |ui| {
                ui.set_min_width(420.0);
                ui.set_max_width(480.0);
                egui::ScrollArea::vertical()
                    .max_height(520.0)
                    .show(ui, |ui| {
                        if let Some(draft) = self.settings_draft.as_mut() {
                            settings_panel::draw(
                                ui,
                                draft,
                                &self.icons,
                                &mut open_shortcuts,
                                settings_panel::UpdatesSection {
                                    status: &status_snapshot,
                                    last_check_unix: last_check,
                                    checking,
                                    check_now: &mut check_now,
                                },
                            );
                        }
                    });
                ui.add_space(8.0);
                ui.separator();
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        // Save — accent, larger, with icon (same spot Close used).
                        let save_btn = if let Some(tex) = self.icons.get("save") {
                            ui.add(
                                egui::Button::image_and_text(
                                    egui::Image::new(tex)
                                        .fit_to_exact_size(egui::vec2(18.0, 18.0)),
                                    RichText::new("Save").strong(),
                                )
                                .fill(CHROME_ACCENT_SOFT)
                                .stroke(egui::Stroke::new(1.0_f32, CHROME_ACCENT_BORDER))
                                .min_size(egui::vec2(96.0, 32.0)),
                            )
                        } else {
                            ui.add(
                                egui::Button::new(RichText::new("Save").strong())
                                    .fill(CHROME_ACCENT_SOFT)
                                    .stroke(egui::Stroke::new(1.0_f32, CHROME_ACCENT_BORDER))
                                    .min_size(egui::vec2(96.0, 32.0)),
                            )
                        };
                        if save_btn.clicked() {
                            save_clicked = true;
                        }
                        if ui
                            .add(egui::Button::new("Cancel").min_size(egui::vec2(72.0, 28.0)))
                            .clicked()
                        {
                            cancel_clicked = true;
                        }
                    });
                });
            });
            if open_shortcuts {
                self.shortcuts_open = true;
            }
            if check_now {
                self.request_update_check(false);
            }
            if save_clicked {
                self.save_settings_modal();
            } else if cancel_clicked || modal.should_close() {
                self.cancel_settings_modal();
            }
        }
    }

    fn draw_update_modal(&mut self, ctx: &egui::Context) {
        let Some(info) = self.update_offer.clone() else {
            self.update_modal_open = false;
            return;
        };
        let installing = matches!(self.update_status, UpdateStatus::Installing);
        let notes = update::truncate_notes(&info.body);
        let fallback = self.update_modal_fallback_note.clone();
        let mut update_clicked = false;
        let mut remind_clicked = false;
        let mut skip_clicked = false;
        let mut dismiss = false;

        let modal = egui::Modal::new(egui::Id::new("mqtt_master_update")).show(ctx, |ui| {
            ui.set_min_width(440.0);
            ui.set_max_width(520.0);

            ui.label(
                RichText::new("Update available")
                    .strong()
                    .size(18.0)
                    .color(CHROME_TEXT),
            );
            ui.add_space(4.0);
            ui.label(
                RichText::new(format!(
                    "{} → {}",
                    update::current_version(),
                    info.tag
                ))
                .color(CHROME_ACCENT_BORDER),
            );
            if !info.name.is_empty() && info.name != info.tag {
                ui.label(RichText::new(&info.name).color(CHROME_MUTED));
            }
            ui.add_space(8.0);
            let bar_w = ui.available_width();
            let (bar, _) = ui.allocate_exact_size(egui::vec2(bar_w, 2.0), egui::Sense::hover());
            ui.painter().rect_filled(bar, 1.0, CHROME_SEP);
            ui.add_space(8.0);

            ui.label(RichText::new("Release notes").strong().color(CHROME_TEXT));
            ui.add_space(4.0);
            egui::ScrollArea::vertical()
                .max_height(220.0)
                .show(ui, |ui| {
                    ui.label(RichText::new(notes).color(CHROME_MUTED));
                });

            if let Some(note) = &fallback {
                ui.add_space(8.0);
                ui.label(
                    RichText::new(format!(
                        "Couldn't install in-app — opened the release page.\n{note}"
                    ))
                    .color(CHROME_MUTED),
                );
            } else if info.asset_url.is_none() || info.sha256sums_url.is_none() {
                ui.add_space(8.0);
                ui.label(
                    RichText::new(
                        "Release assets or SHA256SUMS missing — Update will open the release page in your browser.",
                    )
                    .color(CHROME_MUTED),
                );
            }

            ui.add_space(12.0);
            ui.separator();
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let update_btn = ui.add_enabled(
                        !installing,
                        egui::Button::new(RichText::new(if installing {
                            "Installing…"
                        } else {
                            "Update"
                        }).strong())
                        .fill(CHROME_ACCENT_SOFT)
                        .stroke(egui::Stroke::new(1.0_f32, CHROME_ACCENT_BORDER))
                        .min_size(egui::vec2(96.0, 32.0)),
                    );
                    if update_btn.clicked() {
                        update_clicked = true;
                    }
                    if ui
                        .add_enabled(
                            !installing,
                            egui::Button::new("Remind me later").min_size(egui::vec2(120.0, 28.0)),
                        )
                        .clicked()
                    {
                        remind_clicked = true;
                    }
                    if ui
                        .add_enabled(
                            !installing,
                            egui::Button::new("Skip this version").min_size(egui::vec2(120.0, 28.0)),
                        )
                        .clicked()
                    {
                        skip_clicked = true;
                    }
                });
            });
        });

        if update_clicked {
            self.update_modal_fallback_note = None;
            self.update_status = UpdateStatus::Installing;
            self.update_service.start_install(info);
        } else if remind_clicked {
            self.update_remind_session = true;
            self.settings.remind_later_until =
                Some(update::now_unix() + update::REMIND_LATER.as_secs());
            let _ = self.settings.save();
            self.update_modal_open = false;
            dismiss = true;
        } else if skip_clicked {
            self.settings.skipped_update_version = Some(info.tag.clone());
            self.settings.remind_later_until = None;
            let _ = self.settings.save();
            self.update_modal_open = false;
            dismiss = true;
        } else if modal.should_close() && !installing {
            // Escape / backdrop → same as Remind me later for this session.
            self.update_remind_session = true;
            self.update_modal_open = false;
            dismiss = true;
        }
        let _ = dismiss;
    }
}
