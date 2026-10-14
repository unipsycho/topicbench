//! Compact Stats ribbon group: recv / sent / throughput + /h /d /mo projections.
//!
//! Labels come from a 1 Hz frozen snapshot — do not reformat every frame.
//! Fixed cell width + no-wrap labels avoid reflow jitter as values change length.

use crate::mqtt::traffic_stats::TrafficStats;
use crate::ui::icons::{self, CHROME_CAPTION, CHROME_TEXT, TINT_STATS};
use eframe::egui::{self, Align, FontId, Sense, Vec2};

/// Fixed text block width — sized for longest expected formats (e.g. `9999.99 MB/s`,
/// `9999.99 MB/h  9999.99 MB/d  999.99 GB/mo`) so shorter values never shrink/wrap.
const STATS_W: f32 = 220.0;
const STATS_H: f32 = 64.0;

/// Draw the far-right Stats ribbon category (process-lifetime counters).
pub fn draw_group(ui: &mut egui::Ui, stats: &TrafficStats) {
    let d = stats.display();
    icons::ribbon_group(ui, "Stats", TINT_STATS, |ui| {
        let cell = Vec2::new(STATS_W, STATS_H);
        // Exact-size cell: group width stays constant regardless of string length.
        let (rect, _) = ui.allocate_exact_size(cell, Sense::hover());
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(rect)
                .layout(egui::Layout::top_down(Align::LEFT)),
        );
        child.set_min_size(cell);
        child.set_max_size(cell);
        child.spacing_mut().item_spacing.y = 1.0;

        // No wrap — truncate with ellipsis only if somehow still too wide.
        let wrap = egui::text::TextWrapping {
            max_rows: 1,
            break_anywhere: false,
            overflow_character: Some('…'),
            ..Default::default()
        };

        // Row 1: totals
        stable_line(
            &mut child,
            STATS_W,
            &format!("Recv {}   Sent {}", d.recv, d.sent),
            11.0,
            CHROME_TEXT,
            wrap.clone(),
        );
        // Row 2: combined in+out throughput (~2s window, 1 Hz label)
        stable_line(
            &mut child,
            STATS_W,
            &format!("Thru {}", d.thru),
            11.0,
            CHROME_TEXT,
            wrap.clone(),
        );
        // Row 3: projections from current rate
        stable_line(
            &mut child,
            STATS_W,
            &format!("{}/h  {}/d  {}/mo", d.per_h, d.per_d, d.per_mo),
            10.0,
            CHROME_CAPTION,
            wrap,
        );
    });
}

/// One non-wrapping label row at a fixed width.
fn stable_line(
    ui: &mut egui::Ui,
    width: f32,
    text: &str,
    size: f32,
    color: egui::Color32,
    wrap: egui::text::TextWrapping,
) {
    let mut job = egui::text::LayoutJob::default();
    job.wrap = wrap;
    job.append(
        text,
        0.0,
        egui::TextFormat {
            font_id: FontId::proportional(size),
            color,
            ..Default::default()
        },
    );
    // Reserve full row width so shorter strings don't shrink the group.
    ui.add_sized(
        Vec2::new(width, size + 2.0),
        egui::Label::new(job).truncate(),
    );
}
