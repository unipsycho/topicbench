//! Topic hierarchy grouped per connected broker/profile.
//! Per-broker trees can pop out to their own OS window (egui viewport).

use crate::mqtt::pins::PinBoard;
use crate::mqtt::profile::ProfileStore;
use crate::mqtt::publish::PublishFromTree;
use crate::mqtt::session::{SessionManager, SessionState};
use crate::mqtt::settings::TopicTreeExpandMode;
use crate::mqtt::topic_tree::TopicNode;
use crate::ui::flash;
use crate::ui::hover_tip;
use crate::ui::icons::{self, IconSet, CHROME_MUTED};
use crate::ui::json_view;
use crate::ui::search_bar::SearchFilter;
use crate::ui::shortcuts;
use eframe::egui::{self, Align, Color32, FontId, Layout, RichText, ViewportBuilder, ViewportClass};
use std::collections::HashSet;
use std::time::{Duration, Instant};

/// Stay on the same row this long before a payload hover popup opens.
const TREE_HOVER_DELAY: Duration = Duration::from_millis(500);

/// Exclusive temp key for the one open tree payload/topic tip.
fn hover_active_key() -> egui::Id {
    egui::Id::new("topic_tree_hover_active")
}

/// Drop any open tree tip (call when Settings / Shortcuts modal opens).
pub fn dismiss_hover_tips(ctx: &egui::Context) {
    ctx.data_mut(|d| {
        d.remove::<egui::Id>(hover_active_key());
    });
}

/// Soft fill behind the selected tree row (Payload panel target).
fn select_fill() -> Color32 {
    Color32::from_rgba_unmultiplied(56, 100, 160, 70)
}

pub fn draw(
    ui: &mut egui::Ui,
    sessions: &mut SessionManager,
    profiles: &mut ProfileStore,
    pins: &mut PinBoard,
    expand_mode: TopicTreeExpandMode,
    icons: &IconSet,
    filter: &mut SearchFilter,
    focus_search: &mut bool,
    status: &mut String,
    // Set when a leaf/payload row is double-clicked → open Publish and fill form.
    publish_from_tree: &mut Option<PublishFromTree>,
    // Profile ids whose topic tree is shown in a floating OS window.
    undocked: &mut HashSet<String>,
) {
    icons::section_header_bar(ui, |ui| {
        icons::section_icon(ui, icons, "topic_tree");
        icons::section_title(ui, "Topic tree");
    });
    // Topic filter lives under the section header, above broker/tree rows.
    crate::ui::search_bar::draw(ui, filter, focus_search);
    ui.add_space(4.0);
    ui.weak("Single-click selects for Payload. Double-click a parent to expand/collapse. Double-click a topic/payload row to fill Publish.");
    ui.add_space(4.0);

    // Only currently connected brokers — disconnected sessions are cleared on Disconnect.
    // Broker roots follow Profiles list order (profiles.json array / ProfileStore.order).
    let connected_ids: Vec<String> = connected_profile_ids(sessions, profiles);
    // Drop undocked entries for brokers that are no longer connected.
    undocked.retain(|id| connected_ids.iter().any(|c| c == id));

    if connected_ids.is_empty() {
        ui.weak("Connect a profile and subscribe to see topics.");
        return;
    }

    let now = Instant::now();

    for profile_id in connected_ids {
        draw_broker_block(
            ui,
            &profile_id,
            sessions,
            profiles,
            pins,
            expand_mode,
            icons,
            filter,
            status,
            publish_from_tree,
            undocked,
            now,
            false, // docked panel: undock button pops out
        );
    }
}

/// Floating OS windows for each undocked broker tree (call every frame while any are open).
pub fn draw_undocked_viewports(
    ctx: &egui::Context,
    sessions: &mut SessionManager,
    profiles: &mut ProfileStore,
    pins: &mut PinBoard,
    expand_mode: TopicTreeExpandMode,
    icons: &IconSet,
    filter: &mut SearchFilter,
    status: &mut String,
    publish_from_tree: &mut Option<PublishFromTree>,
    undocked: &mut HashSet<String>,
) {
    let connected = connected_profile_ids(sessions, profiles);
    undocked.retain(|id| connected.iter().any(|c| c == id));
    let ids: Vec<String> = undocked.iter().cloned().collect();
    let now = Instant::now();

    for profile_id in ids {
        let profile_name = sessions
            .open
            .get(&profile_id)
            .map(|s| s.profile_name.clone())
            .or_else(|| {
                profiles
                    .profiles
                    .get(&profile_id)
                    .map(|p| p.name.clone())
            })
            .unwrap_or_else(|| profile_id.clone());
        let title = format!("Topic tree — {profile_name}");
        let vp_id = egui::ViewportId::from_hash_of(("topic_tree_pop", profile_id.as_str()));
        let mut dock_requested = false;

        ctx.show_viewport_immediate(
            vp_id,
            ViewportBuilder::default()
                .with_title(title.clone())
                .with_inner_size([440.0, 560.0])
                .with_min_inner_size([280.0, 200.0]),
            |ctx, class| {
                let mut body = |ui: &mut egui::Ui| {
                    draw_broker_block(
                        ui,
                        &profile_id,
                        sessions,
                        profiles,
                        pins,
                        expand_mode,
                        icons,
                        filter,
                        status,
                        publish_from_tree,
                        undocked,
                        now,
                        true, // floating: dock button restores
                    );
                };

                match class {
                    ViewportClass::Embedded => {
                        // Backend cannot spawn real windows — fall back to an egui Window.
                        egui::Window::new(&title)
                            .id(egui::Id::new(("topic_tree_pop_win", profile_id.as_str())))
                            .default_size([440.0, 560.0])
                            .show(ctx, |ui| body(ui));
                    }
                    _ => {
                        egui::CentralPanel::default().show(ctx, |ui| body(ui));
                    }
                }

                if ctx.input(|i| i.viewport().close_requested()) {
                    dock_requested = true;
                }
            },
        );

        if dock_requested {
            undocked.remove(&profile_id);
        }
    }
}

fn connected_profile_ids(sessions: &SessionManager, profiles: &ProfileStore) -> Vec<String> {
    profiles
        .ids_in_order()
        .into_iter()
        .filter(|id| {
            sessions
                .open
                .get(id)
                .is_some_and(|s| s.state == SessionState::Connected)
        })
        .collect()
}

/// One broker heading (+ pop/dock control) and its tree, or a “popped out” stub when undocked in-panel.
fn draw_broker_block(
    ui: &mut egui::Ui,
    profile_id: &str,
    sessions: &mut SessionManager,
    profiles: &mut ProfileStore,
    pins: &mut PinBoard,
    expand_mode: TopicTreeExpandMode,
    icons: &IconSet,
    filter: &SearchFilter,
    status: &mut String,
    publish_from_tree: &mut Option<PublishFromTree>,
    undocked: &mut HashSet<String>,
    now: Instant,
    // True when drawing inside the floating viewport (show dock control).
    in_viewport: bool,
) {
    let Some(session) = sessions.open.get(profile_id) else {
        return;
    };
    let color = profiles
        .profiles
        .get(&session.profile_id)
        .map(|p| p.color())
        .unwrap_or(Color32::from_rgb(90, 190, 120));
    let profile_name = session.profile_name.clone();
    let state_label = session.state.label();
    let tree_empty = session.topic_tree.root.children.is_empty();
    let is_undocked = undocked.contains(profile_id);

    ui.add_space(if in_viewport { 0.0 } else { 6.0 });
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(format!("▸ {profile_name}"))
                .heading()
                .color(color)
                .strong(),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if in_viewport || is_undocked {
                // Dock back into the main Topic tree panel.
                if icons
                    .tiny(ui, "panel_docked", true, "Dock back into main window")
                    .clicked()
                {
                    undocked.remove(profile_id);
                }
            } else if icons
                .tiny(
                    ui,
                    "panel_undock",
                    false,
                    "Pop out to its own window",
                )
                .clicked()
            {
                undocked.insert(profile_id.to_string());
            }
        });
    });
    ui.label(
        RichText::new(format!("{state_label} · {profile_id}"))
            .small()
            .color(color),
    );

    // In the docked panel, hide the tree body while it's floating.
    if !in_viewport && is_undocked {
        ui.weak("  Popped out — dock from this icon or the floating window to restore.");
        return;
    }

    if tree_empty {
        ui.weak("  No traffic yet on this broker.");
        return;
    }

    let Some(session) = sessions.open.get(profile_id) else {
        return;
    };
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
        None, // no toolbar Expand/Collapse — double-click parents instead
        0,
        icons,
        filter,
        status,
        publish_from_tree,
    );
}

/// Stable id for a tree collapsing node (independent of UI nesting).
fn tree_id(profile_id: &str, path: &str) -> egui::Id {
    egui::Id::new(("topic-tree", profile_id, path))
}

fn is_selected(pins: &PinBoard, profile_id: &str, topic: &str) -> bool {
    pins.last_tree_focus
        .as_ref()
        .is_some_and(|(pid, _, t)| pid == profile_id && t == topic)
}

fn select_topic(pins: &mut PinBoard, profile_id: &str, profile_name: &str, topic: &str) {
    pins.last_tree_focus = Some((
        profile_id.to_string(),
        profile_name.to_string(),
        topic.to_string(),
    ));
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
    publish_from_tree: &mut Option<PublishFromTree>,
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
        // Flash = inset border only (never expands row height when pulsing).
        let flash_stroke = flash::flash_stroke(child.flash_until, now);

        // depth 0 = first segment under broker; Expand N opens depths 0..N-1.
        let default_open = match force_open {
            Some(v) => v,
            None => match expand_mode.levels() {
                None => true,
                Some(n) => depth < n,
            },
        };

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
            let selected = is_selected(pins, profile_id, &child_path);

            // Custom header: we own click vs double-click. Do not use show_header —
            // its ▶ button toggles on every clicked(), so a double-click nets zero
            // and header_resp unions the arrow (wrong !was_open path).
            let header_row = ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                let toggle_size =
                    egui::vec2(ui.spacing().indent, ui.spacing().icon_width);
                let (toggle_id, toggle_rect) = ui.allocate_space(toggle_size);
                // Unique id so we don't share interaction with CollapsingState persistence id.
                let toggle_resp =
                    ui.interact(toggle_rect, toggle_id.with("tree_toggle"), egui::Sense::click());
                let openness = state.openness(ui.ctx());
                egui::collapsing_header::paint_default_icon(ui, openness, &toggle_resp);

                ui.spacing_mut().item_spacing.x = 4.0;
                let label_resp = egui::Frame::NONE
                    .fill(if selected {
                        select_fill()
                    } else {
                        Color32::TRANSPARENT
                    })
                    .inner_margin(egui::Margin::symmetric(3, 1))
                    .corner_radius(2.0)
                    .show(ui, |ui| {
                        ui.add(
                            egui::Label::new(RichText::new(segment).strong())
                                .sense(egui::Sense::click())
                                .selectable(false),
                        )
                    })
                    .inner;
                if !was_open && subtree > 0 {
                    ui.add(
                        egui::Label::new(
                            RichText::new(format!("#{subtree}"))
                                .small()
                                .color(CHROME_MUTED),
                        )
                        .sense(egui::Sense::click())
                        .selectable(false),
                    );
                }
                (toggle_resp, label_resp)
            });
            // Pulse inside the header row rect — layout size stays constant.
            flash::paint_inset(ui, header_row.response.rect, flash_stroke);
            let (toggle_resp, label_resp) = header_row.inner;

            // Double-click folder row → flip open + whole subtree.
            // ▶ single-click toggles this node; on a double-click the first click already
            // flipped was_open, so target is was_open (not !was_open). Label dbl has no
            // prior flip → target !was_open. Single-click label → select for Payload.
            if label_resp.double_clicked() {
                let target = !was_open;
                apply_tree_toggle(ui.ctx(), id, target, profile_id, &child_path, child);
                state.set_open(target);
            } else if toggle_resp.double_clicked() {
                let target = was_open;
                apply_tree_toggle(ui.ctx(), id, target, profile_id, &child_path, child);
                state.set_open(target);
            } else if toggle_resp.clicked() {
                state.toggle(ui);
                state.store(ui.ctx());
            } else if label_resp.clicked() {
                select_topic(pins, profile_id, profile_name, &child_path);
            }

            state.show_body_indented(&header_row.response, ui, |ui| {
                // Folder's own payload row: single-click = Payload; double-click = Publish.
                // Expand/collapse stays on the folder header (label / ▶).
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
                        None, // folder header owns the flash
                        publish_from_tree,
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
                    publish_from_tree,
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
                flash_stroke,
                publish_from_tree,
            );
        }
    }
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
/// Single-click selects for Payload; double-click fills Publish.
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
    flash_stroke: Option<egui::Stroke>,
    publish_from_tree: &mut Option<PublishFromTree>,
) {
    let mut dbl = false;
    let selected = is_selected(pins, profile_id, full_topic);
    let row_fill = if selected {
        select_fill()
    } else {
        Color32::TRANSPARENT
    };

    let row = egui::Frame::NONE.fill(row_fill).show(ui, |ui| {
        // Span the full panel width so payload truncates only at the true right edge.
        let row_w = ui.available_width();
        ui.set_min_width(row_w);
        ui.horizontal(|ui| {
            ui.set_min_width(row_w);
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
                select_topic(pins, profile_id, profile_name, full_topic);
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
            let msg_count = node.message_count;

            // Count first from the right so middle width is the true remainder.
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if msg_count > 0 {
                    ui.label(
                        RichText::new(format!("#{msg_count}"))
                            .small()
                            .color(CHROME_MUTED),
                    );
                }

                // Middle: gray topic · = · copy-payload · highlighted data.
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    ui.set_clip_rect(ui.clip_rect().intersect(ui.max_rect()));
                    ui.spacing_mut().item_spacing.x = 2.0;

                    let topic_resp = ui.add(
                        egui::Label::new(json_view::topic_path_text(label))
                            .sense(egui::Sense::click())
                            .selectable(false),
                    );
                    if topic_resp.double_clicked() {
                        dbl = true;
                    } else if topic_resp.clicked() {
                        select_topic(pins, profile_id, profile_name, full_topic);
                    }
                    // No topic-name hover tip — copy-topic button is enough; tip blocked selection.

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
                        // available_width here is already after topic / = / copy icon.
                        let accent = profiles
                            .profiles
                            .get(profile_id)
                            .map(|p| p.color())
                            .unwrap_or(Color32::from_rgb(90, 190, 120));
                        let font_size = ui.text_style_height(&egui::TextStyle::Monospace);
                        let char_w = ui.fonts(|f| {
                            f.glyph_width(&FontId::monospace(font_size), 'M').max(1.0)
                        });
                        let max_chars =
                            ((ui.available_width() / char_w).floor() as usize).max(12);
                        let is_complex = json_view::try_pretty_json(payload).is_some();
                        let data_resp = if is_complex {
                            let job = json_view::payload_preview_job(payload, max_chars, font_size);
                            ui.add(
                                egui::Label::new(job)
                                    .sense(egui::Sense::click())
                                    .selectable(false),
                            )
                        } else {
                            let line = json_view::preview_line(payload, max_chars);
                            ui.add(
                                egui::Label::new(RichText::new(line).monospace().color(accent))
                                    .sense(egui::Sense::click())
                                    .selectable(false),
                            )
                        };
                        if data_resp.double_clicked() {
                            dbl = true;
                        } else if data_resp.clicked() {
                            select_topic(pins, profile_id, profile_name, full_topic);
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
                });
            });
        });
    });
    // Inset flash — same row size with or without pulse.
    flash::paint_inset(ui, row.response.rect, flash_stroke);

    // Double-click leaf / payload row → Publish (also select for Payload).
    if dbl {
        select_topic(pins, profile_id, profile_name, full_topic);
        *publish_from_tree = Some(PublishFromTree {
            profile_id: profile_id.to_string(),
            profile_name: profile_name.to_string(),
            topic: full_topic.to_string(),
            payload: node.last_payload.clone().unwrap_or_default(),
            retain: node.last_retain,
        });
    }
}

/// One exclusive hover popup for the topic tree.
/// Stays open under wheel; wheel scrolls popup content; moving a few pixels onto
/// the popup (or the gap) does not open tips for rows underneath.
/// Silent while Settings / Keyboard Shortcuts modal is open (underlay must not tip).
fn scrollable_hover(
    ui: &mut egui::Ui,
    resp: &egui::Response,
    id: egui::Id,
    add_contents: impl FnOnce(&mut egui::Ui),
) {
    let ctx = ui.ctx();
    let popup_id = id.with("scroll_hover");
    let active_key = hover_active_key();
    let scroll_key = popup_id.with("scroll_y");
    let pending_key = popup_id.with("hover_since");

    // Modal owns the pointer — never open or keep a tree tip underneath.
    if hover_tip::is_blocked(ctx) {
        let active: Option<egui::Id> = ctx.data(|d| d.get_temp(active_key));
        if active == Some(popup_id) {
            ctx.data_mut(|d| {
                d.remove::<egui::Id>(active_key);
                d.remove_temp::<f32>(scroll_key);
                d.remove_temp::<f64>(pending_key);
            });
        }
        return;
    }

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

    // ~500ms dwell on the same row before opening (scroll-through doesn't trap tips).
    // Store egui time (f64) — IdTypeMap::remove_temp requires Default (Instant lacks it).
    let now_t = ctx.input(|i| i.time);
    let delay_secs = TREE_HOVER_DELAY.as_secs_f64();
    let delay_elapsed = if over_anchor && active.is_none() {
        let since = ctx
            .data(|d| d.get_temp::<f64>(pending_key))
            .unwrap_or(now_t);
        ctx.data_mut(|d| d.insert_temp(pending_key, since));
        let elapsed = (now_t - since).max(0.0);
        if elapsed < delay_secs {
            ctx.request_repaint_after(Duration::from_secs_f64(delay_secs - elapsed));
            false
        } else {
            true
        }
    } else {
        ctx.data_mut(|d| d.remove_temp::<f64>(pending_key));
        false
    };

    // Only one popup: while another is active, ignore hover on other rows.
    let open = if click_outside {
        false
    } else if is_ours {
        over_anchor || over_popup || over_bridge
    } else if active.is_none() && over_anchor && delay_elapsed {
        true
    } else {
        false
    };

    if open {
        ctx.data_mut(|d| {
            d.insert_temp(active_key, popup_id);
            d.remove_temp::<f64>(pending_key);
        });
    } else if is_ours || click_outside {
        ctx.data_mut(|d| {
            d.remove::<egui::Id>(active_key);
            d.remove_temp::<f32>(scroll_key);
            d.remove_temp::<f64>(pending_key);
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
