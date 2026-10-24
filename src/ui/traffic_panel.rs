//! Live traffic panel — own section; toggled from toolbar (off by default).

use crate::mqtt::session::{SessionManager, TrafficLine};
use crate::ui::clipboard;
use crate::ui::icons::{self, IconSet};
use crate::ui::search_bar::SearchFilter;
use eframe::egui;

/// Pause / restart state for the Live traffic panel.
#[derive(Clone, Debug, Default)]
pub struct TrafficViewState {
    /// When true, show [`frozen`] instead of the live session buffer.
    pub paused: bool,
    /// Snapshot taken when Pause was pressed.
    pub frozen: Vec<TrafficLine>,
}

pub fn draw(
    ui: &mut egui::Ui,
    sessions: &SessionManager,
    target_session_id: &Option<String>,
    filter: &SearchFilter,
    icons: &IconSet,
    view: &mut TrafficViewState,
    height: &mut f32,
    status: &mut String,
) {
    icons::section_header_bar(ui, |ui| {
        icons::section_icon(ui, icons, "traffic");
        icons::section_title(ui, "Live traffic");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // Restart — clear buffer view and resume live.
            if icons
                .tiny_header(ui, "refresh", false, "Restart — clear and resume live")
                .clicked()
            {
                view.paused = false;
                view.frozen.clear();
                *status = "Live traffic restarted".into();
            }
            if view.paused {
                if icons
                    .tiny_header(ui, "playback", false, "Continue — resume live append")
                    .clicked()
                {
                    view.paused = false;
                    view.frozen.clear();
                    *status = "Live traffic continued".into();
                }
            } else if icons
                .tiny_header(ui, "panel_docked", false, "Pause — freeze stream for scroll/copy")
                .clicked()
            {
                // Snapshot current lines for the selected session.
                view.frozen.clear();
                if let Some(id) = target_session_id.as_ref() {
                    if let Some(session) = sessions.open.get(id) {
                        view.frozen.extend(session.traffic.iter().cloned().take(200));
                    }
                }
                view.paused = true;
                *status = "Live traffic paused".into();
            }
        });
    });
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

    let lines: Vec<&TrafficLine> = if view.paused {
        view.frozen.iter().collect()
    } else {
        session.traffic.iter().take(50).collect()
    };

    if lines.is_empty() {
        ui.weak(if view.paused {
            "Paused — no messages in snapshot."
        } else {
            "Waiting for messages…"
        });
        return;
    }

    // Drag to resize height (persisted by caller).
    let h = (*height).clamp(80.0, 480.0);
    egui::ScrollArea::vertical()
        .id_salt("live_traffic_panel")
        .max_height(h)
        .show(ui, |ui| {
            for line in lines {
                if !filter.matches(&line.topic, Some(&line.payload)) {
                    continue;
                }
                ui.horizontal(|ui| {
                    clipboard::topic_label(ui, &line.topic, status);
                    clipboard::payload_with_copy(ui, &line.payload, 72, status);
                });
            }
        });

    let resp = ui.allocate_response(
        egui::vec2(ui.available_width(), 6.0),
        egui::Sense::click_and_drag(),
    );
    if resp.hovered() || resp.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
    }
    if resp.dragged() {
        *height = (*height + resp.drag_delta().y).clamp(80.0, 480.0);
    }
}
