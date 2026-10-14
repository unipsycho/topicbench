//! Topic hierarchy grouped per connected broker/profile.

use crate::mqtt::pins::PinBoard;
use crate::mqtt::profile::ProfileStore;
use crate::mqtt::session::{Session, SessionManager, SessionState};
use crate::mqtt::settings::TopicTreeExpandMode;
use crate::mqtt::topic_tree::TopicNode;
use crate::ui::flash;
use crate::ui::icons::IconSet;
use crate::ui::search_bar::SearchFilter;
use eframe::egui::{self, Color32, RichText};
use std::time::Instant;

/// Distinct heading colors per broker index (not used for topic rows).
const BROKER_COLORS: [Color32; 6] = [
    Color32::from_rgb(70, 140, 210),
    Color32::from_rgb(200, 120, 60),
    Color32::from_rgb(90, 170, 110),
    Color32::from_rgb(170, 100, 190),
    Color32::from_rgb(60, 170, 180),
    Color32::from_rgb(210, 90, 110),
];

pub fn draw(
    ui: &mut egui::Ui,
    sessions: &SessionManager,
    profiles: &mut ProfileStore,
    pins: &mut PinBoard,
    expand_mode: TopicTreeExpandMode,
    expand_all_once: &mut Option<bool>,
    icons: &IconSet,
    filter: &SearchFilter,
    status: &mut String,
) {
    ui.horizontal(|ui| {
        if let Some(tex) = icons.get("topic_tree") {
            ui.add(
                egui::Image::new(tex)
                    .fit_to_exact_size(egui::vec2(20.0, 20.0))
                    .sense(egui::Sense::hover()),
            );
        }
        ui.heading("Topic tree");
        if ui.small_button("Expand all").clicked() {
            *expand_all_once = Some(true);
        }
        if ui.small_button("Collapse all").clicked() {
            *expand_all_once = Some(false);
        }
    });
    ui.label("Per connected broker — colored heading, then that broker’s topics.");
    ui.add_space(4.0);

    let connected: Vec<&Session> = sessions
        .open
        .values()
        .filter(|s| s.state == SessionState::Connected || !s.topic_tree.root.children.is_empty())
        .collect();

    if connected.is_empty() {
        ui.weak("Connect a profile and subscribe to see topics.");
        return;
    }

    let force_open = expand_all_once.take();
    let now = Instant::now();

    for (idx, session) in connected.iter().enumerate() {
        let color = BROKER_COLORS[idx % BROKER_COLORS.len()];
        ui.add_space(6.0);
        ui.label(
            RichText::new(format!("▸ {}", session.profile_name))
                .heading()
                .color(color)
                .strong(),
        );
        ui.label(
            RichText::new(format!(
                "{} · {}",
                session.state.label(),
                session.profile_id
            ))
            .small()
            .color(color),
        );

        if session.topic_tree.root.children.is_empty() {
            ui.weak("  No traffic yet on this broker.");
            continue;
        }

        draw_node(
            ui,
            &session.topic_tree.root,
            String::new(),
            &session.profile_id,
            &session.profile_name,
            profiles,
            pins,
            now,
            expand_mode,
            force_open,
            0,
            icons,
            filter,
            status,
        );
    }
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
    profiles: &mut ProfileStore,
    pins: &mut PinBoard,
    now: Instant,
    expand_mode: TopicTreeExpandMode,
    force_open: Option<bool>,
    depth: usize,
    icons: &IconSet,
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
                // CollapsingState so we can show subtree totals while collapsed.
                let id = ui.make_persistent_id(format!("tt-{profile_id}-{child_path}"));
                let mut state =
                    egui::collapsing_header::CollapsingState::load_with_default_open(
                        ui.ctx(),
                        id,
                        default_open,
                    );
                if let Some(v) = force_open {
                    state.set_open(v);
                }
                let open = state.is_open();
                let subtree = child.subtree_message_count();

                state
                    .show_header(ui, |ui| {
                        ui.label(RichText::new(segment).strong());
                        if !open && subtree > 0 {
                            ui.label(RichText::new(format!("#{subtree}")).small().weak());
                        }
                    })
                    .body(|ui| {
                        if is_leaf_msg {
                            leaf_row(
                                ui,
                                segment,
                                &child_path,
                                child,
                                profile_id,
                                profile_name,
                                profiles,
                                pins,
                                icons,
                                status,
                            );
                        }
                        draw_node(
                            ui,
                            child,
                            child_path.clone(),
                            profile_id,
                            profile_name,
                            profiles,
                            pins,
                            now,
                            expand_mode,
                            force_open,
                            depth + 1,
                            icons,
                            filter,
                            status,
                        );
                    });
            } else {
                leaf_row(
                    ui,
                    segment,
                    &child_path,
                    child,
                    profile_id,
                    profile_name,
                    profiles,
                    pins,
                    icons,
                    status,
                );
            }
        });
    }
}

/// Dense row: `[copy topic] topic = payload… [copy payload] [sticky] #n`
fn leaf_row(
    ui: &mut egui::Ui,
    label: &str,
    full_topic: &str,
    node: &TopicNode,
    profile_id: &str,
    profile_name: &str,
    profiles: &mut ProfileStore,
    pins: &mut PinBoard,
    icons: &IconSet,
    status: &mut String,
) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;

        // Left: copy topic
        if icons
            .tiny(ui, "clipboard_checklist", false, "Copy topic")
            .clicked()
        {
            ui.ctx().copy_text(full_topic.to_string());
            *status = format!("Copied topic: {full_topic}");
        }

        let payload = node.last_payload.as_deref().unwrap_or("");
        let line = if payload.is_empty() {
            label.to_string()
        } else {
            format!("{label} = {payload}")
        };
        let tip = if payload.is_empty() {
            full_topic.to_string()
        } else {
            payload.to_string()
        };

        // Middle: topic = payload fills remaining width (truncated); tooltip = full payload.
        let right_reserve = 56.0;
        let width = (ui.available_width() - right_reserve).max(48.0);
        ui.allocate_ui_with_layout(
            egui::vec2(width, ui.spacing().interact_size.y),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.add(egui::Label::new(RichText::new(line).monospace()).truncate())
                    .on_hover_text(tip);
            },
        );

        // Right: copy payload + sticky
        if !payload.is_empty()
            && icons
                .tiny(ui, "clipboard_checklist", false, "Copy payload")
                .clicked()
        {
            ui.ctx().copy_text(payload.to_string());
            *status = "Copied payload".into();
        }

        let sticky = pins.is_sticky(profile_id, full_topic);
        if icons
            .tiny(
                ui,
                "sticky_topic",
                sticky,
                if sticky {
                    "Remove sticky"
                } else {
                    "Pin sticky"
                },
            )
            .clicked()
        {
            if sticky {
                pins.unpin(profile_id, full_topic);
                profiles.set_sticky_and_save(profile_id, full_topic, false);
            } else {
                pins.pin(profile_id, profile_name, full_topic);
                profiles.set_sticky_and_save(profile_id, full_topic, true);
                if let Some(payload) = &node.last_payload {
                    if let Some(s) = pins.items.get_mut(&PinBoard::key(profile_id, full_topic)) {
                        s.last_payload = payload.clone();
                        s.message_count = node.message_count;
                    }
                }
            }
        }

        if node.message_count > 0 {
            ui.label(RichText::new(format!("#{}", node.message_count)).small().weak());
        }
    });
}
