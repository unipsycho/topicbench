//! Settings content (shown in its own viewport window).

use crate::mqtt::settings::{AppSettings, TopicTreeExpandMode};
use crate::ui::icons::IconSet;
use eframe::egui;

pub fn draw(ui: &mut egui::Ui, settings: &mut AppSettings, icons: &IconSet, status: &mut String) {
    ui.horizontal(|ui| {
        if let Some(tex) = icons.get("settings-checklist") {
            ui.add(
                egui::Image::new(tex)
                    .fit_to_exact_size(egui::vec2(22.0, 22.0))
                    .sense(egui::Sense::hover()),
            );
        }
        ui.heading("Settings");
    });
    ui.label("Persisted under ~/.config/mqtt-monitor/settings.json");
    ui.add_space(8.0);

    let mut dirty = false;

    if ui
        .checkbox(
            &mut settings.auto_reconnect_on_reload,
            "Auto Reconnect on Reload",
        )
        .changed()
    {
        dirty = true;
    }
    ui.weak("When enabled, profiles that were connected last time reconnect on startup.");

    ui.add_space(12.0);
    ui.strong("Topic tree");
    ui.horizontal(|ui| {
        ui.label("Default expand");
        egui::ComboBox::from_id_salt("tt_expand_mode")
            .selected_text(settings.topic_tree_expand.label())
            .show_ui(ui, |ui| {
                for mode in [
                    TopicTreeExpandMode::ExpandRoot,
                    TopicTreeExpandMode::ExpandAll,
                    TopicTreeExpandMode::CollapseAll,
                ] {
                    if ui
                        .selectable_value(&mut settings.topic_tree_expand, mode, mode.label())
                        .changed()
                    {
                        dirty = true;
                    }
                }
            });
    });
    ui.weak("Controls how new topic-tree branches open.");

    ui.add_space(12.0);
    ui.weak(format!(
        "Remembered connections: {}",
        if settings.last_connected.is_empty() {
            "(none)".into()
        } else {
            settings.last_connected.join(", ")
        }
    ));

    ui.add_space(16.0);
    ui.separator();
    ui.weak("Branding: logo is in the top ribbon. Splash screen on startup is planned later.");

    if dirty {
        match settings.save() {
            Ok(()) => *status = "Settings saved".into(),
            Err(e) => *status = e,
        }
    }

    ui.add_space(8.0);
    if ui.button("Save settings").clicked() {
        match settings.save() {
            Ok(()) => *status = format!("Saved {}", AppSettings::path().display()),
            Err(e) => *status = e,
        }
    }
}
