//! Topic-tree search / filter bar: All | Topics | Payloads (drawn under Topic Tree header).

use crate::ui::hover_tip;
use crate::ui::shortcuts;
use eframe::egui;

pub use crate::mqtt::settings::SearchScope;

#[derive(Clone, Debug)]
pub struct SearchFilter {
    pub query: String,
    pub scope: SearchScope,
    /// When true, sticky board uses the same search filter.
    pub apply_to_stickies: bool,
}

impl Default for SearchFilter {
    fn default() -> Self {
        Self {
            query: String::new(),
            scope: SearchScope::default(),
            apply_to_stickies: true,
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

pub fn draw(ui: &mut egui::Ui, filter: &mut SearchFilter, request_focus: &mut bool) {
    ui.horizontal(|ui| {
        hover_tip::attach(
            ui.label("Search"),
            shortcuts::tip("Focus search", "Ctrl+F"),
        );
        let resp = hover_tip::attach(
            ui.add(
                egui::TextEdit::singleline(&mut filter.query)
                    .id_salt(SEARCH_QUERY_ID)
                    .desired_width(220.0)
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
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut filter.scope, SearchScope::All, "All");
                ui.selectable_value(&mut filter.scope, SearchScope::Topics, "Topics");
                ui.selectable_value(&mut filter.scope, SearchScope::Payloads, "Payloads");
            });
        if filter.is_active() && ui.small_button("Clear").clicked() {
            filter.query.clear();
        }
        // Separate from Clear — applies to the sticky board, not just the query field.
        ui.add_space(16.0);
        ui.checkbox(&mut filter.apply_to_stickies, "Apply to stickies");
    });
}
