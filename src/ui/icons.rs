//! Embedded UI icons from `assets/icons/` (transparent PNGs for dark chrome).
//!
//! Ribbon groups must **shrink-wrap** — a nested `vertical` inside a horizontal bar
//! otherwise expands and steals the rest of the row (hiding later groups).

use eframe::egui::{self, Color32, RichText, Sense, Stroke, TextureHandle, Vec2};
use std::collections::HashMap;

/// Large ribbon glyph (restored — only margins stay tight).
const CMD_ICON: f32 = 36.0;
/// Hit box: big icon, little padding under the caption.
const CMD_W: f32 = 56.0;
const CMD_H: f32 = 58.0;

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

        // Icon upper, caption tight underneath (minimal empty band).
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
        // Without this, vertical() expands across the whole remaining ribbon width.
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
    ("api-docs", include_bytes!("../../assets/icons/api-docs.png")),
    ("api-methods", include_bytes!("../../assets/icons/api-methods.png")),
    ("auth-user", include_bytes!("../../assets/icons/auth-user.png")),
    ("certification", include_bytes!("../../assets/icons/certification.png")),
    ("cloud-database", include_bytes!("../../assets/icons/cloud-database.png")),
    ("data-bridge", include_bytes!("../../assets/icons/data-bridge.png")),
    ("data-exchange", include_bytes!("../../assets/icons/data-exchange.png")),
    ("data-sync", include_bytes!("../../assets/icons/data-sync.png")),
    ("edge-cloud", include_bytes!("../../assets/icons/edge-cloud.png")),
    ("encrypted-keys", include_bytes!("../../assets/icons/encrypted-keys.png")),
    ("gateway", include_bytes!("../../assets/icons/gateway.png")),
    ("global-wireless", include_bytes!("../../assets/icons/global-wireless.png")),
    ("hierarchy-tree", include_bytes!("../../assets/icons/hierarchy-tree.png")),
    ("idea-bidirectional", include_bytes!("../../assets/icons/idea-bidirectional.png")),
    ("iot-hub", include_bytes!("../../assets/icons/iot-hub.png")),
    ("mesh-network", include_bytes!("../../assets/icons/mesh-network.png")),
    ("mobile-connect", include_bytes!("../../assets/icons/mobile-connect.png")),
    ("mqtt-cloud", include_bytes!("../../assets/icons/mqtt-cloud.png")),
    ("security-lock", include_bytes!("../../assets/icons/security-lock.png")),
    ("security-std", include_bytes!("../../assets/icons/security-std.png")),
    ("settings-checklist", include_bytes!("../../assets/icons/settings-checklist.png")),
    ("smart-home", include_bytes!("../../assets/icons/smart-home.png")),
    ("verified-network", include_bytes!("../../assets/icons/verified-network.png")),
    ("workflow-loop", include_bytes!("../../assets/icons/workflow-loop.png")),
    ("xml-structure", include_bytes!("../../assets/icons/xml-structure.png")),
];
