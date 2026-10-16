//! Settings content (shown in a modal; edits a draft until Save).

use crate::mqtt::settings::{
    AppSettings, StickyLayoutMode, TopicTreeExpandMode, MAX_PACKET_MAX, MAX_PACKET_MIN,
};
use crate::ui::icons::{self, IconSet, CHROME_ACCENT_BORDER};
use crate::ui::shortcuts;
use eframe::egui::{self, Sense};

/// Sticky board column count (Settings Layout dropdown).
#[derive(Clone, Copy, PartialEq, Eq)]
enum StickyLayoutChoice {
    Cols2,
    Cols3,
    Cols4,
}

impl StickyLayoutChoice {
    fn label(self) -> &'static str {
        match self {
            Self::Cols2 => "2 cols",
            Self::Cols3 => "3 cols",
            Self::Cols4 => "4 cols",
        }
    }

    fn from_settings(s: &AppSettings) -> Self {
        // Legacy "Rows" maps to 2 cols in the UI.
        match s.sticky_layout {
            StickyLayoutMode::Rows => Self::Cols2,
            StickyLayoutMode::Columns => match AppSettings::clamp_sticky_columns(s.sticky_columns) {
                4 => Self::Cols4,
                3 => Self::Cols3,
                _ => Self::Cols2,
            },
        }
    }

    fn apply(self, s: &mut AppSettings) {
        s.sticky_layout = StickyLayoutMode::Columns;
        s.sticky_columns = match self {
            Self::Cols2 => 2,
            Self::Cols3 => 3,
            Self::Cols4 => 4,
        };
    }
}

/// Draw settings body into `draft` (caller commits on Save / discards on Cancel).
pub fn draw(
    ui: &mut egui::Ui,
    draft: &mut AppSettings,
    icons: &IconSet,
    open_shortcuts: &mut bool,
) {
    icons::section_header_bar(ui, |ui| {
        icons::section_icon(ui, icons, "configuration_sliders");
        ui.heading("Settings");
    });
    ui.label("Persisted under ~/.config/mqtt-monitor/settings.json");

    // Accent bar under title + persist line.
    ui.add_space(6.0);
    let bar_w = ui.available_width();
    let (bar, _) = ui.allocate_exact_size(egui::vec2(bar_w, 3.0), Sense::hover());
    ui.painter().rect_filled(bar, 1.5, CHROME_ACCENT_BORDER);
    ui.add_space(10.0);

    ui.checkbox(
        &mut draft.auto_reconnect_on_reload,
        "Auto Reconnect on Reload",
    );
    ui.weak("Reconnect last-connected profiles on startup.");

    ui.add_space(12.0);
    ui.strong("Topic tree");
    ui.horizontal(|ui| {
        ui.label("Expand on startup");
        egui::ComboBox::from_id_salt("tt_expand_mode")
            .selected_text(draft.topic_tree_expand.label())
            .show_ui(ui, |ui| {
                for mode in [
                    TopicTreeExpandMode::Expand1,
                    TopicTreeExpandMode::Expand2,
                    TopicTreeExpandMode::Expand3,
                    TopicTreeExpandMode::ExpandAll,
                ] {
                    ui.selectable_value(&mut draft.topic_tree_expand, mode, mode.label());
                }
            });
    });
    ui.weak("Initial expand depth when topics appear.");

    ui.add_space(12.0);
    ui.strong("Sticky topics");
    // Normalize legacy Rows → 2-column grid so Save persists the new model.
    if draft.sticky_layout == StickyLayoutMode::Rows {
        StickyLayoutChoice::Cols2.apply(draft);
    }
    ui.horizontal(|ui| {
        ui.label("Layout");
        let mut choice = StickyLayoutChoice::from_settings(draft);
        egui::ComboBox::from_id_salt("sticky_layout_choice")
            .selected_text(choice.label())
            .show_ui(ui, |ui| {
                for opt in [
                    StickyLayoutChoice::Cols2,
                    StickyLayoutChoice::Cols3,
                    StickyLayoutChoice::Cols4,
                ] {
                    if ui
                        .selectable_value(&mut choice, opt, opt.label())
                        .changed()
                    {
                        choice.apply(draft);
                    }
                }
            });
    });

    ui.add_space(12.0);
    ui.strong("MQTT");
    ui.horizontal(|ui| {
        ui.label("Max packet size");
        let mut kb = draft.max_packet_kb() as u32;
        let min_kb = (MAX_PACKET_MIN / 1024) as u32;
        let max_kb = (MAX_PACKET_MAX / 1024) as u32;
        if ui
            .add(
                egui::DragValue::new(&mut kb)
                    .range(min_kb..=max_kb)
                    .suffix(" KB")
                    .speed(1.0),
            )
            .changed()
        {
            draft.set_max_packet_kb(kb as usize);
        }
    });
    ui.weak("Soft skip limit (2 KB–1 MB). Applied on next Connect.");

    ui.add_space(12.0);
    ui.strong("Keyboard");
    if ui
        .button("Keyboard shortcuts…")
        .on_hover_text(shortcuts::tip("Open Keyboard shortcuts help", "F1 / ?"))
        .clicked()
    {
        *open_shortcuts = true;
    }
    ui.weak("Also F1 or ? when not typing.");
}
