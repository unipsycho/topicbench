//! Compact Stats ribbon group: centered vertical stack that hugs content width.
//!
//! Labels come from a 1 Hz frozen snapshot. Every data row is painted the same
//! way (center of the content column) so Recv / Sent / Thru / projections align.

use crate::mqtt::traffic_stats::TrafficStats;
use crate::ui::icons::{self, CHROME_CAPTION, CHROME_MUTED, CHROME_TEXT, TINT_STATS};
use eframe::egui::{self, Align, FontId, Sense, Vec2};

/// Horizontal padding inside the content column (both sides).
const H_PAD: f32 = 6.0;
/// Breath between Thru and projections.
const MID_GAP: f32 = 6.0;
const ROW_MAIN: f32 = 10.0;
const ROW_PROJ: f32 = 9.0;
const ROW_H_MAIN: f32 = 11.0;
const ROW_H_PROJ: f32 = 10.0;

/// Draw the far-right Stats ribbon category (process-lifetime counters).
pub fn draw_group(ui: &mut egui::Ui, stats: &TrafficStats) {
    let d = stats.display();

    // One metric per line — consistent centering (Recv|Sent side-by-side fought alignment).
    let lines_main = [
        format!("Recv {}", d.recv),
        format!("Sent {}", d.sent),
        format!("Thru {}", d.thru),
    ];
    let lines_proj = [
        format!("{}/h", d.per_h),
        format!("{}/d", d.per_d),
        format!("{}/mo", d.per_mo),
    ];

    let content_w = {
        let mut w = 0.0_f32;
        for s in &lines_main {
            w = w.max(measure_line(ui, s, ROW_MAIN));
        }
        for s in &lines_proj {
            w = w.max(measure_line(ui, s, ROW_PROJ));
        }
        w.ceil() + H_PAD * 2.0
    };
    let content_h =
        ROW_H_MAIN * 3.0 + MID_GAP + ROW_H_PROJ * 3.0;

    // Own compact group — content is exact-sized so the tint hugs the text.
    icons::ribbon_group(ui, "Stats", TINT_STATS, |ui| {
        let (rect, _) = ui.allocate_exact_size(Vec2::new(content_w, content_h), Sense::hover());
        let mut y = rect.top();
        let cx = rect.center().x;

        for s in &lines_main {
            paint_centered(ui, cx, y, ROW_H_MAIN, s, ROW_MAIN, CHROME_TEXT);
            y += ROW_H_MAIN;
        }
        y += MID_GAP;
        // Projections: value in caption, suffix slightly muted — still one centered run.
        paint_centered_proj(ui, cx, y, ROW_H_PROJ, &d.per_h, "/h");
        y += ROW_H_PROJ;
        paint_centered_proj(ui, cx, y, ROW_H_PROJ, &d.per_d, "/d");
        y += ROW_H_PROJ;
        paint_centered_proj(ui, cx, y, ROW_H_PROJ, &d.per_mo, "/mo");
        let _ = y;
    });
}

fn measure_line(ui: &egui::Ui, text: &str, size: f32) -> f32 {
    ui.fonts(|f| {
        f.layout_no_wrap(text.to_owned(), FontId::proportional(size), CHROME_TEXT)
            .size()
            .x
    })
}

fn paint_centered(
    ui: &egui::Ui,
    cx: f32,
    top: f32,
    row_h: f32,
    text: &str,
    size: f32,
    color: egui::Color32,
) {
    let galley = ui.fonts(|f| {
        f.layout_no_wrap(text.to_owned(), FontId::proportional(size), color)
    });
    let pos = egui::pos2(cx - galley.size().x * 0.5, top + (row_h - galley.size().y) * 0.5);
    ui.painter().galley(pos, galley, color);
}

fn paint_centered_proj(ui: &egui::Ui, cx: f32, top: f32, row_h: f32, value: &str, suffix: &str) {
    let mut job = egui::text::LayoutJob::default();
    job.halign = Align::LEFT; // whole job is placed by our center math
    job.append(
        value,
        0.0,
        egui::TextFormat {
            font_id: FontId::proportional(ROW_PROJ),
            color: CHROME_CAPTION,
            ..Default::default()
        },
    );
    job.append(
        suffix,
        0.0,
        egui::TextFormat {
            font_id: FontId::proportional(ROW_PROJ),
            color: CHROME_MUTED,
            ..Default::default()
        },
    );
    let galley = ui.fonts(|f| f.layout_job(job));
    let pos = egui::pos2(cx - galley.size().x * 0.5, top + (row_h - galley.size().y) * 0.5);
    ui.painter().galley(pos, galley, CHROME_CAPTION);
}
