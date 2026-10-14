//! Topic hierarchy grouped per connected broker/profile.

use crate::mqtt::pins::PinBoard;
use crate::mqtt::profile::ProfileStore;
use crate::mqtt::session::{Session, SessionManager, SessionState};
use crate::mqtt::settings::TopicTreeExpandMode;
use crate::mqtt::topic_tree::TopicNode;
use crate::ui::flash;
use crate::ui::icons::IconSet;
use crate::ui::json_view;
use crate::ui::search_bar::SearchFilter;
use eframe::egui::{self, Color32, RichText};
use std::time::Instant;


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
    ui.weak("Double-click a parent to expand/collapse its whole subtree.");
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

    for session in &connected {
        // Profile accent color (user-settable); fallback green if missing.
        let color = profiles
            .profiles
            .get(&session.profile_id)
            .map(|p| p.color())
            .unwrap_or(Color32::from_rgb(90, 190, 120));
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

/// Stable id for a tree collapsing node (independent of UI nesting).
fn tree_id(profile_id: &str, path: &str) -> egui::Id {
    egui::Id::new(("topic-tree", profile_id, path))
}

/// Recursively set open/closed for every collapsing descendant under `node`.
fn set_subtree_open(ctx: &egui::Context, profile_id: &str, path: &str, node: &TopicNode, open: bool) {
    for (seg, child) in &node.children {
        let child_path = if path.is_empty() {
            seg.clone()
        } else {
            format!("{path}/{seg}")
        };
        if !child.children.is_empty() {
            let id = tree_id(profile_id, &child_path);
            let mut state =
                egui::collapsing_header::CollapsingState::load_with_default_open(ctx, id, open);
            state.set_open(open);
            state.store(ctx);
            set_subtree_open(ctx, profile_id, &child_path, child, open);
        }
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
                let id = tree_id(profile_id, &child_path);
                let mut state =
                    egui::collapsing_header::CollapsingState::load_with_default_open(
                        ui.ctx(),
                        id,
                        default_open,
                    );
                if let Some(v) = force_open {
                    state.set_open(v);
                }
                let was_open = state.is_open();
                let subtree = child.subtree_message_count();

                let (toggle_resp, header_resp, _) = state
                    .show_header(ui, |ui| {
                        ui.label(RichText::new(segment).strong());
                        if !was_open && subtree > 0 {
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

                // Double-click parent → toggle this node + entire subtree under it.
                if header_resp.response.double_clicked() || toggle_resp.double_clicked() {
                    let new_open = !was_open;
                    let mut st =
                        egui::collapsing_header::CollapsingState::load_with_default_open(
                            ui.ctx(),
                            id,
                            new_open,
                        );
                    st.set_open(new_open);
                    st.store(ui.ctx());
                    set_subtree_open(ui.ctx(), profile_id, &child_path, child, new_open);
                    ui.ctx().request_repaint();
                }
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

/// Dense row: `[copy topic][sticky] topic = payload… #n [copy payload]`
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

        // Left: copy-topic + sticky toggle (always visible).
        if icons
            .tiny(ui, "clipboard_checklist", false, "Copy topic")
            .clicked()
        {
            ui.ctx().copy_text(full_topic.to_string());
            *status = format!("Copied topic: {full_topic}");
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
            pins.last_tree_focus = Some((
                profile_id.to_string(),
                profile_name.to_string(),
                full_topic.to_string(),
            ));
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

        let payload = node.last_payload.as_deref().unwrap_or("");
        let is_json = json_view::try_pretty_json(payload).is_some();
        let show_copy_payload = !payload.is_empty();
        // Reserve right chrome only — not flush to the window edge.
        let right_reserve = if show_copy_payload { 28.0 } else { 0.0 };
        let count_reserve = if node.message_count > 0 { 28.0 } else { 0.0 };
        let width = (ui.available_width() - right_reserve - count_reserve).max(48.0);

        // Middle: topic = payload (as much as fits).
        ui.allocate_ui_with_layout(
            egui::vec2(width, ui.spacing().interact_size.y),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.set_clip_rect(ui.max_rect());
                let resp = if payload.is_empty() {
                    ui.add(
                        egui::Label::new(RichText::new(label).monospace())
                            .sense(egui::Sense::click()),
                    )
                } else if is_json {
                    let max_json = ((width / 7.0) as usize)
                        .saturating_sub(label.len() + 3)
                        .max(24);
                    let font_size = ui.text_style_height(&egui::TextStyle::Monospace);
                    let job = json_view::row_preview_job(label, payload, max_json, font_size);
                    ui.add(egui::Label::new(job).sense(egui::Sense::click()))
                } else {
                    let line = format!("{label} = {payload}");
                    ui.add(
                        egui::Label::new(RichText::new(line).monospace())
                            .truncate()
                            .sense(egui::Sense::click()),
                    )
                };
                if resp.clicked() {
                    pins.last_tree_focus = Some((
                        profile_id.to_string(),
                        profile_name.to_string(),
                        full_topic.to_string(),
                    ));
                }
                if !payload.is_empty() {
                    resp.on_hover_ui(|ui| {
                        ui.set_max_width(420.0);
                        json_view::hover_payload(
                            ui,
                            payload,
                            ("tree-tip", profile_id, full_topic),
                        );
                    });
                } else {
                    resp.on_hover_text(full_topic);
                }
            },
        );

        if node.message_count > 0 {
            ui.label(RichText::new(format!("#{}", node.message_count)).small().weak());
        }

        // Right: copy-payload (after middle; not forced to window edge).
        if show_copy_payload
            && icons
                .tiny(ui, "clipboard_checklist", false, "Copy payload")
                .clicked()
        {
            let copy = json_view::try_pretty_json(payload).unwrap_or_else(|| payload.to_string());
            ui.ctx().copy_text(copy);
            *status = "Copied payload".into();
        }
    });
}
