//! Settings content (shown in a modal; edits a draft until Save).

use crate::mqtt::paths;
use crate::mqtt::settings::{
    AppSettings, StickyLayoutMode, TopicTreeExpandMode, MAX_PACKET_MAX, MAX_PACKET_MIN,
};
use crate::pro::{self, machine_id};
use crate::ui::icons::{
    self, IconSet, CHROME_ACCENT_BORDER, CHROME_ACCENT_SOFT, CHROME_MUTED, CHROME_SECTION,
    CHROME_TEXT,
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum SettingsTab {
    General,
    Pro,
    Updates,
}

impl SettingsTab {
    const ALL: [Self; 3] = [Self::General, Self::Pro, Self::Updates];

    fn label(self) -> &'static str {
        match self {
            Self::General => "General",
            Self::Pro => "Pro",
            Self::Updates => "Updates",
        }
    }
}

/// Live Updates section inputs (not part of the Save draft).
pub struct UpdatesSection<'a> {
    pub status: &'a UpdateStatus,
    pub checking: bool,
    pub check_now: &'a mut bool,
    /// Pro sidecar version lines — `None` when Pro does not answer (hide entirely unless licensed).
    pub pro: Option<&'a crate::pro::update_client::ProUpdateInfo>,
    /// True when a Pro license is installed (show Pro version slot even if sidecar quiet).
    pub pro_enabled: bool,
    /// Set when user clicks Update Pro (only shown if `pro.update_available`).
    pub update_pro: &'a mut bool,
}

/// Theme ComboBox extras — Pro catalog only when licensed + sidecar up.
pub struct ThemeSection<'a> {
    pub pro_catalog: &'a [crate::pro::theme_client::ThemeInfo],
    pub pro_up: bool,
    pub licensed: bool,
    /// Set when the Theme ComboBox selection changes (live preview).
    pub changed_id: &'a mut Option<String>,
}

/// Draw settings body into `draft` (caller commits on Save / discards on Cancel).
pub fn draw(
    ui: &mut egui::Ui,
    draft: &mut AppSettings,
    icons: &IconSet,
    open_shortcuts: &mut bool,
    updates: UpdatesSection<'_>,
    theme: ThemeSection<'_>,
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
    ui.painter().rect_filled(bar, 1.5, CHROME_ACCENT_BORDER());
    ui.add_space(10.0);

    let tab_id = egui::Id::new("settings_tab");
    let mut tab = ui
        .ctx()
        .data(|d| d.get_temp::<SettingsTab>(tab_id))
        .unwrap_or(SettingsTab::General);
    draw_settings_tabs(ui, &mut tab);
    ui.ctx().data_mut(|d| d.insert_temp(tab_id, tab));
    ui.add_space(10.0);

    match tab {
        SettingsTab::General => draw_general_tab(ui, draft, open_shortcuts),
        SettingsTab::Pro => draw_pro_tab(ui, draft, theme),
        SettingsTab::Updates => draw_updates_tab(ui, updates),
    }
}

/// Marketing page for TopicBench Pro (from README / Agile Automation site).
pub const TOPICBENCH_PRO_URL: &str = "https://www.agileautomation.ca/products/topic-bench";

fn draw_settings_tabs(ui: &mut egui::Ui, tab: &mut SettingsTab) {
    // Reasonable tab font — Body-ish, not heading-sized.
    const TAB_SZ: f32 = 14.0;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        for t in SettingsTab::ALL {
            let selected = *tab == t;
            let label = RichText::new(t.label()).size(TAB_SZ).strong();
            let btn = if selected {
                egui::Button::new(label)
                    .fill(CHROME_ACCENT_SOFT())
                    .stroke(egui::Stroke::new(1.0_f32, CHROME_ACCENT_BORDER()))
                    .min_size(egui::vec2(88.0, 28.0))
            } else {
                egui::Button::new(label)
                    .fill(CHROME_SECTION())
                    .min_size(egui::vec2(88.0, 28.0))
            };
            if ui.add(btn).clicked() {
                *tab = t;
            }
        }
    });
}

fn theme_label(id: &str, catalog: &[crate::pro::theme_client::ThemeInfo]) -> String {
    if id == crate::ui::theme::CLASSIC_ID || id.is_empty() {
        return "Classic".into();
    }
    catalog
        .iter()
        .find(|t| t.id == id)
        .map(|t| t.name.clone())
        .unwrap_or_else(|| id.to_string())
}

fn draw_general_tab(ui: &mut egui::Ui, draft: &mut AppSettings, open_shortcuts: &mut bool) {
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
}

/// Pro feature settings only (themes, etc.) — Machine ID lives under Updates.
fn draw_pro_tab(ui: &mut egui::Ui, draft: &mut AppSettings, theme: ThemeSection<'_>) {
    ui.strong("Themes");
    ui.horizontal(|ui| {
        ui.label("Theme");
        let selected_label = theme_label(&draft.theme_id, theme.pro_catalog);
        let before = draft.theme_id.clone();
        egui::ComboBox::from_id_salt("ui_theme_id")
            .selected_text(selected_label)
            .show_ui(ui, |ui| {
                ui.selectable_value(
                    &mut draft.theme_id,
                    crate::ui::theme::CLASSIC_ID.to_string(),
                    "Classic",
                );
                if theme.licensed && theme.pro_up {
                    for t in theme.pro_catalog {
                        ui.selectable_value(&mut draft.theme_id, t.id.clone(), &t.name);
                    }
                }
            });
        // Live preview as soon as the ComboBox selection changes.
        if draft.theme_id != before {
            *theme.changed_id = Some(draft.theme_id.clone());
        }
    });
    if theme.licensed && !theme.pro_up {
        ui.weak("Pro themes appear when TopicBench Pro is running.");
    } else if !theme.licensed {
        ui.weak("Classic only — Pro unlocks Nord, Tokyo Night, Everforest, Latte.");
    } else {
        ui.weak("Pro presets from the sidecar; falls back to Classic if Pro is down.");
    }

    ui.add_space(14.0);
    ui.hyperlink_to(
        RichText::new("Learn about TopicBench Pro").color(CHROME_ACCENT_BORDER()),
        TOPICBENCH_PRO_URL,
    );
}

fn draw_updates_tab(ui: &mut egui::Ui, updates: UpdatesSection<'_>) {
    ui.strong("TopicBench");
    ui.label(
        RichText::new(format!("Current version: {}", update::current_version())).color(CHROME_TEXT()),
    );
    ui.label(RichText::new(updates.status.settings_line()).color(CHROME_MUTED()));

    // Pro lines when licensed / sidecar answers (no Pro URLs/creds in free paths).
    if updates.pro_enabled || updates.pro.is_some() {
        ui.add_space(10.0);
        ui.strong("TopicBench Pro");
        if let Some(pro) = updates.pro {
            let pro_cur = if pro.current.is_empty() {
                "Running (version unknown)".to_string()
            } else {
                format!("Current version: {}", pro.current)
            };
            ui.label(RichText::new(pro_cur).color(CHROME_TEXT()));
            let pro_line = if pro.installing {
                "Installing update…".to_string()
            } else if pro.checking {
                "Checking…".to_string()
            } else if pro.update_available {
                match &pro.latest {
                    Some(v) => format!("Update available: {v}."),
                    None => "Update available.".to_string(),
                }
            } else if let Some(err) = &pro.error {
                format!("Couldn't check: {err}")
            } else if let Some(latest) = &pro.latest {
                format!("Up to date (latest {latest}).")
            } else {
                "Not checked yet.".to_string()
            };
            ui.label(RichText::new(pro_line).color(CHROME_MUTED()));
            if pro.update_available && !pro.installing {
                ui.add_space(2.0);
                if ui
                    .add(egui::Button::new("Update Pro").min_size(egui::vec2(96.0, 24.0)))
                    .clicked()
                {
                    *updates.update_pro = true;
                }
            }
        } else {
            ui.label(
                RichText::new("Pro is licensed but not answering on localhost.")
                    .color(CHROME_MUTED()),
            );
        }
    }

    ui.add_space(10.0);
    let check_label = if updates.checking {
        "Checking…"
    } else if updates.pro_enabled {
        "Check updates"
    } else {
        "Check now"
    };
    let check_btn = {
        let r = ui.add_enabled(
            !updates.checking,
            egui::Button::new(check_label).min_size(egui::vec2(120.0, 28.0)),
        );
        if updates.pro_enabled {
            r.on_hover_text("Check free GitHub releases and ask Pro to check its channel")
        } else {
            r
        }
    };
    if check_btn.clicked() {
        *updates.check_now = true;
    }
    if updates.pro_enabled {
        ui.weak("Probes TopicBench and TopicBench Pro; reports both when up to date.");
    } else {
        ui.weak("Checks the free TopicBench GitHub release channel.");
    }

    // Machine ID for checkout / license — lives with Updates, not Pro features.
    ui.add_space(16.0);
    ui.strong("Machine ID");
    draw_machine_id_controls(ui);
}

/// Monospace ID + Copy / Save license-request (and license-installed note when present).
fn draw_machine_id_controls(ui: &mut egui::Ui) {
    ui.label(
        RichText::new(
            "Paste this into the Machine ID field when you order TopicBench Pro. \
             After payment you’ll get the Pro download and a license.json for this PC.",
        )
        .color(CHROME_MUTED()),
    );
    ui.add_space(4.0);

    if pro::is_licensed() {
        ui.label(
            RichText::new("License installed for this PC.")
                .small()
                .color(CHROME_TEXT()),
        );
        ui.add_space(2.0);
    }

    let mid = machine_id::current_machine_id();
    match mid {
        Ok(id) => {
            // One horizontal row: label + monospace ID + Copy (ID never wraps).
            ui.horizontal(|ui| {
                ui.label(RichText::new("Machine ID").color(CHROME_TEXT()));
                let mut id_view = id.clone();
                let id_w = (ui.available_width() - 56.0).max(120.0);
                ui.add(
                    egui::TextEdit::singleline(&mut id_view)
                        .font(egui::TextStyle::Monospace)
                        .desired_width(id_w)
                        .frame(false)
                        .interactive(false)
                        .text_color(CHROME_TEXT()),
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
                ui.label(RichText::new("Copied.").small().color(CHROME_MUTED()));
            }
        }
        Err(e) => {
            ui.label(
                RichText::new(format!("Could not read machine ID: {e}")).color(CHROME_MUTED()),
            );
        }
    }

    ui.add_space(6.0);
    // Off-thread native save dialog — avoid Linux Wayland "app hung" ANR.
    let license_dlg = egui::Id::new("dlg_license_request");
    if ui
        .button("Save license-request.json…")
        .on_hover_text(
            "Optional support file (same shape Pro writes). Prefer pasting Machine ID at checkout.",
        )
        .clicked()
    {
        let default_name = pro::LICENSE_REQUEST_FILE_NAME;
        let dir = paths::config_dir();
        crate::ui::native_dialog::begin_path(
            ui.ctx(),
            license_dlg,
            crate::ui::native_dialog::PathOp::SaveFile,
            move |d| d.set_file_name(default_name).set_directory(dir),
        );
    }
    if let Some(Some(path)) = crate::ui::native_dialog::poll_path(ui.ctx(), license_dlg) {
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
    if let Some(msg) = ui.memory(|m| {
        m.data
            .get_temp::<String>(egui::Id::new("settings_mid_save_msg"))
    }) {
        ui.label(RichText::new(msg).small().color(CHROME_MUTED()));
    }
}
