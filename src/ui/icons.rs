//! Embedded UI icons from `assets/icons/`.
//!
//! Ribbon chrome borrows PhotoCraft (ui-egui) cues: solid chrome bar, quiet hover/selected
//! fills, icon ~½ of the hit box, thin group separators — with Word-style captions under groups.

use eframe::egui::{self, Color32, RichText, Sense, Stroke, TextureHandle, Vec2};
use std::collections::HashMap;

/// Hit box for a large ribbon command (Office-style; tighter under-icon padding).
const CMD_BOX: f32 = 44.0;
/// Glyph size ≈ half the box (PhotoCraft toolbar ratio ~0.52).
const CMD_ICON: f32 = 26.0;

/// Light chrome tokens (Studio Light / Breeze-adjacent, like PhotoCraft light themes).
pub const CHROME_BG: Color32 = Color32::from_rgb(246, 246, 248);
pub const CHROME_SEP: Color32 = Color32::from_rgb(214, 218, 222);
pub const CHROME_HOVER: Color32 = Color32::from_rgb(232, 234, 238);
pub const CHROME_ACCENT_SOFT: Color32 = Color32::from_rgb(210, 226, 244);
pub const CHROME_ACCENT_BORDER: Color32 = Color32::from_rgb(120, 160, 200);
pub const CHROME_CAPTION: Color32 = Color32::from_rgb(90, 98, 110);

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

    /// Large ribbon command: icon + caption under; transparent until hover (PhotoCraft-style).
    pub fn command(
        &self,
        ui: &mut egui::Ui,
        name: &str,
        label: &str,
        selected: bool,
        tip: &str,
    ) -> egui::Response {
        // Compact cell: less empty space under the glyph.
        let cell = Vec2::new(CMD_BOX + 2.0, CMD_BOX + 8.0);
        let (rect, response) = ui.allocate_exact_size(cell, Sense::click());
        let response = response.on_hover_text(tip);

        // Quiet chrome: fill only when selected / hovered — no idle border.
        if selected {
            ui.painter()
                .rect_filled(rect, 3.0, CHROME_ACCENT_SOFT);
            ui.painter().rect_stroke(
                rect,
                3.0,
                Stroke::new(1.0_f32, CHROME_ACCENT_BORDER),
                egui::StrokeKind::Inside,
            );
        } else if response.hovered() {
            ui.painter().rect_filled(rect, 3.0, CHROME_HOVER);
        }

        let icon_center = egui::pos2(rect.center().x, rect.top() + CMD_BOX * 0.40);
        if let Some(tex) = self.get(name) {
            let img = egui::Image::new(tex).fit_to_exact_size(Vec2::splat(CMD_ICON));
            let img_rect = egui::Rect::from_center_size(icon_center, Vec2::splat(CMD_ICON));
            img.paint_at(ui, img_rect);
        }

        ui.painter().text(
            egui::pos2(rect.center().x, rect.bottom() - 5.0),
            egui::Align2::CENTER_CENTER,
            label,
            egui::FontId::proportional(10.0),
            if selected {
                Color32::from_rgb(40, 80, 130)
            } else {
                ui.visuals().text_color()
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

/// Labeled ribbon group: controls on top, caption centered underneath (Word layout).
pub fn ribbon_group(ui: &mut egui::Ui, caption: &str, add_contents: impl FnOnce(&mut egui::Ui)) {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 1.0;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 3.0;
            add_contents(ui);
        });
        ui.vertical_centered(|ui| {
            ui.label(RichText::new(caption).small().color(CHROME_CAPTION));
        });
    });
}

/// 1px vertical rule between groups (PhotoCraft options-bar / toolbar separators).
pub fn ribbon_separator(ui: &mut egui::Ui) {
    ui.add_space(6.0);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(1.0, 56.0), Sense::hover());
    ui.painter().line_segment(
        [rect.center_top(), rect.center_bottom()],
        Stroke::new(1.0_f32, CHROME_SEP),
    );
    ui.add_space(6.0);
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
