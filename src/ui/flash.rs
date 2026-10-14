//! Brief receive-pulse border for profile rows, stickies, and tree nodes.
//! Border-only so text/syntax colors stay readable (no fill wash).

use eframe::egui::{Color32, Stroke};
use std::time::Instant;

use crate::mqtt::topic_tree::FLASH_MS;

/// Green flash fill (legacy) — prefer [`flash_stroke`] so content stays readable.
#[allow(dead_code)]
pub fn flash_fill(until: Option<Instant>, now: Instant) -> Option<Color32> {
    let until = until?;
    if now >= until {
        return None;
    }
    let left = until.saturating_duration_since(now).as_secs_f32();
    let t = (left / (FLASH_MS as f32 / 1000.0)).clamp(0.0, 1.0);
    let a = (140.0 * t) as u8;
    Some(Color32::from_rgba_unmultiplied(70, 170, 110, a))
}

/// Fading green border stroke for an update pulse (no background wash).
pub fn flash_stroke(until: Option<Instant>, now: Instant) -> Option<Stroke> {
    let until = until?;
    if now >= until {
        return None;
    }
    let left = until.saturating_duration_since(now).as_secs_f32();
    let t = (left / (FLASH_MS as f32 / 1000.0)).clamp(0.0, 1.0);
    let a = (220.0 * t) as u8;
    let w = 1.5 + 1.0 * t;
    Some(Stroke::new(
        w,
        Color32::from_rgba_unmultiplied(80, 200, 120, a),
    ))
}
