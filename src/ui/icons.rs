//! Embedded UI icons from `assets/icons/` (transparent PNGs for dark chrome).
//!
//! Ribbon groups must **shrink-wrap** — a nested `vertical` inside a horizontal bar
//! otherwise expands and steals the rest of the row (hiding later groups).
//!
//! Source PNGs are 512²; we Lanczos-downsample to [`ICON_TEX_PX`] (2× ribbon glyph)
//! before upload so GPU scaling stays crisp (≈2:1 / 1:1 with DPI) instead of 512→36.

use eframe::egui::{self, Color32, FontId, Sense, Stroke, TextStyle, TextureHandle, Vec2};
use eframe::epaint::TextShape;
use image::imageops::FilterType;
use image::RgbaImage;
use std::collections::HashMap;
use std::f32::consts::FRAC_PI_2;

/// Current font-zoom factor from style Body size vs Style::default() (matches `apply_font_zoom`).
pub fn font_zoom_factor(ui: &egui::Ui) -> f32 {
    let body = ui
        .style()
        .text_styles
        .get(&TextStyle::Body)
        .map(|f| f.size)
        .unwrap_or(14.0);
    let base = egui::Style::default()
        .text_styles
        .get(&TextStyle::Body)
        .map(|f| f.size)
        .unwrap_or(14.0);
    if base > 0.0 {
        body / base
    } else {
        1.0
    }
}

/// Hardcoded ribbon/chrome size × font zoom (floor 8px, same as `apply_font_zoom`).
pub fn zoomed_font_size(ui: &egui::Ui, base: f32) -> f32 {
    (base * font_zoom_factor(ui)).max(8.0)
}

/// Extra width per font-zoom step above default (~4px) so captions aren't clipped.
const CMD_W_EXTRA_PER_STEP: f32 = 4.0;
/// Matches [`crate::mqtt::settings::FONT_ZOOM_STEP_FACTOR`].
const FONT_ZOOM_STEP: f32 = 0.10;

/// Full ribbon command cell width at the given font-zoom factor.
pub fn cmd_cell_w(zoom: f32) -> f32 {
    let steps_up = ((zoom - 1.0).max(0.0) / FONT_ZOOM_STEP).round();
    CMD_W + steps_up * CMD_W_EXTRA_PER_STEP
}

/// Large ribbon glyph (logical px — same for New/Save/Delete/Settings/Panels).
const CMD_ICON: f32 = 36.0;
/// Section header icons (Profiles / Topic tree / …) — same visual weight as ribbon CMD.
pub const SECTION_ICON: f32 = 36.0;
/// Slightly tighter horizontal; taller cell for caption bottom clearance.
const CMD_W: f32 = 52.0;
pub const CMD_H: f32 = 64.0;
/// Compact (collapsed ribbon) icon-only cell — two stack in [`CMD_H`].
const COMPACT_ICON: f32 = 24.0; // trial: 72/3 — was 20
pub const COMPACT_W: f32 = 32.0; // room for 24px glyph (+ was 28)
const COMPACT_H: f32 = 32.0; // was 30
/// Far-right zoom stack (+ / −): fixed width so hit targets never shift with font zoom.
pub const ZOOM_STACK_W: f32 = 40.0;
/// Icon size for stacked zoom — same weight as ribbon CMD / section icons.
const ZOOM_ICON: f32 = 36.0;
/// Dense inline action icons (Profiles body, tree copy/sticky, etc.).
pub const TINY_ICON: f32 = 24.0; // trial: 72/3 — was 18
/// Uploaded texture edge: 2× [`CMD_ICON`] for sharp HiDPI + clean half-scale at 1×.
const ICON_TEX_PX: u32 = 72;
/// Left rail width for rotated section labels.
const SECTION_RAIL_W: f32 = 18.0;
/// Pull rotated section text left so it clears the first command on hover.
const SECTION_LABEL_NUDGE_X: f32 = 14.0;

/// Dark chrome tokens (cool charcoal — ribbon + panels share `CHROME_BG`).
pub const CHROME_BG: Color32 = Color32::from_rgb(34, 36, 42);
pub const CHROME_SEP: Color32 = Color32::from_rgb(70, 74, 84);
pub const CHROME_HOVER: Color32 = Color32::from_rgb(58, 62, 72);
/// Stronger hover fill for icon buttons sitting on [`CHROME_SECTION`] header bars.
pub const CHROME_HEADER_HOVER: Color32 = Color32::from_rgb(78, 96, 128);
pub const CHROME_HEADER_SELECTED: Color32 = Color32::from_rgb(56, 92, 140);
pub const CHROME_ACCENT_SOFT: Color32 = Color32::from_rgb(48, 78, 112);
pub const CHROME_ACCENT_BORDER: Color32 = Color32::from_rgb(100, 160, 220);
/// Selectable list-row idle fill — clearly lifted from [`CHROME_BG`] (was nearly invisible).
pub const CHROME_ROW: Color32 = Color32::from_rgb(46, 50, 58);
/// Quiet strip behind section icon+title rows (stronger than list rows).
pub const CHROME_SECTION: Color32 = Color32::from_rgb(52, 56, 66);
/// Shared accent for all section heading labels (Profiles / Topic tree / …).
pub const CHROME_HEADING: Color32 = Color32::from_rgb(130, 185, 245);
/// Ribbon captions + topic-path gray (readable on CHROME_BG).
pub const CHROME_CAPTION: Color32 = Color32::from_rgb(190, 196, 208);
/// Secondary chrome labels / counts (`#n`, hints) — brighter than egui default `.weak()`.
pub const CHROME_MUTED: Color32 = Color32::from_rgb(175, 182, 196);
pub const CHROME_TEXT: Color32 = Color32::from_rgb(230, 232, 238);
pub const STATUS_BG: Color32 = Color32::from_rgb(26, 28, 34);
/// Subtle per-section ribbon tints (solid, slightly lifted from chrome bg).
pub const TINT_PROFILES: Color32 = Color32::from_rgb(48, 56, 70);
pub const TINT_PANELS: Color32 = Color32::from_rgb(46, 60, 52);
pub const TINT_APP: Color32 = Color32::from_rgb(62, 52, 48);
/// Pro ribbon group (cool teal, distinct from Panels green).
pub const TINT_PRO: Color32 = Color32::from_rgb(40, 58, 62);
/// Cool steel tint for the Stats ribbon group (right edge).
pub const TINT_STATS: Color32 = Color32::from_rgb(42, 54, 64);

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
        self.command_enabled(ui, name, label, selected, tip, true)
    }

    /// Ribbon command with optional disabled (dimmed, no click).
    pub fn command_enabled(
        &self,
        ui: &mut egui::Ui,
        name: &str,
        label: &str,
        selected: bool,
        tip: &str,
        enabled: bool,
    ) -> egui::Response {
        let cell = Vec2::new(cmd_cell_w(font_zoom_factor(ui)), CMD_H);
        let sense = if enabled {
            Sense::click()
        } else {
            Sense::hover()
        };
        let (rect, response) = ui.allocate_exact_size(cell, sense);
        // Suppress tips while Settings/Shortcuts overlay owns the pointer.
        let response = crate::ui::hover_tip::attach(
            response,
            if enabled {
                tip.to_owned()
            } else {
                format!("{tip} (disabled)")
            },
        );

        if enabled && selected {
            ui.painter().rect_filled(rect, 3.0, CHROME_ACCENT_SOFT);
            ui.painter().rect_stroke(
                rect,
                3.0,
                Stroke::new(1.0_f32, CHROME_ACCENT_BORDER),
                egui::StrokeKind::Inside,
            );
        } else if enabled && response.hovered() {
            ui.painter().rect_filled(rect, 3.0, CHROME_HOVER);
        }

        // Caption with a few px clearance above the cell bottom (avoids section chrome collision).
        // `\n` → two-line captions (e.g. Book- / marks, Color / Rules) under the same icon.
        let caption = if !enabled {
            Color32::from_rgb(100, 104, 112)
        } else if selected {
            Color32::from_rgb(180, 210, 255)
        } else {
            CHROME_TEXT
        };
        let font = FontId::proportional(zoomed_font_size(ui, 11.0));
        let lines: Vec<&str> = label.lines().filter(|l| !l.is_empty()).collect();
        // Two-line captions need a slightly higher icon so text doesn't collide.
        let icon_y = if lines.len() > 1 {
            rect.top() + 18.0
        } else {
            rect.top() + 22.0
        };
        let icon_center = egui::pos2(rect.center().x, icon_y);
        if let Some(tex) = self.get(name) {
            // Exact square cell — no stretch; texture already pre-scaled to 2×.
            let mut img = egui::Image::new(tex)
                .fit_to_exact_size(Vec2::splat(CMD_ICON))
                .maintain_aspect_ratio(true);
            if !enabled {
                img = img.tint(Color32::from_rgba_unmultiplied(255, 255, 255, 90));
            }
            let img_rect = egui::Rect::from_center_size(icon_center, Vec2::splat(CMD_ICON));
            img.paint_at(ui, img_rect);
        }

        if lines.len() <= 1 {
            ui.painter().text(
                egui::pos2(rect.center().x, rect.bottom() - 10.0),
                egui::Align2::CENTER_CENTER,
                lines.first().copied().unwrap_or(label),
                font,
                caption,
            );
        } else {
            let line_step = zoomed_font_size(ui, 11.0) + 1.0;
            // Stack upward from near the cell bottom so multi-line captions stay clear of the icon.
            let base_y = rect.bottom() - 6.0;
            let n = lines.len();
            for (i, line) in lines.iter().enumerate() {
                let y = base_y - ((n - 1 - i) as f32) * line_step;
                ui.painter().text(
                    egui::pos2(rect.center().x, y),
                    egui::Align2::CENTER_CENTER,
                    *line,
                    font.clone(),
                    caption,
                );
            }
        }
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
        self.tiny_with_pad(ui, name, selected, tip, 2.0, false)
    }

    /// Icon button on a blue [`CHROME_SECTION`] header — higher-contrast hover chrome.
    pub fn tiny_header(
        &self,
        ui: &mut egui::Ui,
        name: &str,
        selected: bool,
        tip: &str,
    ) -> egui::Response {
        self.tiny_with_pad(ui, name, selected, tip, 2.0, true)
    }

    /// Left-cluster tree icons: same glyph, half pad so neighbors sit closer.
    pub fn tiny_cluster(
        &self,
        ui: &mut egui::Ui,
        name: &str,
        selected: bool,
        tip: &str,
    ) -> egui::Response {
        self.tiny_with_pad(ui, name, selected, tip, 0.5, false)
    }

    fn tiny_with_pad(
        &self,
        ui: &mut egui::Ui,
        name: &str,
        selected: bool,
        tip: &str,
        pad: f32,
        on_header: bool,
    ) -> egui::Response {
        let size = Vec2::splat(TINY_ICON + pad * 2.0);
        let (rect, response) = ui.allocate_exact_size(size, Sense::click());
        let response = crate::ui::hover_tip::attach(response, tip);
        if selected {
            let fill = if on_header {
                CHROME_HEADER_SELECTED
            } else {
                CHROME_ACCENT_SOFT
            };
            ui.painter().rect_filled(rect, 2.0, fill);
        } else if response.hovered() {
            let fill = if on_header {
                CHROME_HEADER_HOVER
            } else {
                CHROME_HOVER
            };
            ui.painter().rect_filled(rect, 2.0, fill);
        }
        if let Some(tex) = self.get(name) {
            let img = egui::Image::new(tex)
                .fit_to_exact_size(Vec2::splat(TINY_ICON))
                .maintain_aspect_ratio(true);
            img.paint_at(ui, egui::Rect::from_center_size(rect.center(), Vec2::splat(TINY_ICON)));
        }
        response
    }

    /// Collapsed ribbon: icon-only (no caption), keeps hover tip + shortcuts.
    pub fn command_compact(
        &self,
        ui: &mut egui::Ui,
        name: &str,
        selected: bool,
        tip: &str,
    ) -> egui::Response {
        self.command_compact_enabled(ui, name, selected, tip, true)
    }

    /// Collapsed ribbon command with optional disabled state.
    pub fn command_compact_enabled(
        &self,
        ui: &mut egui::Ui,
        name: &str,
        selected: bool,
        tip: &str,
        enabled: bool,
    ) -> egui::Response {
        let cell = Vec2::new(COMPACT_W, COMPACT_H);
        let sense = if enabled {
            Sense::click()
        } else {
            Sense::hover()
        };
        let (rect, response) = ui.allocate_exact_size(cell, sense);
        let response = crate::ui::hover_tip::attach(
            response,
            if enabled {
                tip.to_owned()
            } else {
                format!("{tip} (disabled)")
            },
        );

        if enabled && selected {
            ui.painter().rect_filled(rect, 3.0, CHROME_ACCENT_SOFT);
            ui.painter().rect_stroke(
                rect,
                3.0,
                Stroke::new(1.0_f32, CHROME_ACCENT_BORDER),
                egui::StrokeKind::Inside,
            );
        } else if enabled && response.hovered() {
            ui.painter().rect_filled(rect, 3.0, CHROME_HOVER);
        }

        if let Some(tex) = self.get(name) {
            let mut img = egui::Image::new(tex)
                .fit_to_exact_size(Vec2::splat(COMPACT_ICON))
                .maintain_aspect_ratio(true);
            if !enabled {
                img = img.tint(Color32::from_rgba_unmultiplied(255, 255, 255, 90));
            }
            img.paint_at(
                ui,
                egui::Rect::from_center_size(rect.center(), Vec2::splat(COMPACT_ICON)),
            );
        }
        response
    }

    /// Icon-only zoom cell (no caption). Fixed size — independent of font zoom.
    /// `fallback` is painted when the PNG is missing so the control never goes blank.
    pub fn command_zoom_icon(
        &self,
        ui: &mut egui::Ui,
        name: &str,
        fallback: &str,
        tip: &str,
    ) -> egui::Response {
        let cell = Vec2::new(ZOOM_STACK_W, CMD_H * 0.5);
        let (rect, response) = ui.allocate_exact_size(cell, Sense::click());
        let response = crate::ui::hover_tip::attach(response, tip.to_owned());
        if response.hovered() {
            ui.painter().rect_filled(rect, 3.0, CHROME_HOVER);
        }
        if let Some(tex) = self.get(name) {
            let img = egui::Image::new(tex)
                .fit_to_exact_size(Vec2::splat(ZOOM_ICON))
                .maintain_aspect_ratio(true);
            img.paint_at(
                ui,
                egui::Rect::from_center_size(rect.center(), Vec2::splat(ZOOM_ICON)),
            );
        } else {
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                fallback,
                FontId::proportional(22.0),
                CHROME_TEXT,
            );
        }
        response
    }
}

/// Full-width chrome bar behind a section icon+title row (and optional trailing widgets).
pub fn section_header_bar(ui: &mut egui::Ui, add_contents: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::NONE
        .fill(CHROME_SECTION)
        .corner_radius(3.0)
        // Tight around 36px icons — was (8,5) and looked oversized.
        .inner_margin(egui::Margin::symmetric(4, 1))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.spacing_mut().item_spacing.x = 4.0;
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.horizontal(add_contents);
        });
}

/// Section title in the shared [`CHROME_HEADING`] accent (not muted gray).
pub fn section_title(ui: &mut egui::Ui, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .heading()
            .strong()
            .color(CHROME_HEADING),
    );
}

/// Section-header icon at [`SECTION_ICON`] (36px).
pub fn section_icon(ui: &mut egui::Ui, icons: &IconSet, name: &str) {
    if let Some(tex) = icons.get(name) {
        ui.add(
            egui::Image::new(tex)
                .fit_to_exact_size(Vec2::splat(SECTION_ICON))
                .sense(Sense::hover()),
        );
    }
}

/// Tiny inline image at [`TINY_ICON`] (24px) — decorative, not a button.
pub fn tiny_image(ui: &mut egui::Ui, icons: &IconSet, name: &str) {
    if let Some(tex) = icons.get(name) {
        ui.add(
            egui::Image::new(tex)
                .fit_to_exact_size(Vec2::splat(TINY_ICON))
                .sense(Sense::hover()),
        );
    }
}

/// Paint a 2×3 grip (drag handle) centered in `rect`.
pub fn paint_drag_grip(ui: &mut egui::Ui, rect: egui::Rect) {
    let color = CHROME_MUTED;
    let r = 1.2_f32;
    let cx = rect.center().x;
    let cy = rect.center().y;
    let dx = 3.0_f32;
    let dy = 3.5_f32;
    for col in [-1.0_f32, 1.0] {
        for row in [-1.0_f32, 0.0, 1.0] {
            ui.painter()
                .circle_filled(egui::pos2(cx + col * dx, cy + row * dy), r, color);
        }
    }
}

/// Estimated width of a full (icon+caption) ribbon group with `n` commands.
pub fn estimate_group_full_w(n_cmds: usize, zoom: f32) -> f32 {
    // Frame pad 4×2 + rail 18 + gap 4 + n×cmd_w + (n−1)×1.
    let n = n_cmds.max(1) as f32;
    let cw = cmd_cell_w(zoom);
    8.0 + SECTION_RAIL_W + 4.0 + n * cw + (n - 1.0) * 1.0
}

/// Estimated width of a collapsed 2-high icon grid with `n` commands.
pub fn estimate_group_compact_w(n_cmds: usize) -> f32 {
    let cols = n_cmds.div_ceil(2).max(1) as f32;
    8.0 + SECTION_RAIL_W + 4.0 + cols * COMPACT_W + (cols - 1.0) * 2.0
}

/// Width reserved for the far-right zoom +/− stack (no section rail).
pub fn estimate_zoom_stack_w() -> f32 {
    ZOOM_STACK_W
}

/// Ribbon separator footprint (spaces + 1px rule).
pub fn estimate_separator_w() -> f32 {
    4.0 + 1.0 + 4.0
}

/// Lay out `n` compact buttons in columns of two inside a **full [`CMD_H`]** band.
///
/// The allocated block is always `CMD_H` tall (same as expanded ribbon groups).
/// Icons sit at the top and bottom of that band — the tinted panel does not shrink.
pub fn ribbon_compact_columns(ui: &mut egui::Ui, n: usize, mut add_btn: impl FnMut(usize, &mut egui::Ui)) {
    let cols = n.div_ceil(2).max(1);
    let gap_x = 2.0;
    let block_w = cols as f32 * COMPACT_W + (cols.saturating_sub(1) as f32) * gap_x;
    // Exact full ribbon body height — matches Profiles/Panels/App when expanded.
    let (block, _) = ui.allocate_exact_size(Vec2::new(block_w, CMD_H), Sense::hover());

    for col in 0..cols {
        let i0 = col * 2;
        let x = block.left() + col as f32 * (COMPACT_W + gap_x);
        // Top row — pinned to the top of the shared band.
        if i0 < n {
            let top = egui::Rect::from_min_size(egui::pos2(x, block.top()), Vec2::new(COMPACT_W, COMPACT_H));
            let mut child = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(top)
                    .layout(egui::Layout::top_down(egui::Align::Center)),
            );
            add_btn(i0, &mut child);
        }
        // Bottom row — pinned to the bottom of the same CMD_H band.
        if i0 + 1 < n {
            let bot = egui::Rect::from_min_size(
                egui::pos2(x, block.bottom() - COMPACT_H),
                Vec2::new(COMPACT_W, COMPACT_H),
            );
            let mut child = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(bot)
                    .layout(egui::Layout::top_down(egui::Align::Center)),
            );
            add_btn(i0 + 1, &mut child);
        }
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
            // Pin shared body height so compact sections cannot shrink the tint.
            ui.set_min_height(CMD_H);
            ui.set_max_height(CMD_H);
            // Single LTR row of exact-size cells — no nested vertical expanders.
            ui.horizontal(|ui| {
                ui.set_min_height(CMD_H);
                ui.spacing_mut().item_spacing.x = 4.0;
                // Left rail: rotated section name
                let rail = Vec2::new(SECTION_RAIL_W, CMD_H);
                let (rail_rect, _) = ui.allocate_exact_size(rail, Sense::hover());
                paint_rotated_section_label(ui, rail_rect, caption);
                ui.spacing_mut().item_spacing.x = 1.0;
                add_contents(ui);
            });
            ui.shrink_width_to_current();
        });
}

/// Paint `caption` rotated −90° (reads upward) centered in `rect`.
fn paint_rotated_section_label(ui: &mut egui::Ui, rect: egui::Rect, caption: &str) {
    let galley = ui.fonts(|f| {
        f.layout_no_wrap(
            caption.to_owned(),
            FontId::proportional(zoomed_font_size(ui, 11.0)),
            CHROME_CAPTION,
        )
    });
    // Pivot near rail center; rotate CCW so text runs bottom→top. Nudge left so
    // the glyph clears the first command cell on hover.
    let pos = egui::pos2(
        rect.center().x + galley.size().y * 0.5 - SECTION_LABEL_NUDGE_X,
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
    ("add_blue", include_bytes!("../../assets/icons/add_blue.png")),
    ("alerts", include_bytes!("../../assets/icons/alerts.png")),
    ("bookmarks", include_bytes!("../../assets/icons/bookmarks.png")),
    ("broker_server", include_bytes!("../../assets/icons/broker_server.png")),
    ("certificates", include_bytes!("../../assets/icons/certificates.png")),
    ("clipboard_checklist", include_bytes!("../../assets/icons/clipboard_checklist.png")),
    ("close_red", include_bytes!("../../assets/icons/close_red.png")),
    ("cloud_broker", include_bytes!("../../assets/icons/cloud_broker.png")),
    ("color_rules", include_bytes!("../../assets/icons/color_rules.png")),
    ("configuration_sliders", include_bytes!("../../assets/icons/configuration_sliders.png")),
    ("connect", include_bytes!("../../assets/icons/connect.png")),
    ("connection_status", include_bytes!("../../assets/icons/connection_status.png")),
    ("dashboard", include_bytes!("../../assets/icons/dashboard.png")),
    ("data_table", include_bytes!("../../assets/icons/data_table.png")),
    ("delete", include_bytes!("../../assets/icons/delete.png")),
    ("delete_red", include_bytes!("../../assets/icons/delete_red.png")),
    ("disconnect", include_bytes!("../../assets/icons/disconnect.png")),
    ("event_log", include_bytes!("../../assets/icons/event_log.png")),
    ("event_logs", include_bytes!("../../assets/icons/event_logs.png")),
    ("export", include_bytes!("../../assets/icons/export.png")),
    ("export_download", include_bytes!("../../assets/icons/export_download.png")),
    ("import", include_bytes!("../../assets/icons/import.png")),
    ("incoming_messages", include_bytes!("../../assets/icons/incoming_messages.png")),
    ("json_payload", include_bytes!("../../assets/icons/json_payload.png")),
    ("last_will_security", include_bytes!("../../assets/icons/last_will_security.png")),
    ("line_graph", include_bytes!("../../assets/icons/line_graph.png")),
    ("message_history", include_bytes!("../../assets/icons/message_history.png")),
    ("metric_gauge", include_bytes!("../../assets/icons/metric_gauge.png")),
    ("node_graph", include_bytes!("../../assets/icons/node_graph.png")),
    ("panel_docked", include_bytes!("../../assets/icons/panel_docked.png")),
    ("panel_undock", include_bytes!("../../assets/icons/panel_undock.png")),
    ("pause", include_bytes!("../../assets/icons/pause.png")),
    ("play", include_bytes!("../../assets/icons/play.png")),
    ("playback", include_bytes!("../../assets/icons/playback.png")),
    ("profile", include_bytes!("../../assets/icons/profile.png")),
    ("profile_add", include_bytes!("../../assets/icons/profile_add.png")),
    ("profile_duplicate", include_bytes!("../../assets/icons/profile_duplicate.png")),
    ("profile_edit", include_bytes!("../../assets/icons/profile_edit.png")),
    ("publish_message", include_bytes!("../../assets/icons/publish_message.png")),
    ("qos_levels", include_bytes!("../../assets/icons/qos_levels.png")),
    ("reconnect", include_bytes!("../../assets/icons/reconnect.png")),
    ("record", include_bytes!("../../assets/icons/record.png")),
    ("record_red", include_bytes!("../../assets/icons/record_red.png")),
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
    ("traffic", include_bytes!("../../assets/icons/traffic.png")),
    ("unsubscribe_topic", include_bytes!("../../assets/icons/unsubscribe_topic.png")),
    ("user_credentials", include_bytes!("../../assets/icons/user_credentials.png")),
    ("zoom_in", include_bytes!("../../assets/icons/zoom_in.png")),
    ("zoom_out", include_bytes!("../../assets/icons/zoom_out.png"))
];
