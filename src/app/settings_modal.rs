//! Settings + keyboard-shortcuts overlay modals.

use super::MqttMonitorApp;
use crate::mqtt::settings::AppSettings;
use crate::ui::icons::{CHROME_ACCENT_BORDER, CHROME_ACCENT_SOFT};
use crate::ui::{settings_panel, shortcuts};
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
            self.settings = draft;
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

        // Settings — centered modal (Save commits; Cancel / Escape / outside discards).
        if self.settings_open {
            if self.settings_draft.is_none() {
                self.settings_draft = Some(self.settings.clone());
            }
            let mut open_shortcuts = false;
            let mut save_clicked = false;
            let mut cancel_clicked = false;
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
            if save_clicked {
                self.save_settings_modal();
            } else if cancel_clicked || modal.should_close() {
                self.cancel_settings_modal();
            }
        }
    }
}
