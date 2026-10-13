//! Brief receive-pulse background for profile rows and sticky/tree nodes.

use eframe::egui::Color32;
use std::time::Instant;

use crate::mqtt::topic_tree::FLASH_MS;

/// Green flash that fades out over FLASH_MS.
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
