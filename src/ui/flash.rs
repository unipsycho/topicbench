//! Brief receive-pulse border for profile rows, stickies, and tree nodes.
//! 1px border only — snap full-on, fade out over FLASH_MS; each update resets.

use eframe::egui::{Color32, Stroke};
use std::time::Instant;

use crate::mqtt::topic_tree::FLASH_MS;

/// Fading 1px green border for an update pulse (no fill wash).
/// `until` is set to `now + FLASH_MS` on each new value — that resets the fade.
pub fn flash_stroke(until: Option<Instant>, now: Instant) -> Option<Stroke> {
    let until = until?;
    if now >= until {
        return None;
    }
    let left = until.saturating_duration_since(now).as_secs_f32();
    let t = (left / (FLASH_MS as f32 / 1000.0)).clamp(0.0, 1.0);
    // t=1 at pulse start (full on), t→0 over 0.5s
    let a = (255.0 * t) as u8;
    Some(Stroke::new(
        1.0_f32,
        Color32::from_rgba_unmultiplied(80, 200, 120, a),
    ))
}
