//! Clipboard helpers: click topic to copy path; button copies payload.

use eframe::egui::{self, RichText};

/// Clickable topic path — copies the topic string.
pub fn topic_label(ui: &mut egui::Ui, topic: &str, status: &mut String) {
    let response = ui
        .add(egui::Label::new(RichText::new(topic).monospace()).sense(egui::Sense::click()))
        .on_hover_text("Click to copy topic");
    if response.clicked() {
        ui.ctx().copy_text(topic.to_string());
        *status = format!("Copied topic: {topic}");
    }
}

/// Truncated payload label + small copy control.
pub fn payload_with_copy(ui: &mut egui::Ui, payload: &str, max_chars: usize, status: &mut String) {
    let shown = if payload.len() > max_chars {
        format!("{}…", &payload[..max_chars])
    } else {
        payload.to_string()
    };
    ui.label(RichText::new(shown).small());
    if ui.small_button("Copy").on_hover_text("Copy payload").clicked() {
        ui.ctx().copy_text(payload.to_string());
        *status = "Copied payload".into();
    }
}
