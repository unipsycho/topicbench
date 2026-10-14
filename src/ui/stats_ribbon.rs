//! Compact Stats ribbon group: recv / sent / throughput + /h /d /mo projections.

use crate::mqtt::traffic_stats::{
    self, TrafficStats, SECS_PER_DAY, SECS_PER_HOUR, SECS_PER_MONTH,
};
use crate::ui::icons::{self, CHROME_CAPTION, CHROME_TEXT, TINT_STATS};
use eframe::egui::{self, Align, FontId, RichText, Sense, Vec2};

/// Width of the stats text block inside the ribbon group (fits CMD_H).
const STATS_W: f32 = 176.0;
const STATS_H: f32 = 64.0;

/// Draw the far-right Stats ribbon category (process-lifetime counters).
pub fn draw_group(ui: &mut egui::Ui, stats: &TrafficStats) {
    icons::ribbon_group(ui, "Stats", TINT_STATS, |ui| {
        let cell = Vec2::new(STATS_W, STATS_H);
        ui.allocate_ui_with_layout(cell, egui::Layout::top_down(Align::LEFT), |ui| {
            ui.set_min_size(cell);
            ui.set_max_size(cell);
            ui.spacing_mut().item_spacing.y = 1.0;

            let recv = traffic_stats::format_bytes(stats.bytes_recv);
            let sent = traffic_stats::format_bytes(stats.bytes_sent);
            let thru = traffic_stats::format_rate(stats.throughput_bps());
            let per_h = traffic_stats::format_bytes(stats.project_bytes(SECS_PER_HOUR));
            let per_d = traffic_stats::format_bytes(stats.project_bytes(SECS_PER_DAY));
            let per_mo = traffic_stats::format_bytes(stats.project_bytes(SECS_PER_MONTH));

            // Row 1: totals packed left (no stretch between Recv/Sent).
            ui.label(
                RichText::new(format!("Recv {recv}   Sent {sent}"))
                    .font(FontId::proportional(11.0))
                    .color(CHROME_TEXT),
            );
            // Row 2: combined in+out throughput over the ~2s window.
            ui.label(
                RichText::new(format!("Thru {thru}"))
                    .font(FontId::proportional(11.0))
                    .color(CHROME_TEXT),
            );
            // Row 3: projections from current rate (same window as Thru).
            ui.label(
                RichText::new(format!("{per_h}/h  {per_d}/d  {per_mo}/mo"))
                    .font(FontId::proportional(10.0))
                    .color(CHROME_CAPTION),
            );

            // Fill remaining height so the group matches other ribbon cells.
            let _ = ui.allocate_exact_size(
                Vec2::new(STATS_W, ui.available_height().max(0.0)),
                Sense::hover(),
            );
        });
    });
}
