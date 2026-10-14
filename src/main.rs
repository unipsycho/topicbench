//! MQTT Master — egui/eframe shell (ArtCraft Crafting Apps stack).

// Hide the console window on Windows GUI builds (no-op on Linux).
#![cfg_attr(windows, windows_subsystem = "windows")]

mod app;
mod mqtt;
mod ui;

use eframe::egui::{self, IconData, Visuals};
use ui::icons::{CHROME_BG, STATUS_BG};

fn main() -> eframe::Result<()> {
    let icon = load_window_icon();
    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size([1200.0, 760.0])
        .with_title("MQTT Master");
    if let Some(icon) = icon {
        viewport = viewport.with_icon(icon);
    }

    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    eframe::run_native(
        "MQTT Master",
        options,
        Box::new(|cc| {
            apply_dark_chrome(&cc.egui_ctx);
            Ok(Box::new(app::MqttMonitorApp::new(cc)))
        }),
    )
}

/// Window/taskbar icon from the broker_server mark (same art as Windows .ico).
fn load_window_icon() -> Option<IconData> {
    let bytes = include_bytes!("../assets/icons/broker_server.png");
    let img = image::load_from_memory(bytes).ok()?.into_rgba8();
    let (width, height) = img.dimensions();
    Some(IconData {
        rgba: img.into_raw(),
        width,
        height,
    })
}

/// Lock dark mode (ignore OS light theme) and paint every panel with ribbon chrome.
fn apply_dark_chrome(ctx: &egui::Context) {
    // Default ThemePreference::System follows Windows light → light panel fills.
    ctx.set_theme(egui::ThemePreference::Dark);

    let mut visuals = Visuals::dark();
    // Match ribbon CHROME_BG — not egui's deeper neutral gray(27).
    visuals.panel_fill = CHROME_BG;
    visuals.window_fill = CHROME_BG;
    // TextEdit / extreme wells stay cool charcoal, same family as chrome.
    visuals.extreme_bg_color = STATUS_BG;
    visuals.code_bg_color = STATUS_BG;
    ctx.set_visuals(visuals);
}
