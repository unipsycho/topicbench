//! Shared look for Pro tool panels: one styled header (glyph, title, pop-out, close) that
//! hosts a panel body either in an in-app floating window or in its own OS window.
//!
//! Panels open in-app. The header's pop-out button moves a panel to a native window and the
//! same button there docks it back. The choice is remembered per panel in `panel-modes.json`.
//!
//! Bodies are drawn once, by the caller's closure, so the content is identical in both homes.
//! While a body is being drawn [`hosted`] is true: the body should then skip its own title
//! and close button, because the header here already provides them.

use crate::mqtt::window_geom::{self, WinGeom};
use crate::ui::icons::{IconSet, CHROME_BG};
use eframe::egui::{self, Color32, RichText, ViewportBuilder, ViewportClass};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

// Colours follow the active theme, so the header changes with Tokyo Night, Latte, etc.
/// Title strip fill.
pub fn header_bg() -> Color32 {
    crate::ui::theme::chrome().accent_soft
}
/// Title text on the strip.
pub fn header_text() -> Color32 {
    crate::ui::theme::chrome().text
}
/// Subtitle text on the strip.
pub fn header_sub() -> Color32 {
    crate::ui::theme::chrome().caption
}
/// Primary button fill.
pub fn accent() -> Color32 {
    crate::ui::theme::chrome().header_selected
}
/// Accent-coloured text (section headings, "Running" chip).
pub fn accent_text() -> Color32 {
    crate::ui::theme::chrome().heading
}
/// Outline for the window and cards.
pub fn card_edge() -> Color32 {
    crate::ui::theme::chrome().accent_border
}
/// Hover wash on header icon buttons.
fn header_hover() -> Color32 {
    crate::ui::theme::chrome().header_hover
}

const HOSTED_KEY: &str = "tb_panel_window_hosted";

/// True while a panel body is drawn inside [`show`]; the body should not draw its own title/close.
pub fn hosted(ctx: &egui::Context) -> bool {
    ctx.data(|d| d.get_temp::<bool>(egui::Id::new(HOSTED_KEY)).unwrap_or(false))
}

/// What the user did in the header this frame.
#[derive(Default, Clone, Copy)]
pub struct HeaderAction {
    pub close: bool,
    /// Pop-out / dock button clicked.
    pub toggle_pop: bool,
}

/// Everything [`show`] needs to host one panel.
pub struct PanelSpec<'a> {
    /// Stable key: window id, viewport id and the saved-mode key.
    pub key: &'a str,
    pub title: &'a str,
    pub subtitle: &'a str,
    /// Icon name in [`IconSet`] for the header glyph.
    pub glyph: &'a str,
    pub min: [f32; 2],
    pub default_size: [f32; 2],
}

// --- remembered mode (in-app vs pop-out) ---------------------------------------------------

fn modes() -> &'static Mutex<HashMap<String, bool>> {
    static MODES: OnceLock<Mutex<HashMap<String, bool>>> = OnceLock::new();
    MODES.get_or_init(|| {
        let loaded = std::fs::read_to_string(crate::mqtt::paths::config_file("panel-modes.json"))
            .ok()
            .and_then(|s| serde_json::from_str::<HashMap<String, bool>>(&s).ok())
            .unwrap_or_default();
        Mutex::new(loaded)
    })
}

/// Whether this panel is currently popped out into its own window.
pub fn is_popped(key: &str) -> bool {
    modes().lock().map(|m| m.get(key).copied().unwrap_or(false)).unwrap_or(false)
}

fn set_popped(key: &str, popped: bool) {
    if let Ok(mut m) = modes().lock() {
        m.insert(key.to_string(), popped);
        if let Ok(json) = serde_json::to_string_pretty(&*m) {
            let path = crate::mqtt::paths::config_file("panel-modes.json");
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = std::fs::write(path, json);
        }
    }
}

// --- hosting -------------------------------------------------------------------------------

/// Draw a panel in whichever home it currently has. Returns after the frame is drawn;
/// `close` is set when the user closed it (header button or OS window close).
#[allow(clippy::too_many_arguments)]
pub fn show(
    ctx: &egui::Context,
    spec: &PanelSpec<'_>,
    icons: &IconSet,
    seed: WinGeom,
    saved_geom: &mut Option<WinGeom>,
    geom_dirty: &mut bool,
    close: &mut bool,
    mut body: impl FnMut(&mut egui::Ui),
) {
    let tex = icons.get(spec.glyph).cloned();
    let popped = is_popped(spec.key);

    if !popped {
        let mut act = HeaderAction::default();
        egui::Window::new(spec.title)
            .id(egui::Id::new(("tb_panel_window_v2", spec.key)))
            .order(egui::Order::Foreground)
            .title_bar(false)
            .collapsible(false)
            .resizable(true)
            .default_size(spec.default_size)
            .min_size(spec.min)
            .max_size(ctx.screen_rect().size() * 0.95)
            .frame(window_frame(ctx))
            .show(ctx, |ui| {
                act = header_bar(ui, tex.as_ref(), icons, spec.title, spec.subtitle, None, Some(false));
                host_body(ui, &mut body);
            });
        if act.toggle_pop {
            set_popped(spec.key, true);
        }
        if act.close {
            *close = true;
        }
        return;
    }

    let min = spec.min;
    let mut act = HeaderAction::default();
    ctx.show_viewport_immediate(
        egui::ViewportId::from_hash_of(("tb_panel_vp", spec.key)),
        seed.apply(ViewportBuilder::default().with_title(spec.title), min)
            .with_min_inner_size(min),
        |ctx, class| {
            let mut draw = |ui: &mut egui::Ui| {
                act = header_bar(ui, tex.as_ref(), icons, spec.title, spec.subtitle, None, Some(true));
                host_body(ui, &mut body);
            };
            match class {
                ViewportClass::Embedded => {
                    // Platform cannot make native windows: fall back to the in-app frame.
                    egui::Window::new(spec.title)
                        .id(egui::Id::new(("tb_panel_window_v2", spec.key)))
                        .order(egui::Order::Foreground)
                        .title_bar(false)
                        .collapsible(false)
                        .resizable(true)
                        .default_size(spec.default_size)
                        .frame(window_frame(ctx))
                        .show(ctx, |ui| draw(ui));
                }
                _ => {
                    egui::CentralPanel::default()
                        .frame(egui::Frame::NONE.fill(CHROME_BG()))
                        .show(ctx, |ui| draw(ui));
                }
            }
            if ctx.input(|i| i.viewport().close_requested()) {
                act.close = true;
            }
            if let Some(g) = window_geom::capture(ctx) {
                if window_geom::sync_slot(saved_geom, g, min) {
                    *geom_dirty = true;
                }
            }
        },
    );
    if act.toggle_pop {
        set_popped(spec.key, false);
    }
    if act.close {
        *close = true;
    }
}

fn host_body(ui: &mut egui::Ui, body: &mut impl FnMut(&mut egui::Ui)) {
    let ctx = ui.ctx().clone();
    ctx.data_mut(|d| d.insert_temp(egui::Id::new(HOSTED_KEY), true));
    // Decouple the window size from the body. A body that fills "available" space, or that
    // overshoots by a few pixels (spacing, grips), would otherwise be followed by the window
    // every frame and creep wider and taller for ever. Reserve exactly the space we were
    // given, draw the body into a clipped child of that size, and let overflow be cut off.
    let (mx, my) = (12.0, 10.0);
    let avail = ui.available_size();
    let size = egui::vec2(
        if avail.x.is_finite() { avail.x } else { 600.0 },
        if avail.y.is_finite() { avail.y } else { 400.0 },
    );
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    let inner = rect.shrink2(egui::vec2(mx, my));
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    child.set_clip_rect(inner.intersect(ui.clip_rect()));
    body(&mut child);
    ctx.data_mut(|d| d.insert_temp(egui::Id::new(HOSTED_KEY), false));
}

pub fn window_frame(ctx: &egui::Context) -> egui::Frame {
    egui::Frame::window(&ctx.style())
        .fill(CHROME_BG())
        .inner_margin(egui::Margin::ZERO)
        .corner_radius(10.0)
        .stroke(egui::Stroke::new(1.0, card_edge().gamma_multiply(0.7)))
}

// --- header --------------------------------------------------------------------------------

/// Coloured title strip: glyph, title, subtitle, optional status chip, optional pop-out/dock
/// button (`popped` is `Some(current state)`), and a close button.
pub fn header_bar(
    ui: &mut egui::Ui,
    glyph: Option<&egui::TextureHandle>,
    icons: &IconSet,
    title: &str,
    subtitle: &str,
    pill: Option<(&'static str, Color32)>,
    popped: Option<bool>,
) -> HeaderAction {
    let mut act = HeaderAction::default();
    egui::Frame::NONE
        .fill(header_bg())
        .corner_radius(egui::CornerRadius { nw: 10, ne: 10, sw: 0, se: 0 })
        .inner_margin(egui::Margin::symmetric(14, 7))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                if let Some(t) = glyph {
                    ui.add(egui::Image::new(t).fit_to_exact_size(egui::vec2(26.0, 26.0)));
                }
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 1.0;
                    ui.label(RichText::new(title).strong().size(15.5).color(header_text()));
                    if !subtitle.is_empty() {
                        ui.label(RichText::new(subtitle).size(11.5).color(header_sub()));
                    }
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if close_button(ui).clicked() {
                        act.close = true;
                    }
                    if let Some(is_popped) = popped {
                        let (name, tip) = if is_popped {
                            ("panel_docked", "Dock back into the main window")
                        } else {
                            ("panel_undock", "Pop out into its own window")
                        };
                        if pop_button(ui, icons, name, tip).clicked() {
                            act.toggle_pop = true;
                        }
                    }
                    if let Some((text, color)) = pill {
                        egui::Frame::NONE
                            .fill(color.gamma_multiply(0.22))
                            .stroke(egui::Stroke::new(1.0, color))
                            .corner_radius(10.0)
                            .inner_margin(egui::Margin::symmetric(9, 2))
                            .show(ui, |ui| {
                                ui.label(RichText::new(text).size(12.0).strong().color(color));
                            });
                    }
                });
            });
        });
    act
}

/// Painted cross with its own hit area, so it does not depend on a font glyph.
fn close_button(ui: &mut egui::Ui) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(24.0, 24.0), egui::Sense::click());
    let resp = resp
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text("Close");
    if resp.hovered() {
        ui.painter()
            .rect_filled(rect, 6.0, header_hover());
    }
    let c = rect.center();
    let st = egui::Stroke::new(2.0, header_text());
    ui.painter()
        .line_segment([c + egui::vec2(-5.0, -5.0), c + egui::vec2(5.0, 5.0)], st);
    ui.painter()
        .line_segment([c + egui::vec2(-5.0, 5.0), c + egui::vec2(5.0, -5.0)], st);
    resp
}

fn pop_button(ui: &mut egui::Ui, icons: &IconSet, name: &str, tip: &str) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(24.0, 24.0), egui::Sense::click());
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(tip);
    if resp.hovered() {
        ui.painter()
            .rect_filled(rect, 6.0, header_hover());
    }
    match icons.get(name) {
        Some(tex) => {
            let r = egui::Rect::from_center_size(rect.center(), egui::vec2(18.0, 18.0));
            egui::Image::new(tex).paint_at(ui, r);
        }
        None => {
            // Fallback: two offset squares.
            let c = rect.center();
            let st = egui::Stroke::new(1.5, header_text());
            ui.painter().rect_stroke(
                egui::Rect::from_center_size(c + egui::vec2(-2.0, 2.0), egui::vec2(10.0, 10.0)),
                1.0,
                st,
                egui::StrokeKind::Inside,
            );
            ui.painter().rect_stroke(
                egui::Rect::from_center_size(c + egui::vec2(2.0, -2.0), egui::vec2(10.0, 10.0)),
                1.0,
                st,
                egui::StrokeKind::Inside,
            );
        }
    }
    resp
}
