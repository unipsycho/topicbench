//! Settings content (shown in its own viewport window).

use crate::mqtt::settings::{
    AppSettings, StickyLayoutMode, TopicTreeExpandMode, MAX_PACKET_MAX, MAX_PACKET_MIN,
};
use crate::ui::icons::IconSet;
use crate::ui::shortcuts;
use eframe::egui;

pub fn draw(
    ui: &mut egui::Ui,
    settings: &mut AppSettings,
    icons: &IconSet,
    status: &mut String,
    open_shortcuts: &mut bool,
) {
    ui.horizontal(|ui| {
        if let Some(tex) = icons.get("configuration_sliders") {
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
    ui.strong("Sticky topics");
    ui.horizontal(|ui| {
        ui.label("Layout");
        egui::ComboBox::from_id_salt("sticky_layout_mode")
            .selected_text(settings.sticky_layout.label())
            .show_ui(ui, |ui| {
                for mode in [StickyLayoutMode::Rows, StickyLayoutMode::Columns] {
                    if ui
                        .selectable_value(&mut settings.sticky_layout, mode, mode.label())
                        .changed()
                    {
                        dirty = true;
                    }
                }
            });
    });
    if settings.sticky_layout == StickyLayoutMode::Columns {
        ui.horizontal(|ui| {
            ui.label("Columns");
            for n in [2_u8, 3, 4] {
                if ui
                    .selectable_value(&mut settings.sticky_columns, n, n.to_string())
                    .changed()
                {
                    settings.sticky_columns = AppSettings::clamp_sticky_columns(n);
                    dirty = true;
                }
            }
        });
        ui.weak("More columns give each sticky less width — better for comparing several JSON payloads.");
    } else {
        ui.weak("Rows stack stickies full-width (best for long JSON).");
    }

    ui.add_space(12.0);
    ui.strong("Font zoom");
    ui.label(format!(
        "Step {} (range −4…+5). Change with Zoom− / Zoom+ on the ribbon.",
        settings.font_zoom_step
    ));
    ui.weak("Scales egui text styles app-wide; persisted with other settings.");

    ui.add_space(12.0);
    ui.strong("MQTT");
    ui.horizontal(|ui| {
        ui.label("Max packet size");
        let mut kb = settings.max_packet_kb() as u32;
        let min_kb = (MAX_PACKET_MIN / 1024) as u32;
        let max_kb = (MAX_PACKET_MAX / 1024) as u32;
        let resp = ui.add(
            egui::DragValue::new(&mut kb)
                .range(min_kb..=max_kb)
                .suffix(" KB")
                .speed(1.0),
        );
        if resp.changed() {
            settings.set_max_packet_kb(kb as usize);
            dirty = true;
        }
    });
    ui.weak(
        "Caps rumqttc decode size (2 KB–1 MB). Applied on the next Connect — reconnect after changing. A `#` flood with a larger retained/live packet will end the session until this is raised.",
    );

    ui.add_space(12.0);
    ui.strong("Keyboard");
    if ui
        .button("Keyboard shortcuts…")
        .on_hover_text(shortcuts::tip("Open Keyboard shortcuts help", "F1 / ?"))
        .clicked()
    {
        *open_shortcuts = true;
    }
    ui.weak("Also F1 or ? (when not typing). Lists every binding.");

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
