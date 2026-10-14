//! Top-level eframe app: Office-style ribbon chrome, optional panels, settings viewport.
//!
//! Ribbon layout cues from PhotoCraft ui-egui (chrome fill, group separators, quiet hover)
//! plus Word-style labeled groups with captions under the commands.

use crate::mqtt::pins::PinBoard;
use crate::mqtt::profile::ProfileStore;
use crate::mqtt::publish::{PublishDraft, PublishHistory};
use crate::mqtt::session::SessionManager;
use crate::mqtt::settings::AppSettings;
use crate::mqtt::subscribe;
use crate::ui::icons::{
    self, IconSet, CHROME_BG, CHROME_SEP, CHROME_TEXT, STATUS_BG, TINT_APP, TINT_PANELS,
    TINT_PROFILES,
};
use crate::ui::profiles::ProfileListFilter;
use crate::ui::search_bar::SearchFilter;
use crate::ui::shortcuts::{self, Action as ShortcutAction};
use crate::ui::{
    payload_panel, pin_board, profiles, publish_panel, search_bar, settings_panel, topic_tree_panel,
    traffic_panel,
};
use crate::ui::flash;
use eframe::egui::{self, Color32, FontId, RichText, TextStyle, TextureHandle};
use image::RgbaImage;
use std::collections::BTreeMap;
use std::time::Instant;

/// Profiles left panel: resize handle cannot drag narrower than this (no see-through gap).
const PROFILES_PANEL_MIN_W: f32 = 240.0;

/// Which optional panels are visible (ribbon toggles).
#[derive(Clone, Debug)]
struct PanelVisibility {
    /// Left profiles side panel (toggle from Profiles ribbon group).
    profiles: bool,
    /// Off by default — doesn't dominate the UI.
    live_traffic: bool,
    stickies: bool,
    topic_tree: bool,
    /// Off by default — only when publishing.
    publish: bool,
    /// Off by default — Value view for the selected tree topic.
    payload: bool,
}

impl Default for PanelVisibility {
    fn default() -> Self {
        Self {
            profiles: true,
            live_traffic: false,
            stickies: true,
            topic_tree: true,
            publish: false,
            payload: false,
        }
    }
}

/// Root UI state for the MQTT Explorer–style monitor.
pub struct MqttMonitorApp {
    pub profiles: ProfileStore,
    pub sessions: SessionManager,
    pub pins: PinBoard,
    pub publish: PublishDraft,
    pub publish_history: PublishHistory,
    pub settings: AppSettings,
    pub profile_filter: ProfileListFilter,
    pub search: SearchFilter,
    /// Draft topic filter while editing a profile’s subscriptions.
    pub sub_draft: String,
    pub status: String,
    logo: Option<TextureHandle>,
    icons: IconSet,
    panels: PanelVisibility,
    settings_open: bool,
    shortcuts_open: bool,
    focus_search: bool,
    focus_profile_editor: bool,
    /// Profiles panel was opened by New (auto-close on next Save).
    profiles_auto_opened: bool,
    expand_all_once: Option<bool>,
    auto_reconnect_done: bool,
}

impl MqttMonitorApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let profiles = ProfileStore::load_or_sample();
        let settings = AppSettings::load();
        let status = profiles.last_io.clone();
        apply_font_zoom(&cc.egui_ctx, settings.font_zoom_factor());
        // Use our font-zoom chords; disable egui’s pixels_per_point zoom.
        cc.egui_ctx.options_mut(|o| o.zoom_with_keyboard = false);
        let logo = load_logo(&cc.egui_ctx);
        let icons = IconSet::load(&cc.egui_ctx);
        Self {
            profiles,
            sessions: SessionManager::default(),
            pins: PinBoard::default(),
            publish: PublishDraft::default(),
            publish_history: PublishHistory::load(),
            settings,
            profile_filter: ProfileListFilter::All,
            search: SearchFilter::default(),
            sub_draft: String::new(),
            status,
            logo,
            icons,
            panels: PanelVisibility::default(),
            settings_open: false,
            shortcuts_open: false,
            focus_search: false,
            focus_profile_editor: false,
            profiles_auto_opened: false,
            expand_all_once: None,
            auto_reconnect_done: false,
        }
    }

    /// New profile: auto-show Profiles if hidden; remember for Save auto-close.
    fn action_new_profile(&mut self) {
        if !self.panels.profiles {
            self.panels.profiles = true;
            self.profiles_auto_opened = true;
        }
        let id = self.profiles.add_new();
        self.status = format!("Created profile {id}");
        self.focus_profile_editor = true;
    }

    /// Save profiles; if New auto-opened the panel, close it again.
    fn action_save_profiles(&mut self) {
        match self.profiles.save() {
            Ok(()) => self.status = self.profiles.last_io.clone(),
            Err(e) => self.status = e,
        }
        if self.profiles_auto_opened {
            self.panels.profiles = false;
            self.profiles_auto_opened = false;
        }
    }

    fn run_auto_reconnect(&mut self, ctx: &egui::Context) {
        if self.auto_reconnect_done {
            return;
        }
        self.auto_reconnect_done = true;
        if !self.settings.auto_reconnect_on_reload {
            return;
        }
        let ids = self.settings.last_connected.clone();
        let mut n = 0;
        for id in ids {
            if let Some(mut profile) = self.profiles.profiles.get(&id).cloned() {
                if subscribe::prune_redundant(&mut profile.subscriptions) {
                    if let Some(p) = self.profiles.profiles.get_mut(&id) {
                        p.subscriptions = profile.subscriptions.clone();
                    }
                    let _ = self.profiles.save();
                }
                let max_pkt = AppSettings::clamp_max_packet(self.settings.max_packet_bytes);
                if self.sessions.connect(&profile, max_pkt, ctx).is_ok() {
                    self.pins.restore_from_profile(&profile);
                    n += 1;
                }
            }
        }
        if n > 0 {
            self.status = format!("Auto-reconnect: connecting {n} profile(s)");
        }
    }

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        for action in shortcuts::consume(ctx) {
            match action {
                ShortcutAction::ToggleProfiles => {
                    self.panels.profiles = !self.panels.profiles;
                }
                ShortcutAction::ToggleTraffic => {
                    self.panels.live_traffic = !self.panels.live_traffic;
                }
                ShortcutAction::TogglePublish => {
                    self.panels.publish = !self.panels.publish;
                }
                ShortcutAction::TogglePayload => {
                    self.panels.payload = !self.panels.payload;
                }
                ShortcutAction::ToggleStickies => {
                    self.panels.stickies = !self.panels.stickies;
                }
                ShortcutAction::ToggleTopicTree => {
                    self.panels.topic_tree = !self.panels.topic_tree;
                }
                ShortcutAction::ZoomIn => {
                    if self.settings.zoom_font_in() {
                        let _ = self.settings.save();
                        apply_font_zoom(ctx, self.settings.font_zoom_factor());
                        self.status = format!("Font zoom {}", self.settings.font_zoom_step);
                    }
                }
                ShortcutAction::ZoomOut => {
                    if self.settings.zoom_font_out() {
                        let _ = self.settings.save();
                        apply_font_zoom(ctx, self.settings.font_zoom_factor());
                        self.status = format!("Font zoom {}", self.settings.font_zoom_step);
                    }
                }
                ShortcutAction::ZoomReset => {
                    if self.settings.zoom_font_reset() {
                        let _ = self.settings.save();
                        apply_font_zoom(ctx, self.settings.font_zoom_factor());
                        self.status = "Font zoom 0".into();
                    }
                }
                ShortcutAction::OpenSettings => {
                    self.settings_open = true;
                }
                ShortcutAction::OpenShortcutsHelp => {
                    self.shortcuts_open = true;
                }
                ShortcutAction::FocusSearch => {
                    self.focus_search = true;
                }
                ShortcutAction::NewProfile => {
                    self.action_new_profile();
                }
                ShortcutAction::SaveProfiles => {
                    self.action_save_profiles();
                }
                ShortcutAction::DeleteProfile => {
                    // Delete only when Profiles panel is visible.
                    if self.panels.profiles {
                        if let Some(id) = self.profiles.delete_selected() {
                            self.sessions.close(&id);
                            self.status = format!("Deleted profile {id}");
                        }
                    }
                }
                ShortcutAction::ConnectToggle => {
                    self.shortcut_connect_toggle(ctx);
                }
                ShortcutAction::ShowProfiles => {
                    self.panels.profiles = true;
                    self.profiles_auto_opened = false;
                }
                ShortcutAction::FocusProfileEditor => {
                    self.panels.profiles = true;
                    self.profiles_auto_opened = false;
                    self.focus_profile_editor = true;
                }
                ShortcutAction::ProfileSelectPrev => {
                    self.move_profile_selection(-1);
                }
                ShortcutAction::ProfileSelectNext => {
                    self.move_profile_selection(1);
                }
                ShortcutAction::ToggleStickySelectedTopic => {
                    self.shortcut_toggle_sticky();
                }
            }
        }
    }

    fn move_profile_selection(&mut self, delta: i32) {
        let ids: Vec<String> = self.profiles.profiles.keys().cloned().collect();
        if ids.is_empty() {
            return;
        }
        let cur = self
            .profiles
            .selected_id
            .as_ref()
            .and_then(|id| ids.iter().position(|k| k == id))
            .unwrap_or(0);
        let next = (cur as i32 + delta).clamp(0, ids.len() as i32 - 1) as usize;
        self.profiles.selected_id = Some(ids[next].clone());
        self.panels.profiles = true;
    }

    fn shortcut_connect_toggle(&mut self, ctx: &egui::Context) {
        let Some(id) = self.profiles.selected_id.clone() else {
            self.status = "No profile selected".into();
            return;
        };
        let Some(profile) = self.profiles.profiles.get(&id).cloned() else {
            return;
        };
        let connecting = match self.sessions.open.get(&id).map(|s| s.state) {
            Some(crate::mqtt::session::SessionState::Connected)
            | Some(crate::mqtt::session::SessionState::Connecting) => false,
            _ => true,
        };
        let max_pkt = AppSettings::clamp_max_packet(self.settings.max_packet_bytes);
        match self.sessions.toggle(&profile, max_pkt, ctx) {
            Ok(msg) => {
                self.settings.remember_connected(&id, connecting);
                if connecting {
                    let pruned = self
                        .profiles
                        .profiles
                        .get_mut(&id)
                        .map(|p| subscribe::prune_redundant(&mut p.subscriptions))
                        .unwrap_or(false);
                    if pruned {
                        let _ = self.profiles.save();
                    }
                    if let Some(p) = self.profiles.profiles.get(&id) {
                        self.pins.restore_from_profile(p);
                    }
                }
                self.status = msg;
            }
            Err(e) => self.status = e,
        }
    }

    fn shortcut_toggle_sticky(&mut self) {
        let Some((profile_id, profile_name, topic)) = self.pins.last_tree_focus.clone() else {
            self.status = "Click a topic tree row first (Ctrl+Shift+S)".into();
            return;
        };
        if self.pins.is_sticky(&profile_id, &topic) {
            self.pins.unpin(&profile_id, &topic);
            self.profiles
                .set_sticky_and_save(&profile_id, &topic, false);
            self.status = format!("Unstuck {topic}");
        } else {
            self.pins.pin(&profile_id, &profile_name, &topic);
            self.profiles.set_sticky_and_save(&profile_id, &topic, true);
            self.status = format!("Sticky {topic}");
        }
    }

    fn draw_ribbon(&mut self, ctx: &egui::Context) {
        // Dark chrome. Groups shrink-wrap (see icons::ribbon_group) so Profiles cannot
        // expand across the bar and leave Panels/Settings with zero width.
        egui::TopBottomPanel::top("ribbon")
            .resizable(false)
            .show_separator_line(false)
            .frame(
                egui::Frame::NONE
                    .fill(CHROME_BG)
                    .inner_margin(egui::Margin::symmetric(8, 4))
                    .stroke(egui::Stroke::new(1.0_f32, CHROME_SEP)),
            )
            .show(ctx, |ui| {
                ui.visuals_mut().override_text_color = Some(CHROME_TEXT);
                ui.spacing_mut().item_spacing.y = 0.0;

                // Top-align groups so large icons sit snug (horizontal() centers vertically).
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Min), |ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;

                    // Brand stack: fixed column width — nested vertical otherwise expands
                    // and steals the rest of the ribbon row (hiding Profiles/Panels/App).
                    ui.vertical(|ui| {
                        let logo_w = self
                            .logo
                            .as_ref()
                            .map(|l| l.size_vec2().x)
                            .unwrap_or(108.0);
                        let brand_w = logo_w.max(118.0);
                        ui.set_width(brand_w);
                        ui.spacing_mut().item_spacing.y = 0.0;
                        ui.vertical_centered(|ui| {
                            ui.label(
                                RichText::new("MQTT Master")
                                    .strong()
                                    .size(17.0)
                                    // Same green as Connected status.
                                    .color(Color32::from_rgb(90, 190, 120)),
                            );
                            // Padding under title before logo
                            ui.add_space(6.0);
                            if let Some(logo) = &self.logo {
                                let size = logo.size_vec2();
                                ui.add(
                                    egui::Image::new(logo)
                                        .fit_to_exact_size(size)
                                        .maintain_aspect_ratio(true)
                                        .sense(egui::Sense::hover()),
                                );
                            }
                        });
                    });

                    icons::ribbon_separator(ui);

                    // Profiles: New / Save / Delete + Profiles side-panel toggle
                    icons::ribbon_group(ui, "Profiles", TINT_PROFILES, |ui| {
                        if self
                            .icons
                            .command(
                                ui,
                                "profile_add",
                                "New",
                                false,
                                &shortcuts::tip("New profile", "Ctrl+N"),
                            )
                            .clicked()
                        {
                            self.action_new_profile();
                        }
                        if self
                            .icons
                            .command(
                                ui,
                                "save",
                                "Save",
                                false,
                                &shortcuts::tip("Save profiles", "Ctrl+S"),
                            )
                            .clicked()
                        {
                            self.action_save_profiles();
                        }
                        // Delete disabled while Profiles panel is hidden.
                        if self
                            .icons
                            .command_enabled(
                                ui,
                                "disconnect",
                                "Delete",
                                false,
                                &shortcuts::tip(
                                    "Delete selected profile",
                                    "Delete / Ctrl+Backspace",
                                ),
                                self.panels.profiles,
                            )
                            .clicked()
                        {
                            if let Some(id) = self.profiles.delete_selected() {
                                self.sessions.close(&id);
                                self.status = format!("Deleted profile {id}");
                            }
                        }
                        if self
                            .icons
                            .command(
                                ui,
                                "panel_docked",
                                "Profiles",
                                self.panels.profiles,
                                &shortcuts::tip(
                                    "Show or hide the profiles side panel",
                                    "Ctrl+Shift+P",
                                ),
                            )
                            .clicked()
                        {
                            self.panels.profiles = !self.panels.profiles;
                            // User took control — don't auto-close on Save.
                            self.profiles_auto_opened = false;
                        }
                    });

                    icons::ribbon_separator(ui);

                    // Panels: Traffic / Publish / Payload / Stickies / Tree
                    icons::ribbon_group(ui, "Panels", TINT_PANELS, |ui| {
                        if self
                            .icons
                            .command(
                                ui,
                                "incoming_messages",
                                "Traffic",
                                self.panels.live_traffic,
                                &shortcuts::tip("Live traffic", "Ctrl+Shift+T"),
                            )
                            .clicked()
                        {
                            self.panels.live_traffic = !self.panels.live_traffic;
                        }
                        if self
                            .icons
                            .command(
                                ui,
                                "publish_message",
                                "Publish",
                                self.panels.publish,
                                &shortcuts::tip("Publish panel", "Ctrl+Shift+U"),
                            )
                            .clicked()
                        {
                            self.panels.publish = !self.panels.publish;
                        }
                        if self
                            .icons
                            .command(
                                ui,
                                "panel_undock",
                                "Payload",
                                self.panels.payload,
                                &shortcuts::tip("Payload panel", "Ctrl+Shift+L"),
                            )
                            .clicked()
                        {
                            self.panels.payload = !self.panels.payload;
                        }
                        if self
                            .icons
                            .command(
                                ui,
                                "sticky_topic",
                                "Stickies",
                                self.panels.stickies,
                                &shortcuts::tip("Sticky topics", "Ctrl+Shift+K"),
                            )
                            .clicked()
                        {
                            self.panels.stickies = !self.panels.stickies;
                        }
                        if self
                            .icons
                            .command(
                                ui,
                                "topic_tree",
                                "Tree",
                                self.panels.topic_tree,
                                &shortcuts::tip("Topic tree", "Ctrl+Shift+R"),
                            )
                            .clicked()
                        {
                            self.panels.topic_tree = !self.panels.topic_tree;
                        }
                    });

                    icons::ribbon_separator(ui);

                    // App: Settings + font zoom (−4…+5 steps from default).
                    icons::ribbon_group(ui, "App", TINT_APP, |ui| {
                        if self
                            .icons
                            .command(
                                ui,
                                "configuration_sliders",
                                "Settings",
                                self.settings_open,
                                &shortcuts::tip("Open settings window", "Ctrl+,"),
                            )
                            .clicked()
                        {
                            self.settings_open = true;
                        }
                        if self
                            .icons
                            .command(
                                ui,
                                "zoom_out",
                                "Zoom−",
                                false,
                                &shortcuts::tip(
                                    "Decrease font size (−4 steps from default)",
                                    "Ctrl+-",
                                ),
                            )
                            .clicked()
                        {
                            if self.settings.zoom_font_out() {
                                let _ = self.settings.save();
                                apply_font_zoom(ctx, self.settings.font_zoom_factor());
                                self.status = format!(
                                    "Font zoom {}",
                                    self.settings.font_zoom_step
                                );
                            }
                        }
                        if self
                            .icons
                            .command(
                                ui,
                                "zoom_in",
                                "Zoom+",
                                false,
                                &shortcuts::tip(
                                    "Increase font size (+5 steps from default)",
                                    "Ctrl+= / Ctrl++",
                                ),
                            )
                            .clicked()
                        {
                            if self.settings.zoom_font_in() {
                                let _ = self.settings.save();
                                apply_font_zoom(ctx, self.settings.font_zoom_factor());
                                self.status = format!(
                                    "Font zoom {}",
                                    self.settings.font_zoom_step
                                );
                            }
                        }
                    });
                });
            });

        // Dark status strip under the ribbon.
        egui::TopBottomPanel::top("status_strip")
            .exact_height(24.0)
            .show_separator_line(false)
            .frame(
                egui::Frame::NONE
                    .fill(STATUS_BG)
                    .inner_margin(egui::Margin::symmetric(10, 3))
                    .stroke(egui::Stroke::new(1.0_f32, CHROME_SEP)),
            )
            .show(ctx, |ui| {
                ui.visuals_mut().override_text_color = Some(CHROME_TEXT);
                ui.horizontal_centered(|ui| {
                    ui.label(RichText::new(&self.status).color(CHROME_TEXT));
                });
            });
    }
}

/// Apply persisted font zoom to all egui text styles (relative to Style::default()).
fn apply_font_zoom(ctx: &egui::Context, factor: f32) {
    let base = egui::Style::default().text_styles;
    let mut styles: BTreeMap<TextStyle, FontId> = BTreeMap::new();
    for (style, id) in base {
        styles.insert(
            style,
            FontId::new((id.size * factor).max(8.0), id.family),
        );
    }
    ctx.style_mut(|s| s.text_styles = styles);
}

/// Load AA logo at an **integer** 1/N of native pixels (box-average), painted 1:1 — no blurry fractional scale.
fn load_logo(ctx: &egui::Context) -> Option<TextureHandle> {
    let bytes = include_bytes!("../assets/agile-automation-logo.png");
    let raw = image::load_from_memory(bytes).ok()?.into_rgba8();
    let (w, h) = raw.dimensions();
    // Fit ~40px tall brand slot with integer factor only.
    const MAX_H: u32 = 40;
    let factor = (h / MAX_H).max(1);
    let cw = (w / factor) * factor;
    let ch = (h / factor) * factor;
    let ox = (w - cw) / 2;
    let oy = (h - ch) / 2;
    let cropped = image::imageops::crop_imm(&raw, ox, oy, cw, ch).to_image();
    let logo = integer_box_downscale(&cropped, factor);
    let size = [logo.width() as usize, logo.height() as usize];
    let color = egui::ColorImage::from_rgba_unmultiplied(size, logo.as_raw());
    // NEAREST keeps integer-scale glyphs crisp when DPI is 1× / 2×.
    Some(ctx.load_texture(
        "aa-logo",
        color,
        egui::TextureOptions {
            magnification: egui::TextureFilter::Nearest,
            minification: egui::TextureFilter::Nearest,
            ..egui::TextureOptions::LINEAR
        },
    ))
}

fn integer_box_downscale(src: &RgbaImage, factor: u32) -> RgbaImage {
    if factor <= 1 {
        return src.clone();
    }
    let w = src.width() / factor;
    let h = src.height() / factor;
    let mut out = RgbaImage::new(w, h);
    let area = (factor * factor) as u32;
    for y in 0..h {
        for x in 0..w {
            let mut r = 0u32;
            let mut g = 0u32;
            let mut b = 0u32;
            let mut a = 0u32;
            for dy in 0..factor {
                for dx in 0..factor {
                    let p = src.get_pixel(x * factor + dx, y * factor + dy).0;
                    r += p[0] as u32;
                    g += p[1] as u32;
                    b += p[2] as u32;
                    a += p[3] as u32;
                }
            }
            out.put_pixel(
                x,
                y,
                image::Rgba([
                    (r / area) as u8,
                    (g / area) as u8,
                    (b / area) as u8,
                    (a / area) as u8,
                ]),
            );
        }
    }
    out
}

impl eframe::App for MqttMonitorApp {
    /// Opaque clear — default eframe clear is semi-transparent and shows the desktop
    /// through any uncovered strip (e.g. SidePanel dragged past its content).
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        Color32::from_rgb(27, 27, 27).to_normalized_gamma_f32()
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.run_auto_reconnect(ctx);

        if let Some(msg) = self.sessions.poll_events(&mut self.pins) {
            self.status = msg;
        }

        // egui idles without input — keep frames flowing while any fade is live.
        let now = Instant::now();
        let flashing = self.sessions.open.values().any(|s| {
            flash::flash_active(s.flash_until, now)
                || flash::tree_flash_active(&s.topic_tree.root, now)
        }) || self
            .pins
            .items
            .values()
            .any(|p| flash::flash_active(p.flash_until, now));
        if flashing {
            ctx.request_repaint();
        }

        self.handle_shortcuts(ctx);
        self.draw_ribbon(ctx);

        // Keyboard shortcuts help popup (F1 / ? / Settings button).
        if self.shortcuts_open {
            egui::Window::new("Keyboard shortcuts")
                .open(&mut self.shortcuts_open)
                .default_width(520.0)
                .default_height(480.0)
                .resizable(true)
                .collapsible(false)
                .show(ctx, |ui| {
                    shortcuts::draw_help(ui);
                });
        }

        // Settings — centered modal overlay (same family as shortcuts Window; Escape / outside closes).
        if self.settings_open {
            let mut status = self.status.clone();
            let mut open_shortcuts = false;
            let mut close_clicked = false;
            let modal = egui::Modal::new(egui::Id::new("mqtt_master_settings")).show(ctx, |ui| {
                ui.set_min_width(420.0);
                ui.set_max_width(480.0);
                egui::ScrollArea::vertical()
                    .max_height(520.0)
                    .show(ui, |ui| {
                        settings_panel::draw(
                            ui,
                            &mut self.settings,
                            &self.icons,
                            &mut status,
                            &mut open_shortcuts,
                        );
                    });
                ui.add_space(8.0);
                ui.separator();
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Close").clicked() {
                            close_clicked = true;
                        }
                    });
                });
            });
            self.status = status;
            if open_shortcuts {
                self.shortcuts_open = true;
            }
            if close_clicked || modal.should_close() {
                self.settings_open = false;
            }
        }

        if self.panels.profiles {
            // Hard min width — resize handle cannot drag past this (avoids see-through gap).
            egui::SidePanel::left("profiles")
                .default_width(320.0)
                .min_width(PROFILES_PANEL_MIN_W)
                .width_range(PROFILES_PANEL_MIN_W..=560.0)
                .resizable(true)
                .show(ctx, |ui| {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        profiles::draw(
                            ui,
                            &mut self.profiles,
                            &mut self.sessions,
                            &mut self.pins,
                            &mut self.settings,
                            &mut self.profile_filter,
                            &self.icons,
                            &mut self.sub_draft,
                            &mut self.status,
                            ctx,
                            &mut self.focus_profile_editor,
                        );
                    });
                });
        }

        if self.panels.publish {
            egui::SidePanel::right("publish")
                .default_width(280.0)
                .min_width(200.0)
                .width_range(200.0..=480.0)
                .resizable(true)
                .show(ctx, |ui| {
                    publish_panel::draw(
                        ui,
                        &mut self.publish,
                        &mut self.publish_history,
                        &self.sessions,
                        &self.profiles,
                        &self.icons,
                        &mut self.status,
                    );
                });
        }

        if self.panels.payload {
            egui::SidePanel::right("payload")
                .default_width(320.0)
                .min_width(220.0)
                .width_range(220.0..=560.0)
                .resizable(true)
                .show(ctx, |ui| {
                    payload_panel::draw(
                        ui,
                        &self.sessions,
                        &self.profiles,
                        &self.pins,
                        &self.icons,
                    );
                });
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            search_bar::draw(ui, &mut self.search, &mut self.focus_search);
            ui.add_space(6.0);
            ui.separator();
            ui.add_space(6.0);
            egui::ScrollArea::vertical().show(ui, |ui| {
                if self.panels.live_traffic {
                    traffic_panel::draw(
                        ui,
                        &self.sessions,
                        &self.profiles.selected_id,
                        &self.search,
                        &mut self.status,
                    );
                    ui.add_space(8.0);
                    ui.separator();
                    ui.add_space(8.0);
                }

                if self.panels.stickies {
                    pin_board::draw(
                        ui,
                        &mut self.pins,
                        &mut self.profiles,
                        &self.sessions,
                        &self.settings,
                        &self.icons,
                        &self.search,
                        &mut self.status,
                    );
                    ui.add_space(8.0);
                    ui.separator();
                    ui.add_space(8.0);
                }

                if self.panels.topic_tree {
                    topic_tree_panel::draw(
                        ui,
                        &self.sessions,
                        &mut self.profiles,
                        &mut self.pins,
                        self.settings.topic_tree_expand,
                        &mut self.expand_all_once,
                        &self.icons,
                        &self.search,
                        &mut self.status,
                    );
                }
            });
        });
    }
}
