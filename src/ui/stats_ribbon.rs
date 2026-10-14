//! Compact Stats ribbon group: centered vertical stack that hugs content width.
//!
//! Exact-size tinted block matching other ribbon groups' height (`CMD_H` + frame
//! pad) so top/bottom edges align with App / Panels / Profiles.

use crate::mqtt::traffic_stats::TrafficStats;
use crate::ui::icons::{CHROME_CAPTION, CHROME_MUTED, CHROME_TEXT, TINT_STATS};
use eframe::egui::{self, Align, FontId, Sense, Vec2};
use eframe::epaint::TextShape;
use std::f32::consts::FRAC_PI_2;

/// Match `icons` ribbon_group: rail + `CMD_H` body + `Margin::symmetric(4, 3)`.
const RAIL_W: f32 = 18.0;
const RAIL_NUDGE_X: f32 = 14.0;
const FRAME_PAD_X: f32 = 4.0;
const FRAME_PAD_Y: f32 = 3.0;
/// Same content height as New/Save/Settings command cells (`icons::CMD_H`).
const BODY_H: f32 = 64.0;
const RAIL_GAP: f32 = 4.0;
const H_PAD: f32 = 6.0;
/// Breath between Thru and /hr /day (tight enough to fit BODY_H).
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
    // Five lines + mid-gap must fit inside BODY_H (same as other ribbon sections).
    let content_h = ROW_H_MAIN * 3.0 + MID_GAP + ROW_H_PROJ * 2.0;
    debug_assert!(content_h <= BODY_H);

    let total_w = FRAME_PAD_X * 2.0 + RAIL_W + RAIL_GAP + content_w;
    // Outer height matches `ribbon_group`: pad + CMD_H + pad.
    let total_h = FRAME_PAD_Y * 2.0 + BODY_H;

    let (outer, _) = ui.allocate_exact_size(Vec2::new(total_w, total_h), Sense::hover());
    ui.painter().rect_filled(outer, 3.0, TINT_STATS);

    let inner = egui::Rect::from_min_max(
        egui::pos2(outer.left() + FRAME_PAD_X, outer.top() + FRAME_PAD_Y),
        egui::pos2(outer.right() - FRAME_PAD_X, outer.bottom() - FRAME_PAD_Y),
    );
    let rail = egui::Rect::from_min_size(inner.min, Vec2::new(RAIL_W, BODY_H));
    paint_rotated_label(ui, rail, "Stats");

    let content = egui::Rect::from_min_max(
        egui::pos2(rail.right() + RAIL_GAP, inner.top()),
        egui::pos2(inner.right(), inner.top() + BODY_H),
    );
    let cx = content.center().x;
    // Vertically center the five-line stack in the shared BODY_H.
    let mut y = content.top() + (BODY_H - content_h) * 0.5;

    for s in &lines_main {
        paint_centered(ui, cx, y, ROW_H_MAIN, s, ROW_MAIN, CHROME_TEXT);
        y += ROW_H_MAIN;
    }
    y += MID_GAP;
    paint_centered_proj(ui, cx, y, ROW_H_PROJ, &d.per_h, "/hr");
    y += ROW_H_PROJ;
    paint_centered_proj(ui, cx, y, ROW_H_PROJ, &d.per_d, "/day");
}

fn measure_line(ui: &egui::Ui, text: &str, size: f32) -> f32 {
    ui.fonts(|f| {
        f.layout_no_wrap(text.to_owned(), FontId::proportional(size), CHROME_TEXT)
            .size()
            .x
    })
}

fn paint_rotated_label(ui: &egui::Ui, rect: egui::Rect, caption: &str) {
    let galley = ui.fonts(|f| {
        f.layout_no_wrap(
            caption.to_owned(),
            FontId::proportional(11.0),
            CHROME_CAPTION,
        )
    });
    let pos = egui::pos2(
        rect.center().x + galley.size().y * 0.5 - RAIL_NUDGE_X,
        rect.center().y + galley.size().x * 0.5,
    );
    ui.painter().add(
        TextShape::new(pos, galley, CHROME_CAPTION).with_angle(-FRAC_PI_2),
    );
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
