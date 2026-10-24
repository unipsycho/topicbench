//! Bookmarks popup (Pro) — list, expand checkbox, Export, jump-and-dismiss.

use crate::mqtt::pins::PinBoard;
use crate::mqtt::session::SessionManager;
use crate::mqtt::topic_tree::TopicNode;
use crate::pro::bookmarks::{Bookmark, BookmarksStore};
use crate::ui::icons::{self, IconSet, CHROME_BG, CHROME_MUTED, CHROME_TEXT};
use eframe::egui::{self, RichText, Sense};

/// Pending jump after popup click (applied once tree paints).
#[derive(Clone, Debug)]
pub struct PendingBookmarkJump {
    pub profile_id: String,
    pub topic: String,
    pub expand_subtree: bool,
}

pub fn draw_popup(
    ctx: &egui::Context,
    open: &mut bool,
    store: &mut BookmarksStore,
    profiles_names: &[(String, String)], // id, name
    icons: &IconSet,
    status: &mut String,
    pending_jump: &mut Option<PendingBookmarkJump>,
) {
    if !*open {
        return;
    }
    let mut close = false;
    // Same chrome as Simulation: no outer title bar; section icon + heading inside.
    egui::Window::new("Bookmarks")
        .id(egui::Id::new("topicbench_bookmarks"))
        .title_bar(false)
        .collapsible(false)
        .resizable(true)
        .default_width(360.0)
        .default_height(280.0)
        .frame(egui::Frame::window(&ctx.style()).fill(CHROME_BG))
        .show(ctx, |ui| {
            icons::section_header_bar(ui, |ui| {
                icons::section_icon(ui, icons, "message_history");
                icons::section_title(ui, "Bookmarks");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if icons
                        .tiny(ui, "remove_x", false, "Close Bookmarks")
                        .clicked()
                    {
                        close = true;
                    }
                    if icons
                        .tiny(ui, "export_download", false, "Export bookmarks.json")
                        .clicked()
                    {
                        if let Some(path) = rfd::FileDialog::new()
                            .set_title("Export bookmarks")
                            .set_file_name("bookmarks.json")
                            .add_filter("JSON", &["json"])
                            .save_file()
                        {
                            match store.export_to(&path) {
                                Ok(()) => {
                                    *status = format!("Exported bookmarks → {}", path.display())
                                }
                                Err(e) => *status = format!("Export failed: {e}"),
                            }
                        }
                    }
                });
            });
            ui.separator();
            if store.items.is_empty() {
                ui.weak("No bookmarks yet — use the tree Bookmark icon or Ctrl+Shift+B.");
                return;
            }

            let mut remove_id: Option<String> = None;
            let mut jump: Option<PendingBookmarkJump> = None;
            let mut expand_change: Option<(String, bool)> = None;
            let mut drag_reorder: Option<(usize, usize)> = None;

            egui::ScrollArea::vertical().show(ui, |ui| {
                let n = store.items.len();
                for i in 0..n {
                    let b = store.items[i].clone();
                    let profile_name = profiles_names
                        .iter()
                        .find(|(id, _)| id == &b.profile_id)
                        .map(|(_, n)| n.as_str())
                        .unwrap_or(b.profile_id.as_str());
                    let row = ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        let (grip, _) =
                            ui.allocate_exact_size(egui::vec2(14.0, 16.0), Sense::drag());
                        icons::paint_drag_grip(ui, grip);
                        let grip_id = egui::Id::new(("bm_grip", &b.id));
                        let grip_r = ui.interact(grip, grip_id, Sense::drag());
                        if grip_r.dragged() {
                            grip_r.dnd_set_drag_payload(i);
                        }
                        if let Some(from) = grip_r.dnd_release_payload::<usize>() {
                            if *from != i {
                                drag_reorder = Some((*from, i));
                            }
                        }

                        let label = if b.label.is_empty() {
                            b.topic.clone()
                        } else {
                            b.label.clone()
                        };
                        let tip = format!("{} · {}", b.topic, profile_name);
                        let clicked = ui
                            .add(
                                egui::Label::new(RichText::new(label).color(CHROME_TEXT))
                                    .sense(Sense::click())
                                    .selectable(false),
                            )
                            .on_hover_text(&tip)
                            .clicked();
                        ui.label(RichText::new(profile_name).small().color(CHROME_MUTED));
                        let mut exp = b.expand_subtree;
                        if ui
                            .checkbox(&mut exp, "expand")
                            .on_hover_text("Expand subtree when jumping")
                            .changed()
                        {
                            expand_change = Some((b.id.clone(), exp));
                        }
                        if icons.tiny(ui, "trash", false, "Remove bookmark").clicked() {
                            remove_id = Some(b.id.clone());
                        }
                        clicked
                    });
                    if row.inner {
                        jump = Some(PendingBookmarkJump {
                            profile_id: b.profile_id.clone(),
                            topic: b.topic.clone(),
                            expand_subtree: b.expand_subtree,
                        });
                        close = true;
                    }
                }
            });

            if let Some(id) = remove_id {
                store.remove_id(&id);
            }
            if let Some((id, exp)) = expand_change {
                store.set_expand(&id, exp);
            }
            if let Some((from, to)) = drag_reorder {
                store.reorder(from, to);
            }
            if let Some(j) = jump {
                *pending_jump = Some(j);
            }
        });
    if close {
        *open = false;
    }
}

/// Expand ancestors (and optional subtree), select topic, request scroll.
pub fn apply_jump(
    ctx: &egui::Context,
    sessions: &SessionManager,
    pins: &mut PinBoard,
    jump: &PendingBookmarkJump,
    scroll_to: &mut Option<(String, String)>,
    status: &mut String,
) {
    let Some(session) = sessions.open.get(&jump.profile_id) else {
        *status = format!(
            "Bookmark: profile {} not connected — jump when online",
            jump.profile_id
        );
        return;
    };
    let profile_name = session.profile_name.clone();
    // Open each ancestor so the path is visible.
    open_ancestors(ctx, &jump.profile_id, &jump.topic);
    if jump.expand_subtree {
        if let Some(node) = session.topic_tree.root.node_at(&jump.topic) {
            // Open the bookmarked node itself when it has children.
            if !node.children.is_empty() {
                set_path_open(ctx, &jump.profile_id, &jump.topic, true);
            }
            set_subtree_open_pub(ctx, &jump.profile_id, &jump.topic, node, true);
        }
    }
    pins.last_tree_focus = Some((
        jump.profile_id.clone(),
        profile_name,
        jump.topic.clone(),
    ));
    *scroll_to = Some((jump.profile_id.clone(), jump.topic.clone()));
    *status = format!("Jumped to {}", jump.topic);
}

fn open_ancestors(ctx: &egui::Context, profile_id: &str, topic: &str) {
    let parts: Vec<&str> = topic.split('/').filter(|p| !p.is_empty()).collect();
    if parts.is_empty() {
        return;
    }
    // Open every prefix path that can be a collapsing folder.
    for i in 1..parts.len() {
        let path = parts[..i].join("/");
        set_path_open(ctx, profile_id, &path, true);
    }
}

fn set_path_open(ctx: &egui::Context, profile_id: &str, path: &str, open: bool) {
    let id = egui::Id::new(("topic-tree", profile_id, path));
    let mut state =
        egui::collapsing_header::CollapsingState::load_with_default_open(ctx, id, open);
    state.set_open(open);
    state.store(ctx);
}

fn set_subtree_open_pub(
    ctx: &egui::Context,
    profile_id: &str,
    path: &str,
    node: &TopicNode,
    open: bool,
) {
    for (seg, child) in &node.children {
        let child_path = if path.is_empty() {
            seg.clone()
        } else {
            format!("{path}/{seg}")
        };
        if !child.children.is_empty() {
            set_path_open(ctx, profile_id, &child_path, open);
            set_subtree_open_pub(ctx, profile_id, &child_path, child, open);
        }
    }
}

/// Tiny bookmark toggle for a tree row (Pro).
pub fn bookmark_toggle_icon(
    ui: &mut egui::Ui,
    icons: &IconSet,
    store: &mut BookmarksStore,
    profile_id: &str,
    topic: &str,
    status: &mut String,
) {
    let on = store.is_bookmarked(profile_id, topic);
    let tip = if on {
        "Remove bookmark"
    } else {
        "Add bookmark"
    };
    // message_history (not topic_tags — that is copy-topic on the same row).
    if icons
        .tiny(ui, "message_history", on, &shortcuts_tip(tip))
        .clicked()
    {
        let now_on = store.toggle(profile_id, topic);
        *status = if now_on {
            format!("Bookmarked {topic}")
        } else {
            format!("Removed bookmark {topic}")
        };
    }
}

fn shortcuts_tip(label: &str) -> String {
    crate::ui::shortcuts::tip(label, "Ctrl+Shift+B")
}

#[allow(dead_code)]
fn _use_bookmark_type(_: &Bookmark) {}
