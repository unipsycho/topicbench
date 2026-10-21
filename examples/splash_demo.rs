//! Temporary splash demo — transparent window + AA logo crossfade.
//!
//!   cargo run --example splash_demo
//!
//! Needs compositor transparency (Hyprland/Wayland usually OK).
//! Click / Space / Esc to replay. Close the window when done.

use eframe::egui::{self, Color32, FontId, Pos2, Rect, Sense, Vec2};
use eframe::egui::TextureHandle;
use image::RgbaImage;
use std::time::Instant;

const CHROME_BG: Color32 = Color32::from_rgb(34, 36, 42);

/// Logo alone on glass — fade transparent → full in 0.5s.
const LOGO_IN: f32 = 0.50;
const LOGO_HOLD: f32 = 0.35;
/// App chrome fades up while logo fades down.
const CROSSFADE: f32 = 0.85;
const TOTAL: f32 = LOGO_IN + LOGO_HOLD + CROSSFADE;

struct SplashDemo {
    started: Instant,
    logo: Option<TextureHandle>,
    done: bool,
}

impl SplashDemo {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        cc.egui_ctx.set_theme(egui::ThemePreference::Dark);
        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = Color32::TRANSPARENT;
        visuals.window_fill = Color32::TRANSPARENT;
        cc.egui_ctx.set_visuals(visuals);

        Self {
            started: Instant::now(),
            logo: load_splash_logo(&cc.egui_ctx),
            done: false,
        }
    }

    fn restart(&mut self) {
        self.started = Instant::now();
        self.done = false;
    }

    fn t(&self) -> f32 {
        self.started.elapsed().as_secs_f32()
    }

    /// 0 = fully transparent window, 1 = solid app chrome.
    fn window_alpha(&self) -> f32 {
        let t = self.t();
        let cross_start = LOGO_IN + LOGO_HOLD;
        if t < cross_start {
            0.0
        } else {
            ((t - cross_start) / CROSSFADE).clamp(0.0, 1.0)
        }
    }

    /// Logo opacity over the timeline.
    fn logo_alpha(&self) -> f32 {
        let t = self.t();
        if t < LOGO_IN {
            (t / LOGO_IN).clamp(0.0, 1.0)
        } else if t < LOGO_IN + LOGO_HOLD {
            1.0
        } else {
            1.0 - ((t - LOGO_IN - LOGO_HOLD) / CROSSFADE).clamp(0.0, 1.0)
        }
    }

    /// Subtle shrink over the whole logo life (fade-in → out): 1.0 → 0.85 (−15%).
    fn logo_scale(&self) -> f32 {
        let p = (self.t() / TOTAL).clamp(0.0, 1.0);
        1.0 - 0.15 * p
    }
}

impl eframe::App for SplashDemo {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        // Keep GL clear fully transparent — we paint the fading chrome ourselves.
        [0.0, 0.0, 0.0, 0.0]
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let t = self.t();
        if t >= TOTAL {
            self.done = true;
        }

        let skip = ctx.input(|i| {
            i.key_pressed(egui::Key::Escape)
                || i.pointer.any_click()
                || i.key_pressed(egui::Key::Space)
        });
        if skip && (self.done || t > 0.12) {
            self.restart();
        }

        if !self.done {
            ctx.request_repaint();
        }

        let win_a = self.window_alpha();
        let logo_a = self.logo_alpha();

        // Transparent panel — no opaque egui frame filling the viewport.
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ctx, |ui| {
                let full = ui.max_rect();
                let painter = ui.painter_at(full);
                ui.allocate_rect(full, Sense::click());

                // Simulated app chrome fading in under the logo.
                if win_a > 0.01 {
                    let a = (255.0 * win_a) as u8;
                    painter.rect_filled(
                        full,
                        0.0,
                        Color32::from_rgba_unmultiplied(
                            CHROME_BG.r(),
                            CHROME_BG.g(),
                            CHROME_BG.b(),
                            a,
                        ),
                    );
                    // Fake ribbon strip so “app arriving” is visible.
                    if win_a > 0.2 {
                        let strip = Rect::from_min_size(
                            full.min,
                            Vec2::new(full.width(), 56.0),
                        );
                        let strip_a = ((win_a - 0.2) / 0.8).clamp(0.0, 1.0);
                        let sa = (220.0 * strip_a) as u8;
                        painter.rect_filled(
                            strip,
                            0.0,
                            Color32::from_rgba_unmultiplied(42, 44, 52, sa),
                        );
                        painter.text(
                            Pos2::new(full.left() + 16.0, full.top() + 28.0),
                            egui::Align2::LEFT_CENTER,
                            "MQTT Master",
                            FontId::proportional(16.0),
                            Color32::from_rgba_unmultiplied(200, 205, 215, sa),
                        );
                    }
                }

                // AA logo ~ half the window; slow −15% shrink while visible.
                if logo_a > 0.01 {
                    if let Some(logo) = &self.logo {
                        let tex = logo.size_vec2();
                        let target = full.size() * 0.5;
                        let fit = (target.x / tex.x).min(target.y / tex.y);
                        let draw = tex * fit * self.logo_scale();
                        let rect = Rect::from_center_size(full.center(), draw);
                        let a = (255.0 * logo_a) as u8;
                        painter.image(
                            logo.id(),
                            rect,
                            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                            Color32::from_rgba_unmultiplied(255, 255, 255, a),
                        );
                    }
                }

                let hint = if self.done {
                    "Done — click / Space / Esc to replay"
                } else if win_a < 0.05 {
                    "Transparent window — logo only"
                } else {
                    "Window fading in — logo fading out"
                };
                let hint_a = if win_a < 0.05 {
                    (180.0 * logo_a) as u8
                } else {
                    (160.0 * win_a) as u8
                };
                painter.text(
                    Pos2::new(full.center().x, full.bottom() - 24.0),
                    egui::Align2::CENTER_BOTTOM,
                    hint,
                    FontId::proportional(12.0),
                    Color32::from_rgba_unmultiplied(160, 165, 180, hint_a.max(40)),
                );
            });
    }
}

fn load_splash_logo(ctx: &egui::Context) -> Option<TextureHandle> {
    let bytes = include_bytes!("../assets/agile-automation-logo.png");
    let raw = image::load_from_memory(bytes).ok()?.into_rgba8();
    let (w, h) = raw.dimensions();
    // Keep more resolution — splash paints ~half the window.
    const MAX_H: u32 = 512;
    let factor = (h / MAX_H).max(1);
    let cw = (w / factor) * factor;
    let ch = (h / factor) * factor;
    let ox = (w - cw) / 2;
    let oy = (h - ch) / 2;
    let cropped = image::imageops::crop_imm(&raw, ox, oy, cw, ch).to_image();
    let logo = integer_box_downscale(&cropped, factor);
    let size = [logo.width() as usize, logo.height() as usize];
    let color = egui::ColorImage::from_rgba_unmultiplied(size, logo.as_raw());
    Some(ctx.load_texture(
        "splash-aa-logo",
        color,
        egui::TextureOptions {
            magnification: egui::TextureFilter::Linear,
            minification: egui::TextureFilter::Linear,
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

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([900.0, 560.0])
            .with_title("MQTT splash demo (temporary)")
            .with_transparent(true)
            // No title bar so only the logo floats on the desktop.
            .with_decorations(false),
        vsync: false,
        ..Default::default()
    };
    eframe::run_native(
        "MQTT splash demo",
        options,
        Box::new(|cc| Ok(Box::new(SplashDemo::new(cc)))),
    )
}
