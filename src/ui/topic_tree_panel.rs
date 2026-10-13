//! MQTT Explorer–style browsable topic hierarchy + sticky/clipboard actions.

use crate::mqtt::pins::PinBoard;
use crate::mqtt::session::Session;
use crate::mqtt::settings::TopicTreeExpandMode;
use crate::mqtt::topic_tree::TopicNode;
use crate::ui::clipboard;
use crate::ui::flash;
use crate::ui::search_bar::SearchFilter;
use eframe::egui::{self, Color32, RichText};
use std::time::Instant;

pub fn draw(
    ui: &mut egui::Ui,
    session: Option<&Session>,
    pins: &mut PinBoard,
    expand_mode: TopicTreeExpandMode,
    expand_all_once: &mut Option<bool>,
    filter: &SearchFilter,
    status: &mut String,
) {
    ui.horizontal(|ui| {
        ui.heading("Topic tree");
        if ui.small_button("Expand all").clicked() {
            *expand_all_once = Some(true);
        }
        if ui.small_button("Collapse all").clicked() {
            *expand_all_once = Some(false);
        }
    });
    ui.label("Hierarchy from subscribed traffic — expand and mark topics sticky.");
    ui.add_space(4.0);

    let Some(session) = session else {
        ui.weak("Connect a profile and subscribe to see topics.");
        return;
    };

    if session.topic_tree.root.children.is_empty() {
        ui.weak("No traffic yet on this profile.");
        return;
    }

    let force_open = expand_all_once.take();
    let now = Instant::now();
    let profile_id = session.profile_id.clone();
    let profile_name = session.profile_name.clone();
    draw_node(
        ui,
        &session.topic_tree.root,
        String::new(),
        &profile_id,
        &profile_name,
        pins,
        now,
        expand_mode,
        force_open,
        0,
        filter,
        status,
    );
}

fn node_matches(node: &TopicNode, path: &str, filter: &SearchFilter) -> bool {
    if !filter.is_active() {
        return true;
    }
    if filter.matches(path, node.last_payload.as_deref()) {
        return true;
    }
    for (seg, child) in &node.children {
        let child_path = if path.is_empty() {
            seg.clone()
        } else {
            format!("{path}/{seg}")
        };
        if node_matches(child, &child_path, filter) {
            return true;
        }
    }
    false
}

#[allow(clippy::too_many_arguments)]
fn draw_node(
    ui: &mut egui::Ui,
    node: &TopicNode,
    path: String,
    profile_id: &str,
    profile_name: &str,
    pins: &mut PinBoard,
    now: Instant,
    expand_mode: TopicTreeExpandMode,
    force_open: Option<bool>,
    depth: usize,
    filter: &SearchFilter,
    status: &mut String,
) {
    for (segment, child) in &node.children {
        let child_path = if path.is_empty() {
            segment.clone()
        } else {
            format!("{path}/{segment}")
        };
        if !node_matches(child, &child_path, filter) {
            continue;
        }
        let has_kids = !child.children.is_empty();
        let is_leaf_msg = child.last_payload.is_some();
        let fill = flash::flash_fill(child.flash_until, now).unwrap_or(Color32::TRANSPARENT);

        let default_open = match force_open {
            Some(v) => v,
            None => match expand_mode {
                TopicTreeExpandMode::ExpandAll => true,
                TopicTreeExpandMode::CollapseAll => false,
                TopicTreeExpandMode::ExpandRoot => depth == 0,
            },
        };

        egui::Frame::NONE.fill(fill).show(ui, |ui| {
            if has_kids {
                egui::CollapsingHeader::new(RichText::new(segment).strong())
                    .id_salt(format!("tt-{profile_id}-{child_path}"))
                    .default_open(default_open)
                    .show(ui, |ui| {
                        if is_leaf_msg {
                            leaf_row(ui, &child_path, child, profile_id, profile_name, pins, status);
                        }
                        draw_node(
                            ui,
                            child,
                            child_path.clone(),
                            profile_id,
                            profile_name,
                            pins,
                            now,
                            expand_mode,
                            force_open,
                            depth + 1,
                            filter,
                            status,
                        );
                    });
            } else {
                leaf_row(ui, &child_path, child, profile_id, profile_name, pins, status);
            }
        });
    }
}

fn leaf_row(
    ui: &mut egui::Ui,
    topic: &str,
    node: &TopicNode,
    profile_id: &str,
    profile_name: &str,
    pins: &mut PinBoard,
    status: &mut String,
) {
    ui.horizontal(|ui| {
        clipboard::topic_label(ui, topic, status);
        if let Some(payload) = &node.last_payload {
            clipboard::payload_with_copy(ui, payload, 48, status);
        }
        if node.message_count > 0 {
            ui.label(RichText::new(format!("#{}", node.message_count)).small());
        }
        let sticky = pins.is_sticky(profile_id, topic);
        let label = if sticky { "Unstick" } else { "Sticky" };
        if ui.small_button(label).clicked() {
            if sticky {
                pins.unpin(profile_id, topic);
            } else {
                pins.pin(profile_id, profile_name, topic);
                if let Some(payload) = &node.last_payload {
                    if let Some(s) = pins.items.get_mut(&PinBoard::key(profile_id, topic)) {
                        s.last_payload = payload.clone();
                        s.message_count = node.message_count;
                    }
                }
            }
        }
    });
}
