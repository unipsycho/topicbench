//! Settings content (shown in a modal; edits a draft until Save).

use crate::mqtt::paths;
use crate::mqtt::settings::{
    AppSettings, StickyLayoutMode, TopicTreeExpandMode, MAX_PACKET_MAX, MAX_PACKET_MIN,
};
use crate::pro::{self, machine_id};
use crate::ui::icons::{
    self, IconSet, CHROME_ACCENT_BORDER, CHROME_ACCENT_SOFT, CHROME_MUTED, CHROME_TEXT,
};
use crate::ui::shortcuts;
use crate::update::{self, UpdateStatus};
use eframe::egui::{self, RichText, Sense};

/// Sticky board layout (Settings dropdown): one sticky per row, or a column grid.
#[derive(Clone, Copy, PartialEq, Eq)]
enum StickyLayoutChoice {
    Rows,
    Cols2,
    Cols3,
    Cols4,
}

impl StickyLayoutChoice {
    fn label(self) -> &'static str {
        match self {
            Self::Rows => "Rows",
            Self::Cols2 => "2 cols",
            Self::Cols3 => "3 cols",
            Self::Cols4 => "4 cols",
        }
    }

    fn from_settings(s: &AppSettings) -> Self {
        match s.sticky_layout {
            StickyLayoutMode::Rows => Self::Rows,
            StickyLayoutMode::Columns => match AppSettings::clamp_sticky_columns(s.sticky_columns) {
                4 => Self::Cols4,
                3 => Self::Cols3,
                _ => Self::Cols2,
            },
        }
    }

    fn apply(self, s: &mut AppSettings) {
        match self {
            Self::Rows => {
                s.sticky_layout = StickyLayoutMode::Rows;
            }
            Self::Cols2 | Self::Cols3 | Self::Cols4 => {
                s.sticky_layout = StickyLayoutMode::Columns;
                s.sticky_columns = match self {
                    Self::Cols2 => 2,
                    Self::Cols3 => 3,
                    Self::Cols4 => 4,
                    Self::Rows => unreachable!(),
                };
            }
        }
    }
}

/// Live Updates section inputs (not part of the Save draft).
pub struct UpdatesSection<'a> {
    pub status: &'a UpdateStatus,
    pub checking: bool,
    pub check_now: &'a mut bool,
}

/// Draw settings body into `draft` (caller commits on Save / discards on Cancel).
pub fn draw(
    ui: &mut egui::Ui,
    draft: &mut AppSettings,
    icons: &IconSet,
    open_shortcuts: &mut bool,
    updates: UpdatesSection<'_>,
) {
    icons::section_header_bar(ui, |ui| {
        icons::section_icon(ui, icons, "configuration_sliders");
        icons::section_title(ui, "Settings");
    });
    ui.label("Persisted under ~/.config/topicbench/settings.json (Windows portable: .\\topicbench\\)");

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
    ui.horizontal(|ui| {
        ui.label("Layout");
        let mut choice = StickyLayoutChoice::from_settings(draft);
        egui::ComboBox::from_id_salt("sticky_layout_choice")
            .selected_text(choice.label())
            .show_ui(ui, |ui| {
                for opt in [
                    StickyLayoutChoice::Rows,
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
    ui.weak("Rows: one sticky per line under each broker.");

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

    // Updates | Pro — narrow Updates; Pro claims remaining width for one-line machine ID.
    ui.add_space(12.0);
    const UPDATES_W: f32 = 168.0;
    const GAP: f32 = 16.0;
    ui.horizontal(|ui| {
        // Nested verticals required — children of `horizontal` inherit row layout.
        ui.vertical(|ui| {
            ui.set_width(UPDATES_W);
            draw_updates_column(ui, updates);
        });
        ui.add_space(GAP);
        let pro_w = ui.available_width();
        ui.vertical(|ui| {
            ui.set_min_width(pro_w);
            ui.set_max_width(pro_w);
            draw_pro_interest_column(ui);
        });
    });
}

/// Marketing page for TopicBench Pro (from README / Agile Automation site).
pub const TOPICBENCH_PRO_URL: &str = "https://www.agileautomation.ca/products/topic-bench";

fn draw_updates_column(ui: &mut egui::Ui, updates: UpdatesSection<'_>) {
    ui.strong("Updates");
    ui.label(
        RichText::new(format!("Current version: {}", update::current_version())).color(CHROME_TEXT),
    );
    ui.label(RichText::new(updates.status.settings_line()).color(CHROME_MUTED));
    ui.add_space(4.0);
    let check_btn = ui.add_enabled(
        !updates.checking,
        egui::Button::new(if updates.checking {
            "Checking…"
        } else {
            "Check now"
        })
        .min_size(egui::vec2(96.0, 28.0)),
    );
    if check_btn.clicked() {
        *updates.check_now = true;
    }
}

/// Pro interest column (OSS alone — no Pro sidecar). Collapsed = one button; expand in place.
fn draw_pro_interest_column(ui: &mut egui::Ui) {
    let reveal_id = egui::Id::new("settings_pro_reveal_mid");
    // Screenshot / demo: TOPICBENCH_EXPAND_PRO=1 starts expanded (no Pro install needed).
    if std::env::var_os("TOPICBENCH_EXPAND_PRO").is_some() {
        ui.memory_mut(|m| {
            m.data.insert_temp(reveal_id, true);
        });
    }
    let revealed = ui.memory(|m| m.data.get_temp::<bool>(reveal_id).unwrap_or(false));

    if !revealed {
        // Collapsed: only the interest button — no Pro copy / machine ID yet.
        if ui
            .add(
                egui::Button::new(RichText::new("Interested in TopicBench Pro?").strong())
                    .fill(CHROME_ACCENT_SOFT)
                    .stroke(egui::Stroke::new(1.0_f32, CHROME_ACCENT_BORDER))
                    .min_size(egui::vec2(0.0, 28.0)),
            )
            .on_hover_text("Show TopicBench Pro details and this PC’s machine ID")
            .clicked()
        {
            // First click expands only — website opens from the link inside.
            ui.memory_mut(|m| {
                m.data.insert_temp(reveal_id, true);
            });
        }
        return;
    }

    ui.strong("TopicBench Pro");
    ui.hyperlink_to(
        RichText::new("Learn about TopicBench Pro").color(CHROME_ACCENT_BORDER),
        TOPICBENCH_PRO_URL,
    );
    ui.add_space(6.0);
    draw_machine_id_controls(ui);
}

/// Monospace ID + Copy / Save license-request (and license-installed note when present).
fn draw_machine_id_controls(ui: &mut egui::Ui) {
    ui.label(
        RichText::new(
            "Paste this into the Machine ID field when you order TopicBench Pro. \
             After payment you’ll get the Pro download and a license.json for this PC.",
        )
        .color(CHROME_MUTED),
    );
    ui.add_space(4.0);

    if pro::is_licensed() {
        ui.label(
            RichText::new("License installed for this PC.")
                .small()
                .color(CHROME_TEXT),
        );
        ui.add_space(2.0);
    }

    let mid = machine_id::current_machine_id();
    match mid {
        Ok(id) => {
            // One horizontal row: label + monospace ID + Copy (ID never wraps).
            ui.horizontal(|ui| {
                ui.label(RichText::new("Machine ID").color(CHROME_TEXT));
                let mut id_view = id.clone();
                let id_w = (ui.available_width() - 56.0).max(120.0);
                ui.add(
                    egui::TextEdit::singleline(&mut id_view)
                        .font(egui::TextStyle::Monospace)
                        .desired_width(id_w)
                        .frame(false)
                        .interactive(false)
                        .text_color(CHROME_TEXT),
                );
                if ui
                    .button("Copy")
                    .on_hover_text("Copy machine ID to clipboard")
                    .clicked()
                {
                    ui.ctx().copy_text(id.clone());
                    ui.memory_mut(|m| {
                        m.data.insert_temp(egui::Id::new("settings_mid_copied"), true);
                    });
                }
            });
            if ui.memory(|m| {
                m.data
                    .get_temp::<bool>(egui::Id::new("settings_mid_copied"))
                    .unwrap_or(false)
            }) {
                ui.label(RichText::new("Copied.").small().color(CHROME_MUTED));
            }
        }
        Err(e) => {
            ui.label(
                RichText::new(format!("Could not read machine ID: {e}")).color(CHROME_MUTED),
            );
        }
    }

    ui.add_space(6.0);
    if ui
        .button("Save license-request.json…")
        .on_hover_text(
            "Optional support file (same shape Pro writes). Prefer pasting Machine ID at checkout.",
        )
        .clicked()
    {
        let default_name = pro::LICENSE_REQUEST_FILE_NAME;
        if let Some(path) = rfd::FileDialog::new()
            .set_file_name(default_name)
            .set_directory(paths::config_dir())
            .save_file()
        {
            let msg = match pro::LicenseRequestV1::for_this_machine(None) {
                Ok(req) => match pro::write_license_request(&path, &req) {
                    Ok(()) => format!("Saved {}", path.display()),
                    Err(e) => format!("Save failed: {e}"),
                },
                Err(e) => format!("Request failed: {e}"),
            };
            ui.memory_mut(|m| {
                m.data
                    .insert_temp(egui::Id::new("settings_mid_save_msg"), msg);
            });
        }
    }
    if let Some(msg) = ui.memory(|m| {
        m.data
            .get_temp::<String>(egui::Id::new("settings_mid_save_msg"))
    }) {
        ui.label(RichText::new(msg).small().color(CHROME_MUTED));
    }
}
