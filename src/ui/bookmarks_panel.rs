//! Bookmarks OS viewport (Pro) — list, expand, Export, jump-and-dismiss, Add/Edit.

use crate::mqtt::pins::PinBoard;
use crate::mqtt::session::SessionManager;
use crate::mqtt::topic_tree::TopicNode;
use crate::pro::bookmarks::{Bookmark, BookmarksStore};
use crate::ui::icons::{
    self, IconSet, CHROME_ACCENT_BORDER, CHROME_BG, CHROME_HOVER, CHROME_MUTED, CHROME_ROW,
    CHROME_TEXT,
};
use eframe::egui::{
    self, CursorIcon, RichText, Sense, ViewportBuilder, ViewportClass,
};

const ROW_H: f32 = 28.0;
const GRIP_W: f32 = 14.0;
const TRASH_W: f32 = 22.0;
const EDIT_W: f32 = 22.0;

/// Pending jump after popup click (applied once tree paints).
#[derive(Clone, Debug)]
pub struct PendingBookmarkJump {
    pub profile_id: String,
    pub topic: String,
    pub expand_subtree: bool,
}

/// Add / edit draft kept in egui temp memory across frames.
#[derive(Clone, Default)]
struct BookmarkForm {
    /// None = new bookmark; Some(id) = editing.
    edit_id: Option<String>,
    profile_id: String,
    topic: String,
    label: String,
    expand_subtree: bool,
}

#[derive(Clone)]
struct BookmarkDnd(String);

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
    ctx.show_viewport_immediate(
        egui::ViewportId::from_hash_of("topicbench_bookmarks_vp"),
        ViewportBuilder::default()
            .with_title("Bookmarks")
            .with_inner_size([420.0, 320.0])
            .with_min_inner_size([320.0, 200.0]),
        |ctx, class| {
            let mut paint = |ui: &mut egui::Ui| {
                paint_bookmarks_body(
                    ui,
                    store,
                    profiles_names,
                    icons,
                    status,
                    pending_jump,
                    &mut close,
                );
            };
            match class {
                ViewportClass::Embedded => {
                    egui::Window::new("Bookmarks")
                        .id(egui::Id::new("topicbench_bookmarks"))
                        .title_bar(false)
                        .collapsible(false)
                        .resizable(true)
                        .default_width(360.0)
                        .default_height(280.0)
                        .frame(egui::Frame::window(&ctx.style()).fill(CHROME_BG))
                        .show(ctx, |ui| paint(ui));
                }
                _ => {
                    egui::CentralPanel::default()
                        .frame(
                            egui::Frame::central_panel(&ctx.style())
                                .fill(CHROME_BG)
                                .inner_margin(egui::Margin::same(8)),
                        )
                        .show(ctx, |ui| paint(ui));
                }
            }
            if ctx.input(|i| i.viewport().close_requested()) {
                close = true;
            }
        },
    );
    if close {
        *open = false;
        // Drop any in-progress form when the window closes.
        ctx.data_mut(|d| d.remove::<BookmarkForm>(egui::Id::new("bm_form")));
    }
}

fn paint_bookmarks_body(
    ui: &mut egui::Ui,
    store: &mut BookmarksStore,
    profiles_names: &[(String, String)],
    icons: &IconSet,
    status: &mut String,
    pending_jump: &mut Option<PendingBookmarkJump>,
    close: &mut bool,
) {
    let mut open_add = false;
    icons::section_header_bar(ui, |ui| {
        icons::section_icon(ui, icons, "message_history");
        icons::section_title(ui, "Bookmarks");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if icons
                .tiny(ui, "remove_x", false, "Close Bookmarks")
                .clicked()
            {
                *close = true;
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
                        Ok(()) => *status = format!("Exported bookmarks → {}", path.display()),
                        Err(e) => *status = format!("Export failed: {e}"),
                    }
                }
            }
            if ui
                .button(RichText::new("+ Add").color(CHROME_TEXT))
                .on_hover_text("Add bookmark manually")
                .clicked()
            {
                open_add = true;
            }
        });
    });
    ui.separator();

    if open_add {
        let default_profile = profiles_names
            .first()
            .map(|(id, _)| id.clone())
            .unwrap_or_default();
        ui.ctx().data_mut(|d| {
            d.insert_temp(
                egui::Id::new("bm_form"),
                BookmarkForm {
                    edit_id: None,
                    profile_id: default_profile,
                    topic: String::new(),
                    label: String::new(),
                    expand_subtree: false,
                },
            );
        });
    }

    // Add / edit form (when open).
    let mut form = ui
        .ctx()
        .data(|d| d.get_temp::<BookmarkForm>(egui::Id::new("bm_form")));
    if let Some(ref mut f) = form {
        let mut dismiss = false;
        let mut save = false;
        egui::Frame::NONE
            .fill(CHROME_ROW)
            .corner_radius(3.0)
            .inner_margin(egui::Margin::same(8))
            .show(ui, |ui| {
                let heading = if f.edit_id.is_some() {
                    "Edit bookmark"
                } else {
                    "Add bookmark"
                };
                ui.label(RichText::new(heading).strong().color(CHROME_TEXT));
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Profile").small().color(CHROME_MUTED));
                    let selected_name = profiles_names
                        .iter()
                        .find(|(id, _)| id == &f.profile_id)
                        .map(|(_, n)| n.as_str())
                        .unwrap_or("(pick)");
                    egui::ComboBox::from_id_salt("bm_form_profile")
                        .selected_text(selected_name)
                        .width(140.0)
                        .show_ui(ui, |ui| {
                            for (id, name) in profiles_names {
                                if ui
                                    .selectable_label(f.profile_id == *id, name)
                                    .clicked()
                                {
                                    f.profile_id = id.clone();
                                }
                            }
                        });
                });
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Topic").small().color(CHROME_MUTED));
                    ui.add(
                        egui::TextEdit::singleline(&mut f.topic)
                            .desired_width(260.0)
                            .hint_text("plant/line1/pump"),
                    );
                });
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Label").small().color(CHROME_MUTED));
                    ui.add(
                        egui::TextEdit::singleline(&mut f.label)
                            .desired_width(180.0)
                            .hint_text("optional"),
                    );
                    ui.checkbox(&mut f.expand_subtree, "expand");
                });
                ui.horizontal(|ui| {
                    if ui.button("Save").clicked() {
                        save = true;
                    }
                    if ui.button("Cancel").clicked() {
                        dismiss = true;
                    }
                });
            });
        ui.add_space(6.0);
        if save {
            let result = match &f.edit_id {
                Some(id) => store.update(
                    id,
                    &f.profile_id,
                    &f.topic,
                    &f.label,
                    f.expand_subtree,
                ),
                None => store.add(&f.profile_id, &f.topic, &f.label, f.expand_subtree),
            };
            match result {
                Ok(()) => {
                    *status = if f.edit_id.is_some() {
                        "Bookmark updated".into()
                    } else {
                        "Bookmark added".into()
                    };
                    dismiss = true;
                }
                Err(e) => *status = e,
            }
        }
        if dismiss {
            ui.ctx()
                .data_mut(|d| d.remove::<BookmarkForm>(egui::Id::new("bm_form")));
            form = None;
        } else {
            ui.ctx()
                .data_mut(|d| d.insert_temp(egui::Id::new("bm_form"), f.clone()));
        }
    }

    if store.items.is_empty() && form.is_none() {
        ui.weak("No bookmarks yet — use the tree Bookmark icon, Ctrl+Shift+B, or + Add.");
        return;
    }

    let mut remove_id: Option<String> = None;
    let mut jump: Option<PendingBookmarkJump> = None;
    let mut expand_change: Option<(String, bool)> = None;
    let mut pending_reorder: Option<(String, String, bool)> = None;
    let mut start_edit: Option<Bookmark> = None;

    egui::ScrollArea::vertical().show(ui, |ui| {
        let ids: Vec<String> = store.items.iter().map(|b| b.id.clone()).collect();
        for id in &ids {
            let Some(b) = store.items.iter().find(|x| x.id == *id).cloned() else {
                continue;
            };
            let profile_name = profiles_names
                .iter()
                .find(|(pid, _)| pid == &b.profile_id)
                .map(|(_, n)| n.as_str())
                .unwrap_or(b.profile_id.as_str());

            let (row_rect, drop_response) =
                ui.allocate_exact_size(egui::vec2(ui.available_width(), ROW_H), Sense::hover());
            ui.painter().rect_filled(row_rect, 2.0, CHROME_ROW);

            let grip_rect = egui::Rect::from_min_size(
                egui::pos2(row_rect.left() + 2.0, row_rect.top()),
                egui::vec2(GRIP_W, ROW_H),
            );
            // Trash + edit sit right-justified.
            let trash_rect = egui::Rect::from_min_size(
                egui::pos2(row_rect.right() - TRASH_W - 2.0, row_rect.top()),
                egui::vec2(TRASH_W, ROW_H),
            );
            let edit_rect = egui::Rect::from_min_size(
                egui::pos2(trash_rect.left() - EDIT_W - 2.0, row_rect.top()),
                egui::vec2(EDIT_W, ROW_H),
            );
            let body_rect = egui::Rect::from_min_max(
                egui::pos2(grip_rect.right() + 4.0, row_rect.top()),
                egui::pos2(edit_rect.left() - 4.0, row_rect.bottom()),
            );

            // Grip — Profiles-list DnD (payload while dragged; drop on whole row).
            icons::paint_drag_grip(ui, grip_rect);
            let grip_id = egui::Id::new(("bm_grip", id.as_str()));
            let grip_response = ui
                .interact(grip_rect, grip_id, Sense::drag())
                .on_hover_cursor(CursorIcon::Grab)
                .on_hover_text("Drag to reorder");
            if grip_response.dragged() {
                grip_response.dnd_set_drag_payload(BookmarkDnd(id.clone()));
            }
            if ui.ctx().is_being_dragged(grip_id) {
                if let Some(pos) = ui.ctx().pointer_interact_pos() {
                    let layer = egui::LayerId::new(egui::Order::Tooltip, grip_id);
                    let preview = egui::Rect::from_center_size(pos, egui::vec2(GRIP_W, ROW_H));
                    let painter = ui.ctx().layer_painter(layer);
                    painter.rect_filled(preview, 2.0, CHROME_HOVER);
                    // Grip dots on the floating preview (same as Profiles).
                    let color = CHROME_MUTED;
                    let r = 1.2_f32;
                    let cx = preview.center().x;
                    let cy = preview.center().y;
                    for col in [-1.0_f32, 1.0] {
                        for row in [-1.0_f32, 0.0, 1.0] {
                            painter.circle_filled(
                                egui::pos2(cx + col * 3.0, cy + row * 3.5),
                                r,
                                color,
                            );
                        }
                    }
                }
            }

            // Drop target indicators (before/after by pointer Y).
            if let Some(payload) = drop_response.dnd_hover_payload::<BookmarkDnd>() {
                if payload.0.as_str() != id.as_str() {
                    let after = drop_response
                        .hover_pos()
                        .map(|p| p.y > drop_response.rect.center().y)
                        .unwrap_or(false);
                    let y = if after {
                        drop_response.rect.bottom()
                    } else {
                        drop_response.rect.top()
                    };
                    ui.painter().hline(
                        drop_response.rect.x_range(),
                        y,
                        egui::Stroke::new(2.0_f32, CHROME_ACCENT_BORDER),
                    );
                    ui.painter().rect_filled(
                        drop_response.rect,
                        2.0,
                        egui::Color32::from_rgba_unmultiplied(
                            CHROME_HOVER.r(),
                            CHROME_HOVER.g(),
                            CHROME_HOVER.b(),
                            80,
                        ),
                    );
                }
            }
            if let Some(payload) = drop_response.dnd_release_payload::<BookmarkDnd>() {
                if payload.0.as_str() != id.as_str() {
                    let after = drop_response
                        .interact_pointer_pos()
                        .or_else(|| drop_response.hover_pos())
                        .map(|p| p.y > drop_response.rect.center().y)
                        .unwrap_or(false);
                    pending_reorder = Some((payload.0.clone(), id.clone(), after));
                }
            }

            // Jumpable name + profile + expand (child Ui clipped to body_rect).
            let label = if b.label.is_empty() {
                b.topic.clone()
            } else {
                b.label.clone()
            };
            let tip = format!("{} · {} — click to jump", b.topic, profile_name);
            ui.allocate_new_ui(egui::UiBuilder::new().max_rect(body_rect), |ui| {
                ui.set_min_height(ROW_H);
                ui.horizontal_centered(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    // Soft row hover so jump-clickability is obvious.
                    let hover_pad = ui.available_rect_before_wrap();
                    let hovering_body = ui.rect_contains_pointer(hover_pad);
                    if hovering_body {
                        ui.painter().rect_filled(body_rect, 2.0, CHROME_HOVER);
                    }
                    let name_color = if hovering_body {
                        CHROME_ACCENT_BORDER
                    } else {
                        CHROME_TEXT
                    };
                    let name_resp = ui
                        .add(
                            egui::Label::new(RichText::new(&label).color(name_color))
                                .sense(Sense::click())
                                .selectable(false),
                        )
                        .on_hover_cursor(CursorIcon::PointingHand)
                        .on_hover_text(&tip);
                    if name_resp.hovered() {
                        ui.painter().hline(
                            name_resp.rect.x_range(),
                            name_resp.rect.bottom() - 1.0,
                            egui::Stroke::new(1.0_f32, CHROME_ACCENT_BORDER),
                        );
                    }
                    if name_resp.clicked() {
                        jump = Some(PendingBookmarkJump {
                            profile_id: b.profile_id.clone(),
                            topic: b.topic.clone(),
                            expand_subtree: b.expand_subtree,
                        });
                        *close = true;
                    }
                    ui.label(RichText::new(profile_name).small().color(CHROME_MUTED));
                    let mut exp = b.expand_subtree;
                    if ui
                        .checkbox(&mut exp, "expand")
                        .on_hover_text("Expand subtree when jumping")
                        .changed()
                    {
                        expand_change = Some((b.id.clone(), exp));
                    }
                });
            });

            // Edit then trash — right-justified slots.
            ui.allocate_new_ui(egui::UiBuilder::new().max_rect(edit_rect), |ui| {
                ui.with_layout(
                    egui::Layout::centered_and_justified(egui::Direction::LeftToRight),
                    |ui| {
                        if icons
                            .tiny(ui, "profile_edit", false, "Edit bookmark")
                            .clicked()
                        {
                            start_edit = Some(b.clone());
                        }
                    },
                );
            });
            ui.allocate_new_ui(egui::UiBuilder::new().max_rect(trash_rect), |ui| {
                ui.with_layout(
                    egui::Layout::centered_and_justified(egui::Direction::LeftToRight),
                    |ui| {
                        if icons.tiny(ui, "trash", false, "Remove bookmark").clicked() {
                            remove_id = Some(b.id.clone());
                        }
                    },
                );
            });
        }
    });

    if let Some(b) = start_edit {
        ui.ctx().data_mut(|d| {
            d.insert_temp(
                egui::Id::new("bm_form"),
                BookmarkForm {
                    edit_id: Some(b.id),
                    profile_id: b.profile_id,
                    topic: b.topic,
                    label: b.label,
                    expand_subtree: b.expand_subtree,
                },
            );
        });
    }
    if let Some(id) = remove_id {
        store.remove_id(&id);
    }
    if let Some((id, exp)) = expand_change {
        store.set_expand(&id, exp);
    }
    if let Some((dragged, target, after)) = pending_reorder {
        if store.reorder_ids(&dragged, &target, after) {
            *status = "Reordered bookmarks".into();
        }
    }
    if let Some(j) = jump {
        *pending_jump = Some(j);
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
