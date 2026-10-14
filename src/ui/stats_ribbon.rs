//! Compact Stats ribbon group: recv / sent / throughput + stacked /h /d /mo.
//!
//! Labels come from a 1 Hz frozen snapshot — do not reformat every frame.
//! Fixed cell width + no-wrap labels avoid reflow jitter as values change length.

use crate::mqtt::traffic_stats::TrafficStats;
use crate::ui::icons::{self, CHROME_CAPTION, CHROME_MUTED, CHROME_TEXT, TINT_STATS};
use eframe::egui::{self, Align, FontId, Sense, Vec2};

/// Fixed text block width — fits `Recv …  Sent …` on one row without wrap jitter.
const STATS_W: f32 = 148.0;
/// Compact height: Recv|Sent + Thru + mid-gap + 3 projection rows.
const STATS_H: f32 = 66.0;
/// Breath between Thru and projections.
const MID_GAP: f32 = 6.0;

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
                .layout(egui::Layout::top_down(Align::Center)),
        );
        child.set_min_size(cell);
        child.set_max_size(cell);
        child.spacing_mut().item_spacing.y = 0.0;

        let wrap = egui::text::TextWrapping {
            max_rows: 1,
            break_anywhere: false,
            overflow_character: Some('…'),
            ..Default::default()
        };

        // Row 1: Recv + Sent side by side (centered).
        centered_line(
            &mut child,
            STATS_W,
            &format!("Recv {}  Sent {}", d.recv, d.sent),
            10.0,
            CHROME_TEXT,
            wrap.clone(),
        );
        // Row 2: Thru under totals (centered).
        centered_line(
            &mut child,
            STATS_W,
            &format!("Thru {}", d.thru),
            10.0,
            CHROME_TEXT,
            wrap.clone(),
        );
        child.add_space(MID_GAP);
        // /h /d /mo stacked, centered in the cell.
        centered_proj(&mut child, STATS_W, &d.per_h, "/h", wrap.clone());
        centered_proj(&mut child, STATS_W, &d.per_d, "/d", wrap.clone());
        centered_proj(&mut child, STATS_W, &d.per_mo, "/mo", wrap);
    });
}

/// One non-wrapping label row, horizontally centered in `width`.
fn centered_line(
    ui: &mut egui::Ui,
    width: f32,
    text: &str,
    size: f32,
    color: egui::Color32,
    wrap: egui::text::TextWrapping,
) {
    let row_h = size + 1.0;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, row_h), Sense::hover());
    let mut job = egui::text::LayoutJob::default();
    job.wrap = wrap;
    job.halign = Align::Center;
    job.append(
        text,
        0.0,
        egui::TextFormat {
            font_id: FontId::proportional(size),
            color,
            ..Default::default()
        },
    );
    let galley = ui.fonts(|f| f.layout_job(job));
    let pos = egui::pos2(
        rect.center().x - galley.size().x * 0.5,
        rect.center().y - galley.size().y * 0.5,
    );
    ui.painter().galley(pos, galley, color);
}

/// Projection row `value/suffix`, centered in the Stats cell.
fn centered_proj(
    ui: &mut egui::Ui,
    width: f32,
    value: &str,
    suffix: &str,
    wrap: egui::text::TextWrapping,
) {
    let size = 9.0;
    let row_h = size + 1.0;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, row_h), Sense::hover());

    let mut job = egui::text::LayoutJob::default();
    job.wrap = wrap;
    job.halign = Align::Center;
    job.append(
        value,
        0.0,
        egui::TextFormat {
            font_id: FontId::proportional(size),
            color: CHROME_CAPTION,
            ..Default::default()
        },
    );
    job.append(
        suffix,
        0.0,
        egui::TextFormat {
            font_id: FontId::proportional(size),
            color: CHROME_MUTED,
            ..Default::default()
        },
    );
    let galley = ui.fonts(|f| f.layout_job(job));
    let pos = egui::pos2(
        rect.center().x - galley.size().x * 0.5,
        rect.center().y - galley.size().y * 0.5,
    );
    ui.painter().galley(pos, galley, CHROME_CAPTION);
}
