//! TopicBench — egui/eframe shell (ArtCraft Crafting Apps stack).

// Hide the console window on Windows GUI builds (no-op on Linux).
#![cfg_attr(windows, windows_subsystem = "windows")]

mod app;
mod mqtt;
mod pro;
mod ui;
mod update;

use eframe::egui;
use ui::theme;

fn main() -> eframe::Result<()> {
    // Linux: undecorated + transparent for the floating AA splash, then hand off to OS chrome.
    // Windows: start decorated/opaque — winit undecorated + later Decorations(true) leaves a
    // caption-height black strip and Y-offsets hit tests (WS_CAPTION client inset mismatch).
    // One-shot migrate before reading settings for initial window size.
    mqtt::paths::migrate_legacy_config_dir();
    let settings = mqtt::settings::AppSettings::load();
    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size([1200.0, 760.0])
        .with_title("TopicBench")
        // Wayland app_id / X11 WM_CLASS — matches StartupWMClass in packaging/*.desktop.
        // Child pop-outs get the same id via WinGeom::apply (do not invent per-panel ids).
        .with_app_id(mqtt::window_geom::APP_ID);
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
    // Taskbar/window icon from Mike’s ICO→PNG (also stamped on pop-outs in WinGeom::apply).
    if let Some(icon) = mqtt::window_geom::app_icon() {
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
            // Classic until ThemeClient / settings resolve a Pro palette.
            theme::apply_theme(&cc.egui_ctx, &theme::classic());
            Ok(Box::new(app::MqttMonitorApp::new(cc)))
        }),
    )
}
