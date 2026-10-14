//! MQTT Master — egui/eframe shell (ArtCraft Crafting Apps stack).

mod app;
mod mqtt;
mod ui;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([1200.0, 760.0])
            .with_title("MQTT Master"),
        ..Default::default()
    };

    eframe::run_native(
        "MQTT Master",
        options,
        Box::new(|cc| {
            // Dark chrome for ribbon + body (Mike: no white ribbon).
            cc.egui_ctx.set_visuals(eframe::egui::Visuals::dark());
            Ok(Box::new(app::MqttMonitorApp::new(cc)))
        }),
    )
}
