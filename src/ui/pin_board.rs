//! Shared pin board across all open profiles for comparing brokers/devices.

use crate::mqtt::pins::PinBoard;
use crate::mqtt::session::SessionManager;

pub fn draw(ui: &mut eframe::egui::Ui, pins: &mut PinBoard, _sessions: &SessionManager) {
    ui.heading("Pin board");
    ui.label("Pins from all open profiles — compare devices/brokers side by side.");
    ui.add_space(4.0);

    if pins.items.is_empty() {
        ui.weak("No pins yet. Subscribe or enter a topic, then Pin.");
        return;
    }

    eframe::egui::Grid::new("pin_board_grid")
        .striped(true)
        .num_columns(4)
        .min_col_width(100.0)
        .show(ui, |ui| {
            ui.strong("Profile");
            ui.strong("Topic");
            ui.strong("Last payload");
            ui.strong("");
            ui.end_row();

            let rows: Vec<_> = pins.items.values().cloned().collect();
            for pin in rows {
                ui.label(&pin.profile_name);
                ui.monospace(&pin.topic);
                ui.label(&pin.last_payload);
                if ui.small_button("Unpin").clicked() {
                    pins.unpin(&pin.profile_id, &pin.topic);
                }
                ui.end_row();
            }
        });
}
