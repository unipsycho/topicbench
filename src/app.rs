//! Top-level eframe app: Office-style ribbon chrome, optional panels, settings viewport.
//!
//! Ribbon layout cues from PhotoCraft ui-egui (chrome fill, group separators, quiet hover)
//! plus Word-style labeled groups with captions under the commands.

use crate::mqtt::pins::PinBoard;
use crate::mqtt::profile::ProfileStore;
use crate::mqtt::publish::{PublishDraft, PublishHistory, PublishFromTree};
use crate::mqtt::session::SessionManager;
use crate::mqtt::settings::AppSettings;
use crate::mqtt::subscribe;
use crate::ui::icons::{
    self, ribbon_compact_columns, IconSet, CHROME_ACCENT_BORDER, CHROME_ACCENT_SOFT, CHROME_BG,
    CHROME_SEP, CHROME_TEXT, TINT_APP, TINT_PANELS, TINT_PROFILES,
};
use crate::ui::profiles::ProfileListFilter;
use crate::ui::ribbon_layout::{self, SectionMode};
use crate::ui::search_bar::SearchFilter;
use crate::ui::shortcuts::{self, Action as ShortcutAction};
use crate::ui::payload_panel::PayloadSearch;
use crate::ui::{
    payload_panel, pin_board, profiles, publish_panel, search_bar, settings_panel, stats_ribbon,
    topic_tree_panel, traffic_panel,
};
use crate::ui::flash;
use eframe::egui::{self, Align, Color32, FontFamily, FontId, RichText, TextStyle, TextureHandle};
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
    /// Find-in-payload query / match cursor for the Payload side panel.
    payload_search: PayloadSearch,
    /// Profile password field: plain text when true (session UI only; resets on profile switch).
    show_profile_password: bool,
    /// Settings modal draft — applied on Save, discarded on Cancel / Escape / backdrop.
    settings_draft: Option<AppSettings>,
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
        // Restore ribbon panel open/closed (incl. Payload) from settings.json.
        let panels = PanelVisibility {
            profiles: settings.show_profiles,
            live_traffic: settings.show_live_traffic,
            publish: settings.show_publish,
            payload: settings.show_payload,
            stickies: settings.show_stickies,
            topic_tree: settings.show_topic_tree,
        };
        // Restore search text + scope so the filter is active as the tree paints.
        let search = SearchFilter::from_settings(
            settings.search_query.clone(),
            settings.search_scope,
        );
        Self {
            profiles,
            sessions: SessionManager::default(),
            pins: PinBoard::default(),
            publish: PublishDraft::default(),
            publish_history: PublishHistory::load(),
            settings,
            profile_filter: ProfileListFilter::All,
            search,
            sub_draft: String::new(),
            status,
            logo,
            icons,
            panels,
            settings_open: false,
            shortcuts_open: false,
            focus_search: false,
            focus_profile_editor: false,
            profiles_auto_opened: false,
            expand_all_once: None,
            auto_reconnect_done: false,
            payload_search: PayloadSearch::default(),
            show_profile_password: false,
            settings_draft: None,
        }
    }

    /// Open Settings with a draft snapshot of current settings.
    fn open_settings(&mut self) {
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

    /// Persist search query + scope to settings.json.
    fn persist_search(&mut self) {
        self.settings
            .set_search(&self.search.query, self.search.scope);
        if let Err(e) = self.settings.save() {
            self.status = format!("Settings save failed: {e}");
        }
    }

    /// Persist current panel visibility (Payload included) to settings.json.
    fn persist_panels(&mut self) {
        self.settings.set_panels(
            self.panels.profiles,
            self.panels.live_traffic,
            self.panels.publish,
            self.panels.payload,
            self.panels.stickies,
            self.panels.topic_tree,
        );
        let _ = self.settings.save();
    }

    /// New profile: auto-show Profiles if hidden; remember for Save auto-close.
    fn action_new_profile(&mut self) {
        if !self.panels.profiles {
            self.panels.profiles = true;
            self.profiles_auto_opened = true;
            self.persist_panels();
        }
        let id = self.profiles.add_new();
        self.show_profile_password = false;
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
            self.persist_panels();
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
                    self.persist_panels();
                }
                ShortcutAction::ToggleTraffic => {
                    self.panels.live_traffic = !self.panels.live_traffic;
                    self.persist_panels();
                }
                ShortcutAction::TogglePublish => {
                    self.panels.publish = !self.panels.publish;
                    self.persist_panels();
                }
                ShortcutAction::TogglePayload => {
                    self.panels.payload = !self.panels.payload;
                    self.persist_panels();
                }
                ShortcutAction::ToggleStickies => {
                    self.panels.stickies = !self.panels.stickies;
                    self.persist_panels();
                }
                ShortcutAction::ToggleTopicTree => {
                    self.panels.topic_tree = !self.panels.topic_tree;
                    self.persist_panels();
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
                    self.open_settings();
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
        if self.profiles.selected_id.as_deref() != Some(ids[next].as_str()) {
            self.show_profile_password = false;
        }
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
        match self.sessions.toggle(&profile, max_pkt, ctx, &mut self.pins) {
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

    /// Profiles ribbon group (full captions or compact 2-high icons).
    fn draw_ribbon_profiles(&mut self, ui: &mut egui::Ui, mode: SectionMode) {
        icons::ribbon_group(ui, "Profiles", TINT_PROFILES, |ui| {
            let tip_new = shortcuts::tip("New profile", "Ctrl+N");
            let tip_save = shortcuts::tip("Save profiles", "Ctrl+S");
            let tip_del = shortcuts::tip("Delete selected profile", "Delete / Ctrl+Backspace");
            let tip_panel = shortcuts::tip("Show or hide the profiles side panel", "Ctrl+Shift+P");
            if mode == SectionMode::Full {
                if self
                    .icons
                    .command(ui, "profile_add", "New", false, &tip_new)
                    .clicked()
                {
                    self.action_new_profile();
                }
                if self
                    .icons
                    .command(ui, "save", "Save", false, &tip_save)
                    .clicked()
                {
                    self.action_save_profiles();
                }
                if self
                    .icons
                    .command_enabled(
                        ui,
                        "disconnect",
                        "Delete",
                        false,
                        &tip_del,
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
                    .command(ui, "panel_docked", "Profiles", self.panels.profiles, &tip_panel)
                    .clicked()
                {
                    self.panels.profiles = !self.panels.profiles;
                    self.profiles_auto_opened = false;
                    self.persist_panels();
                }
            } else {
                let mut clicks = [false; 4];
                ribbon_compact_columns(ui, 4, |i, ui| {
                    clicks[i] = match i {
                        0 => self
                            .icons
                            .command_compact(ui, "profile_add", false, &tip_new)
                            .clicked(),
                        1 => self
                            .icons
                            .command_compact(ui, "save", false, &tip_save)
                            .clicked(),
                        2 => self
                            .icons
                            .command_compact_enabled(
                                ui,
                                "disconnect",
                                false,
                                &tip_del,
                                self.panels.profiles,
                            )
                            .clicked(),
                        _ => self
                            .icons
                            .command_compact(
                                ui,
                                "panel_docked",
                                self.panels.profiles,
                                &tip_panel,
                            )
                            .clicked(),
                    };
                });
                if clicks[0] {
                    self.action_new_profile();
                }
                if clicks[1] {
                    self.action_save_profiles();
                }
                if clicks[2] {
                    if let Some(id) = self.profiles.delete_selected() {
                        self.sessions.close(&id);
                        self.status = format!("Deleted profile {id}");
                    }
                }
                if clicks[3] {
                    self.panels.profiles = !self.panels.profiles;
                    self.profiles_auto_opened = false;
                    self.persist_panels();
                }
            }
        });
    }

    /// Panels ribbon group (full or compact).
    fn draw_ribbon_panels(&mut self, ui: &mut egui::Ui, mode: SectionMode) {
        icons::ribbon_group(ui, "Panels", TINT_PANELS, |ui| {
            let tips = [
                shortcuts::tip("Live traffic", "Ctrl+Shift+T"),
                shortcuts::tip("Publish panel", "Ctrl+Shift+U"),
                shortcuts::tip("Payload panel", "Ctrl+Shift+L"),
                shortcuts::tip("Sticky topics", "Ctrl+Shift+K"),
                shortcuts::tip("Topic tree", "Ctrl+Shift+R"),
            ];
            let icons_names = [
                "incoming_messages",
                "publish_message",
                "panel_undock",
                "sticky_topic",
                "topic_tree",
            ];
            let labels = ["Traffic", "Publish", "Payload", "Stickies", "Tree"];
            let selected = [
                self.panels.live_traffic,
                self.panels.publish,
                self.panels.payload,
                self.panels.stickies,
                self.panels.topic_tree,
            ];
            let mut clicks = [false; 5];
            if mode == SectionMode::Full {
                for i in 0..5 {
                    clicks[i] = self
                        .icons
                        .command(ui, icons_names[i], labels[i], selected[i], &tips[i])
                        .clicked();
                }
            } else {
                ribbon_compact_columns(ui, 5, |i, ui| {
                    clicks[i] = self
                        .icons
                        .command_compact(ui, icons_names[i], selected[i], &tips[i])
                        .clicked();
                });
            }
            if clicks[0] {
                self.panels.live_traffic = !self.panels.live_traffic;
                self.persist_panels();
            }
            if clicks[1] {
                self.panels.publish = !self.panels.publish;
                self.persist_panels();
            }
            if clicks[2] {
                self.panels.payload = !self.panels.payload;
                self.persist_panels();
            }
            if clicks[3] {
                self.panels.stickies = !self.panels.stickies;
                self.persist_panels();
            }
            if clicks[4] {
                self.panels.topic_tree = !self.panels.topic_tree;
                self.persist_panels();
            }
        });
    }

    /// App ribbon group: Settings + zoom (full or compact).
    fn draw_ribbon_app(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, mode: SectionMode) {
        icons::ribbon_group(ui, "App", TINT_APP, |ui| {
            let tip_set = shortcuts::tip("Open settings window", "Ctrl+,");
            let tip_out = shortcuts::tip("Decrease font size (−4 steps from default)", "Ctrl+-");
            let tip_in =
                shortcuts::tip("Increase font size (+5 steps from default)", "Ctrl+= / Ctrl++");
            let mut clicks = [false; 3];
            if mode == SectionMode::Full {
                clicks[0] = self
                    .icons
                    .command(
                        ui,
                        "configuration_sliders",
                        "Settings",
                        self.settings_open,
                        &tip_set,
                    )
                    .clicked();
                clicks[1] = self
                    .icons
                    .command(ui, "zoom_out", "Zoom−", false, &tip_out)
                    .clicked();
                clicks[2] = self
                    .icons
                    .command(ui, "zoom_in", "Zoom+", false, &tip_in)
                    .clicked();
            } else {
                ribbon_compact_columns(ui, 3, |i, ui| {
                    clicks[i] = match i {
                        0 => self
                            .icons
                            .command_compact(
                                ui,
                                "configuration_sliders",
                                self.settings_open,
                                &tip_set,
                            )
                            .clicked(),
                        1 => self
                            .icons
                            .command_compact(ui, "zoom_out", false, &tip_out)
                            .clicked(),
                        _ => self
                            .icons
                            .command_compact(ui, "zoom_in", false, &tip_in)
                            .clicked(),
                    };
                });
            }
            if clicks[0] {
                self.open_settings();
            }
            if clicks[1] {
                if self.settings.zoom_font_out() {
                    let _ = self.settings.save();
                    apply_font_zoom(ctx, self.settings.font_zoom_factor());
                    self.status = format!("Font zoom {}", self.settings.font_zoom_step);
                }
            }
            if clicks[2] {
                if self.settings.zoom_font_in() {
                    let _ = self.settings.save();
                    apply_font_zoom(ctx, self.settings.font_zoom_factor());
                    self.status = format!("Font zoom {}", self.settings.font_zoom_step);
                }
            }
        });
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

                // Top-align the ribbon row (Align::Center left a clear-color void above the
                // frame while egui still reserved space from the window top). Brand logo +
                // two-line title stay vertically centered inside brand_h below.
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Min), |ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;

                    // Brand metrics (also feed collapse width planning).
                    let logo_size = self
                        .logo
                        .as_ref()
                        .map(|l| l.size_vec2())
                        .unwrap_or(egui::vec2(108.0, 54.0));
                    let brand_h = 70.0; // match ribbon group / separator height
                    let green = Color32::from_rgb(90, 190, 120); // Connected/status green
                    let title_size = 24.0;
                    let line_gap = -2.0_f32;
                    let strong_family = ui
                        .style()
                        .text_styles
                        .get(&TextStyle::Name("Strong".into()))
                        .map(|f| f.family.clone())
                        .unwrap_or(FontFamily::Proportional);
                    let font_id = FontId::new(title_size, strong_family);
                    let (mqtt_g, master_g) = ui.fonts(|f| {
                        (
                            f.layout_no_wrap("MQTT".into(), font_id.clone(), green),
                            f.layout_no_wrap("Master".into(), font_id, green),
                        )
                    });
                    let title_w = mqtt_g.size().x.max(master_g.size().x).ceil();
                    let stack_h = mqtt_g.size().y + master_g.size().y + line_gap;
                    let logo_w = logo_size.x;

                    // Collapse: App → Profiles → Panels → hide title → hide Stats.
                    let avail_w = ui.available_width();
                    let plan = ribbon_layout::plan_for_width(
                        avail_w,
                        logo_w,
                        title_w,
                        stats_ribbon::estimate_width(),
                    );
                    let brand_w = if plan.show_title {
                        logo_w + 10.0 + title_w + 10.0
                    } else {
                        logo_w + 10.0
                    };

                    // Brand: logo always; MQTT/Master title when width allows.
                    ui.allocate_ui_with_layout(
                        egui::vec2(brand_w, brand_h),
                        egui::Layout::left_to_right(Align::Center),
                        |ui| {
                            ui.spacing_mut().item_spacing.x = 0.0;
                            if let Some(logo) = &self.logo {
                                ui.add(
                                    egui::Image::new(logo)
                                        .fit_to_exact_size(logo_size)
                                        .maintain_aspect_ratio(true)
                                        .sense(egui::Sense::hover()),
                                );
                            }
                            if plan.show_title {
                                ui.add_space(10.0);
                                let (title_rect, _) = ui.allocate_exact_size(
                                    egui::vec2(title_w, brand_h),
                                    egui::Sense::hover(),
                                );
                                let top = title_rect.center().y - stack_h * 0.5;
                                let mqtt_x =
                                    title_rect.left() + (title_w - mqtt_g.size().x) * 0.5;
                                let master_x =
                                    title_rect.left() + (title_w - master_g.size().x) * 0.5;
                                ui.painter()
                                    .galley(egui::pos2(mqtt_x, top), mqtt_g, green);
                                ui.painter().galley(
                                    egui::pos2(master_x, top + stack_h - master_g.size().y),
                                    master_g,
                                    green,
                                );
                                ui.add_space(10.0);
                            }
                        },
                    );

                    icons::ribbon_separator(ui);
                    self.draw_ribbon_profiles(ui, plan.profiles);
                    icons::ribbon_separator(ui);
                    self.draw_ribbon_panels(ui, plan.panels);
                    icons::ribbon_separator(ui);
                    self.draw_ribbon_app(ui, ctx, plan.app);

                    // Stats far-right (RTL: group then sep → App | Stats).
                    if plan.show_stats {
                        let remaining = ui.available_width();
                        if remaining > 8.0 {
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Min),
                                |ui| {
                                    ui.set_max_width(remaining);
                                    stats_ribbon::draw_group(
                                        ui,
                                        &self.sessions.traffic_stats,
                                    );
                                    icons::ribbon_separator(ui);
                                },
                            );
                        }
                    }
                });
            });

        // Status strip: same cool charcoal as ribbon (not a deeper neutral band).
        egui::TopBottomPanel::top("status_strip")
            .exact_height(24.0)
            .show_separator_line(false)
            .frame(
                egui::Frame::NONE
                    .fill(CHROME_BG)
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
    /// Opaque clear — match ribbon chrome (not egui gray(27) / transparent desktop).
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        CHROME_BG.to_normalized_gamma_f32()
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.run_auto_reconnect(ctx);

        if let Some(msg) = self.sessions.poll_events(&mut self.pins) {
            self.status = msg;
        }
        // Stats ribbon labels: rebuild frozen snapshot at most once per second.
        let _ = self.sessions.traffic_stats.refresh_display_if_due();

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
        } else if self.sessions.traffic_stats.wants_periodic_refresh() {
            // 1 Hz Stats refresh (Thru decay + frozen labels) — not every frame.
            ctx.request_repaint_after(self.sessions.traffic_stats.time_until_display_refresh());
        }

        self.handle_shortcuts(ctx);
        // Suppress tips from panels under Settings / Keyboard Shortcuts overlays
        // (including custom tree payload/topic popups — not only egui on_hover_text).
        let block_tips = self.settings_open || self.shortcuts_open;
        crate::ui::hover_tip::set_block_hover_tips(ctx, block_tips);
        if block_tips {
            topic_tree_panel::dismiss_hover_tips(ctx);
        }
        self.draw_ribbon(ctx);

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
                            &mut self.show_profile_password,
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
                        &mut self.sessions,
                        &self.profiles,
                        &self.icons,
                        &mut self.status,
                    );
                });
        }

        if self.panels.payload {
            let stickies_before = self.panels.stickies;
            egui::SidePanel::right("payload")
                .default_width(320.0)
                .min_width(220.0)
                .width_range(220.0..=560.0)
                .resizable(true)
                .show(ctx, |ui| {
                    payload_panel::draw(
                        ui,
                        &mut self.sessions,
                        &mut self.profiles,
                        &mut self.pins,
                        &self.icons,
                        &mut self.payload_search,
                        &mut self.panels.stickies,
                        &mut self.status,
                    );
                });
            // Graph-pin can open Stickies — persist that panel change too.
            if self.panels.stickies != stickies_before {
                self.persist_panels();
            }
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            let search_before = (self.search.query.clone(), self.search.scope);
            search_bar::draw(ui, &mut self.search, &mut self.focus_search);
            if self.search.query != search_before.0 || self.search.scope != search_before.1 {
                self.persist_search();
            }
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
                    let mut publish_from_tree: Option<PublishFromTree> = None;
                    topic_tree_panel::draw(
                        ui,
                        &mut self.sessions,
                        &mut self.profiles,
                        &mut self.pins,
                        self.settings.topic_tree_expand,
                        &mut self.expand_all_once,
                        &self.icons,
                        &self.search,
                        &mut self.status,
                        &mut publish_from_tree,
                    );
                    // Leaf double-click → open Publish and fill broker / topic / payload.
                    if let Some(fill) = publish_from_tree {
                        self.publish.apply_from_tree(&fill);
                        if !self.panels.publish {
                            self.panels.publish = true;
                            self.persist_panels();
                        }
                        self.status = format!(
                            "Publish filled from tree: {} · {} (retain={})",
                            fill.profile_name, fill.topic, fill.retain
                        );
                    }
                }
            });
        });

        // Overlays last so they own pointer/hover above Payload and other panels.
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
                                .stroke(egui::Stroke::new(1.0, CHROME_ACCENT_BORDER))
                                .min_size(egui::vec2(96.0, 32.0)),
                            )
                        } else {
                            ui.add(
                                egui::Button::new(RichText::new("Save").strong())
                                    .fill(CHROME_ACCENT_SOFT)
                                    .stroke(egui::Stroke::new(1.0, CHROME_ACCENT_BORDER))
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
