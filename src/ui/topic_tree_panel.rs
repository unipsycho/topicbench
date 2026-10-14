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
use crate::ui::shortcuts;
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
        // Flash = border only so gray topic / highlighted payload stay readable.
        let flash_stroke = flash::flash_stroke(child.flash_until, now)
            .unwrap_or(egui::Stroke::NONE);

        let default_open = match force_open {
            Some(v) => v,
            None => match expand_mode {
                TopicTreeExpandMode::ExpandAll => true,
                TopicTreeExpandMode::CollapseAll => false,
                TopicTreeExpandMode::ExpandRoot => depth == 0,
            },
        };

        egui::Frame::NONE.stroke(flash_stroke).show(ui, |ui| {
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
                        // Sense click so double-click on the topic text toggles expand.
                        ui.add(
                            egui::Label::new(RichText::new(segment).strong())
                                .sense(egui::Sense::click()),
                        );
                        if !was_open && subtree > 0 {
                            ui.add(
                                egui::Label::new(
                                    RichText::new(format!("#{subtree}")).small().weak(),
                                )
                                .sense(egui::Sense::click()),
                            );
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
                                Some(ToggleParent {
                                    id,
                                    was_open,
                                    profile_id,
                                    path: &child_path,
                                    node: child,
                                }),
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

                // Double-click topic row (header text or ▶) → toggle this node + subtree.
                if header_resp.response.double_clicked() || toggle_resp.double_clicked() {
                    apply_tree_toggle(ui.ctx(), id, !was_open, profile_id, &child_path, child);
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
                    None,
                );
            }
        });
    }
}

/// Parent node to toggle when its own payload row is double-clicked.
struct ToggleParent<'a> {
    id: egui::Id,
    was_open: bool,
    profile_id: &'a str,
    path: &'a str,
    node: &'a TopicNode,
}

fn apply_tree_toggle(
    ctx: &egui::Context,
    id: egui::Id,
    new_open: bool,
    profile_id: &str,
    path: &str,
    node: &TopicNode,
) {
    let mut st =
        egui::collapsing_header::CollapsingState::load_with_default_open(ctx, id, new_open);
    st.set_open(new_open);
    st.store(ctx);
    set_subtree_open(ctx, profile_id, path, node, new_open);
    ctx.request_repaint();
}

/// Dense row: `[copy topic][sticky] gray_topic = [copy payload] highlighted_payload… #n`
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
    toggle_parent: Option<ToggleParent<'_>>,
) {
    let mut dbl = false;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;

        // Left: copy-topic (topic_tags) + sticky toggle (always visible).
        if icons
            .tiny(ui, "topic_tags", false, "Copy topic")
            .clicked()
        {
            ui.ctx().copy_text(full_topic.to_string());
            *status = format!("Copied topic: {full_topic}");
        }

        let sticky = pins.is_sticky(profile_id, full_topic);
        let sticky_tip = shortcuts::tip(
            if sticky {
                "Remove sticky"
            } else {
                "Pin sticky"
            },
            "Ctrl+Shift+S",
        );
        if icons
            .tiny(ui, "sticky_topic", sticky, &sticky_tip)
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
        let show_copy_payload = !payload.is_empty();
        let count_reserve = if node.message_count > 0 { 28.0 } else { 0.0 };
        let width = (ui.available_width() - count_reserve).max(48.0);

        // Middle: gray topic · = · copy-payload · highlighted data (clip like icons).
        ui.allocate_ui_with_layout(
            egui::vec2(width, ui.spacing().interact_size.y),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.set_clip_rect(ui.clip_rect().intersect(ui.max_rect()));
                ui.spacing_mut().item_spacing.x = 2.0;

                let topic_resp = ui.add(
                    egui::Label::new(
                        RichText::new(label)
                            .monospace()
                            .color(json_view::C_TOPIC),
                    )
                    .sense(egui::Sense::click()),
                );
                if topic_resp.double_clicked() {
                    dbl = true;
                }
                if topic_resp.clicked() {
                    pins.last_tree_focus = Some((
                        profile_id.to_string(),
                        profile_name.to_string(),
                        full_topic.to_string(),
                    ));
                }
                // Full path hover — stays open under wheel; scrolls when tall.
                scrollable_hover(ui, &topic_resp, egui::Id::new(("topic-tip", profile_id, full_topic)), |ui| {
                    ui.label(RichText::new(full_topic).monospace());
                });

                if show_copy_payload {
                    ui.label(RichText::new("=").monospace().color(json_view::C_TOPIC));
                    if icons
                        .tiny(ui, "clipboard_checklist", false, "Copy payload")
                        .clicked()
                    {
                        let copy = json_view::try_pretty_json(payload)
                            .unwrap_or_else(|| payload.to_string());
                        ui.ctx().copy_text(copy);
                        *status = "Copied payload".into();
                    }

                    // Complex JSON → syntax highlight; simple values → profile accent.
                    let accent = profiles
                        .profiles
                        .get(profile_id)
                        .map(|p| p.color())
                        .unwrap_or(Color32::from_rgb(90, 190, 120));
                    let used = label.len() as f32 * 7.0 + 36.0;
                    let max_chars = ((ui.available_width() - used).max(40.0) / 7.0) as usize;
                    let max_chars = max_chars.max(12);
                    let is_complex = json_view::try_pretty_json(payload).is_some();
                    let data_resp = if is_complex {
                        let font_size = ui.text_style_height(&egui::TextStyle::Monospace);
                        let job = json_view::payload_preview_job(payload, max_chars, font_size);
                        ui.add(egui::Label::new(job).sense(egui::Sense::click()))
                    } else {
                        let line = json_view::preview_line(payload, max_chars);
                        ui.add(
                            egui::Label::new(RichText::new(line).monospace().color(accent))
                                .sense(egui::Sense::click()),
                        )
                    };
                    if data_resp.double_clicked() {
                        dbl = true;
                    }
                    if data_resp.clicked() {
                        pins.last_tree_focus = Some((
                            profile_id.to_string(),
                            profile_name.to_string(),
                            full_topic.to_string(),
                        ));
                    }
                    scrollable_hover(
                        ui,
                        &data_resp,
                        egui::Id::new(("tree-tip", profile_id, full_topic)),
                        |ui| {
                            json_view::hover_payload_body(ui, payload);
                        },
                    );
                }
            },
        );

        if node.message_count > 0 {
            ui.label(RichText::new(format!("#{}", node.message_count)).small().weak());
        }
    });

    // Double-click payload row of a branch node → same expand/collapse as its header.
    if dbl {
        if let Some(t) = toggle_parent {
            apply_tree_toggle(ui.ctx(), t.id, !t.was_open, t.profile_id, t.path, t.node);
        }
    }
}

/// One exclusive hover popup for the topic tree.
/// Stays open under wheel; wheel scrolls popup content; moving a few pixels onto
/// the popup (or the gap) does not open tips for rows underneath.
fn scrollable_hover(
    ui: &egui::Ui,
    resp: &egui::Response,
    id: egui::Id,
    add_contents: impl FnOnce(&mut egui::Ui),
) {
    let ctx = ui.ctx();
    let popup_id = id.with("scroll_hover");
    let active_key = egui::Id::new("topic_tree_hover_active");
    let scroll_key = popup_id.with("scroll_y");

    let pointer = ctx
        .pointer_interact_pos()
        .or_else(|| ctx.pointer_hover_pos());
    let popup_rect = egui::AreaState::load(ctx, popup_id).map(|a| a.rect());

    let over_anchor = resp.hovered()
        || pointer.is_some_and(|p| resp.rect.expand2(egui::vec2(2.0, 2.0)).contains(p));
    let over_popup =
        pointer.zip(popup_rect).is_some_and(|(p, r)| r.expand(6.0).contains(p));
    // Bridge the 4px gap between anchor and popup so a small move doesn't dismiss.
    let over_bridge = pointer.zip(popup_rect).is_some_and(|(p, pr)| {
        let left = resp.rect.left().min(pr.left()) - 4.0;
        let right = resp.rect.right().max(pr.right()) + 4.0;
        let top = resp.rect.bottom().min(pr.top()) - 2.0;
        let bottom = resp.rect.bottom().max(pr.top()) + 2.0;
        egui::Rect::from_min_max(egui::pos2(left, top), egui::pos2(right, bottom)).contains(p)
    });

    let active: Option<egui::Id> = ctx.data(|d| d.get_temp(active_key));
    let is_ours = active == Some(popup_id);

    // Click outside the sticky zone (popup / anchor / gap) dismisses.
    let click_outside = is_ours
        && ctx.input(|i| i.pointer.primary_clicked())
        && pointer.is_some_and(|p| {
            let in_anchor = resp.rect.expand2(egui::vec2(2.0, 2.0)).contains(p);
            let in_popup = popup_rect.is_some_and(|r| r.expand(2.0).contains(p));
            let in_bridge = popup_rect.is_some_and(|pr| {
                let left = resp.rect.left().min(pr.left()) - 4.0;
                let right = resp.rect.right().max(pr.right()) + 4.0;
                let top = resp.rect.bottom().min(pr.top()) - 2.0;
                let bottom = resp.rect.bottom().max(pr.top()) + 2.0;
                egui::Rect::from_min_max(egui::pos2(left, top), egui::pos2(right, bottom))
                    .contains(p)
            });
            !(in_anchor || in_popup || in_bridge)
        });

    // Only one popup: while another is active, ignore hover on other rows.
    let open = if click_outside {
        false
    } else if is_ours {
        over_anchor || over_popup || over_bridge
    } else if active.is_none() && over_anchor {
        true
    } else {
        false
    };

    if open {
        ctx.data_mut(|d| d.insert_temp(active_key, popup_id));
    } else if is_ours || click_outside {
        ctx.data_mut(|d| {
            d.remove::<egui::Id>(active_key);
            d.remove_temp::<f32>(scroll_key);
        });
        return;
    } else {
        return;
    }

    let mut pos = resp.rect.left_bottom() + egui::vec2(0.0, 4.0);
    if let Some(to_global) = ctx.layer_transform_to_global(resp.layer_id) {
        pos = to_global * pos;
    }

    // Steal wheel from the parent tree ScrollArea; drive our own offset.
    let mut scroll_y = ctx.data(|d| d.get_temp::<f32>(scroll_key).unwrap_or(0.0));
    let delta = ctx.input_mut(|i| {
        let d = i.smooth_scroll_delta;
        if d != egui::Vec2::ZERO {
            i.smooth_scroll_delta = egui::Vec2::ZERO;
        }
        d
    });
    // smooth_scroll_delta.y > 0 → scroll up → smaller offset.
    scroll_y = (scroll_y - delta.y).max(0.0);

    let sa_out = egui::Area::new(popup_id)
        .kind(egui::UiKind::Popup)
        .order(egui::Order::Foreground)
        .fixed_pos(pos)
        .constrain(true)
        .sense(egui::Sense::click_and_drag())
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_max_width(420.0);
                egui::ScrollArea::vertical()
                    .id_salt(popup_id.with("sa"))
                    .max_height(280.0)
                    .auto_shrink([false, true])
                    .vertical_scroll_offset(scroll_y)
                    .show(ui, |ui| {
                        add_contents(ui);
                    })
            })
        });

    let new_y = sa_out
        .inner
        .inner
        .state
        .offset
        .y;
    ctx.data_mut(|d| d.insert_temp(scroll_key, new_y));
    // Keep animating while the user is scrolling the popup.
    if delta != egui::Vec2::ZERO {
        ctx.request_repaint();
    }
}
