//! Brief receive-pulse border for profile rows, stickies, and tree nodes.
//! 1px border only — snap full-on, fade out over FLASH_MS; each update resets.
//! Paint with [`paint_inset`] so layout size never grows when the pulse is on.

use eframe::egui::{Color32, Rect, Stroke, StrokeKind, Ui};
use std::time::Instant;

use crate::mqtt::topic_tree::{TopicNode, FLASH_MS};

/// Per-item stroked frames must end this many px left of the ScrollArea *content*
/// clip (inner_rect.max.x). Full `available_width()` puts the right Inside stroke on
/// that plane → missing/square right edge. Sticky heading is fill+corner only (no
/// stroke) so it can span full width. Separate from scrollbar gutter
/// (`floating_allocated_width` in layout) — do not conflate the two.
pub const ITEM_CLIP_CLEAR: f32 = 3.0;

/// True while a flash pulse is still fading.
pub fn flash_active(until: Option<Instant>, now: Instant) -> bool {
    until.map(|t| now < t).unwrap_or(false)
}

/// Any node in the topic tree still fading (quiet single-topic case).
/// Not used on the hot path — walking a large tree every frame caused ANRs;
/// session/pin `flash_until` already cover continuous inbound traffic.
#[allow(dead_code)]
pub fn tree_flash_active(node: &TopicNode, now: Instant) -> bool {
    if flash_active(node.flash_until, now) {
        return true;
    }
    node.children
        .values()
        .any(|child| tree_flash_active(child, now))
}

/// Peak alpha for the update flash (was 255 — too bright on charcoal chrome).
const FLASH_PEAK_ALPHA: f32 = 110.0;

/// Fading 1px green border for an update pulse (no fill wash).
/// `until` is set to `now + FLASH_MS` on each new value — that resets the fade.
pub fn flash_stroke(until: Option<Instant>, now: Instant) -> Option<Stroke> {
    let until = until?;
    if now >= until {
        return None;
    }
    let left = until.saturating_duration_since(now).as_secs_f32();
    let t = (left / (FLASH_MS as f32 / 1000.0)).clamp(0.0, 1.0);
    // t=1 at pulse start (subtle peak), t→0 over 0.5s — same timing, lower opacity.
    let a = (FLASH_PEAK_ALPHA * t) as u8;
    Some(Stroke::new(
        1.0_f32,
        Color32::from_rgba_unmultiplied(80, 200, 120, a),
    ))
}

/// Draw a 1px flash (or fallback) stroke **inside** `rect` — never expands layout.
/// Shrink 1px so right-edge AA / corner curves clear the ScrollArea clip (sticky heading
/// fill curves stay visible; flush Inside strokes were still getting chopped).
pub fn paint_inset(ui: &Ui, rect: Rect, stroke: Option<Stroke>) {
    if let Some(stroke) = stroke {
        ui.painter()
            .rect_stroke(rect.shrink(1.0), 2.0, stroke, StrokeKind::Inside);
    }
}

/// Sticky/card chrome: always 1px inside; flash color when pulsing.
pub fn paint_inset_or(ui: &Ui, rect: Rect, flash: Option<Stroke>, fallback: Stroke) {
    ui.painter().rect_stroke(
        rect.shrink(1.0),
        2.0,
        flash.unwrap_or(fallback),
        StrokeKind::Inside,
    );
}
