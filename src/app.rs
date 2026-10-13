//! Top-level eframe app: Office-style ribbon chrome, optional panels, settings viewport.
//!
//! Ribbon layout cues from PhotoCraft ui-egui (chrome fill, group separators, quiet hover)
//! plus Word-style labeled groups with captions under the commands.

use crate::mqtt::pins::PinBoard;
use crate::mqtt::profile::ProfileStore;
use crate::mqtt::publish::PublishDraft;
use crate::mqtt::session::SessionManager;
use crate::mqtt::settings::AppSettings;
use crate::mqtt::subscribe;
use crate::ui::icons::{self, IconSet, CHROME_BG, CHROME_SEP};
use crate::ui::profiles::ProfileListFilter;
use crate::ui::search_bar::SearchFilter;
use crate::ui::{
    pin_board, profiles, publish_panel, search_bar, settings_panel, topic_tree_panel, traffic_panel,
};
use eframe::egui::{self, Color32, RichText, TextureHandle, ViewportBuilder, ViewportId};
use std::time::{Duration, Instant};

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
}

impl Default for PanelVisibility {
    fn default() -> Self {
        Self {
            profiles: true,
            live_traffic: false,
            stickies: true,
            topic_tree: true,
            publish: false,
        }
    }
}

/// Root UI state for the MQTT Explorer–style monitor.
pub struct MqttMonitorApp {
    pub profiles: ProfileStore,
    pub sessions: SessionManager,
    pub pins: PinBoard,
    pub publish: PublishDraft,
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
    expand_all_once: Option<bool>,
    auto_reconnect_done: bool,
    last_flash_repaint: Instant,
}

impl MqttMonitorApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let profiles = ProfileStore::load_or_sample();
        let settings = AppSettings::load();
        let status = profiles.last_io.clone();
        let logo = load_logo(&cc.egui_ctx);
        let icons = IconSet::load(&cc.egui_ctx);
        Self {
            profiles,
            sessions: SessionManager::default(),
            pins: PinBoard::default(),
            publish: PublishDraft::default(),
            settings,
            profile_filter: ProfileListFilter::All,
            search: SearchFilter::default(),
            sub_draft: String::new(),
            status,
            logo,
            icons,
            panels: PanelVisibility::default(),
            settings_open: false,
            expand_all_once: None,
            auto_reconnect_done: false,
            last_flash_repaint: Instant::now(),
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
                if self.sessions.connect(&profile, ctx).is_ok() {
                    self.pins.restore_from_profile(&profile);
                    n += 1;
                }
            }
        }
        if n > 0 {
            self.status = format!("Auto-reconnect: connecting {n} profile(s)");
        }
    }

    fn draw_ribbon(&mut self, ctx: &egui::Context) {
        // Full-width chrome bar, visually separated from the body (PhotoCraft options-bar pattern).
        egui::TopBottomPanel::top("ribbon")
            .exact_height(88.0)
            .frame(
                egui::Frame::NONE
                    .fill(CHROME_BG)
                    .inner_margin(egui::Margin::symmetric(10, 4))
                    .stroke(egui::Stroke::new(1.0_f32, CHROME_SEP)),
            )
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    if let Some(logo) = &self.logo {
                        ui.add(
                            egui::Image::new(logo)
                                .fit_to_exact_size(egui::vec2(100.0, 48.0))
                                .maintain_aspect_ratio(true)
                                .sense(egui::Sense::hover()),
                        );
                        icons::ribbon_separator(ui);
                    }

                    // Profiles: New / Save / Delete + show/hide left profiles panel
                    icons::ribbon_group(ui, "Profiles", |ui| {
                        if self
                            .icons
                            .command(ui, "api-methods", "New", false, "New profile")
                            .clicked()
                        {
                            let id = self.profiles.add_new();
                            self.status = format!("Created profile {id}");
                        }
                        if self
                            .icons
                            .command(ui, "cloud-database", "Save", false, "Save profiles")
                            .clicked()
                        {
                            match self.profiles.save() {
                                Ok(()) => self.status = self.profiles.last_io.clone(),
                                Err(e) => self.status = e,
                            }
                        }
                        if self
                            .icons
                            .command(ui, "security-lock", "Delete", false, "Delete selected profile")
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
                                "gateway",
                                "Profiles",
                                self.panels.profiles,
                                "Show or hide the profiles side panel",
                            )
                            .clicked()
                        {
                            self.panels.profiles = !self.panels.profiles;
                        }
                    });

                    icons::ribbon_separator(ui);

                    // Panels: optional views only (not profile workflow)
                    icons::ribbon_group(ui, "Panels", |ui| {
                        if self
                            .icons
                            .command(
                                ui,
                                "data-sync",
                                "Traffic",
                                self.panels.live_traffic,
                                "Live traffic",
                            )
                            .clicked()
                        {
                            self.panels.live_traffic = !self.panels.live_traffic;
                        }
                        if self
                            .icons
                            .command(
                                ui,
                                "data-exchange",
                                "Publish",
                                self.panels.publish,
                                "Publish panel",
                            )
                            .clicked()
                        {
                            self.panels.publish = !self.panels.publish;
                        }
                        if self
                            .icons
                            .command(
                                ui,
                                "iot-hub",
                                "Stickies",
                                self.panels.stickies,
                                "Sticky topics",
                            )
                            .clicked()
                        {
                            self.panels.stickies = !self.panels.stickies;
                        }
                        if self
                            .icons
                            .command(
                                ui,
                                "hierarchy-tree",
                                "Tree",
                                self.panels.topic_tree,
                                "Topic tree",
                            )
                            .clicked()
                        {
                            self.panels.topic_tree = !self.panels.topic_tree;
                        }
                    });

                    icons::ribbon_separator(ui);

                    // Slot for future Format / Views groups
                    icons::ribbon_group(ui, "Views", |ui| {
                        ui.add_space(6.0);
                        ui.vertical_centered(|ui| {
                            ui.label(RichText::new("(soon)").small().weak());
                        });
                        ui.add_space(6.0);
                    });

                    // Settings gear — opens the separate settings window
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        icons::ribbon_group(ui, "App", |ui| {
                            if self
                                .icons
                                .command(
                                    ui,
                                    "settings-checklist",
                                    "Settings",
                                    self.settings_open,
                                    "Open settings window",
                                )
                                .clicked()
                            {
                                self.settings_open = true;
                            }
                        });
                    });
                });
            });

        // Thin status strip under the ribbon (clear separation from body content).
        egui::TopBottomPanel::top("status_strip")
            .exact_height(28.0)
            .frame(
                egui::Frame::NONE
                    .fill(Color32::from_rgb(236, 238, 242))
                    .inner_margin(egui::Margin::symmetric(10, 4))
                    .stroke(egui::Stroke::new(1.0_f32, CHROME_SEP)),
            )
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.label(RichText::new("MQTT Monitor").strong());
                    ui.separator();
                    ui.label(&self.status);
                });
            });
    }
}

fn load_logo(ctx: &egui::Context) -> Option<TextureHandle> {
    let bytes = include_bytes!("../assets/agile-automation-logo.png");
    let image = image::load_from_memory(bytes).ok()?.into_rgba8();
    let size = [image.width() as usize, image.height() as usize];
    let color = egui::ColorImage::from_rgba_unmultiplied(size, image.as_raw());
    Some(ctx.load_texture("aa-logo", color, egui::TextureOptions::LINEAR))
}

impl eframe::App for MqttMonitorApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.run_auto_reconnect(ctx);

        if let Some(msg) = self.sessions.poll_events(&mut self.pins) {
            self.status = msg;
        }

        let flashing = self.sessions.open.values().any(|s| {
            s.flash_until
                .map(|t| Instant::now() < t)
                .unwrap_or(false)
        }) || self.pins.items.values().any(|p| {
            p.flash_until
                .map(|t| Instant::now() < t)
                .unwrap_or(false)
        });
        if flashing && self.last_flash_repaint.elapsed() > Duration::from_millis(32) {
            self.last_flash_repaint = Instant::now();
            ctx.request_repaint_after(Duration::from_millis(32));
        }

        self.draw_ribbon(ctx);

        if self.settings_open {
            let mut close = false;
            let mut status = self.status.clone();
            ctx.show_viewport_immediate(
                ViewportId::from_hash_of("mqtt_monitor_settings"),
                ViewportBuilder::default()
                    .with_title("MQTT Monitor — Settings")
                    .with_inner_size([460.0, 420.0])
                    .with_resizable(true),
                |ctx, _class| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        settings_panel::draw(ui, &mut self.settings, &self.icons, &mut status);
                    });
                    if ctx.input(|i| i.viewport().close_requested()) {
                        close = true;
                    }
                },
            );
            self.status = status;
            if close {
                self.settings_open = false;
            }
        }

        if self.panels.profiles {
            egui::SidePanel::left("profiles")
                .default_width(320.0)
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
                        );
                    });
                });
        }

        if self.panels.publish {
            egui::SidePanel::right("publish")
                .default_width(280.0)
                .show(ctx, |ui| {
                    publish_panel::draw(ui, &mut self.publish, &self.sessions, &mut self.status);
                });
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            search_bar::draw(ui, &mut self.search);
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
