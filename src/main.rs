//! TopicBench — egui/eframe shell (ArtCraft Crafting Apps stack).

// Hide the console window on Windows GUI builds (no-op on Linux).
#![cfg_attr(windows, windows_subsystem = "windows")]

mod app;
mod mqtt;
mod pro;
mod ui;
mod update;

use eframe::egui::{self, Color32, IconData, Visuals};
use ui::icons::{self, CHROME_BG, STATUS_BG};

fn main() -> eframe::Result<()> {
    let icon = load_window_icon();
    // Linux: undecorated + transparent for the floating AA splash, then hand off to OS chrome.
    // Windows: start decorated/opaque — winit undecorated + later Decorations(true) leaves a
    // caption-height black strip and Y-offsets hit tests (WS_CAPTION client inset mismatch).
    // One-shot migrate before reading settings for initial window size.
    mqtt::paths::migrate_legacy_config_dir();
    let settings = mqtt::settings::AppSettings::load();
    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size([1200.0, 760.0])
        .with_title("TopicBench")
        // Wayland app_id — keep stable so Hyprland rules keep matching.
        .with_app_id("com.agileautomation.mqtt-master");
    // Prefer last session size from settings.json (eframe also restores when available).
    if let Some(g) = settings.window_main {
        viewport = g.apply(viewport, [400.0, 300.0]);
    }
    #[cfg(windows)]
    {
        viewport = viewport.with_decorations(true).with_transparent(false);
    }
    #[cfg(not(windows))]
    {
        viewport = viewport.with_transparent(true).with_decorations(false);
    }
    if let Some(icon) = icon {
        viewport = viewport.with_icon(icon);
    }

    // Durable eframe state beside settings.json (window + egui memory / splitters).
    // Note: this path is the storage *file* (eframe creates/overwrites it).
    let persistence_path = mqtt::paths::config_dir().join("eframe");
    let options = eframe::NativeOptions {
        viewport,
        // Vsync Wait + off-workspace frame callbacks = swap_buffers can stall the
        // event loop so Hyprland xdg_wm_base pings miss → ANR on desktop switch.
        vsync: false,
        persist_window: true,
        persistence_path: Some(persistence_path),
        ..Default::default()
    };

    eframe::run_native(
        "TopicBench",
        options,
        Box::new(|cc| {
            apply_dark_chrome(&cc.egui_ctx);
            Ok(Box::new(app::MqttMonitorApp::new(cc)))
        }),
    )
}

/// Window/taskbar icon — TB monogram (same art as Windows PE app.ico).
fn load_window_icon() -> Option<IconData> {
    let bytes = include_bytes!("../assets/icons/topicbench-app-icon-256.png");
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
    // `.weak()` fades toward this — keep it mid-charcoal so muted labels stay readable
    // (default gray(27) made counts / chrome hints too dark on the panel).
    visuals.widgets.noninteractive.weak_bg_fill = Color32::from_rgb(88, 92, 104);
    // ComboBox / selectors: cool charcoal wells (not egui gray(60)).
    icons::apply_selector_widget_visuals(&mut visuals);
    // Checkbox / slider: sunk well + brighter blue fill (readable on CHROME_ROW).
    icons::apply_control_accent_visuals(&mut visuals);
    ctx.set_visuals(visuals);
}
