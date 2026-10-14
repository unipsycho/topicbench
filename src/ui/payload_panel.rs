//! Payload side panel — live Value view for the selected topic tree row.

use crate::mqtt::pins::PinBoard;
use crate::mqtt::profile::ProfileStore;
use crate::mqtt::session::SessionManager;
use crate::ui::icons::IconSet;
use crate::ui::pin_board;
use eframe::egui::{self, Color32, RichText};

pub fn draw(
    ui: &mut egui::Ui,
    sessions: &SessionManager,
    profiles: &ProfileStore,
    pins: &PinBoard,
    icons: &IconSet,
) {
    ui.horizontal(|ui| {
        if let Some(tex) = icons.get("panel_undock") {
            ui.add(
                egui::Image::new(tex)
                    .fit_to_exact_size(egui::vec2(20.0, 20.0))
                    .sense(egui::Sense::hover()),
            );
        }
        ui.heading("Payload");
    });
    ui.label("Live Value view for the selected topic.");
    ui.add_space(4.0);

    let Some((profile_id, profile_name, topic)) = pins.last_tree_focus.as_ref() else {
        ui.weak("Select a topic in the tree.");
        return;
    };

    let accent = profiles
        .profiles
        .get(profile_id)
        .map(|p| p.color())
        .unwrap_or(Color32::from_rgb(90, 190, 120));

    // Header: broker (accent) + full topic path.
    ui.horizontal(|ui| {
        ui.label(RichText::new(profile_name).strong().color(accent));
        ui.label(RichText::new(topic).monospace().small());
    });
    ui.add_space(6.0);

    let node = sessions
        .open
        .get(profile_id)
        .and_then(|s| s.topic_tree.root.node_at(topic));

    match node {
        Some(n) if n.last_payload.as_deref().is_some_and(|p| !p.is_empty()) => {
            if n.message_count > 0 {
                ui.label(
                    RichText::new(format!("#{}", n.message_count))
                        .small()
                        .weak(),
                );
            }
            let payload = n.last_payload.as_deref().unwrap_or("");
            // Full-height Value view (same helpers as sticky Value mode).
            pin_board::draw_value_payload(
                ui,
                payload,
                ("payload-panel", profile_id.as_str(), topic.as_str()),
                None,
            );
        }
        _ => {
            ui.weak("No payload yet for this topic.");
        }
    }
}
