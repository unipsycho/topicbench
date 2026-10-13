//! Live traffic panel — own section; toggled from toolbar (off by default).

use crate::mqtt::session::SessionManager;
use crate::ui::clipboard;
use crate::ui::search_bar::SearchFilter;
use eframe::egui;

pub fn draw(
    ui: &mut egui::Ui,
    sessions: &SessionManager,
    target_session_id: &Option<String>,
    filter: &SearchFilter,
    status: &mut String,
) {
    ui.heading("Live traffic");
    ui.label("Recent messages for the selected device (not the focus of the UI).");
    ui.add_space(4.0);

    let Some(id) = target_session_id.as_ref() else {
        ui.weak("Select a connected profile to see traffic.");
        return;
    };
    let Some(session) = sessions.open.get(id) else {
        ui.weak("No open session.");
        return;
    };

    if session.traffic.is_empty() {
        ui.weak("Waiting for messages…");
        return;
    }

    egui::ScrollArea::vertical()
        .id_salt("live_traffic_panel")
        .max_height(160.0)
        .show(ui, |ui| {
            for line in session.traffic.iter().take(50) {
                if !filter.matches(&line.topic, Some(&line.payload)) {
                    continue;
                }
                ui.horizontal(|ui| {
                    clipboard::topic_label(ui, &line.topic, status);
                    clipboard::payload_with_copy(ui, &line.payload, 72, status);
                });
            }
        });
}
