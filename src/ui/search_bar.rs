//! Search / filter bar: All | Topics | Payloads.

use eframe::egui;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SearchScope {
    #[default]
    All,
    Topics,
    Payloads,
}

impl SearchScope {
    pub fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::Topics => "Topics",
            Self::Payloads => "Payloads",
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct SearchFilter {
    pub query: String,
    pub scope: SearchScope,
}

impl SearchFilter {
    pub fn is_active(&self) -> bool {
        !self.query.trim().is_empty()
    }

    pub fn matches(&self, topic: &str, payload: Option<&str>) -> bool {
        let q = self.query.trim();
        if q.is_empty() {
            return true;
        }
        let q = q.to_lowercase();
        let topic_hit = topic.to_lowercase().contains(&q);
        let payload_hit = payload
            .map(|p| p.to_lowercase().contains(&q))
            .unwrap_or(false);
        match self.scope {
            SearchScope::All => topic_hit || payload_hit,
            SearchScope::Topics => topic_hit,
            SearchScope::Payloads => payload_hit,
        }
    }
}

/// Id salt for the search query field (Ctrl+F focus).
pub const SEARCH_QUERY_ID: &str = "mqtt_search_query";

pub fn draw(ui: &mut egui::Ui, filter: &mut SearchFilter, request_focus: &mut bool) {
    ui.horizontal(|ui| {
        ui.label("Search");
        let resp = ui.add(
            egui::TextEdit::singleline(&mut filter.query)
                .id_salt(SEARCH_QUERY_ID)
                .desired_width(220.0)
                .hint_text("filter topics / payloads…"),
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
    });
}
