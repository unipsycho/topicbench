//! Embedded UI icons from `assets/icons/`.

use eframe::egui::{self, ImageButton, TextureHandle, Vec2};
use std::collections::HashMap;

const ICON_PX: f32 = 18.0;

/// Named textures loaded once at startup.
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

    /// Image-only button (toolbar toggles).
    pub fn button(&self, ui: &mut egui::Ui, name: &str, selected: bool, tip: &str) -> egui::Response {
        if let Some(tex) = self.get(name) {
            let mut img = egui::Image::new(tex).fit_to_exact_size(Vec2::splat(ICON_PX));
            if selected {
                img = img.tint(egui::Color32::from_rgb(120, 200, 150));
            }
            ui.add(ImageButton::new(img)).on_hover_text(tip)
        } else {
            ui.small_button(tip).on_hover_text(tip)
        }
    }

    /// Icon + label button.
    pub fn labeled_button(&self, ui: &mut egui::Ui, name: &str, label: &str) -> egui::Response {
        ui.horizontal(|ui| {
            if let Some(tex) = self.get(name) {
                ui.add(
                    egui::Image::new(tex)
                        .fit_to_exact_size(Vec2::splat(ICON_PX))
                        .sense(egui::Sense::hover()),
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

/// Icons we ship (filenames without .png). Sheet excluded.
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
