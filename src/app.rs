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
use crate::ui::{
    pin_board, profiles, publish_panel, search_bar, settings_panel, topic_tree_panel, traffic_panel,
};
use eframe::egui::{self, Color32, FontId, RichText, TextStyle, TextureHandle, ViewportBuilder, ViewportId};
use image::RgbaImage;
use std::collections::BTreeMap;
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
    expand_all_once: Option<bool>,
    auto_reconnect_done: bool,
    last_flash_repaint: Instant,
}

impl MqttMonitorApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let profiles = ProfileStore::load_or_sample();
        let settings = AppSettings::load();
        let status = profiles.last_io.clone();
        apply_font_zoom(&cc.egui_ctx, settings.font_zoom_factor());
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
                                RichText::new("MQTT Monitor")
                                    .strong()
                                    .size(13.0)
                                    .color(Color32::WHITE),
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
                            .command(ui, "profile_add", "New", false, "New profile")
                            .clicked()
                        {
                            let id = self.profiles.add_new();
                            self.status = format!("Created profile {id}");
                        }
                        if self
                            .icons
                            .command(ui, "save", "Save", false, "Save profiles")
                            .clicked()
                        {
                            match self.profiles.save() {
                                Ok(()) => self.status = self.profiles.last_io.clone(),
                                Err(e) => self.status = e,
                            }
                        }
                        if self
                            .icons
                            .command(ui, "disconnect", "Delete", false, "Delete selected profile")
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
                                "Show or hide the profiles side panel",
                            )
                            .clicked()
                        {
                            self.panels.profiles = !self.panels.profiles;
                        }
                    });

                    icons::ribbon_separator(ui);

                    // Panels: Traffic / Publish / Stickies / Tree
                    icons::ribbon_group(ui, "Panels", TINT_PANELS, |ui| {
                        if self
                            .icons
                            .command(
                                ui,
                                "incoming_messages",
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
                                "publish_message",
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
                                "sticky_topic",
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
                                "topic_tree",
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

                    // App: Settings + font zoom (−2…+3 steps from default).
                    icons::ribbon_group(ui, "App", TINT_APP, |ui| {
                        if self
                            .icons
                            .command(
                                ui,
                                "configuration_sliders",
                                "Settings",
                                self.settings_open,
                                "Open settings window",
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
                                "Decrease font size (−2 steps from default)",
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
                                "Increase font size (+3 steps from default)",
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
                    publish_panel::draw(
                        ui,
                        &mut self.publish,
                        &mut self.publish_history,
                        &self.sessions,
                        &self.icons,
                        &mut self.status,
                    );
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
