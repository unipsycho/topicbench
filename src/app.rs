//! Top-level eframe app: ribbon chrome, optional panels, settings viewport.

use crate::mqtt::pins::PinBoard;
use crate::mqtt::profile::ProfileStore;
use crate::mqtt::publish::PublishDraft;
use crate::mqtt::session::SessionManager;
use crate::mqtt::settings::AppSettings;
use crate::ui::icons::IconSet;
use crate::ui::profiles::ProfileListFilter;
use crate::ui::search_bar::SearchFilter;
use crate::ui::{
    pin_board, profiles, publish_panel, search_bar, settings_panel, topic_tree_panel, traffic_panel,
};
use eframe::egui::{self, TextureHandle, ViewportBuilder, ViewportId};
use std::time::{Duration, Instant};

/// Which optional panels are visible (ribbon toggles).
#[derive(Clone, Debug)]
struct PanelVisibility {
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
            if let Some(profile) = self.profiles.profiles.get(&id).cloned() {
                if self.sessions.connect(&profile, ctx).is_ok() {
                    n += 1;
                }
            }
        }
        if n > 0 {
            self.status = format!("Auto-reconnect: connecting {n} profile(s)");
        }
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

        // Word/Excel-style ribbon: logo + labeled groups (Panels ready for Format/Views later)
        egui::TopBottomPanel::top("ribbon").exact_height(96.0).show(ctx, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if let Some(logo) = &self.logo {
                    // ~2× wider than prior 64px square → ~128×64 wordmark-friendly box
                    ui.add(
                        egui::Image::new(logo)
                            .fit_to_exact_size(egui::vec2(128.0, 64.0))
                            .maintain_aspect_ratio(true)
                            .sense(egui::Sense::hover()),
                    );
                    ui.add_space(12.0);
                }

                // Group: Panels
                egui::Frame::NONE
                    .stroke(egui::Stroke::new(
                        1.0_f32,
                        ui.visuals().widgets.noninteractive.bg_stroke.color,
                    ))
                    .inner_margin(egui::Margin::symmetric(8, 4))
                    .show(ui, |ui| {
                        ui.vertical(|ui| {
                            ui.label(egui::RichText::new("Panels").strong().small());
                            ui.horizontal(|ui| {
                                if self
                                    .icons
                                    .button(ui, "data-sync", self.panels.live_traffic, "Live traffic")
                                    .clicked()
                                {
                                    self.panels.live_traffic = !self.panels.live_traffic;
                                }
                                if self
                                    .icons
                                    .button(ui, "data-exchange", self.panels.publish, "Publish")
                                    .clicked()
                                {
                                    self.panels.publish = !self.panels.publish;
                                }
                                if self
                                    .icons
                                    .button(ui, "iot-hub", self.panels.stickies, "Sticky topics")
                                    .clicked()
                                {
                                    self.panels.stickies = !self.panels.stickies;
                                }
                                if self
                                    .icons
                                    .button(ui, "hierarchy-tree", self.panels.topic_tree, "Topic tree")
                                    .clicked()
                                {
                                    self.panels.topic_tree = !self.panels.topic_tree;
                                }
                            });
                        });
                    });

                // Placeholder group slot for future Format / Views
                ui.add_space(8.0);
                egui::Frame::NONE
                    .stroke(egui::Stroke::new(
                        1.0_f32,
                        ui.visuals().widgets.noninteractive.bg_stroke.color,
                    ))
                    .inner_margin(egui::Margin::symmetric(8, 4))
                    .show(ui, |ui| {
                        ui.vertical(|ui| {
                            ui.label(egui::RichText::new("Views").strong().small());
                            ui.weak("(soon)");
                        });
                    });

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    egui::Frame::NONE
                        .stroke(egui::Stroke::new(
                            1.0_f32,
                            ui.visuals().widgets.noninteractive.bg_stroke.color,
                        ))
                        .inner_margin(egui::Margin::symmetric(8, 4))
                        .show(ui, |ui| {
                            ui.vertical(|ui| {
                                ui.label(egui::RichText::new("App").strong().small());
                                if self
                                    .icons
                                    .button(
                                        ui,
                                        "settings-checklist",
                                        self.settings_open,
                                        "Settings",
                                    )
                                    .clicked()
                                {
                                    self.settings_open = true;
                                }
                            });
                        });
                });
            });
            ui.add_space(2.0);
            ui.horizontal(|ui| {
                ui.heading("MQTT Monitor");
                ui.separator();
                ui.label(&self.status);
            });
        });

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

        egui::SidePanel::left("profiles")
            .default_width(320.0)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    profiles::draw(
                        ui,
                        &mut self.profiles,
                        &mut self.sessions,
                        &mut self.settings,
                        &mut self.profile_filter,
                        &self.icons,
                        &mut self.sub_draft,
                        &mut self.status,
                        ctx,
                    );
                });
            });

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
                    pin_board::draw(ui, &mut self.pins, &self.search, &mut self.status);
                    ui.add_space(8.0);
                    ui.separator();
                    ui.add_space(8.0);
                }

                if self.panels.topic_tree {
                    topic_tree_panel::draw(
                        ui,
                        &self.sessions,
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
