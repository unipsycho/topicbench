//! Clipboard helpers: click topic to copy path.

use crate::ui::hover_tip;
use crate::ui::json_view;
use eframe::egui;

/// Clickable topic path — copies the topic string.
pub fn topic_label(ui: &mut egui::Ui, topic: &str, status: &mut String) {
    let response = hover_tip::attach(
        ui.add(
            egui::Label::new(json_view::topic_path_text(topic)).sense(egui::Sense::click()),
        ),
        "Click to copy topic",
    );
    if response.clicked() {
        ui.ctx().copy_text(topic.to_string());
        *status = format!("Copied topic: {topic}");
    }
}
