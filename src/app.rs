//! Top-level eframe app: wires shell panels to MQTT placeholders.

use crate::mqtt::{pins::PinBoard, profile::ProfileStore, publish::PublishDraft, session::SessionManager};
use crate::ui::{pin_board, profiles, publish_panel, subscribe_panel};

/// Root UI state for the MQTT Explorer–style monitor.
pub struct MqttMonitorApp {
    pub profiles: ProfileStore,
    pub sessions: SessionManager,
    pub pins: PinBoard,
    pub publish: PublishDraft,
    pub subscribe_topic: String,
    pub status: String,
}

impl Default for MqttMonitorApp {
    fn default() -> Self {
        Self {
            profiles: ProfileStore::with_sample(),
            sessions: SessionManager::default(),
            pins: PinBoard::default(),
            publish: PublishDraft::default(),
            subscribe_topic: "sensors/+/temp".into(),
            status: "Shell ready — connect / subscribe / pin / publish are placeholders.".into(),
        }
    }
}

impl eframe::App for MqttMonitorApp {
    fn update(&mut self, ctx: &eframe::egui::Context, _frame: &mut eframe::Frame) {
        eframe::egui::TopBottomPanel::top("status").show(ctx, |ui| {
            ui.label(&self.status);
        });

        eframe::egui::SidePanel::left("profiles")
            .default_width(260.0)
            .show(ctx, |ui| {
                profiles::draw(ui, &mut self.profiles, &mut self.sessions, &mut self.status);
            });

        eframe::egui::SidePanel::right("publish")
            .default_width(280.0)
            .show(ctx, |ui| {
                publish_panel::draw(ui, &mut self.publish, &self.sessions, &mut self.status);
            });

        eframe::egui::CentralPanel::default().show(ctx, |ui| {
            subscribe_panel::draw(
                ui,
                &mut self.subscribe_topic,
                &mut self.sessions,
                &mut self.pins,
                &mut self.status,
            );
            ui.add_space(8.0);
            ui.separator();
            ui.add_space(8.0);
            pin_board::draw(ui, &mut self.pins, &self.sessions);
        });
    }
}
