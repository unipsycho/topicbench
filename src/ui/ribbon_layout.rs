//! Progressive ribbon collapse for narrow windows.
//!
//! Collapse order (first → last):
//! App compact → Profiles compact → Panels compact → hide MQTT Master title → hide Stats.
//! Expand reverses that order when width returns.

use crate::ui::icons::{estimate_group_compact_w, estimate_group_full_w, estimate_separator_w};

/// How a command section is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SectionMode {
    /// Icon + caption in a single row.
    Full,
    /// Icon-only, two-high grid (panel height stays [`crate::ui::icons::CMD_H`]).
    Compact,
}

/// Planned ribbon density for the current available width.
#[derive(Clone, Copy, Debug)]
pub struct RibbonPlan {
    pub profiles: SectionMode,
    pub panels: SectionMode,
    pub app: SectionMode,
    /// When false, logo stays but “MQTT” / “Master” title text is hidden.
    pub show_title: bool,
    pub show_stats: bool,
}

impl Default for RibbonPlan {
    fn default() -> Self {
        Self {
            profiles: SectionMode::Full,
            panels: SectionMode::Full,
            app: SectionMode::Full,
            show_title: true,
            show_stats: true,
        }
    }
}

fn section_w(mode: SectionMode, n: usize) -> f32 {
    match mode {
        SectionMode::Full => estimate_group_full_w(n),
        SectionMode::Compact => estimate_group_compact_w(n),
    }
}

fn brand_w(logo_w: f32, title_w: f32, show_title: bool) -> f32 {
    if show_title {
        logo_w + 10.0 + title_w + 10.0
    } else {
        // Logo only — drop the two-line title to free width.
        logo_w + 10.0
    }
}

/// Total ribbon content width for a plan.
pub fn plan_width(logo_w: f32, title_w: f32, stats_w: f32, plan: RibbonPlan) -> f32 {
    let sep = estimate_separator_w();
    let mut w = brand_w(logo_w, title_w, plan.show_title) + sep;
    w += section_w(plan.profiles, 4) + sep;
    w += section_w(plan.panels, 5) + sep;
    w += section_w(plan.app, 3);
    if plan.show_stats {
        w += sep + stats_w;
    }
    // Small slack so we don't thrash on rounding / frame pads.
    w + 4.0
}

/// Choose the least-collapsed layout that fits `avail_w`.
///
/// Steps: App compact → Profiles compact → Panels compact → hide title → hide Stats.
pub fn plan_for_width(avail_w: f32, logo_w: f32, title_w: f32, stats_w: f32) -> RibbonPlan {
    let mut plan = RibbonPlan::default();
    if plan_width(logo_w, title_w, stats_w, plan) <= avail_w {
        return plan;
    }
    plan.app = SectionMode::Compact;
    if plan_width(logo_w, title_w, stats_w, plan) <= avail_w {
        return plan;
    }
    plan.profiles = SectionMode::Compact;
    if plan_width(logo_w, title_w, stats_w, plan) <= avail_w {
        return plan;
    }
    plan.panels = SectionMode::Compact;
    if plan_width(logo_w, title_w, stats_w, plan) <= avail_w {
        return plan;
    }
    plan.show_title = false;
    if plan_width(logo_w, title_w, stats_w, plan) <= avail_w {
        return plan;
    }
    plan.show_stats = false;
    plan
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collapse_hides_title_before_stats() {
        let logo = 108.0;
        let title = 120.0;
        let stats = 120.0;
        let full = plan_for_width(10_000.0, logo, title, stats);
        assert!(full.show_title && full.show_stats);
        assert_eq!(full.app, SectionMode::Full);

        let tiny = plan_for_width(100.0, logo, title, stats);
        assert_eq!(tiny.app, SectionMode::Compact);
        assert_eq!(tiny.profiles, SectionMode::Compact);
        assert_eq!(tiny.panels, SectionMode::Compact);
        assert!(!tiny.show_title);
        assert!(!tiny.show_stats);

        // Width that fits all-compact + logo-only + Stats, but not with title.
        let mut p = RibbonPlan {
            profiles: SectionMode::Compact,
            panels: SectionMode::Compact,
            app: SectionMode::Compact,
            show_title: false,
            show_stats: true,
        };
        let w_logo_stats = plan_width(logo, title, stats, p);
        p.show_title = true;
        let w_title_stats = plan_width(logo, title, stats, p);
        assert!(w_logo_stats < w_title_stats);
        let mid = plan_for_width((w_logo_stats + w_title_stats) * 0.5, logo, title, stats);
        assert!(!mid.show_title);
        assert!(mid.show_stats);
    }
}
