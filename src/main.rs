//! MQTT Monitor — egui/eframe shell (ArtCraft Crafting Apps stack).

mod app;
mod mqtt;
mod ui;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([1100.0, 700.0])
            .with_title("MQTT Monitor"),
        ..Default::default()
    };

    eframe::run_native(
        "MQTT Monitor",
        options,
        Box::new(|_cc| Ok(Box::new(app::MqttMonitorApp::default()))),
    )
}
