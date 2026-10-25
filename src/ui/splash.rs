//! Startup splash: floating AA logo on a transparent window, then chrome crossfade.
//!
//! Timeline (~1.7s): logo fades in 0.5s → brief hold → window/chrome up + logo out.
//! Subtle −15% scale over the whole logo life. Click / Space / Esc skips.

use crate::ui::icons::CHROME_BG;
use eframe::egui::{self, Color32, Pos2, Rect, Sense, TextureHandle};
#[cfg(not(windows))]
use eframe::egui::ViewportCommand;
use std::time::Instant;

const LOGO_IN: f32 = 0.50;
const LOGO_HOLD: f32 = 0.35;
const CROSSFADE: f32 = 0.85;
const TOTAL: f32 = LOGO_IN + LOGO_HOLD + CROSSFADE;

pub struct SplashIntro {
    started: Instant,
    logo: Option<TextureHandle>,
    /// Viewport still undecorated / transparent until we finish.
    handed_off: bool,
}

impl SplashIntro {
    pub fn new(logo: Option<TextureHandle>) -> Self {
        Self {
            started: Instant::now(),
            logo,
            handed_off: false,
        }
    }

    fn t(&self) -> f32 {
        self.started.elapsed().as_secs_f32()
    }

    /// 0 = glass, 1 = solid chrome wash (then real UI takes over).
    fn window_alpha(&self) -> f32 {
        let t = self.t();
        let cross_start = LOGO_IN + LOGO_HOLD;
        if t < cross_start {
            0.0
        } else {
            ((t - cross_start) / CROSSFADE).clamp(0.0, 1.0)
        }
    }

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

    /// 1.0 → 0.85 across the full intro.
    fn logo_scale(&self) -> f32 {
        1.0 - 0.15 * (self.t() / TOTAL).clamp(0.0, 1.0)
    }

    pub fn finished(&self) -> bool {
        self.t() >= TOTAL
    }

    /// Restore normal window chrome (title bar, opaque). Call once when splash ends.
    pub fn handoff_viewport(ctx: &egui::Context) {
        // Windows already starts decorated/opaque (see main.rs) — flipping decorations
        // after create is what produces the black top strip + click Y-offset there.
        #[cfg(not(windows))]
        {
            ctx.send_viewport_cmd(ViewportCommand::Decorations(true));
            ctx.send_viewport_cmd(ViewportCommand::Transparent(false));
        }
        // Opaque panel fills again (splash left them transparent on glass platforms).
        ctx.style_mut(|s| {
            s.visuals.panel_fill = CHROME_BG;
            s.visuals.window_fill = CHROME_BG;
        });
    }

    /// Paint splash; returns `true` while still active (caller should skip normal UI).
    pub fn tick_and_paint(&mut self, ctx: &egui::Context) -> bool {
        let skip = ctx.input(|i| {
            i.key_pressed(egui::Key::Escape)
                || i.key_pressed(egui::Key::Space)
                || i.pointer.any_click()
        }) && self.t() > 0.12;

        if skip || self.finished() {
            if !self.handed_off {
                Self::handoff_viewport(ctx);
                self.handed_off = true;
            }
            return false;
        }

        ctx.request_repaint();

        // Keep egui panels from painting an opaque sheet over the glass.
        ctx.style_mut(|s| {
            s.visuals.panel_fill = Color32::TRANSPARENT;
            s.visuals.window_fill = Color32::TRANSPARENT;
        });

        let win_a = self.window_alpha();
        let logo_a = self.logo_alpha();
        let logo_scale = self.logo_scale();
        let logo = self.logo.clone();

        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ctx, |ui| {
                let full = ui.max_rect();
                let painter = ui.painter_at(full);
                ui.allocate_rect(full, Sense::click());

                // Solid chrome wash rising under the logo (matches app clear color).
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
                }

                if logo_a > 0.01 {
                    if let Some(logo) = &logo {
                        let tex = logo.size_vec2();
                        let target = full.size() * 0.5;
                        let fit = (target.x / tex.x).min(target.y / tex.y);
                        let draw = tex * fit * logo_scale;
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
            });

        true
    }
}
