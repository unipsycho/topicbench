//! Embedded UI icons from `assets/icons/` (transparent PNGs for dark chrome).
//!
//! Ribbon groups must **shrink-wrap** — a nested `vertical` inside a horizontal bar
//! otherwise expands and steals the rest of the row (hiding later groups).
//!
//! Source PNGs are 512²; we Lanczos-downsample to [`ICON_TEX_PX`] (2× ribbon glyph)
//! before upload so GPU scaling stays crisp (≈2:1 / 1:1 with DPI) instead of 512→36.

use eframe::egui::{self, Color32, FontId, Sense, Stroke, TextureHandle, Vec2};
use eframe::epaint::TextShape;
use image::imageops::FilterType;
use image::RgbaImage;
use std::collections::HashMap;
use std::f32::consts::FRAC_PI_2;

/// Large ribbon glyph (logical px — same for New/Save/Delete/Settings/Panels).
const CMD_ICON: f32 = 36.0;
/// Slightly tighter horizontal; taller cell for caption bottom clearance.
const CMD_W: f32 = 52.0;
const CMD_H: f32 = 64.0;
/// Dense topic-tree / inline action icons.
const TINY_ICON: f32 = 14.0;
/// Uploaded texture edge: 2× [`CMD_ICON`] for sharp HiDPI + clean half-scale at 1×.
const ICON_TEX_PX: u32 = 72;
/// Left rail width for rotated section labels.
const SECTION_RAIL_W: f32 = 18.0;

/// Dark chrome tokens.
pub const CHROME_BG: Color32 = Color32::from_rgb(40, 42, 48);
pub const CHROME_SEP: Color32 = Color32::from_rgb(70, 74, 84);
pub const CHROME_HOVER: Color32 = Color32::from_rgb(58, 62, 72);
pub const CHROME_ACCENT_SOFT: Color32 = Color32::from_rgb(48, 78, 112);
pub const CHROME_ACCENT_BORDER: Color32 = Color32::from_rgb(100, 160, 220);
pub const CHROME_CAPTION: Color32 = Color32::from_rgb(170, 176, 188);
pub const CHROME_TEXT: Color32 = Color32::from_rgb(230, 232, 238);
pub const STATUS_BG: Color32 = Color32::from_rgb(28, 30, 36);
/// Subtle per-section ribbon tints (solid, slightly lifted from chrome bg).
pub const TINT_PROFILES: Color32 = Color32::from_rgb(48, 56, 70);
pub const TINT_PANELS: Color32 = Color32::from_rgb(46, 60, 52);
pub const TINT_APP: Color32 = Color32::from_rgb(62, 52, 48);

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
            // Exact square cell — no stretch; texture already pre-scaled to 2×.
            let img = egui::Image::new(tex)
                .fit_to_exact_size(Vec2::splat(CMD_ICON))
                .maintain_aspect_ratio(true);
            let img_rect = egui::Rect::from_center_size(icon_center, Vec2::splat(CMD_ICON));
            img.paint_at(ui, img_rect);
        }

        // Caption with a few px clearance above the cell bottom (avoids section chrome collision).
        ui.painter().text(
            egui::pos2(rect.center().x, rect.bottom() - 10.0),
            egui::Align2::CENTER_CENTER,
            label,
            FontId::proportional(11.0),
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
            let img = egui::Image::new(tex)
                .fit_to_exact_size(Vec2::splat(TINY_ICON))
                .maintain_aspect_ratio(true);
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

/// Content-sized ribbon group: tinted panel + 90° section label on the left rail.
/// Shrink-wraps so later groups are not clipped to 0 width.
pub fn ribbon_group(
    ui: &mut egui::Ui,
    caption: &str,
    tint: Color32,
    add_contents: impl FnOnce(&mut egui::Ui),
) {
    egui::Frame::NONE
        .fill(tint)
        .corner_radius(3.0)
        .inner_margin(egui::Margin::symmetric(4, 3))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                // Left rail: rotated section name
                let rail = Vec2::new(SECTION_RAIL_W, CMD_H);
                let (rail_rect, _) = ui.allocate_exact_size(rail, Sense::hover());
                paint_rotated_section_label(ui, rail_rect, caption);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 1.0;
                    add_contents(ui);
                });
            });
            ui.shrink_width_to_current();
        });
}

/// Paint `caption` rotated −90° (reads upward) centered in `rect`.
fn paint_rotated_section_label(ui: &mut egui::Ui, rect: egui::Rect, caption: &str) {
    let galley = ui.fonts(|f| {
        f.layout_no_wrap(
            caption.to_owned(),
            FontId::proportional(11.0),
            CHROME_CAPTION,
        )
    });
    // Pivot at bottom-center of rail; rotate CCW so text runs bottom→top along the edge.
    let pos = egui::pos2(
        rect.center().x + galley.size().y * 0.5,
        rect.center().y + galley.size().x * 0.5,
    );
    ui.painter().add(
        TextShape::new(pos, galley, CHROME_CAPTION).with_angle(-FRAC_PI_2),
    );
}

/// 1px vertical rule between groups (content-sized, not expanding).
pub fn ribbon_separator(ui: &mut egui::Ui) {
    ui.add_space(4.0);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(1.0, CMD_H + 6.0), Sense::hover());
    ui.painter().line_segment(
        [rect.center_top(), rect.center_bottom()],
        Stroke::new(1.0_f32, CHROME_SEP),
    );
    ui.add_space(4.0);
}

fn load_png(ctx: &egui::Context, id: &str, bytes: &[u8]) -> Option<TextureHandle> {
    let raw = image::load_from_memory(bytes).ok()?.into_rgba8();
    let image = prepare_icon_texture(&raw, ICON_TEX_PX);
    let size = [image.width() as usize, image.height() as usize];
    let color = egui::ColorImage::from_rgba_unmultiplied(size, image.as_raw());
    // LINEAR on a 72px atlas + mipmaps: crisp at ribbon size, clean when used tiny.
    let opts = egui::TextureOptions::LINEAR.with_mipmap_mode(Some(egui::TextureFilter::Linear));
    Some(ctx.load_texture(id, color, opts))
}

/// Downsample source icon to a square `target` edge without stretching.
/// Integer Triangle halvings while possible (512→256→128→64), then Lanczos3 to `target`.
fn prepare_icon_texture(src: &RgbaImage, target: u32) -> RgbaImage {
    let (w, h) = src.dimensions();
    if w == 0 || h == 0 || target == 0 {
        return src.clone();
    }
    let square = fit_to_square(src);
    let side = square.width();
    if side == target {
        return square;
    }
    if side < target {
        return pad_to_size(&square, target);
    }

    let mut img = square;
    while img.width() >= target * 2 && img.width() % 2 == 0 {
        let next = img.width() / 2;
        img = image::imageops::resize(&img, next, next, FilterType::Triangle);
    }
    if img.width() == target {
        img
    } else {
        image::imageops::resize(&img, target, target, FilterType::Lanczos3)
    }
}

/// Center `src` on a transparent square whose side is max(w, h).
fn fit_to_square(src: &RgbaImage) -> RgbaImage {
    let (w, h) = src.dimensions();
    if w == h {
        return src.clone();
    }
    let side = w.max(h);
    let mut out = RgbaImage::new(side, side);
    let ox = (side - w) / 2;
    let oy = (side - h) / 2;
    for y in 0..h {
        for x in 0..w {
            out.put_pixel(ox + x, oy + y, *src.get_pixel(x, y));
        }
    }
    out
}

fn pad_to_size(src: &RgbaImage, target: u32) -> RgbaImage {
    let (w, h) = src.dimensions();
    if w == target && h == target {
        return src.clone();
    }
    let mut out = RgbaImage::new(target, target);
    let ox = (target.saturating_sub(w)) / 2;
    let oy = (target.saturating_sub(h)) / 2;
    for y in 0..h.min(target) {
        for x in 0..w.min(target) {
            out.put_pixel(ox + x, oy + y, *src.get_pixel(x, y));
        }
    }
    out
}

const ICONS: &[(&str, &[u8])] = &[
    ("access_key", include_bytes!("../../assets/icons/access_key.png")),
    ("alerts", include_bytes!("../../assets/icons/alerts.png")),
    ("broker_server", include_bytes!("../../assets/icons/broker_server.png")),
    ("clipboard_checklist", include_bytes!("../../assets/icons/clipboard_checklist.png")),
    ("cloud_broker", include_bytes!("../../assets/icons/cloud_broker.png")),
    ("configuration_sliders", include_bytes!("../../assets/icons/configuration_sliders.png")),
    ("connect", include_bytes!("../../assets/icons/connect.png")),
    ("connection_status", include_bytes!("../../assets/icons/connection_status.png")),
    ("dashboard", include_bytes!("../../assets/icons/dashboard.png")),
    ("data_table", include_bytes!("../../assets/icons/data_table.png")),
    ("disconnect", include_bytes!("../../assets/icons/disconnect.png")),
    ("event_log", include_bytes!("../../assets/icons/event_log.png")),
    ("event_logs", include_bytes!("../../assets/icons/event_logs.png")),
    ("export_download", include_bytes!("../../assets/icons/export_download.png")),
    ("incoming_messages", include_bytes!("../../assets/icons/incoming_messages.png")),
    ("json_payload", include_bytes!("../../assets/icons/json_payload.png")),
    ("last_will_security", include_bytes!("../../assets/icons/last_will_security.png")),
    ("line_graph", include_bytes!("../../assets/icons/line_graph.png")),
    ("message_history", include_bytes!("../../assets/icons/message_history.png")),
    ("metric_gauge", include_bytes!("../../assets/icons/metric_gauge.png")),
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
    ("server_settings", include_bytes!("../../assets/icons/server_settings.png")),
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
    ("zoom_in", include_bytes!("../../assets/icons/zoom_in.png")),
    ("zoom_out", include_bytes!("../../assets/icons/zoom_out.png"))
];
