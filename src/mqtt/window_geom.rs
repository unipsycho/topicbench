//! OS window geometry helpers for session restore (main + pop-out viewports).

use eframe::egui::{self, Pos2, Vec2, ViewportBuilder};
use serde::{Deserialize, Serialize};

/// Outer position + inner size for a native / immediate viewport.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct WinGeom {
    /// Outer top-left X (points). Absent when the platform does not report position (e.g. some Wayland).
    #[serde(default)]
    pub x: Option<f32>,
    #[serde(default)]
    pub y: Option<f32>,
    pub w: f32,
    pub h: f32,
    #[serde(default)]
    pub maximized: bool,
}

impl WinGeom {
    pub fn size(w: f32, h: f32) -> Self {
        Self {
            x: None,
            y: None,
            w,
            h,
            maximized: false,
        }
    }

    pub fn sane(self, min: Vec2) -> Self {
        let w = if self.w.is_finite() {
            self.w.max(min.x)
        } else {
            min.x
        };
        let h = if self.h.is_finite() {
            self.h.max(min.y)
        } else {
            min.y
        };
        let x = self.x.filter(|v| v.is_finite());
        let y = self.y.filter(|v| v.is_finite());
        Self {
            x,
            y,
            w,
            h,
            maximized: self.maximized,
        }
    }

    /// Apply saved size/pos to a builder. Call with a **session-frozen** geom so
    /// egui's patch does not fight live user resize/move.
    pub fn apply(self, builder: ViewportBuilder, min: [f32; 2]) -> ViewportBuilder {
        let g = self.sane(Vec2::new(min[0], min[1]));
        let mut b = builder.with_inner_size([g.w, g.h]);
        if let (Some(x), Some(y)) = (g.x, g.y) {
            b = b.with_position(Pos2::new(x, y));
        }
        if g.maximized {
            b = b.with_maximized(true);
        }
        b
    }
}

/// Read current viewport geometry from egui input (this viewport's context).
///
/// On Wayland, `inner_position` often fails so egui leaves `inner_rect`/`outer_rect`
/// unset — fall back to `screen_rect` for size (position stays `None`).
pub fn capture(ctx: &egui::Context) -> Option<WinGeom> {
    ctx.input(|i| {
        let vp = i.viewport();
        let (w, h) = if let Some(inner) = vp.inner_rect {
            (inner.width(), inner.height())
        } else {
            let r = i.screen_rect();
            if r.width() < 32.0 || r.height() < 32.0 {
                return None;
            }
            (r.width(), r.height())
        };
        let (x, y) = match vp.outer_rect {
            Some(r) => (Some(r.min.x), Some(r.min.y)),
            None => (None, None),
        };
        Some(WinGeom {
            x,
            y,
            w,
            h,
            maximized: vp.maximized.unwrap_or(false),
        })
    })
}

/// Update `slot` when the live window moved/resized; returns true if changed enough to save.
pub fn sync_slot(slot: &mut Option<WinGeom>, live: WinGeom, min: [f32; 2]) -> bool {
    let live = live.sane(Vec2::new(min[0], min[1]));
    match slot {
        Some(prev) if geom_close(*prev, live) => false,
        _ => {
            *slot = Some(live);
            true
        }
    }
}

fn geom_close(a: WinGeom, b: WinGeom) -> bool {
    const EPS: f32 = 1.0;
    a.maximized == b.maximized
        && (a.w - b.w).abs() < EPS
        && (a.h - b.h).abs() < EPS
        && opt_close(a.x, b.x, EPS)
        && opt_close(a.y, b.y, EPS)
}

fn opt_close(a: Option<f32>, b: Option<f32>, eps: f32) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => (a - b).abs() < eps,
        (None, None) => true,
        _ => false,
    }
}

/// Stable map key for a payload pop-out.
pub fn payload_key(profile_id: &str, topic: &str) -> String {
    format!("{profile_id}\n{topic}")
}
