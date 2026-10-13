//! Embedded UI icons from `assets/icons/`.

use eframe::egui::{self, Color32, Sense, Stroke, TextureHandle, Vec2};
use std::collections::HashMap;

/// Ribbon icon size (larger / more readable).
const ICON_PX: f32 = 28.0;

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

    /// Ribbon toggle: larger icon, **1px** border only (no padded multi-layer frame).
    pub fn button(&self, ui: &mut egui::Ui, name: &str, selected: bool, tip: &str) -> egui::Response {
        let size = Vec2::splat(ICON_PX + 4.0);
        let (rect, response) = ui.allocate_exact_size(size, Sense::click());
        let response = response.on_hover_text(tip);

        let border = if selected {
            Color32::from_rgb(70, 140, 200)
        } else {
            ui.visuals().widgets.inactive.fg_stroke.color
        };
        ui.painter().rect_stroke(
            rect,
            2.0,
            Stroke::new(1.0_f32, border),
            egui::StrokeKind::Outside,
        );

        if let Some(tex) = self.get(name) {
            let mut img = egui::Image::new(tex).fit_to_exact_size(Vec2::splat(ICON_PX));
            if selected {
                img = img.tint(Color32::from_rgb(100, 180, 230));
            }
            let img_rect = egui::Rect::from_center_size(rect.center(), Vec2::splat(ICON_PX));
            img.paint_at(ui, img_rect);
        } else {
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "·",
                egui::FontId::proportional(16.0),
                border,
            );
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
