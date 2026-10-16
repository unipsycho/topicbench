//! Payload side panel — live Value view for the selected topic tree row + find.

use crate::mqtt::pins::{DetectedValue, PinBoard};
use crate::mqtt::profile::{ProfileStore, StickyViewMode};
use crate::mqtt::session::SessionManager;
use crate::mqtt::value_path;
use crate::ui::hover_tip;
use crate::ui::icons::{self, IconSet};
use crate::ui::json_view;
use eframe::egui::{self, Align, Color32, Key, RichText};
use std::time::Instant;

fn match_line_fill() -> Color32 {
    Color32::from_rgba_unmultiplied(90, 80, 30, 90)
}
fn match_current_fill() -> Color32 {
    Color32::from_rgba_unmultiplied(140, 120, 40, 140)
}

/// Find-in-payload state (lives on the app so it survives panel redraws).
#[derive(Default)]
pub struct PayloadSearch {
    pub query: String,
    /// 0-based index into the current match list.
    pub current: usize,
    /// After Next/Prev / query change, scroll the active match into view.
    pub scroll_to_current: bool,
}

impl PayloadSearch {
    fn goto_next(&mut self, n: usize) {
        if n == 0 {
            return;
        }
        self.current = (self.current + 1) % n;
        self.scroll_to_current = true;
    }

    fn goto_prev(&mut self, n: usize) {
        if n == 0 {
            return;
        }
        self.current = if self.current == 0 { n - 1 } else { self.current - 1 };
        self.scroll_to_current = true;
    }
}

/// One substring hit in the displayed payload text.
#[derive(Clone, Copy, Debug)]
struct MatchHit {
    /// Line index in the displayed text (0-based).
    line: usize,
    /// Byte range within the full display string.
    start: usize,
    end: usize,
}

pub fn draw(
    ui: &mut egui::Ui,
    sessions: &mut SessionManager,
    profiles: &mut ProfileStore,
    pins: &mut PinBoard,
    icons: &IconSet,
    search: &mut PayloadSearch,
    open_stickies: &mut bool,
    status: &mut String,
) {
    // Stay within the side-panel width — long topic paths wrap; they must not widen the column.
    let panel_w = ui.available_width();
    ui.set_max_width(panel_w);

    icons::section_header_bar(ui, |ui| {
        if let Some(tex) = icons.get("panel_undock") {
            ui.add(
                egui::Image::new(tex)
                    .fit_to_exact_size(egui::vec2(20.0, 20.0))
                    .sense(egui::Sense::hover()),
            );
        }
        ui.heading("Payload");
    });

    // Caption + trash on one row (trash top-right of Payload, not beside the topic path).
    let focus = pins.last_tree_focus.clone();
    let mut delete_hierarchy = false;
    ui.horizontal(|ui| {
        ui.label("Live Value view for the selected topic.");
        if focus.is_some() {
            ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                if icons
                    .tiny(ui, "trash", false, "Delete topic hierarchy")
                    .clicked()
                {
                    delete_hierarchy = true;
                }
            });
        }
    });
    ui.add_space(4.0);

    let Some((profile_id, profile_name, topic)) = focus else {
        ui.weak("Select a topic in the tree.");
        return;
    };

    // Drop selected topic + descendants from the in-memory display tree (not broker).
    if delete_hierarchy {
        pins.last_tree_focus = None;
        if let Some(session) = sessions.open.get_mut(&profile_id) {
            if session.topic_tree.remove_hierarchy(&topic) {
                *status = format!("Deleted topic hierarchy: {topic}");
            } else {
                *status = format!("Topic not in tree: {topic}");
            }
        } else {
            *status = format!("Broker not connected: {topic}");
        }
        return;
    }

    let accent = profiles
        .profiles
        .get(&profile_id)
        .map(|p| p.color())
        .unwrap_or(Color32::from_rgb(90, 190, 120));

    // Stacked header: profile name, then wrapping topic path on its own line.
    ui.label(RichText::new(&profile_name).color(accent));
    ui.add(
        egui::Label::new(json_view::topic_path_text(&topic))
            .wrap()
            .selectable(true),
    );
    ui.add_space(4.0);

    let node = sessions
        .open
        .get(&profile_id)
        .and_then(|s| s.topic_tree.root.node_at(&topic));

    match node {
        Some(n) if n.last_payload.as_deref().is_some_and(|p| !p.is_empty()) => {
            let payload = n.last_payload.as_deref().unwrap_or("").to_string();
            let display = display_text(&payload);
            let is_json = json_view::try_pretty_json(&payload).is_some();
            draw_search_bar(ui, icons, search, &display);
            ui.add_space(4.0);
            // Message count sits on the JSON/Value intro row (not its own line above).
            draw_searchable_body(
                ui,
                &display,
                is_json,
                ("payload-panel", profile_id.as_str(), topic.as_str()),
                search,
                n.message_count,
                icons,
                &profile_id,
                &profile_name,
                &topic,
                &payload,
                pins,
                profiles,
                open_stickies,
                status,
            );
        }
        _ => {
            ui.weak("No payload yet for this topic.");
        }
    }
}

/// Text shown in the Value body (pretty JSON when possible).
fn display_text(payload: &str) -> String {
    if let Some(pretty) = json_view::try_pretty_json(payload) {
        return pretty;
    }
    match DetectedValue::from_payload(payload) {
        DetectedValue::Bool(on) => {
            if on {
                "true".into()
            } else {
                "false".into()
            }
        }
        DetectedValue::Number(n) => format!("{n}"),
        DetectedValue::Text(s) => s,
    }
}

fn draw_search_bar(
    ui: &mut egui::Ui,
    icons: &IconSet,
    search: &mut PayloadSearch,
    display: &str,
) {
    let matches = find_matches(display, &search.query);
    let n = matches.len();
    if n > 0 {
        search.current = search.current.min(n - 1);
    } else {
        search.current = 0;
    }

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        icons.tiny(ui, "clipboard_checklist", false, "Find in payload");
        let edit = ui.add(
            egui::TextEdit::singleline(&mut search.query)
                .desired_width(ui.available_width().min(180.0).max(80.0))
                .hint_text("Find…")
                .font(egui::TextStyle::Monospace),
        );
        if edit.changed() {
            search.current = 0;
            search.scroll_to_current = !search.query.trim().is_empty();
        }
        // Enter / Shift+Enter while the field has focus.
        if edit.has_focus() {
            let enter = ui.input(|i| i.key_pressed(Key::Enter));
            let shift = ui.input(|i| i.modifiers.shift);
            if enter {
                if shift {
                    search.goto_prev(n);
                } else {
                    search.goto_next(n);
                }
            }
        }
        let label = if search.query.trim().is_empty() {
            "—".to_string()
        } else if n == 0 {
            "0".into()
        } else {
            format!("{}/{}", search.current + 1, n)
        };
        ui.label(RichText::new(label).small().monospace().weak());
        if hover_tip::attach(
            ui.add_enabled(n > 0, egui::Button::new("Prev").small()),
            "Previous match (Shift+Enter)",
        )
        .clicked()
        {
            search.goto_prev(n);
        }
        if hover_tip::attach(
            ui.add_enabled(n > 0, egui::Button::new("Next").small()),
            "Next match (Enter)",
        )
        .clicked()
        {
            search.goto_next(n);
        }
    });
}

fn draw_searchable_body(
    ui: &mut egui::Ui,
    display: &str,
    is_json: bool,
    id_salt: impl std::hash::Hash,
    search: &mut PayloadSearch,
    message_count: u64,
    icons: &IconSet,
    profile_id: &str,
    profile_name: &str,
    topic: &str,
    raw_payload: &str,
    pins: &mut PinBoard,
    profiles: &mut ProfileStore,
    open_stickies: &mut bool,
    status: &mut String,
) {
    let matches = find_matches(display, &search.query);
    let n = matches.len();
    if n > 0 {
        search.current = search.current.min(n - 1);
    }
    let current_line = matches.get(search.current).map(|m| m.line);
    let do_scroll = search.scroll_to_current && current_line.is_some();

    // Kind label + message count on one row.
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        if is_json {
            ui.label(RichText::new("JSON").small().weak());
        } else {
            ui.label(RichText::new("Value").small().weak());
        }
        if message_count > 0 {
            ui.label(
                RichText::new(format!("#{message_count}"))
                    .small()
                    .weak(),
            );
        }
    });

    let height = ui.available_height().max(80.0);
    let font_size = ui.text_style_height(&egui::TextStyle::Monospace);
    let lines: Vec<&str> = display.split('\n').collect();
    // Per-line JSON field path when the leaf is number/bool (graphable).
    let graph_paths: Vec<Option<String>> = if is_json {
        value_path::graphable_paths_per_line(display)
    } else {
        // Whole-payload scalar Value: one graphable row, no field path.
        let graphable = matches!(
            DetectedValue::from_payload(raw_payload),
            DetectedValue::Bool(_) | DetectedValue::Number(_)
        );
        lines
            .iter()
            .enumerate()
            .map(|(i, _)| if graphable && i == 0 { Some(String::new()) } else { None })
            .collect()
    };
    // Byte offset of each line start in `display`.
    let mut line_starts = Vec::with_capacity(lines.len());
    let mut off = 0usize;
    for (i, line) in lines.iter().enumerate() {
        line_starts.push(off);
        off += line.len();
        if i + 1 < lines.len() {
            off += 1; // '\n'
        }
    }

    // Lines that contain any match (for row highlight).
    let mut matched_lines = vec![false; lines.len()];
    for m in &matches {
        if m.line < matched_lines.len() {
            matched_lines[m.line] = true;
        }
    }

    egui::ScrollArea::vertical()
        .id_salt(id_salt)
        .max_height(height)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.set_max_width(ui.available_width());
            for (li, line) in lines.iter().enumerate() {
                let is_current = current_line == Some(li);
                let fill = if is_current {
                    match_current_fill()
                } else if matched_lines[li] {
                    match_line_fill()
                } else {
                    Color32::TRANSPARENT
                };

                let line_start = line_starts[li];
                // Match spans on this line, relative to line start.
                let line_hits: Vec<(usize, usize)> = matches
                    .iter()
                    .filter(|m| m.line == li)
                    .map(|m| (m.start - line_start, m.end - line_start))
                    .collect();
                let current_hit = matches.get(search.current).and_then(|m| {
                    if m.line == li {
                        Some((m.start - line_start, m.end - line_start))
                    } else {
                        None
                    }
                });

                let job = if is_json {
                    json_view::line_job_with_search(line, font_size, &line_hits, current_hit)
                } else {
                    json_view::plain_line_job_with_search(line, font_size, &line_hits, current_hit)
                };

                let graph_path = graph_paths.get(li).and_then(|p| p.as_ref());
                let resp = egui::Frame::NONE
                    .fill(fill)
                    .inner_margin(egui::Margin::symmetric(2, 0))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 2.0;
                            // Far-left graph pin for numeric / boolean leaves.
                            if let Some(path) = graph_path {
                                let path_opt = if path.is_empty() {
                                    None
                                } else {
                                    Some(path.as_str())
                                };
                                let sticky_topic = value_path::sticky_id(topic, path_opt);
                                let tip = if let Some(p) = path_opt {
                                    format!("Sticky Graph: {topic} · {p}")
                                } else {
                                    format!("Sticky Graph: {topic}")
                                };
                                if icons.tiny(ui, "line_graph", false, &tip).clicked() {
                                    let current = if let Some(p) = path_opt {
                                        value_path::extract_path_value(raw_payload, p)
                                            .unwrap_or_else(|| line.trim().trim_end_matches(',').to_string())
                                    } else {
                                        display.trim().to_string()
                                    };
                                    pins.pin_graph(
                                        profile_id,
                                        profile_name,
                                        &sticky_topic,
                                        &current,
                                        Instant::now(),
                                    );
                                    profiles.set_sticky_and_save(profile_id, &sticky_topic, true);
                                    profiles.set_sticky_view_mode_and_save(
                                        profile_id,
                                        &sticky_topic,
                                        StickyViewMode::Graph,
                                    );
                                    *open_stickies = true;
                                    *status = format!("Sticky Graph · {sticky_topic}");
                                }
                            }
                            ui.add(
                                egui::Label::new(job)
                                    .wrap()
                                    .selectable(true)
                                    .halign(Align::LEFT),
                            );
                        });
                    })
                    .response;

                if do_scroll && is_current {
                    resp.scroll_to_me(Some(Align::Center));
                }
            }
        });

    if do_scroll {
        search.scroll_to_current = false;
    }
}

/// Case-insensitive substring matches; each hit records its line + byte range.
fn find_matches(text: &str, query: &str) -> Vec<MatchHit> {
    let q = query.trim();
    if q.is_empty() {
        return Vec::new();
    }
    let lower_text = text.to_lowercase();
    let lower_q = q.to_lowercase();
    let q_len = lower_q.len();
    if q_len == 0 {
        return Vec::new();
    }

    let mut out = Vec::new();
    let mut search_from = 0usize;
    while let Some(rel) = lower_text[search_from..].find(&lower_q) {
        let start = search_from + rel;
        let end = start + q_len;
        // Line index = number of '\n' before start.
        let line = text[..start].bytes().filter(|&b| b == b'\n').count();
        out.push(MatchHit { line, start, end });
        search_from = start + q_len.max(1);
        if search_from >= lower_text.len() {
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_case_insensitive() {
        let t = "{\n  \"Name\": \"Citric Acid\"\n}";
        let m = find_matches(t, "citric");
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].line, 1);
        assert!(t[m[0].start..m[0].end].eq_ignore_ascii_case("citric"));
    }

    #[test]
    fn multiple_on_two_lines() {
        let t = "foo\nbar foo\nfoo";
        let m = find_matches(t, "foo");
        assert_eq!(m.len(), 3);
        assert_eq!(m[0].line, 0);
        assert_eq!(m[1].line, 1);
        assert_eq!(m[2].line, 2);
    }
}
