//! Top-level eframe app: live brokers, stickies, search, settings, branding.

use crate::mqtt::pins::PinBoard;
use crate::mqtt::profile::ProfileStore;
use crate::mqtt::publish::PublishDraft;
use crate::mqtt::session::SessionManager;
use crate::mqtt::settings::AppSettings;
use crate::ui::profiles::ProfileListFilter;
use crate::ui::search_bar::SearchFilter;
use crate::ui::{pin_board, profiles, publish_panel, search_bar, settings_panel, subscribe_panel, topic_tree_panel};
use eframe::egui::{self, TextureHandle};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, PartialEq, Eq)]
enum AppPage {
    Monitor,
    Settings,
}

/// Root UI state for the MQTT Explorer–style monitor.
pub struct MqttMonitorApp {
    pub profiles: ProfileStore,
    pub sessions: SessionManager,
    pub pins: PinBoard,
    pub publish: PublishDraft,
    pub settings: AppSettings,
    pub subscribe_topic: String,
    pub subscribe_target: Option<String>,
    pub profile_filter: ProfileListFilter,
    pub search: SearchFilter,
    pub page: AppPage,
    pub status: String,
    logo: Option<TextureHandle>,
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
        Self {
            profiles,
            sessions: SessionManager::default(),
            pins: PinBoard::default(),
            publish: PublishDraft::default(),
            settings,
            subscribe_topic: "sensors/#".into(),
            subscribe_target: None,
            profile_filter: ProfileListFilter::All,
            search: SearchFilter::default(),
            page: AppPage::Monitor,
            status,
            logo,
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

        // Top chrome: logo (top-left) + status + Settings
        egui::TopBottomPanel::top("chrome").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if let Some(logo) = &self.logo {
                    ui.add(
                        egui::Image::new(logo)
                            .fit_to_exact_size(egui::vec2(28.0, 28.0))
                            .sense(egui::Sense::hover()),
                    );
                    ui.add_space(6.0);
                }
                ui.heading("MQTT Monitor");
                ui.separator();
                ui.label(&self.status);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .selectable_label(self.page == AppPage::Settings, "Settings")
                        .clicked()
                    {
                        self.page = AppPage::Settings;
                    }
                    if ui
                        .selectable_label(self.page == AppPage::Monitor, "Monitor")
                        .clicked()
                    {
                        self.page = AppPage::Monitor;
                    }
                });
            });
        });

        match self.page {
            AppPage::Settings => {
                egui::CentralPanel::default().show(ctx, |ui| {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        settings_panel::draw(ui, &mut self.settings, &mut self.status);
                    });
                });
            }
            AppPage::Monitor => {
                egui::SidePanel::left("profiles")
                    .default_width(300.0)
                    .show(ctx, |ui| {
                        egui::ScrollArea::vertical().show(ui, |ui| {
                            profiles::draw(
                                ui,
                                &mut self.profiles,
                                &mut self.sessions,
                                &mut self.settings,
                                &mut self.profile_filter,
                                &mut self.status,
                                ctx,
                            );
                        });
                    });

                egui::SidePanel::right("publish")
                    .default_width(280.0)
                    .show(ctx, |ui| {
                        publish_panel::draw(
                            ui,
                            &mut self.publish,
                            &self.sessions,
                            &mut self.status,
                        );
                    });

                egui::CentralPanel::default().show(ctx, |ui| {
                    search_bar::draw(ui, &mut self.search);
                    ui.add_space(6.0);
                    ui.separator();
                    ui.add_space(6.0);
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        pin_board::draw(ui, &mut self.pins, &self.search, &mut self.status);
                        ui.add_space(8.0);
                        ui.separator();
                        ui.add_space(8.0);
                        subscribe_panel::draw(
                            ui,
                            &mut self.subscribe_topic,
                            &mut self.subscribe_target,
                            &mut self.sessions,
                            &mut self.pins,
                            &self.search,
                            &mut self.status,
                        );
                        ui.add_space(8.0);
                        ui.separator();
                        ui.add_space(8.0);
                        let session = self
                            .subscribe_target
                            .as_ref()
                            .and_then(|id| self.sessions.open.get(id));
                        topic_tree_panel::draw(
                            ui,
                            session,
                            &mut self.pins,
                            self.settings.topic_tree_expand,
                            &mut self.expand_all_once,
                            &self.search,
                            &mut self.status,
                        );
                    });
                });
            }
        }
    }
}
