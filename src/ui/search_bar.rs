//! Topic-tree search / filter bar: All | Topics | Payloads (drawn under Topic Tree header).

use crate::ui::hover_tip;
use crate::ui::icons::{CHROME_CAPTION, CHROME_ROW, CHROME_SEP};
use crate::ui::shortcuts;
use eframe::egui;

pub use crate::mqtt::settings::SearchScope;

/// “Updated within …” time filter for the topic tree (session-only, not persisted).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RecentWindow {
    /// No time filter.
    #[default]
    Any,
    S5,
    S15,
    S30,
    S60,
    M5,
}

impl RecentWindow {
    pub const ALL: [Self; 6] = [Self::Any, Self::S5, Self::S15, Self::S30, Self::S60, Self::M5];

    pub fn label(self) -> &'static str {
        match self {
            Self::Any => "Any time",
            Self::S5 => "Last 5 s",
            Self::S15 => "Last 15 s",
            Self::S30 => "Last 30 s",
            Self::S60 => "Last 60 s",
            Self::M5 => "Last 5 min",
        }
    }

    pub fn window(self) -> Option<std::time::Duration> {
        let secs = match self {
            Self::Any => return None,
            Self::S5 => 5,
            Self::S15 => 15,
            Self::S30 => 30,
            Self::S60 => 60,
            Self::M5 => 300,
        };
        Some(std::time::Duration::from_secs(secs))
    }

    pub fn is_active(self) -> bool {
        self != Self::Any
    }
}

#[derive(Clone, Debug)]
pub struct SearchFilter {
    pub query: String,
    pub scope: SearchScope,
    /// When true, sticky board uses the same search filter.
    pub apply_to_stickies: bool,
    /// Only show topics that received data within this window (topic tree only).
    pub recent: RecentWindow,
    /// Only show topics whose last message had the MQTT RETAIN flag (topic tree only).
    pub retained_only: bool,
}

impl Default for SearchFilter {
    fn default() -> Self {
        Self {
            query: String::new(),
            scope: SearchScope::default(),
            apply_to_stickies: true,
            recent: RecentWindow::Any,
            retained_only: false,
        }
    }
}

impl SearchFilter {
    /// Restore from settings.json fields (applied at startup before tree paint).
    pub fn from_settings(query: String, scope: SearchScope, apply_to_stickies: bool) -> Self {
        Self {
            query,
            scope,
            apply_to_stickies,
            recent: RecentWindow::Any,
            retained_only: false,
        }
    }

    pub fn is_active(&self) -> bool {
        !self.terms().is_empty()
    }

    /// Non-empty search terms split on `|` (trimmed). Empty segments ignored.
    pub fn terms(&self) -> Vec<String> {
        self.query
            .split('|')
            .map(|t| t.trim().to_lowercase())
            .filter(|t| !t.is_empty())
            .collect()
    }

    /// OR match: any term hits topic and/or payload per scope.
    pub fn matches(&self, topic: &str, payload: Option<&str>) -> bool {
        let terms = self.terms();
        if terms.is_empty() {
            return true;
        }
        let topic_l = topic.to_lowercase();
        let payload_l = payload.map(|p| p.to_lowercase());
        terms.iter().any(|q| {
            let topic_hit = topic_l.contains(q);
            let payload_hit = payload_l.as_ref().is_some_and(|p| p.contains(q));
            match self.scope {
                SearchScope::All => topic_hit || payload_hit,
                SearchScope::Topics => topic_hit,
                SearchScope::Payloads => payload_hit,
            }
        })
    }
}

/// Id salt for the search query field (Ctrl+F focus).
pub const SEARCH_QUERY_ID: &str = "mqtt_search_query";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_and_blank_terms_pass() {
        let f = SearchFilter::default();
        assert!(f.matches("a/b", Some("x")));
        let f = SearchFilter {
            query: "  |  | ".into(),
            scope: SearchScope::All,
            apply_to_stickies: true,
            recent: RecentWindow::Any,
            retained_only: false,
        };
        assert!(!f.is_active());
        assert!(f.matches("a/b", Some("x")));
    }

    #[test]
    fn or_terms_any_match() {
        let f = SearchFilter {
            query: "temp | humidity".into(),
            scope: SearchScope::Topics,
            apply_to_stickies: true,
            recent: RecentWindow::Any,
            retained_only: false,
        };
        assert!(f.matches("sensors/temp", None));
        assert!(f.matches("lab/humidity", None));
        assert!(!f.matches("sensors/pressure", None));
    }

    #[test]
    fn or_terms_payload_scope() {
        let f = SearchFilter {
            query: "acid | base".into(),
            scope: SearchScope::Payloads,
            apply_to_stickies: true,
            recent: RecentWindow::Any,
            retained_only: false,
        };
        assert!(f.matches("chem/x", Some(r#"{"name":"Citric Acid"}"#)));
        assert!(!f.matches("chem/x", Some(r#"{"name":"water"}"#)));
    }

    #[test]
    fn from_settings_roundtrip_fields() {
        let f = SearchFilter::from_settings("a | b".into(), SearchScope::Topics, false);
        assert_eq!(f.query, "a | b");
        assert_eq!(f.scope, SearchScope::Topics);
        assert!(!f.apply_to_stickies);
        assert!(f.is_active());
    }
}

const FIELD_MIN: f32 = 72.0;
const FIELD_MAX: f32 = 220.0;
/// Frame chrome: inner margin 8+8 + stroke 1+1.
const FRAME_PAD: f32 = 18.0;
const GROUP_GAP: f32 = 6.0;

/// Natural (single-row) content widths measured on earlier frames, so the two groups can
/// sit side by side when they fit and stack only when they don't.
#[derive(Clone, Copy)]
struct Measured {
    /// Group 1 width excluding the flexible text field.
    fixed1: f32,
    /// Group 2 content width.
    w2: f32,
}

impl Default for Measured {
    fn default() -> Self {
        Self {
            fixed1: 300.0,
            w2: 290.0,
        }
    }
}

/// Grouped chrome strip: related controls share one rounded, tinted frame.
/// Returns the content width when it fit on a single row (else `None`).
fn group_frame(
    ui: &mut egui::Ui,
    fill_width: bool,
    add: impl FnOnce(&mut egui::Ui),
) -> Option<f32> {
    let mut measured = None;
    egui::Frame::NONE
        .fill(CHROME_ROW())
        .stroke(egui::Stroke::new(1.0_f32, CHROME_SEP()))
        .corner_radius(4.0)
        .inner_margin(egui::Margin::symmetric(8, 5))
        .show(ui, |ui| {
            if fill_width {
                ui.set_min_width(ui.available_width());
            }
            let r = ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                add(ui);
            });
            if r.response.rect.height() < ui.spacing().interact_size.y * 1.6 {
                measured = Some(r.response.rect.width());
            }
        });
    measured
}

/// Group 1 — text search: terms, scope, clear, and whether stickies follow it.
fn group_search(
    ui: &mut egui::Ui,
    filter: &mut SearchFilter,
    request_focus: &mut bool,
    field_w: f32,
    fill_width: bool,
) -> Option<f32> {
    group_frame(ui, fill_width, |ui| {
        hover_tip::attach(
            ui.label("Search"),
            shortcuts::tip("Focus search", "Ctrl+F"),
        );
        let resp = hover_tip::attach(
            ui.add(
                egui::TextEdit::singleline(&mut filter.query)
                    .id_salt(SEARCH_QUERY_ID)
                    .desired_width(field_w)
                    .hint_text("terms | separated…"),
            ),
            shortcuts::tip("Focus search", "Ctrl+F"),
        );
        if *request_focus {
            resp.request_focus();
            *request_focus = false;
        }
        egui::ComboBox::from_id_salt("search_scope")
            .selected_text(filter.scope.label())
            .width(88.0)
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut filter.scope, SearchScope::All, "All");
                ui.selectable_value(&mut filter.scope, SearchScope::Topics, "Topics");
                ui.selectable_value(&mut filter.scope, SearchScope::Payloads, "Payloads");
            });
        if filter.is_active() && ui.small_button("Clear").clicked() {
            filter.query.clear();
        }
        // Applies to the sticky board, not just the query field.
        ui.checkbox(&mut filter.apply_to_stickies, "Apply to stickies");
    })
}

/// Group 2 — tree-only state filters: recency and retained.
fn group_state(ui: &mut egui::Ui, filter: &mut SearchFilter, fill_width: bool) -> Option<f32> {
    group_frame(ui, fill_width, |ui| {
        ui.label(egui::RichText::new("Show only").color(CHROME_CAPTION()));
        let recent_resp = egui::ComboBox::from_id_salt("search_recent")
            .selected_text(if filter.recent.is_active() {
                format!("Updated: {}", filter.recent.label().trim_start_matches("Last "))
            } else {
                "Updated: any".to_string()
            })
            .width(120.0)
            .show_ui(ui, |ui| {
                for w in RecentWindow::ALL {
                    ui.selectable_value(&mut filter.recent, w, w.label());
                }
            });
        hover_tip::attach(
            recent_resp.response,
            "Show only topics that received data within the chosen time window",
        );
        hover_tip::attach(
            ui.checkbox(&mut filter.retained_only, "Retained"),
            "Show only topics whose last message was retained by the broker",
        );
        if filter.recent.is_active() {
            // Rows age out of the window while idle — keep re-evaluating.
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(500));
        }
    })
}

pub fn draw(ui: &mut egui::Ui, filter: &mut SearchFilter, request_focus: &mut bool) {
    let mid = egui::Id::new("search_bar_measured");
    let mut m: Measured = ui.ctx().data_mut(|d| d.get_temp(mid)).unwrap_or_default();
    let avail = ui.available_width();
    // Side by side only when both groups fit on one row; otherwise stack.
    let inline = m.fixed1 + FIELD_MIN + m.w2 + 2.0 * FRAME_PAD + GROUP_GAP <= avail;

    let mut new_m = m;
    if inline {
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = GROUP_GAP;
            let room = avail - m.w2 - 2.0 * FRAME_PAD - GROUP_GAP - m.fixed1;
            let field_w = room.clamp(FIELD_MIN, FIELD_MAX);
            if let Some(w) = group_search(ui, filter, request_focus, field_w, false) {
                new_m.fixed1 = (w - field_w).max(0.0);
            }
            if let Some(w) = group_state(ui, filter, false) {
                new_m.w2 = w;
            }
        });
    } else {
        let field_w = (avail - FRAME_PAD - m.fixed1).clamp(FIELD_MIN, FIELD_MAX);
        if let Some(w) = group_search(ui, filter, request_focus, field_w, true) {
            new_m.fixed1 = (w - field_w).max(0.0);
        }
        ui.add_space(3.0);
        if let Some(w) = group_state(ui, filter, true) {
            new_m.w2 = w;
        }
    }
    m = new_m;
    ui.ctx().data_mut(|d| d.insert_temp(mid, m));
}
