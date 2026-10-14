//! Embedded UI icons from `assets/icons/` (transparent PNGs for dark chrome).
//!
//! Ribbon groups must **shrink-wrap** — a nested `vertical` inside a horizontal bar
//! otherwise expands and steals the rest of the row (hiding later groups).

use eframe::egui::{self, Color32, RichText, Sense, Stroke, TextureHandle, Vec2};
use std::collections::HashMap;

/// Large ribbon glyph (margins stay tight).
const CMD_ICON: f32 = 36.0;
const CMD_W: f32 = 56.0;
const CMD_H: f32 = 58.0;
/// Dense topic-tree / inline action icons.
const TINY_ICON: f32 = 14.0;

/// Dark chrome tokens.
pub const CHROME_BG: Color32 = Color32::from_rgb(40, 42, 48);
pub const CHROME_SEP: Color32 = Color32::from_rgb(70, 74, 84);
pub const CHROME_HOVER: Color32 = Color32::from_rgb(58, 62, 72);
pub const CHROME_ACCENT_SOFT: Color32 = Color32::from_rgb(48, 78, 112);
pub const CHROME_ACCENT_BORDER: Color32 = Color32::from_rgb(100, 160, 220);
pub const CHROME_CAPTION: Color32 = Color32::from_rgb(170, 176, 188);
pub const CHROME_TEXT: Color32 = Color32::from_rgb(230, 232, 238);
pub const STATUS_BG: Color32 = Color32::from_rgb(28, 30, 36);

pub struct IconSet {
    textures: HashMap<&'static str, TextureHandle>,
}

impl IconSet {
    pub fn load(ctx: &egui::Context) -> Self {
        let mut textures = HashMap::new();
        for &(name, bytes) in ICONS {
            if let Some(tex) = load_png(ctx, name, bytes) {
                textures.insert(name, tex);
            }
        }
        Self { textures }
    }

    pub fn get(&self, name: &str) -> Option<&TextureHandle> {
        self.textures.get(name)
    }

    /// Large ribbon command: icon + caption; quiet hover/selected on dark chrome.
    pub fn command(
        &self,
        ui: &mut egui::Ui,
        name: &str,
        label: &str,
        selected: bool,
        tip: &str,
    ) -> egui::Response {
        let cell = Vec2::new(CMD_W, CMD_H);
        let (rect, response) = ui.allocate_exact_size(cell, Sense::click());
        let response = response.on_hover_text(tip);

        if selected {
            ui.painter().rect_filled(rect, 3.0, CHROME_ACCENT_SOFT);
            ui.painter().rect_stroke(
                rect,
                3.0,
                Stroke::new(1.0_f32, CHROME_ACCENT_BORDER),
                egui::StrokeKind::Inside,
            );
        } else if response.hovered() {
            ui.painter().rect_filled(rect, 3.0, CHROME_HOVER);
        }

        let icon_center = egui::pos2(rect.center().x, rect.top() + 22.0);
        if let Some(tex) = self.get(name) {
            let img = egui::Image::new(tex).fit_to_exact_size(Vec2::splat(CMD_ICON));
            let img_rect = egui::Rect::from_center_size(icon_center, Vec2::splat(CMD_ICON));
            img.paint_at(ui, img_rect);
        }

        ui.painter().text(
            egui::pos2(rect.center().x, rect.bottom() - 6.0),
            egui::Align2::CENTER_CENTER,
            label,
            egui::FontId::proportional(11.0),
            if selected {
                Color32::from_rgb(180, 210, 255)
            } else {
                CHROME_TEXT
            },
        );
        response
    }

    /// Small dense-row icon button (topic tree copy / sticky).
    pub fn tiny(
        &self,
        ui: &mut egui::Ui,
        name: &str,
        selected: bool,
        tip: &str,
    ) -> egui::Response {
        let pad = 2.0;
        let size = Vec2::splat(TINY_ICON + pad * 2.0);
        let (rect, response) = ui.allocate_exact_size(size, Sense::click());
        let response = response.on_hover_text(tip);
        if selected {
            ui.painter().rect_filled(rect, 2.0, CHROME_ACCENT_SOFT);
        } else if response.hovered() {
            ui.painter().rect_filled(rect, 2.0, CHROME_HOVER);
        }
        if let Some(tex) = self.get(name) {
            let img = egui::Image::new(tex).fit_to_exact_size(Vec2::splat(TINY_ICON));
            img.paint_at(ui, egui::Rect::from_center_size(rect.center(), Vec2::splat(TINY_ICON)));
        }
        response
    }

    pub fn labeled_button(&self, ui: &mut egui::Ui, name: &str, label: &str) -> egui::Response {
        ui.horizontal(|ui| {
            if let Some(tex) = self.get(name) {
                ui.add(
                    egui::Image::new(tex)
                        .fit_to_exact_size(Vec2::splat(20.0))
                        .sense(Sense::hover()),
                );
            }
            ui.button(label)
        })
        .inner
    }
}

/// Content-sized ribbon group. Critical: shrink-wrap so later groups are not clipped to 0 width.
pub fn ribbon_group(ui: &mut egui::Ui, caption: &str, add_contents: impl FnOnce(&mut egui::Ui)) {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 1.0;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            add_contents(ui);
        });
        ui.label(RichText::new(caption).small().color(CHROME_CAPTION));
        ui.shrink_width_to_current();
    });
}

/// 1px vertical rule between groups (content-sized, not expanding).
pub fn ribbon_separator(ui: &mut egui::Ui) {
    ui.add_space(4.0);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(1.0, CMD_H + 4.0), Sense::hover());
    ui.painter().line_segment(
        [rect.center_top(), rect.center_bottom()],
        Stroke::new(1.0_f32, CHROME_SEP),
    );
    ui.add_space(4.0);
}

fn load_png(ctx: &egui::Context, id: &str, bytes: &[u8]) -> Option<TextureHandle> {
    let image = image::load_from_memory(bytes).ok()?.into_rgba8();
    let size = [image.width() as usize, image.height() as usize];
    let color = egui::ColorImage::from_rgba_unmultiplied(size, image.as_raw());
    Some(ctx.load_texture(id, color, egui::TextureOptions::LINEAR))
}

const ICONS: &[(&str, &[u8])] = &[
    ("access_key", include_bytes!("../../assets/icons/access_key.png")),
    ("alerts", include_bytes!("../../assets/icons/alerts.png")),
    ("api-docs", include_bytes!("../../assets/icons/api-docs.png")),
    ("api-methods", include_bytes!("../../assets/icons/api-methods.png")),
    ("auth-user", include_bytes!("../../assets/icons/auth-user.png")),
    ("broker_server", include_bytes!("../../assets/icons/broker_server.png")),
    ("certification", include_bytes!("../../assets/icons/certification.png")),
    ("clipboard_checklist", include_bytes!("../../assets/icons/clipboard_checklist.png")),
    ("cloud-database", include_bytes!("../../assets/icons/cloud-database.png")),
    ("cloud_broker", include_bytes!("../../assets/icons/cloud_broker.png")),
    ("configuration_sliders", include_bytes!("../../assets/icons/configuration_sliders.png")),
    ("connect", include_bytes!("../../assets/icons/connect.png")),
    ("connection_status", include_bytes!("../../assets/icons/connection_status.png")),
    ("dashboard", include_bytes!("../../assets/icons/dashboard.png")),
    ("data-bridge", include_bytes!("../../assets/icons/data-bridge.png")),
    ("data-exchange", include_bytes!("../../assets/icons/data-exchange.png")),
    ("data-sync", include_bytes!("../../assets/icons/data-sync.png")),
    ("data_table", include_bytes!("../../assets/icons/data_table.png")),
    ("disconnect", include_bytes!("../../assets/icons/disconnect.png")),
    ("edge-cloud", include_bytes!("../../assets/icons/edge-cloud.png")),
    ("encrypted-keys", include_bytes!("../../assets/icons/encrypted-keys.png")),
    ("event_log", include_bytes!("../../assets/icons/event_log.png")),
    ("event_logs", include_bytes!("../../assets/icons/event_logs.png")),
    ("export_download", include_bytes!("../../assets/icons/export_download.png")),
    ("gateway", include_bytes!("../../assets/icons/gateway.png")),
    ("global-wireless", include_bytes!("../../assets/icons/global-wireless.png")),
    ("hierarchy-tree", include_bytes!("../../assets/icons/hierarchy-tree.png")),
    ("idea-bidirectional", include_bytes!("../../assets/icons/idea-bidirectional.png")),
    ("incoming_messages", include_bytes!("../../assets/icons/incoming_messages.png")),
    ("iot-hub", include_bytes!("../../assets/icons/iot-hub.png")),
    ("json_payload", include_bytes!("../../assets/icons/json_payload.png")),
    ("last_will_security", include_bytes!("../../assets/icons/last_will_security.png")),
    ("line_graph", include_bytes!("../../assets/icons/line_graph.png")),
    ("mesh-network", include_bytes!("../../assets/icons/mesh-network.png")),
    ("message_history", include_bytes!("../../assets/icons/message_history.png")),
    ("metric_gauge", include_bytes!("../../assets/icons/metric_gauge.png")),
    ("mobile-connect", include_bytes!("../../assets/icons/mobile-connect.png")),
    ("mqtt-cloud", include_bytes!("../../assets/icons/mqtt-cloud.png")),
    ("node_graph", include_bytes!("../../assets/icons/node_graph.png")),
    ("panel_docked", include_bytes!("../../assets/icons/panel_docked.png")),
    ("panel_undock", include_bytes!("../../assets/icons/panel_undock.png")),
    ("profile", include_bytes!("../../assets/icons/profile.png")),
    ("profile_add", include_bytes!("../../assets/icons/profile_add.png")),
    ("profile_duplicate", include_bytes!("../../assets/icons/profile_duplicate.png")),
    ("profile_edit", include_bytes!("../../assets/icons/profile_edit.png")),
    ("publish_message", include_bytes!("../../assets/icons/publish_message.png")),
    ("qos_levels", include_bytes!("../../assets/icons/qos_levels.png")),
    ("reconnect", include_bytes!("../../assets/icons/reconnect.png")),
    ("refresh", include_bytes!("../../assets/icons/refresh.png")),
    ("retained_message", include_bytes!("../../assets/icons/retained_message.png")),
    ("save", include_bytes!("../../assets/icons/save.png")),
    ("search", include_bytes!("../../assets/icons/search.png")),
    ("security-lock", include_bytes!("../../assets/icons/security-lock.png")),
    ("security-std", include_bytes!("../../assets/icons/security-std.png")),
    ("server_settings", include_bytes!("../../assets/icons/server_settings.png")),
    ("settings-checklist", include_bytes!("../../assets/icons/settings-checklist.png")),
    ("smart-home", include_bytes!("../../assets/icons/smart-home.png")),
    ("split_view", include_bytes!("../../assets/icons/split_view.png")),
    ("sticky_topic", include_bytes!("../../assets/icons/sticky_topic.png")),
    ("subscribe_topic", include_bytes!("../../assets/icons/subscribe_topic.png")),
    ("terminal", include_bytes!("../../assets/icons/terminal.png")),
    ("tls_security", include_bytes!("../../assets/icons/tls_security.png")),
    ("topic_filter", include_bytes!("../../assets/icons/topic_filter.png")),
    ("topic_tags", include_bytes!("../../assets/icons/topic_tags.png")),
    ("topic_tree", include_bytes!("../../assets/icons/topic_tree.png")),
    ("topic_wildcards", include_bytes!("../../assets/icons/topic_wildcards.png")),
    ("unsubscribe_topic", include_bytes!("../../assets/icons/unsubscribe_topic.png")),
    ("user_credentials", include_bytes!("../../assets/icons/user_credentials.png")),
    ("verified-network", include_bytes!("../../assets/icons/verified-network.png")),
    ("workflow-loop", include_bytes!("../../assets/icons/workflow-loop.png")),
    ("xml-structure", include_bytes!("../../assets/icons/xml-structure.png")),
    ("zoom_in", include_bytes!("../../assets/icons/zoom_in.png")),
    ("zoom_out", include_bytes!("../../assets/icons/zoom_out.png")),
];
