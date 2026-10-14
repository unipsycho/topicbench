//! Progressive ribbon collapse for narrow windows.
//!
//! Collapse order (first → last): App → Profiles → Panels → hide Stats.
//! Expand reverses that order when width returns.

use crate::ui::icons::{estimate_group_compact_w, estimate_group_full_w, estimate_separator_w};

/// How a command section is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SectionMode {
    /// Icon + caption in a single row.
    Full,
    /// Icon-only, two-high grid.
    Compact,
}

/// Planned ribbon density for the current available width.
#[derive(Clone, Copy, Debug)]
pub struct RibbonPlan {
    pub profiles: SectionMode,
    pub panels: SectionMode,
    pub app: SectionMode,
    pub show_stats: bool,
}

impl Default for RibbonPlan {
    fn default() -> Self {
        Self {
            profiles: SectionMode::Full,
            panels: SectionMode::Full,
            app: SectionMode::Full,
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

/// Total ribbon content width for a plan (brand + groups + separators + optional Stats).
pub fn plan_width(brand_w: f32, stats_w: f32, plan: RibbonPlan) -> f32 {
    let sep = estimate_separator_w();
    let mut w = brand_w + sep;
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
/// Steps applied while over budget: App compact → Profiles compact → Panels compact → hide Stats.
pub fn plan_for_width(avail_w: f32, brand_w: f32, stats_w: f32) -> RibbonPlan {
    let mut plan = RibbonPlan::default();
    if plan_width(brand_w, stats_w, plan) <= avail_w {
        return plan;
    }
    plan.app = SectionMode::Compact;
    if plan_width(brand_w, stats_w, plan) <= avail_w {
        return plan;
    }
    plan.profiles = SectionMode::Compact;
    if plan_width(brand_w, stats_w, plan) <= avail_w {
        return plan;
    }
    plan.panels = SectionMode::Compact;
    if plan_width(brand_w, stats_w, plan) <= avail_w {
        return plan;
    }
    plan.show_stats = false;
    plan
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collapse_order_app_then_profiles_then_panels_then_stats() {
        let brand = 250.0;
        let stats = 120.0;
        let full = plan_for_width(10_000.0, brand, stats);
        assert_eq!(full.app, SectionMode::Full);
        assert!(full.show_stats);

        // Force first step only: width between full and app-compact.
        let w_full = plan_width(brand, stats, RibbonPlan::default());
        let mut app_c = RibbonPlan::default();
        app_c.app = SectionMode::Compact;
        let w_app = plan_width(brand, stats, app_c);
        let p = plan_for_width((w_full + w_app) * 0.5, brand, stats);
        assert_eq!(p.app, SectionMode::Compact);
        assert_eq!(p.profiles, SectionMode::Full);
        assert!(p.show_stats);

        let tiny = plan_for_width(100.0, brand, stats);
        assert_eq!(tiny.app, SectionMode::Compact);
        assert_eq!(tiny.profiles, SectionMode::Compact);
        assert_eq!(tiny.panels, SectionMode::Compact);
        assert!(!tiny.show_stats);
    }
}
