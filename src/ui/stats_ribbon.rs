//! Compact Stats ribbon group: centered vertical stack that hugs content width.
//!
//! Uses `ribbon_group` so height matches App / Panels / Profiles exactly
//! (`CMD_H` body + same frame pad). Content column is exact-width (hug).

use crate::mqtt::traffic_stats::TrafficStats;
use crate::ui::icons::{self, CHROME_CAPTION, CHROME_MUTED, CHROME_TEXT, TINT_STATS};
use eframe::egui::{self, Align, FontId, Sense, Vec2};

/// Same as `icons` command cell height (`CMD_H`).
const BODY_H: f32 = 64.0;
const H_PAD: f32 = 6.0;
/// Breath between Thru and /hr /day (tight so five lines fit BODY_H).
const MID_GAP: f32 = 4.0;
const ROW_MAIN: f32 = 9.0;
const ROW_PROJ: f32 = 8.5;
const ROW_H_MAIN: f32 = 10.0;
const ROW_H_PROJ: f32 = 9.0;

/// Draw the far-right Stats ribbon category (process-lifetime counters).
pub fn draw_group(ui: &mut egui::Ui, stats: &TrafficStats) {
    let d = stats.display();

    let lines_main = [
        format!("Recv {}", d.recv),
        format!("Sent {}", d.sent),
        format!("Thru {}", d.thru),
    ];
    let lines_proj = [format!("{}/hr", d.per_h), format!("{}/day", d.per_d)];

    let text_w = {
        let mut w = 0.0_f32;
        for s in &lines_main {
            w = w.max(measure_line(ui, s, ROW_MAIN));
        }
        for s in &lines_proj {
            w = w.max(measure_line(ui, s, ROW_PROJ));
        }
        w.ceil()
    };
    let content_w = text_w + H_PAD * 2.0;
    let content_h = ROW_H_MAIN * 3.0 + MID_GAP + ROW_H_PROJ * 2.0;
    debug_assert!(content_h <= BODY_H);

    // Cap width before ribbon_group so the tint cannot expand into leftover ribbon.
    ui.set_max_width(content_w + 40.0);

    icons::ribbon_group(ui, "Stats", TINT_STATS, |ui| {
        // Exact CMD_H cell — same vertical size as Settings/Zoom command cells.
        let (rect, _) = ui.allocate_exact_size(Vec2::new(content_w, BODY_H), Sense::hover());
        let cx = rect.center().x;
        let mut y = rect.top() + (BODY_H - content_h) * 0.5;

        for s in &lines_main {
            paint_centered(ui, cx, y, ROW_H_MAIN, s, ROW_MAIN, CHROME_TEXT);
            y += ROW_H_MAIN;
        }
        y += MID_GAP;
        paint_centered_proj(ui, cx, y, ROW_H_PROJ, &d.per_h, "/hr");
        y += ROW_H_PROJ;
        paint_centered_proj(ui, cx, y, ROW_H_PROJ, &d.per_d, "/day");
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
    job.halign = Align::LEFT;
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
