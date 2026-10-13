//! Settings page: auto-reconnect + topic tree expand defaults.

use crate::mqtt::settings::{AppSettings, TopicTreeExpandMode};
use eframe::egui;

pub fn draw(ui: &mut egui::Ui, settings: &mut AppSettings, status: &mut String) {
    ui.heading("Settings");
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
    ui.weak("Controls how new topic-tree branches open. Use Expand all / Collapse all on the tree for one-shot actions.");

    ui.add_space(12.0);
    ui.weak(format!(
        "Remembered connections: {}",
        if settings.last_connected.is_empty() {
            "(none)".into()
        } else {
            settings.last_connected.join(", ")
        }
    ));

    // Future branding note (splash not built yet).
    ui.add_space(16.0);
    ui.separator();
    ui.weak("Branding: logo is in the top bar. Splash screen on startup is planned later.");

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
