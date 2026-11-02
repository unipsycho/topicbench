//! Active UI chrome tokens — Classic in-tree; Pro palettes fetched from sidecar.
//!
//! No Pro RGB literals live here. Classic is the only baked palette; licensed
//! presets arrive as JSON [`ChromeTokens`] from `GET /themes/:id`.

use eframe::egui::{self, Color32, Stroke, ThemePreference, Visuals};
use serde::{Deserialize, Serialize};
use std::sync::{Mutex, OnceLock};

/// Flat chrome roles — wire JSON matches Pro `ChromeTokens` field names.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChromeTokens {
    #[serde(with = "rgb_serde")]
    pub bg: Color32,
    #[serde(with = "rgb_serde")]
    pub status: Color32,
    #[serde(with = "rgb_serde")]
    pub sep: Color32,
    #[serde(with = "rgb_serde")]
    pub hover: Color32,
    #[serde(with = "rgb_serde")]
    pub header_hover: Color32,
    #[serde(with = "rgb_serde")]
    pub header_selected: Color32,
    #[serde(with = "rgb_serde")]
    pub accent_soft: Color32,
    #[serde(with = "rgb_serde")]
    pub accent_border: Color32,
    #[serde(with = "rgb_serde")]
    pub row: Color32,
    #[serde(with = "rgb_serde")]
    pub section: Color32,
    #[serde(with = "rgb_serde")]
    pub heading: Color32,
    #[serde(with = "rgb_serde")]
    pub caption: Color32,
    #[serde(with = "rgb_serde")]
    pub muted: Color32,
    #[serde(with = "rgb_serde")]
    pub text: Color32,
    #[serde(with = "rgb_serde")]
    pub tint_profiles: Color32,
    #[serde(with = "rgb_serde")]
    pub tint_panels: Color32,
    #[serde(with = "rgb_serde")]
    pub tint_app: Color32,
    #[serde(with = "rgb_serde")]
    pub tint_pro: Color32,
    #[serde(with = "rgb_serde")]
    pub tint_stats: Color32,
    /// When true, [`apply_theme`] uses light Visuals (Catppuccin Latte).
    #[serde(default)]
    pub is_light: bool,
}

mod rgb_serde {
    use eframe::egui::Color32;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S: Serializer>(c: &Color32, s: S) -> Result<S::Ok, S::Error> {
        [c.r(), c.g(), c.b()].serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Color32, D::Error> {
        let a = <[u8; 3]>::deserialize(d)?;
        Ok(Color32::from_rgb(a[0], a[1], a[2]))
    }
}

/// Built-in free palette (cool charcoal) — sole RGB source in free tree.
pub fn classic() -> ChromeTokens {
    ChromeTokens {
        bg: Color32::from_rgb(34, 36, 42),
        status: Color32::from_rgb(26, 28, 34),
        sep: Color32::from_rgb(70, 74, 84),
        hover: Color32::from_rgb(58, 62, 72),
        header_hover: Color32::from_rgb(78, 96, 128),
        header_selected: Color32::from_rgb(56, 92, 140),
        accent_soft: Color32::from_rgb(48, 78, 112),
        accent_border: Color32::from_rgb(100, 160, 220),
        row: Color32::from_rgb(46, 50, 58),
        section: Color32::from_rgb(52, 56, 66),
        heading: Color32::from_rgb(130, 185, 245),
        caption: Color32::from_rgb(190, 196, 208),
        muted: Color32::from_rgb(175, 182, 196),
        text: Color32::from_rgb(230, 232, 238),
        tint_profiles: Color32::from_rgb(48, 56, 70),
        tint_panels: Color32::from_rgb(46, 60, 52),
        tint_app: Color32::from_rgb(62, 52, 48),
        tint_pro: Color32::from_rgb(40, 58, 62),
        tint_stats: Color32::from_rgb(42, 54, 64),
        is_light: false,
    }
}

pub const CLASSIC_ID: &str = "classic";

fn active_lock() -> &'static Mutex<ChromeTokens> {
    static LOCK: OnceLock<Mutex<ChromeTokens>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(classic()))
}

/// Snapshot of the currently applied chrome tokens.
pub fn chrome() -> ChromeTokens {
    *active_lock().lock().unwrap_or_else(|e| e.into_inner())
}

/// Install tokens as the active chrome (call before / with [`apply_theme`]).
pub fn set_active(tokens: ChromeTokens) {
    if let Ok(mut g) = active_lock().lock() {
        *g = tokens;
    }
}

/// Apply tokens to egui visuals. Honors `is_light` — Latte gets Light preference.
pub fn apply_theme(ctx: &egui::Context, tokens: &ChromeTokens) {
    set_active(*tokens);
    if tokens.is_light {
        ctx.set_theme(ThemePreference::Light);
    } else {
        ctx.set_theme(ThemePreference::Dark);
    }

    let mut visuals = if tokens.is_light {
        Visuals::light()
    } else {
        Visuals::dark()
    };
    visuals.panel_fill = tokens.bg;
    visuals.window_fill = tokens.bg;
    visuals.extreme_bg_color = tokens.status;
    visuals.code_bg_color = tokens.status;
    // Panel splitters / separators use noninteractive.bg_stroke — theme from sep
    // so Latte (and other light palettes) keep a readable grip line.
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, tokens.sep);
    // Mid well so `.weak()` muted labels stay readable on panel fill.
    visuals.widgets.noninteractive.weak_bg_fill = if tokens.is_light {
        Color32::from_rgb(
            tokens.muted.r().saturating_add(20).min(255),
            tokens.muted.g().saturating_add(20).min(255),
            tokens.muted.b().saturating_add(20).min(255),
        )
    } else {
        Color32::from_rgb(88, 92, 104)
    };
    apply_selector_widget_visuals(&mut visuals, tokens);
    apply_control_accent_visuals(&mut visuals, tokens);
    ctx.set_visuals(visuals);
    // Slightly wider than egui's 5px default — easier to grab beside Middle overlays.
    ctx.style_mut(|s| {
        s.interaction.resize_grab_radius_side = s.interaction.resize_grab_radius_side.max(8.0);
    });
}

/// ComboBox / default egui button fills from active chrome roles.
pub fn apply_selector_widget_visuals(visuals: &mut Visuals, t: &ChromeTokens) {
    let stroke = Stroke::new(1.0_f32, t.sep);
    visuals.widgets.inactive.weak_bg_fill = t.status;
    visuals.widgets.inactive.bg_stroke = stroke;
    visuals.widgets.hovered.weak_bg_fill = t.hover;
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, t.accent_border);
    visuals.widgets.active.weak_bg_fill = t.section;
    visuals.widgets.active.bg_stroke = Stroke::new(1.0_f32, t.accent_border);
    visuals.widgets.open.weak_bg_fill = t.section;
    visuals.widgets.open.bg_stroke = Stroke::new(1.0_f32, t.accent_border);
}

/// Checkbox / slider chrome derived from tokens (works for light + dark).
pub fn apply_control_accent_visuals(visuals: &mut Visuals, t: &ChromeTokens) {
    let idle = t.status;
    let hover = t.hover;
    // Checked / trailing fill: lift accent_border toward white (dark) or soften (light).
    let active = if t.is_light {
        mix(t.accent_border, Color32::WHITE, 40)
    } else {
        mix(t.accent_soft, t.accent_border, 140)
    };
    let edge = Stroke::new(1.0_f32, t.accent_border);
    let handle = Stroke::new(
        1.5_f32,
        if t.is_light {
            mix(t.text, t.muted, 80)
        } else {
            mix(t.caption, Color32::WHITE, 40)
        },
    );
    visuals.widgets.inactive.bg_fill = idle;
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, t.sep);
    visuals.widgets.inactive.fg_stroke = handle;
    visuals.widgets.hovered.bg_fill = hover;
    visuals.widgets.hovered.bg_stroke = edge;
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.5_f32, t.accent_border);
    visuals.widgets.active.bg_fill = active;
    visuals.widgets.active.bg_stroke = edge;
    visuals.widgets.active.fg_stroke = Stroke::new(
        2.0_f32,
        if t.is_light {
            Color32::WHITE
        } else {
            mix(t.caption, Color32::WHITE, 80)
        },
    );
    visuals.widgets.open.bg_fill = active;
    visuals.widgets.open.bg_stroke = edge;
    visuals.selection.bg_fill = active;
    visuals.selection.stroke = Stroke::new(1.0_f32, t.accent_border);
    visuals.slider_trailing_fill = true;
}

fn mix(a: Color32, b: Color32, t: u16) -> Color32 {
    let t = t.min(255);
    let u = 255 - t;
    Color32::from_rgb(
        ((a.r() as u16 * u + b.r() as u16 * t) / 255) as u8,
        ((a.g() as u16 * u + b.g() as u16 * t) / 255) as u8,
        ((a.b() as u16 * u + b.b() as u16 * t) / 255) as u8,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classic_not_light() {
        assert!(!classic().is_light);
        assert_eq!(classic().bg, Color32::from_rgb(34, 36, 42));
    }

    #[test]
    fn json_roundtrip_matches_pro_shape() {
        let t = classic();
        let v = serde_json::to_value(&t).unwrap();
        assert!(v.get("bg").unwrap().as_array().unwrap().len() == 3);
        assert!(v.get("tint_stats").is_some());
        assert_eq!(v.get("is_light").and_then(|x| x.as_bool()), Some(false));
        let back: ChromeTokens = serde_json::from_value(v).unwrap();
        assert_eq!(back, t);
    }

    #[test]
    fn latte_json_deserializes_light() {
        // Minimal Pro Latte-shaped payload (subset of roles from Omarchy).
        let raw = r#"{
            "bg":[239,241,245],"status":[227,228,232],"sep":[180,184,194],
            "hover":[210,214,222],"header_hover":[180,190,220],"header_selected":[160,180,230],
            "accent_soft":[200,210,230],"accent_border":[30,102,245],
            "row":[220,224,232],"section":[210,214,224],
            "heading":[30,102,245],"caption":[92,95,119],"muted":[140,144,160],
            "text":[76,79,105],
            "tint_profiles":[220,226,240],"tint_panels":[220,235,220],
            "tint_app":[235,220,215],"tint_pro":[215,230,230],"tint_stats":[220,225,235],
            "is_light":true
        }"#;
        let t: ChromeTokens = serde_json::from_str(raw).unwrap();
        assert!(t.is_light);
        assert_eq!(t.accent_border, Color32::from_rgb(30, 102, 245));
    }
}
