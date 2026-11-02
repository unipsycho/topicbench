//! Scenarios OS viewport (Pro) — left runners + transport; right step editor.

use crate::mqtt::settings::{AppSettings, SCENARIOS_LEFT_W_MIN};
use crate::pro::scenario_client::{
    blank_scenario, blank_step, ScenarioClientState, ScenarioPlacement, ScenarioStep, WaitOp,
};
use crate::ui::hover_tip;
use crate::ui::icons::{
    self, IconSet, CHROME_ACCENT_BORDER, CHROME_ACCENT_SOFT, CHROME_BG, CHROME_HOVER, CHROME_MUTED,
    CHROME_ROW, CHROME_SEP, CHROME_TEXT, TINY_ICON,
};
use eframe::egui::{self, Color32, CursorIcon, RichText, Sense, ViewportBuilder, ViewportClass};
use std::collections::HashSet;

const ROW_H: f32 = 28.0;
const GRIP_W: f32 = 16.0;
const TRASH_W: f32 = 22.0;
const SPLIT_GRAB: f32 = 6.0;
/// Minimum right column: grip + checkbox + ellipsis + trash (text yields first).
const RIGHT_COL_MIN: f32 = 100.0;
const CHECK_W: f32 = 18.0;
/// Icon inside Add button — smaller than TINY so padding insets the glyph.
const ADD_STEP_ICON: f32 = 14.0;
/// Match Publish chrome (120×28) so + has room beside the label.
const ADD_STEP_BTN: egui::Vec2 = egui::vec2(120.0, 28.0);
const SCENARIO_INDENT: f32 = 14.0;

/// DnD payload for reordering steps (index into the current draft).
#[derive(Clone, Debug)]
struct StepDnd(usize);

/// DnD: scenario id (reorder within/between groups).
#[derive(Clone, Debug)]
struct ScenarioDnd(String);

/// DnD: named group id (reorder groups).
#[derive(Clone, Debug)]
struct GroupDnd(String);

#[derive(Clone, Copy, PartialEq, Eq)]
enum ExportScopeChoice {
    SelectedGroup,
    Everything,
}

#[derive(Clone, PartialEq, Eq)]
enum ImportModeChoice {
    Replace,
    Merge,
    IntoGroup { group_id: String, new_name: String },
}

/// Draw Scenarios as an OS viewport (Bookmarks / Color Rules pattern).
#[allow(clippy::too_many_arguments)]
pub fn draw_popup(
    ctx: &egui::Context,
    open: &mut bool,
    client: &mut ScenarioClientState,
    profiles_names: &[(String, String)],
    icons: &IconSet,
    status: &mut String,
    builder_seed: crate::mqtt::window_geom::WinGeom,
    saved_geom: &mut Option<crate::mqtt::window_geom::WinGeom>,
    left_width: &mut f32,
    geom_dirty: &mut bool,
) {
    if !*open {
        return;
    }
    client.poll_if_due(false);
    client.arm_toolbar_after_open();

    let mut close = false;
    const MIN: [f32; 2] = [720.0, 420.0];
    ctx.show_viewport_immediate(
        egui::ViewportId::from_hash_of("topicbench_scenarios_vp"),
        builder_seed
            .apply(ViewportBuilder::default().with_title("Scenarios"), MIN)
            .with_min_inner_size(MIN),
        |ctx, class| {
            let mut paint = |ui: &mut egui::Ui| {
                icons::apply_pro_panel_chrome(ui);
                paint_body(
                    ui,
                    client,
                    profiles_names,
                    icons,
                    status,
                    left_width,
                    geom_dirty,
                    &mut close,
                );
            };
            match class {
                ViewportClass::Embedded => {
                    egui::Window::new("Scenarios")
                        .id(egui::Id::new("topicbench_scenarios"))
                        .title_bar(false)
                        .collapsible(false)
                        .resizable(true)
                        .default_width(900.0)
                        .default_height(520.0)
                        .frame(egui::Frame::window(&ctx.style()).fill(CHROME_BG()))
                        .show(ctx, |ui| paint(ui));
                }
                _ => {
                    egui::CentralPanel::default()
                        .frame(
                            egui::Frame::central_panel(&ctx.style())
                                .fill(CHROME_BG())
                                .inner_margin(egui::Margin::same(8)),
                        )
                        .show(ctx, |ui| paint(ui));
                }
            }
            if ctx.input(|i| i.viewport().close_requested()) {
                close = true;
            }
            if let Some(g) = crate::mqtt::window_geom::capture(ctx) {
                if crate::mqtt::window_geom::sync_slot(saved_geom, g, MIN) {
                    *geom_dirty = true;
                }
            }
        },
    );
    if close {
        *open = false;
        client.disarm_toolbar();
    }
}

fn paint_body(
    ui: &mut egui::Ui,
    client: &mut ScenarioClientState,
    profiles_names: &[(String, String)],
    icons: &IconSet,
    status: &mut String,
    left_width: &mut f32,
    geom_dirty: &mut bool,
    close: &mut bool,
) {
    // Header
    icons::section_header_bar(ui, |ui| {
        icons::section_icon(ui, icons, "clipboard_checklist");
        icons::section_title(ui, "Scenarios");
        if !client.sidecar_up {
            ui.label(
                RichText::new("Pro sidecar offline")
                    .color(Color32::from_rgb(220, 140, 80))
                    .small(),
            );
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if icons.tiny_header(ui, "close_red", false, "Close").clicked() {
                *close = true;
            }
            handle_import_export(ui, client, icons, status);
        });
    });
    ui.add_space(6.0);

    let avail = ui.available_size();
    // Allow a narrow right column (icons stay; text truncates) while left can grow.
    let max_left = (avail.x - RIGHT_COL_MIN - SPLIT_GRAB).max(SCENARIOS_LEFT_W_MIN);
    *left_width = AppSettings::clamp_scenarios_left_width(*left_width).min(max_left);
    let left_w = *left_width;

    ui.horizontal(|ui| {
        // ── Left: runners + transport ──────────────────────────────────────
        ui.allocate_ui_with_layout(
            egui::vec2(left_w, avail.y),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                paint_left(ui, client, profiles_names, icons, status);
            },
        );
        // Draggable vertical split (persisted like main side panels).
        let (split_rect, split_resp) = ui.allocate_exact_size(
            egui::vec2(SPLIT_GRAB, avail.y),
            Sense::click_and_drag(),
        );
        let split_resp = split_resp.on_hover_cursor(CursorIcon::ResizeHorizontal);
        if split_resp.hovered() || split_resp.dragged() {
            ui.painter().rect_filled(
                split_rect,
                0.0,
                CHROME_HOVER().gamma_multiply(0.55),
            );
        }
        ui.painter().vline(
            split_rect.center().x,
            split_rect.y_range(),
            egui::Stroke::new(
                if split_resp.hovered() || split_resp.dragged() {
                    2.0
                } else {
                    1.0
                },
                if split_resp.hovered() || split_resp.dragged() {
                    CHROME_ACCENT_BORDER()
                } else {
                    CHROME_SEP()
                },
            ),
        );
        if split_resp.dragged() {
            let next = AppSettings::clamp_scenarios_left_width(
                *left_width + split_resp.drag_delta().x,
            )
            .min(max_left);
            if (next - *left_width).abs() > 0.5 {
                *left_width = next;
                *geom_dirty = true;
            }
        }
        // ── Right: step editor only ───────────────────────────────────────
        ui.allocate_ui_with_layout(
            egui::vec2(ui.available_width(), avail.y),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                paint_right(ui, client, profiles_names, icons, status);
            },
        );
    });

    if let Some(err) = &client.last_error {
        ui.add_space(4.0);
        ui.label(
            RichText::new(err)
                .color(Color32::from_rgb(220, 120, 100))
                .small(),
        );
    }
}

fn paint_left(
    ui: &mut egui::Ui,
    client: &mut ScenarioClientState,
    profiles_names: &[(String, String)],
    icons: &IconSet,
    status: &mut String,
) {
    // One-row transport: Play / Pause / Step / Add / Add group — trash is per-row.
    let sel = client.selected_id.clone();
    let group_sel = client.selected_group_id.clone();
    let st = sel.as_ref().and_then(|id| client.status_for(id).cloned());
    // Group pause = any member paused (resume those); else pause running members.
    let group_paused = group_sel.as_ref().is_some_and(|gid| {
        client
            .rows
            .iter()
            .filter(|r| r.group_id == *gid)
            .any(|r| client.status_for(&r.id).is_some_and(|s| s.is_paused()))
    });
    let paused = if group_sel.is_some() {
        group_paused
    } else {
        st.as_ref().map(|s| s.is_paused()).unwrap_or(false)
    };
    ui.horizontal(|ui| {
        let can = (sel.is_some() || group_sel.is_some()) && client.sidecar_up;
        let can_step = sel.is_some() && client.sidecar_up;
        let play_tip = if group_sel.is_some() {
            "Play — start all scenarios in the selected group"
        } else {
            "Play — start the selected scenario"
        };
        if icons.tiny_header(ui, "play", false, play_tip).clicked() && can {
            if let Some(gid) = &group_sel {
                match client.start_group(gid) {
                    Ok(()) => *status = "Group started".into(),
                    Err(e) => *status = e,
                }
            } else if let Some(id) = &sel {
                match client.start(id) {
                    Ok(()) => *status = format!("Scenario started: {id}"),
                    Err(e) => *status = e,
                }
            }
        }
        let pause_tip = if group_sel.is_some() {
            if paused {
                "Resume — resume all paused scenarios in the group"
            } else {
                "Pause — pause all running scenarios in the group"
            }
        } else if paused {
            "Resume — continue the selected scenario"
        } else {
            "Pause — pause the selected scenario"
        };
        if icons
            .tiny_header(ui, "pause", false, pause_tip)
            .clicked()
            && can
        {
            if let Some(gid) = &group_sel {
                let res = if paused {
                    client.resume_group(gid)
                } else {
                    client.pause_group(gid)
                };
                match res {
                    Ok(()) => {
                        *status = if paused {
                            "Group resumed".into()
                        } else {
                            "Group paused".into()
                        }
                    }
                    Err(e) => *status = e,
                }
            } else if let Some(id) = &sel {
                let res = if paused {
                    client.resume(id)
                } else {
                    client.pause(id)
                };
                match res {
                    Ok(()) => {
                        *status = if paused {
                            "Scenario resumed".into()
                        } else {
                            "Scenario paused".into()
                        }
                    }
                    Err(e) => *status = e,
                }
            }
        }
        // Step is single-scenario only.
        let step_tip = if group_sel.is_some() {
            "Step — select one scenario (not available for a group)"
        } else {
            "Step — run one enabled step, then pause"
        };
        if icons.tiny_header(ui, "playback", false, step_tip).clicked() && can_step {
            if let Some(id) = &sel {
                match client.step(id) {
                    Ok(()) => *status = "Scenario stepped".into(),
                    Err(e) => *status = e,
                }
            }
        }
        let add_scn_tip = if group_sel.is_some() {
            "Add scenario — create in the selected group"
        } else {
            "Add scenario — create a new script"
        };
        if icons.tiny_header(ui, "add_blue", false, add_scn_tip).clicked() {
            let profile = profiles_names
                .first()
                .map(|(id, _)| id.as_str())
                .unwrap_or("");
            let mut row = blank_scenario(profile);
            // New scenario joins the selected group (incl. Ungrouped "").
            if let Some(gid) = &group_sel {
                row.group_id = gid.clone();
            }
            match client.upsert(&row) {
                Ok(r) => *status = format!("Added scenario {}", r.name),
                Err(e) => *status = e,
            }
        }
        if icons
            .tiny_header(ui, "profile_add", false, "Add group — create a named folder")
            .clicked()
        {
            ui.ctx().data_mut(|d| {
                d.insert_temp(egui::Id::new("scn_add_group_open"), true);
                d.insert_temp(egui::Id::new("scn_add_group_name"), String::new());
            });
        }
    });
    draw_add_group_modal(ui.ctx(), client, icons, status);
    ui.add_space(4.0);
    ui.label(RichText::new("Scenarios").color(CHROME_MUTED()).small());

    let mut delete_id: Option<String> = None;
    let mut delete_group_id: Option<String> = None;
    let mut pending_scn_drop: Option<(String, String, String, bool)> = None; // drag, target, target_gid, after
    let mut pending_group_drop: Option<(String, String, bool)> = None; // drag, target, after
    let mut pending_drop_on_group: Option<(String, String)> = None; // drag scn, target group_id
    let is_scn_dragging = egui::DragAndDrop::has_payload_of_type::<ScenarioDnd>(ui.ctx());

    // Span the visible left pane so trash stays on the right edge and text ellipsizes.
    let list_w = ui.max_rect().width().max(ui.available_width());
    egui::ScrollArea::vertical()
        .id_salt("scenarios_list")
        .auto_shrink([false, false])
        .max_width(list_w)
        .show(ui, |ui| {
            ui.set_min_width(list_w);
            ui.set_max_width(list_w);

            // Collapse state: local HashSet of closed group ids (not persisted).
            let collapsed_key = egui::Id::new("scn_collapsed_groups");
            let mut collapsed: HashSet<String> = ui
                .ctx()
                .data(|d| d.get_temp::<HashSet<String>>(collapsed_key))
                .unwrap_or_default();

            let groups = client.groups.clone();
            let mut any_row = false;

            for g in &groups {
                any_row = true;
                let members: Vec<(String, String, bool, i64)> = {
                    let mut v: Vec<_> = client
                        .rows
                        .iter()
                        .filter(|r| r.group_id == g.id)
                        .map(|r| (r.id.clone(), r.name.clone(), r.loop_, r.sort))
                        .collect();
                    v.sort_by_key(|(_, _, _, sort)| *sort);
                    v
                };
                let open = !collapsed.contains(&g.id);
                paint_group_header(
                    ui,
                    client,
                    icons,
                    list_w,
                    &g.id,
                    &g.name,
                    open,
                    true,
                    &mut collapsed,
                    &mut delete_group_id,
                    &mut pending_group_drop,
                    &mut pending_drop_on_group,
                );
                if open {
                    for (id, name, loop_on, _) in members {
                        paint_scenario_row(
                            ui,
                            client,
                            icons,
                            list_w,
                            &id,
                            &name,
                            loop_on,
                            &g.id,
                            &mut delete_id,
                            &mut pending_scn_drop,
                            &mut pending_drop_on_group,
                        );
                    }
                }
            }

            // Ungrouped at end — show when non-empty OR while dragging (drop target).
            let ungrouped: Vec<(String, String, bool, i64)> = {
                let mut v: Vec<_> = client
                    .rows
                    .iter()
                    .filter(|r| r.group_id.is_empty())
                    .map(|r| (r.id.clone(), r.name.clone(), r.loop_, r.sort))
                    .collect();
                v.sort_by_key(|(_, _, _, sort)| *sort);
                v
            };
            let show_ungrouped = !ungrouped.is_empty() || is_scn_dragging;
            if show_ungrouped {
                any_row = true;
                let ug_key = "";
                let open = !collapsed.contains(ug_key);
                paint_group_header(
                    ui,
                    client,
                    icons,
                    list_w,
                    ug_key,
                    "Ungrouped",
                    open,
                    false, // no group reorder / trash on Ungrouped
                    &mut collapsed,
                    &mut delete_group_id,
                    &mut pending_group_drop,
                    &mut pending_drop_on_group,
                );
                if open {
                    for (id, name, loop_on, _) in ungrouped {
                        paint_scenario_row(
                            ui,
                            client,
                            icons,
                            list_w,
                            &id,
                            &name,
                            loop_on,
                            "",
                            &mut delete_id,
                            &mut pending_scn_drop,
                            &mut pending_drop_on_group,
                        );
                    }
                }
            }

            ui.ctx().data_mut(|d| d.insert_temp(collapsed_key, collapsed));

            if !any_row && client.rows.is_empty() {
                ui.label(
                    RichText::new("No scenarios yet — Add to author a script.")
                        .color(CHROME_MUTED())
                        .italics(),
                );
            }
        });

    if let Some(id) = delete_id {
        match client.delete(&id) {
            Ok(()) => *status = "Scenario deleted".into(),
            Err(e) => *status = e,
        }
    }
    if let Some(gid) = delete_group_id {
        match client.delete_group(&gid) {
            Ok(()) => *status = "Group deleted (members ungrouped)".into(),
            Err(e) => *status = e,
        }
    }
    if let Some((drag, target, target_gid, after)) = pending_scn_drop {
        apply_scenario_reorder(client, status, &drag, &target, &target_gid, after);
    } else if let Some((drag, target_gid)) = pending_drop_on_group {
        apply_scenario_to_group(client, status, &drag, &target_gid);
    }
    if let Some((drag, target, after)) = pending_group_drop {
        apply_group_reorder(client, status, &drag, &target, after);
    }
}

fn paint_group_header(
    ui: &mut egui::Ui,
    client: &mut ScenarioClientState,
    icons: &IconSet,
    list_w: f32,
    group_id: &str,
    title: &str,
    open: bool,
    named: bool,
    collapsed: &mut HashSet<String>,
    delete_group_id: &mut Option<String>,
    pending_group_drop: &mut Option<(String, String, bool)>,
    pending_drop_on_group: &mut Option<(String, String)>,
) {
    let selected = client.selected_group_id.as_deref() == Some(group_id);
    let row_w = list_w.max(ui.available_width());
    let (rect, drop_resp) = ui.allocate_exact_size(egui::vec2(row_w, ROW_H), Sense::click());
    let fill = if selected {
        CHROME_ACCENT_SOFT()
    } else {
        Color32::from_rgba_unmultiplied(CHROME_ROW().r(), CHROME_ROW().g(), CHROME_ROW().b(), 180)
    };
    ui.painter().rect_filled(rect, 4.0, fill);
    if selected {
        ui.painter().rect_stroke(
            rect,
            4.0,
            egui::Stroke::new(1.0, CHROME_ACCENT_BORDER()),
            egui::StrokeKind::Inside,
        );
    }

    let mut x = rect.left() + 2.0;
    // Grip only on named groups (reorder).
    if named {
        let g = egui::Rect::from_min_size(egui::pos2(x, rect.top()), egui::vec2(GRIP_W, ROW_H));
        icons::paint_drag_grip(ui, g);
        let grip_id = egui::Id::new(("scn_grp_grip", group_id));
        let grip_resp = hover_tip::attach(
            ui.interact(g, grip_id, Sense::drag())
                .on_hover_cursor(CursorIcon::Grab),
            "Drag to reorder groups",
        );
        if grip_resp.dragged() {
            grip_resp.dnd_set_drag_payload(GroupDnd(group_id.to_string()));
        }
        if ui.ctx().is_being_dragged(grip_id) {
            if let Some(pos) = ui.ctx().pointer_interact_pos() {
                let layer = egui::LayerId::new(egui::Order::Tooltip, grip_id);
                ui.ctx().layer_painter(layer).rect_filled(
                    egui::Rect::from_center_size(pos, egui::vec2(GRIP_W, ROW_H)),
                    2.0,
                    CHROME_HOVER(),
                );
            }
        }
        x = g.right() + 2.0;
    }

    // Disclosure chevron (tree-style ▸/▾ — theme stroke, not egui's washed triangle).
    let chev_rect = egui::Rect::from_min_size(egui::pos2(x, rect.top()), egui::vec2(14.0, ROW_H));
    let tri_resp = ui.interact(
        chev_rect,
        egui::Id::new(("scn_grp_tri", group_id)),
        Sense::click(),
    );
    paint_disclosure_chevron(ui, chev_rect, open, tri_resp.hovered());
    if tri_resp.clicked() {
        if open {
            collapsed.insert(group_id.to_string());
        } else {
            collapsed.remove(group_id);
        }
    }
    x = chev_rect.right() + 4.0;

    let trash_rect = if named {
        egui::Rect::from_min_size(
            egui::pos2((rect.right() - TRASH_W - 4.0).max(x), rect.top()),
            egui::vec2(TRASH_W.min(rect.width()), ROW_H),
        )
    } else {
        egui::Rect::NOTHING
    };
    let mut trash_clicked = false;
    if named {
        let trash_resp = ui.interact(
            trash_rect,
            egui::Id::new(("scn_grp_trash", group_id)),
            Sense::click(),
        );
        if let Some(tex) = icons.get("delete_red") {
            let img = egui::Rect::from_center_size(trash_rect.center(), egui::vec2(14.0, 14.0));
            ui.painter().image(
                tex.id(),
                img,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        }
        if trash_resp.clicked() {
            *delete_group_id = Some(group_id.to_string());
            trash_clicked = true;
        }
    }
    let text_right = if named {
        (trash_rect.left() - 6.0).max(x)
    } else {
        rect.right() - 6.0
    };
    paint_truncated_label(
        ui,
        egui::Rect::from_min_max(egui::pos2(x, rect.top()), egui::pos2(text_right, rect.bottom())),
        title.to_string(),
        CHROME_TEXT(),
        13.0,
    );

    // Click header body → select group (not trash/triangle).
    if drop_resp.clicked() && !tri_resp.clicked() && !trash_clicked {
        client.select_group(group_id.to_string());
    }

    // Drop scenario onto group header → move into group (end).
    if let Some(payload) = drop_resp.dnd_hover_payload::<ScenarioDnd>() {
        ui.painter().rect_stroke(
            rect,
            4.0,
            egui::Stroke::new(2.0, CHROME_ACCENT_BORDER()),
            egui::StrokeKind::Inside,
        );
        let _ = payload;
    }
    if let Some(payload) = drop_resp.dnd_release_payload::<ScenarioDnd>() {
        *pending_drop_on_group = Some((payload.0.clone(), group_id.to_string()));
    }

    // Drop group onto group header → reorder named groups.
    if named {
        if let Some(payload) = drop_resp.dnd_hover_payload::<GroupDnd>() {
            if payload.0.as_str() != group_id {
                let after = drop_resp
                    .hover_pos()
                    .map(|p| p.y > rect.center().y)
                    .unwrap_or(false);
                let y = if after { rect.bottom() } else { rect.top() };
                ui.painter().hline(
                    rect.x_range(),
                    y,
                    egui::Stroke::new(2.0, CHROME_ACCENT_BORDER()),
                );
            }
        }
        if let Some(payload) = drop_resp.dnd_release_payload::<GroupDnd>() {
            if payload.0.as_str() != group_id {
                let after = drop_resp
                    .interact_pointer_pos()
                    .or_else(|| drop_resp.hover_pos())
                    .map(|p| p.y > rect.center().y)
                    .unwrap_or(false);
                *pending_group_drop = Some((payload.0.clone(), group_id.to_string(), after));
            }
        }
    }
}

fn paint_scenario_row(
    ui: &mut egui::Ui,
    client: &mut ScenarioClientState,
    icons: &IconSet,
    list_w: f32,
    id: &str,
    name: &str,
    loop_on: bool,
    group_id: &str,
    delete_id: &mut Option<String>,
    pending_scn_drop: &mut Option<(String, String, String, bool)>,
    _pending_drop_on_group: &mut Option<(String, String)>,
) {
    let st = client.status_for(id);
    let state = st.map(|s| s.state.as_str()).unwrap_or("stopped");
    let loop_n = st.map(|s| s.loop_n).unwrap_or(0);
    let selected = client.selected_id.as_deref() == Some(id);
    // Running = green; paused = amber (profiles Connected green pattern).
    let (fill, text_color) = match (selected, state) {
        (true, _) => (CHROME_ACCENT_SOFT(), CHROME_TEXT()),
        (false, "running") => (
            Color32::from_rgba_unmultiplied(90, 190, 120, 40),
            Color32::from_rgb(90, 190, 120),
        ),
        (false, "paused") => (
            Color32::from_rgba_unmultiplied(220, 180, 80, 40),
            Color32::from_rgb(220, 180, 80),
        ),
        _ => (CHROME_ROW(), CHROME_MUTED()),
    };
    let row_w = list_w.max(ui.available_width());
    let (rect, drop_resp) = ui.allocate_exact_size(egui::vec2(row_w, ROW_H), Sense::click());
    if !ui.is_rect_visible(rect) {
        return;
    }
    // Highlight/chip starts after the indent gutter (tree child-row pattern).
    let chip = egui::Rect::from_min_max(
        egui::pos2(rect.left() + SCENARIO_INDENT, rect.top()),
        egui::pos2(rect.right(), rect.bottom()),
    );
    ui.painter().rect_filled(chip, 4.0, fill);
    if selected {
        ui.painter().rect_stroke(
            chip,
            4.0,
            egui::Stroke::new(1.0, CHROME_ACCENT_BORDER()),
            egui::StrokeKind::Inside,
        );
    }

    let grip_rect = egui::Rect::from_min_size(
        egui::pos2(chip.left() + 2.0, rect.top()),
        egui::vec2(GRIP_W, ROW_H),
    );
    icons::paint_drag_grip(ui, grip_rect);
    let grip_id = egui::Id::new(("scn_row_grip", id));
    let grip_resp = hover_tip::attach(
        ui.interact(grip_rect, grip_id, Sense::drag())
            .on_hover_cursor(CursorIcon::Grab),
        "Drag to reorder",
    );
    if grip_resp.dragged() {
        grip_resp.dnd_set_drag_payload(ScenarioDnd(id.to_string()));
    }
    if ui.ctx().is_being_dragged(grip_id) {
        if let Some(pos) = ui.ctx().pointer_interact_pos() {
            let layer = egui::LayerId::new(egui::Order::Tooltip, grip_id);
            ui.ctx().layer_painter(layer).rect_filled(
                egui::Rect::from_center_size(pos, egui::vec2(GRIP_W, ROW_H)),
                2.0,
                CHROME_HOVER(),
            );
        }
    }

    let trash_rect = egui::Rect::from_min_size(
        egui::pos2((rect.right() - TRASH_W - 4.0).max(grip_rect.right()), rect.top()),
        egui::vec2(TRASH_W.min(rect.width()), ROW_H),
    );
    let mut label = name.to_string();
    if label.is_empty() {
        label = id.to_string();
    }
    let loop_bit = if loop_on {
        format!(" · loop {loop_n}")
    } else if loop_n > 0 {
        format!(" · ×{loop_n}")
    } else {
        String::new()
    };
    let text_rect = egui::Rect::from_min_max(
        egui::pos2(grip_rect.right() + 4.0, rect.top()),
        egui::pos2((trash_rect.left() - 6.0).max(grip_rect.right() + 4.0), rect.bottom()),
    );
    paint_truncated_label(
        ui,
        text_rect,
        format!("{label}  ·  {state}{loop_bit}"),
        text_color,
        13.0,
    );

    let trash_resp = ui.interact(
        trash_rect,
        egui::Id::new(("scn_trash", id)),
        Sense::click(),
    );
    if let Some(tex) = icons.get("delete_red") {
        let img = egui::Rect::from_center_size(trash_rect.center(), egui::vec2(14.0, 14.0));
        ui.painter().image(
            tex.id(),
            img,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            Color32::WHITE,
        );
    }
    if trash_resp.clicked() {
        *delete_id = Some(id.to_string());
    } else if drop_resp.hovered() && !selected && !trash_resp.hovered() && !grip_resp.hovered() {
        ui.painter()
            .rect_filled(chip, 4.0, CHROME_HOVER().gamma_multiply(0.35));
    }
    if drop_resp.clicked() && !trash_resp.clicked() && !grip_resp.dragged() {
        client.select_scenario(id.to_string());
    }

    // Drop scenario relative to this row (same or cross-group).
    if let Some(payload) = drop_resp.dnd_hover_payload::<ScenarioDnd>() {
        if payload.0.as_str() != id {
            let after = drop_resp
                .hover_pos()
                .map(|p| p.y > rect.center().y)
                .unwrap_or(false);
            let y = if after { chip.bottom() } else { chip.top() };
            ui.painter().hline(
                chip.x_range(),
                y,
                egui::Stroke::new(2.0, CHROME_ACCENT_BORDER()),
            );
        }
    }
    if let Some(payload) = drop_resp.dnd_release_payload::<ScenarioDnd>() {
        if payload.0.as_str() != id {
            let after = drop_resp
                .interact_pointer_pos()
                .or_else(|| drop_resp.hover_pos())
                .map(|p| p.y > rect.center().y)
                .unwrap_or(false);
            *pending_scn_drop = Some((
                payload.0.clone(),
                id.to_string(),
                group_id.to_string(),
                after,
            ));
        }
    }
}

fn apply_scenario_reorder(
    client: &mut ScenarioClientState,
    status: &mut String,
    drag_id: &str,
    target_id: &str,
    target_gid: &str,
    after: bool,
) {
    let mut members: Vec<String> = client
        .rows
        .iter()
        .filter(|r| r.group_id == target_gid && r.id != drag_id)
        .map(|r| r.id.clone())
        .collect();
    // Preserve current relative order among remaining (already sorted by poll).
    members.sort_by_key(|id| {
        client
            .rows
            .iter()
            .find(|r| r.id == *id)
            .map(|r| r.sort)
            .unwrap_or(0)
    });
    let insert_at = members
        .iter()
        .position(|id| id == target_id)
        .map(|i| if after { i + 1 } else { i })
        .unwrap_or(members.len());
    members.insert(insert_at.min(members.len()), drag_id.to_string());
    let placements: Vec<ScenarioPlacement> = members
        .into_iter()
        .enumerate()
        .map(|(i, id)| ScenarioPlacement {
            id,
            group_id: target_gid.to_string(),
            sort: i as i64,
        })
        .collect();
    match client.reorder(None, placements) {
        Ok(()) => *status = "Reordered scenarios".into(),
        Err(e) => *status = e,
    }
}

fn apply_scenario_to_group(
    client: &mut ScenarioClientState,
    status: &mut String,
    drag_id: &str,
    target_gid: &str,
) {
    let mut members: Vec<String> = client
        .rows
        .iter()
        .filter(|r| r.group_id == target_gid && r.id != drag_id)
        .map(|r| r.id.clone())
        .collect();
    members.sort_by_key(|id| {
        client
            .rows
            .iter()
            .find(|r| r.id == *id)
            .map(|r| r.sort)
            .unwrap_or(0)
    });
    members.push(drag_id.to_string());
    let placements: Vec<ScenarioPlacement> = members
        .into_iter()
        .enumerate()
        .map(|(i, id)| ScenarioPlacement {
            id,
            group_id: target_gid.to_string(),
            sort: i as i64,
        })
        .collect();
    match client.reorder(None, placements) {
        Ok(()) => *status = "Moved scenario to group".into(),
        Err(e) => *status = e,
    }
}

fn apply_group_reorder(
    client: &mut ScenarioClientState,
    status: &mut String,
    drag_id: &str,
    target_id: &str,
    after: bool,
) {
    let mut ids: Vec<String> = client
        .groups
        .iter()
        .filter(|g| g.id != drag_id)
        .map(|g| g.id.clone())
        .collect();
    let insert_at = ids
        .iter()
        .position(|id| id == target_id)
        .map(|i| if after { i + 1 } else { i })
        .unwrap_or(ids.len());
    ids.insert(insert_at.min(ids.len()), drag_id.to_string());
    match client.reorder(Some(ids), Vec::new()) {
        Ok(()) => *status = "Reordered groups".into(),
        Err(e) => *status = e,
    }
}

fn draw_add_group_modal(
    ctx: &egui::Context,
    client: &mut ScenarioClientState,
    icons: &IconSet,
    status: &mut String,
) {
    let open_key = egui::Id::new("scn_add_group_open");
    let name_key = egui::Id::new("scn_add_group_name");
    let open = ctx.data(|d| d.get_temp::<bool>(open_key)).unwrap_or(false);
    if !open {
        return;
    }
    let mut name = ctx
        .data(|d| d.get_temp::<String>(name_key))
        .unwrap_or_default();
    let mut close = false;
    let mut do_create = false;
    egui::Modal::new(egui::Id::new("topicbench_scn_add_group")).show(ctx, |ui| {
        ui.set_min_width(360.0);
        ui.set_max_width(400.0);
        icons::section_header_bar(ui, |ui| {
            icons::section_icon(ui, icons, "profile_add");
            icons::section_title(ui, "Add group");
        });
        ui.add_space(6.0);
        let bar_w = ui.available_width();
        let (bar, _) = ui.allocate_exact_size(egui::vec2(bar_w, 3.0), egui::Sense::hover());
        ui.painter().rect_filled(bar, 1.5, CHROME_ACCENT_BORDER());
        ui.add_space(12.0);
        ui.label(RichText::new("Name").strong());
        ui.add_space(4.0);
        let resp = ui.add(
            egui::TextEdit::singleline(&mut name)
                .desired_width(ui.available_width())
                .hint_text("Group name"),
        );
        if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            do_create = !name.trim().is_empty();
        }
        ui.add_space(14.0);
        ui.separator();
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                let ok = ui.add_enabled(
                    !name.trim().is_empty(),
                    egui::Button::new(RichText::new("OK").strong())
                        .fill(CHROME_ACCENT_SOFT())
                        .stroke(egui::Stroke::new(1.0_f32, CHROME_ACCENT_BORDER()))
                        .min_size(egui::vec2(120.0, 32.0)),
                );
                if ok.clicked() {
                    do_create = true;
                }
                let cancel = if let Some(tex) = icons.get("close_red") {
                    ui.add(
                        egui::Button::image_and_text(
                            egui::Image::new(tex)
                                .fit_to_exact_size(egui::vec2(TINY_ICON, TINY_ICON)),
                            "Cancel",
                        )
                        .min_size(egui::vec2(104.0, 32.0)),
                    )
                } else {
                    ui.add(egui::Button::new("Cancel").min_size(egui::vec2(104.0, 32.0)))
                };
                if cancel.clicked() {
                    close = true;
                }
            });
        });
    });
    ctx.data_mut(|d| d.insert_temp(name_key, name.clone()));
    if do_create {
        match client.upsert_group(None, name.trim()) {
            Ok(g) => *status = format!("Added group {}", g.name),
            Err(e) => *status = e,
        }
        close = true;
    }
    if close {
        ctx.data_mut(|d| {
            d.insert_temp(open_key, false);
            d.remove_temp::<String>(name_key);
        });
    }
}

/// Right pane when a group header is selected — rename only.
fn paint_group_rename(
    ui: &mut egui::Ui,
    client: &mut ScenarioClientState,
    pane_w: f32,
    group_id: &str,
    status: &mut String,
) {
    // Ungrouped is implicit — no rename.
    if group_id.is_empty() {
        ui.label(
            RichText::new("Ungrouped — scenarios with no group. Select a named group to rename it.")
                .color(CHROME_MUTED())
                .italics(),
        );
        return;
    }
    let Some(idx) = client.groups.iter().position(|g| g.id == group_id) else {
        ui.label(
            RichText::new("Group not found.")
                .color(CHROME_MUTED())
                .italics(),
        );
        return;
    };
    ui.label(RichText::new("Group").color(CHROME_MUTED()).small());
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new("Name").color(CHROME_MUTED()).small());
        let name_w = (pane_w - 52.0).clamp(80.0, 280.0);
        let mut name = client.groups[idx].name.clone();
        let resp = ui.add(
            egui::TextEdit::singleline(&mut name)
                .desired_width(name_w)
                .hint_text("Group name"),
        );
        if resp.changed() {
            let trimmed = name.trim().to_string();
            if trimmed.is_empty() {
                *status = "Group name cannot be empty".into();
            } else {
                match client.upsert_group(Some(group_id), &trimmed) {
                    Ok(g) => {
                        *status = format!("Renamed group to {}", g.name);
                    }
                    Err(e) => *status = e,
                }
            }
        }
    });
    ui.add_space(8.0);
    ui.label(
        RichText::new("Play / Pause on the left runs every scenario in this group.")
            .color(CHROME_MUTED())
            .small(),
    );
}

fn paint_right(
    ui: &mut egui::Ui,
    client: &mut ScenarioClientState,
    profiles_names: &[(String, String)],
    icons: &IconSet,
    status: &mut String,
) {
    // Capture before any desired_width widgets expand the pane past the viewport
    // (fixed Name/TextEdit widths were shoving step trash off-screen while leaving
    // empty clipped space — grip/check stayed, trash got chopped).
    let pane_w = ui.available_width().max(1.0);
    ui.set_max_width(pane_w);

    // Group selected → rename only (no step editor).
    if client.selected_id.is_none() {
        if let Some(gid) = client.selected_group_id.clone() {
            paint_group_rename(ui, client, pane_w, &gid, status);
            return;
        }
        ui.label(
            RichText::new("Select a scenario on the left to edit its steps.")
                .color(CHROME_MUTED())
                .italics(),
        );
        return;
    }
    let sel_id = client.selected_id.clone().unwrap();

    let mut dirty = false;
    let mut draft = client
        .selected_row()
        .cloned()
        .unwrap_or_else(|| blank_scenario(""));
    // Default profile for new steps = first profile, or first step that has one.
    let default_profile = draft
        .steps
        .iter()
        .find_map(|s| match s {
            ScenarioStep::Publish { profile, .. } | ScenarioStep::WaitValue { profile, .. }
                if !profile.is_empty() =>
            {
                Some(profile.clone())
            }
            _ => None,
        })
        .or_else(|| profiles_names.first().map(|(id, _)| id.clone()))
        .unwrap_or_default();

    // Name + Loop wrap when the right pane is tight so Loop isn't clipped.
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new("Name").color(CHROME_MUTED()).small());
        let name_w = (pane_w - 52.0).clamp(48.0, 200.0);
        if ui
            .add(
                egui::TextEdit::singleline(&mut draft.name)
                    .desired_width(name_w)
                    .hint_text("Scenario name"),
            )
            .changed()
        {
            dirty = true;
        }
        if ui
            .checkbox(&mut draft.loop_, "Loop")
            .on_hover_text("Restart from step 0 when finished")
            .changed()
        {
            dirty = true;
        }
    });

    // Live status line for this runner
    if let Some(st) = client.status_for(&sel_id) {
        let rem = st
            .remaining_ms
            .map(|ms| format!(" · {ms} ms left"))
            .unwrap_or_default();
        let step = st
            .step_index
            .map(|i| format!("step {}", i + 1))
            .unwrap_or_else(|| "—".into());
        ui.label(
            RichText::new(format!(
                "{} · {}{}{}",
                st.state,
                step,
                rem,
                st.last_error
                    .as_ref()
                    .map(|e| format!(" · {e}"))
                    .unwrap_or_default()
            ))
            .color(CHROME_MUTED())
            .small(),
        );
    }
    ui.add_space(4.0);

    // Step toolbar — Publish-sized chrome; flat selectable menu (combo style, not boxed buttons).
    ui.horizontal(|ui| {
        ui.label(RichText::new("Steps").color(CHROME_MUTED()).small());
        // Inset the + glyph; match Publish padding so icon isn't flush to the chrome edge.
        ui.spacing_mut().button_padding = egui::vec2(12.0, 5.0);
        ui.spacing_mut().item_spacing.x = 8.0;
        let add_min = egui::vec2(ADD_STEP_BTN.x.min(pane_w.max(72.0)), ADD_STEP_BTN.y);
        let add_btn = if let Some(tex) = icons.get("add_blue") {
            egui::Button::image_and_text(
                egui::Image::new(tex).fit_to_exact_size(egui::vec2(ADD_STEP_ICON, ADD_STEP_ICON)),
                "Add",
            )
            .min_size(add_min)
        } else {
            egui::Button::new("Add").min_size(add_min)
        };
        egui::menu::menu_custom_button(ui, add_btn, |ui| {
            ui.set_min_width(140.0);
            ui.spacing_mut().item_spacing.y = 2.0;
            for (k, label) in [
                ("publish", "Publish"),
                ("wait_time", "Wait time"),
                ("wait_value", "Wait value"),
            ] {
                // selectable_label = ComboBox / Profiles filter style (no per-row button boxes).
                if ui.selectable_label(false, label).clicked() {
                    draft.steps.push(blank_step(k, &default_profile));
                    dirty = true;
                    client.selected_step = Some(draft.steps.len().saturating_sub(1));
                    ui.close_menu();
                }
            }
        });
    });

    // Step list with drag-reorder + per-row trash.
    let current_step = client.status_for(&sel_id).and_then(|s| s.step_index);
    let mut pending_reorder: Option<(usize, usize, bool)> = None;
    let mut delete_step: Option<usize> = None;

    // Rows span the captured pane width so trash stays on the visible right edge
    // and text ellipsizes into the middle (never leave empty pane while chopping icons).
    egui::ScrollArea::vertical()
        .id_salt("scenarios_steps")
        .auto_shrink([false, false])
        .max_width(pane_w)
        .show(ui, |ui| {
            ui.set_min_width(pane_w);
            ui.set_max_width(pane_w);
            let n = draft.steps.len();
            for i in 0..n {
                let selected = client.selected_step == Some(i);
                let is_current = current_step == Some(i);
                let fill = if selected {
                    CHROME_ACCENT_SOFT()
                } else if is_current {
                    Color32::from_rgb(50, 70, 55)
                } else {
                    CHROME_ROW()
                };
                let kind = draft.steps[i].kind_label().to_string();
                let enabled = draft.steps[i].enabled();
                let name_preview = draft.steps[i].name().to_string();

                let row_w = pane_w.max(ui.available_width());
                let (row_rect, drop_response) =
                    ui.allocate_exact_size(egui::vec2(row_w, ROW_H), Sense::hover());
                ui.painter().rect_filled(row_rect, 4.0, fill);
                if selected {
                    ui.painter().rect_stroke(
                        row_rect,
                        4.0,
                        egui::Stroke::new(1.0, CHROME_ACCENT_BORDER()),
                        egui::StrokeKind::Inside,
                    );
                }

                // Pack chrome first: grip | check | …text… | trash — text yields on narrow.
                let grip_rect = egui::Rect::from_min_size(
                    egui::pos2(row_rect.left() + 2.0, row_rect.top()),
                    egui::vec2(GRIP_W.min(row_rect.width()), ROW_H),
                );
                let trash_left = (row_rect.right() - TRASH_W - 2.0)
                    .max(grip_rect.right() + CHECK_W + 4.0)
                    .min(row_rect.right().max(grip_rect.right()));
                let trash_rect = egui::Rect::from_min_size(
                    egui::pos2(trash_left, row_rect.top()),
                    egui::vec2(
                        TRASH_W.min((row_rect.right() - trash_left).max(0.0)),
                        ROW_H,
                    ),
                );
                let check_w = CHECK_W.min((trash_rect.left() - grip_rect.right() - 4.0).max(0.0));
                let check_rect = egui::Rect::from_min_size(
                    egui::pos2(grip_rect.right() + 2.0, row_rect.top()),
                    egui::vec2(check_w, ROW_H),
                );
                let label_left = check_rect.right() + 4.0;
                let label_right = (trash_rect.left() - 4.0).max(label_left);
                let label_rect = egui::Rect::from_min_max(
                    egui::pos2(label_left, row_rect.top()),
                    egui::pos2(label_right, row_rect.bottom()),
                );
                let body_rect = egui::Rect::from_min_max(
                    egui::pos2(grip_rect.right() + 2.0, row_rect.top()),
                    egui::pos2(trash_rect.left() - 2.0, row_rect.bottom()),
                );

                icons::paint_drag_grip(ui, grip_rect);
                let grip_id = egui::Id::new(("scn_step_grip", &sel_id, i));
                let grip_response = hover_tip::attach(
                    ui.interact(grip_rect, grip_id, Sense::drag())
                        .on_hover_cursor(CursorIcon::Grab),
                    "Drag to reorder",
                );
                if grip_response.dragged() {
                    grip_response.dnd_set_drag_payload(StepDnd(i));
                }
                if ui.ctx().is_being_dragged(grip_id) {
                    if let Some(pos) = ui.ctx().pointer_interact_pos() {
                        let layer = egui::LayerId::new(egui::Order::Tooltip, grip_id);
                        let preview = egui::Rect::from_center_size(pos, egui::vec2(GRIP_W, ROW_H));
                        ui.ctx()
                            .layer_painter(layer)
                            .rect_filled(preview, 2.0, CHROME_HOVER());
                    }
                }

                let mut en = enabled;
                let check_resp = ui.put(
                    check_rect,
                    egui::Checkbox::without_text(&mut en),
                );
                if check_resp.changed() {
                    draft.steps[i].set_enabled(en);
                    dirty = true;
                }
                let label = format!(
                    "{}. {} {}",
                    i + 1,
                    kind,
                    if name_preview.is_empty() {
                        String::new()
                    } else {
                        format!("· {name_preview}")
                    }
                );
                paint_truncated_label(ui, label_rect, label, CHROME_TEXT(), 13.0);
                let body_resp = ui.interact(
                    body_rect,
                    egui::Id::new(("scn_step_body", &sel_id, i)),
                    Sense::click(),
                );
                if body_resp.clicked() {
                    client.selected_step = Some(i);
                }

                // Per-step trash — always drawn in reserved right chrome.
                let trash_resp = ui.interact(
                    trash_rect,
                    egui::Id::new(("scn_step_trash", &sel_id, i)),
                    Sense::click(),
                );
                if let Some(tex) = icons.get("delete_red") {
                    let sz = 14.0_f32;
                    let img = egui::Rect::from_center_size(trash_rect.center(), egui::vec2(sz, sz));
                    ui.painter().image(
                        tex.id(),
                        img,
                        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                        Color32::WHITE,
                    );
                }
                if trash_resp.clicked() {
                    delete_step = Some(i);
                }

                // Drop target insert line.
                if let Some(payload) = drop_response.dnd_hover_payload::<StepDnd>() {
                    if payload.0 != i {
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
                            egui::Stroke::new(2.0, CHROME_ACCENT_BORDER()),
                        );
                    }
                }
                if let Some(payload) = drop_response.dnd_release_payload::<StepDnd>() {
                    if payload.0 != i {
                        let after = drop_response
                            .interact_pointer_pos()
                            .or_else(|| drop_response.hover_pos())
                            .map(|p| p.y > drop_response.rect.center().y)
                            .unwrap_or(false);
                        pending_reorder = Some((payload.0, i, after));
                    }
                }

                // Expanded editor under the row when selected.
                if selected {
                    ui.add_space(2.0);
                    egui::Frame::NONE
                        .fill(CHROME_ROW())
                        .corner_radius(4.0)
                        .inner_margin(egui::Margin::symmetric(8, 6))
                        .show(ui, |ui| {
                            ui.set_max_width((pane_w - 4.0).max(80.0));
                            if edit_step_fields(ui, &mut draft.steps[i], profiles_names) {
                                dirty = true;
                            }
                        });
                    ui.add_space(4.0);
                } else {
                    ui.add_space(2.0);
                }
            }
            if draft.steps.is_empty() {
                ui.label(
                    RichText::new("No steps — Add a Publish, Wait time, or Wait value step.")
                        .color(CHROME_MUTED())
                        .italics(),
                );
            }
        });

    if let Some((from, target, after)) = pending_reorder {
        if from < draft.steps.len() && target < draft.steps.len() {
            let step = draft.steps.remove(from);
            let mut insert_at = if after { target + 1 } else { target };
            if from < insert_at {
                insert_at -= 1;
            }
            insert_at = insert_at.min(draft.steps.len());
            draft.steps.insert(insert_at, step);
            client.selected_step = Some(insert_at);
            dirty = true;
        }
    }
    if let Some(i) = delete_step {
        if i < draft.steps.len() {
            draft.steps.remove(i);
            client.selected_step = None;
            dirty = true;
        }
    }

    if dirty {
        draft.id = sel_id.clone();
        match client.upsert(&draft) {
            Ok(_) => {}
            Err(e) => *status = e,
        }
    }
}

fn edit_step_fields(
    ui: &mut egui::Ui,
    step: &mut ScenarioStep,
    profiles_names: &[(String, String)],
) -> bool {
    let mut dirty = false;
    ui.horizontal(|ui| {
        ui.label(RichText::new("Label").color(CHROME_MUTED()).small());
        let lw = (ui.available_width() - 8.0).clamp(40.0, 140.0);
        if ui
            .add(egui::TextEdit::singleline(step.name_mut()).desired_width(lw))
            .changed()
        {
            dirty = true;
        }
    });
    match step {
        ScenarioStep::Publish {
            topic,
            payload,
            qos,
            retain,
            profile,
            ..
        } => {
            dirty |= edit_step_profile(ui, profile, profiles_names, "pub_profile");
            ui.horizontal(|ui| {
                ui.label(RichText::new("Topic").color(CHROME_MUTED()).small());
                let tw = (ui.available_width() - 8.0).clamp(40.0, 280.0);
                if ui
                    .add(egui::TextEdit::singleline(topic).desired_width(tw))
                    .changed()
                {
                    dirty = true;
                }
            });
            ui.horizontal(|ui| {
                ui.label(RichText::new("Payload").color(CHROME_MUTED()).small());
                let pw = (ui.available_width() - 8.0).max(40.0);
                if ui
                    .add(
                        egui::TextEdit::multiline(payload)
                            .desired_width(pw)
                            .desired_rows(2),
                    )
                    .changed()
                {
                    dirty = true;
                }
            });
            ui.horizontal(|ui| {
                ui.label(RichText::new("QoS").color(CHROME_MUTED()).small());
                for q in 0..=2u8 {
                    if ui.selectable_label(*qos == q, q.to_string()).clicked() {
                        *qos = q;
                        dirty = true;
                    }
                }
                if ui.checkbox(retain, "Retain").changed() {
                    dirty = true;
                }
            });
        }
        ScenarioStep::WaitTime {
            duration_ms,
            max_ms,
            ..
        } => {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Duration ms").color(CHROME_MUTED()).small());
                let mut d = *duration_ms as i64;
                if ui
                    .add(egui::DragValue::new(&mut d).range(0..=3_600_000))
                    .changed()
                {
                    *duration_ms = d.max(0) as u64;
                    dirty = true;
                }
                ui.label(
                    RichText::new("Max ms (optional)")
                        .color(CHROME_MUTED())
                        .small(),
                );
                let mut use_max = max_ms.is_some();
                if ui.checkbox(&mut use_max, "").changed() {
                    *max_ms = if use_max {
                        Some((*duration_ms).saturating_add(500))
                    } else {
                        None
                    };
                    dirty = true;
                }
                if let Some(m) = max_ms.as_mut() {
                    let mut mv = *m as i64;
                    if ui
                        .add(egui::DragValue::new(&mut mv).range(0..=3_600_000))
                        .changed()
                    {
                        *m = mv.max(0) as u64;
                        dirty = true;
                    }
                }
            });
            ui.label(
                RichText::new("When max is set, each run picks a random delay in [duration, max].")
                    .color(CHROME_MUTED())
                    .small(),
            );
        }
        ScenarioStep::WaitValue {
            topic,
            field,
            op,
            value,
            match_retained,
            profile,
            ..
        } => {
            dirty |= edit_step_profile(ui, profile, profiles_names, "wv_profile");
            ui.horizontal(|ui| {
                ui.label(RichText::new("Topic").color(CHROME_MUTED()).small());
                let tw = (ui.available_width() * 0.55).clamp(40.0, 220.0);
                if ui
                    .add(egui::TextEdit::singleline(topic).desired_width(tw))
                    .changed()
                {
                    dirty = true;
                }
                ui.label(RichText::new("Field").color(CHROME_MUTED()).small());
                let fw = (ui.available_width() - 8.0).clamp(40.0, 120.0);
                if ui
                    .add(
                        egui::TextEdit::singleline(field)
                            .desired_width(fw)
                            .hint_text("optional"),
                    )
                    .changed()
                {
                    dirty = true;
                }
            });
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("Operation").color(CHROME_MUTED()).small());
                // Symbols for numeric/eq; contains for string substring match.
                let ops = [
                    (WaitOp::Eq, "="),
                    (WaitOp::Neq, "!="),
                    (WaitOp::Gt, ">"),
                    (WaitOp::Gte, ">="),
                    (WaitOp::Lt, "<"),
                    (WaitOp::Lte, "<="),
                    (WaitOp::Contains, "contains"),
                    (WaitOp::NotContains, "does not contain"),
                ];
                for (cand, label) in ops {
                    if ui.selectable_label(*op == cand, label).clicked() {
                        *op = cand;
                        dirty = true;
                    }
                }
            });
            ui.horizontal(|ui| {
                ui.label(RichText::new("Value").color(CHROME_MUTED()).small());
                let vw = (ui.available_width() - 8.0).clamp(40.0, 200.0);
                if ui
                    .add(egui::TextEdit::singleline(value).desired_width(vw))
                    .changed()
                {
                    dirty = true;
                }
                if ui
                    .checkbox(match_retained, "Match retained")
                    .on_hover_text("Current last/retained value can satisfy immediately")
                    .changed()
                {
                    dirty = true;
                }
            });
            ui.label(
                RichText::new("Waits until match — or Pause / Stop. No timeout.")
                    .color(CHROME_MUTED())
                    .small(),
            );
        }
    }
    dirty
}

/// Tree/list disclosure: closed = ▸ (right), open = ▾ (down). Theme stroke, not egui default.
fn paint_disclosure_chevron(ui: &mut egui::Ui, rect: egui::Rect, open: bool, hovered: bool) {
    let color = if hovered {
        CHROME_TEXT()
    } else {
        CHROME_MUTED()
    };
    let stroke = egui::Stroke::new(1.6, color);
    let c = rect.center();
    let s = 4.0_f32;
    let painter = ui.painter();
    if open {
        // Down chevron: two legs of a V.
        painter.line_segment([egui::pos2(c.x - s, c.y - s * 0.35), egui::pos2(c.x, c.y + s * 0.55)], stroke);
        painter.line_segment([egui::pos2(c.x + s, c.y - s * 0.35), egui::pos2(c.x, c.y + s * 0.55)], stroke);
    } else {
        // Right chevron: two legs of a >.
        painter.line_segment([egui::pos2(c.x - s * 0.35, c.y - s), egui::pos2(c.x + s * 0.55, c.y)], stroke);
        painter.line_segment([egui::pos2(c.x - s * 0.35, c.y + s), egui::pos2(c.x + s * 0.55, c.y)], stroke);
    }
}

/// Single-line label clipped with ellipsis so it never paints past `rect`.
/// Painter-only — does not expand parent layout (avoids shoving trash off-screen).
fn paint_truncated_label(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    text: impl Into<String>,
    color: Color32,
    size: f32,
) {
    let max_w = rect.width().max(0.0);
    if max_w < 4.0 || !ui.is_rect_visible(rect) {
        return;
    }
    let mut job = egui::text::LayoutJob {
        wrap: egui::text::TextWrapping {
            max_width: max_w,
            max_rows: 1,
            break_anywhere: true,
            overflow_character: Some('…'),
        },
        ..Default::default()
    };
    job.append(
        &text.into(),
        0.0,
        egui::TextFormat {
            font_id: egui::FontId::proportional(size),
            color,
            ..Default::default()
        },
    );
    let galley = ui.fonts(|f| f.layout_job(job));
    let painter = ui.painter().with_clip_rect(rect);
    painter.galley(
        egui::pos2(rect.left(), rect.center().y - galley.size().y * 0.5),
        galley,
        color,
    );
}

fn edit_step_profile(
    ui: &mut egui::Ui,
    profile: &mut String,
    profiles_names: &[(String, String)],
    id_salt: &str,
) -> bool {
    let mut dirty = false;
    ui.horizontal(|ui| {
        ui.label(RichText::new("Profile").color(CHROME_MUTED()).small());
        let label = if profile.is_empty() {
            "(choose profile)".to_string()
        } else {
            profiles_names
                .iter()
                .find(|(id, _)| id == profile)
                .map(|(_, n)| n.clone())
                .unwrap_or_else(|| profile.clone())
        };
        let cw = (ui.available_width() - 8.0).clamp(40.0, 200.0);
        egui::ComboBox::from_id_salt((id_salt, profile.as_str()))
            .selected_text(label)
            .width(cw)
            .show_ui(ui, |ui| {
                for (id, name) in profiles_names {
                    if ui.selectable_label(profile == id, name).clicked() {
                        *profile = id.clone();
                        dirty = true;
                    }
                }
            });
    });
    dirty
}

fn handle_import_export(
    ui: &mut egui::Ui,
    client: &mut ScenarioClientState,
    icons: &IconSet,
    status: &mut String,
) {
    let export_dlg = egui::Id::new("dlg_scn_export");
    let export_text_key = egui::Id::new("dlg_scn_export_text");
    let export_modal_key = egui::Id::new("scn_export_modal_open");
    let export_scope_key = egui::Id::new("scn_export_scope");
    let import_dlg = egui::Id::new("dlg_scn_import");
    let import_path_key = egui::Id::new("dlg_scn_import_path");
    let import_modal_key = egui::Id::new("scn_import_modal_open");
    let import_choice_key = egui::Id::new("scn_import_choice");
    let armed = client.toolbar_armed();

    if icons
        .tiny_header(ui, "export", false, "Export scenarios")
        .clicked()
        && armed
    {
        let default = if client.selected_group_id.is_some() {
            ExportScopeChoice::SelectedGroup
        } else {
            ExportScopeChoice::Everything
        };
        ui.ctx().data_mut(|d| {
            d.insert_temp(export_modal_key, true);
            d.insert_temp(export_scope_key, default);
        });
    }

    // Export modal (scope) then native SaveFile.
    let export_open = ui
        .ctx()
        .data(|d| d.get_temp::<bool>(export_modal_key))
        .unwrap_or(false);
    if export_open {
        let has_group = client.selected_group_id.is_some();
        let mut scope = ui
            .ctx()
            .data(|d| d.get_temp::<ExportScopeChoice>(export_scope_key))
            .unwrap_or(if has_group {
                ExportScopeChoice::SelectedGroup
            } else {
                ExportScopeChoice::Everything
            });
        let mut close = false;
        let mut do_export = false;
        egui::Modal::new(egui::Id::new("topicbench_scn_export")).show(ui.ctx(), |ui| {
            ui.set_min_width(380.0);
            ui.set_max_width(420.0);
            icons::section_header_bar(ui, |ui| {
                icons::section_icon(ui, icons, "export");
                icons::section_title(ui, "Export scenarios");
            });
            ui.add_space(6.0);
            let bar_w = ui.available_width();
            let (bar, _) = ui.allocate_exact_size(egui::vec2(bar_w, 3.0), egui::Sense::hover());
            ui.painter().rect_filled(bar, 1.5, CHROME_ACCENT_BORDER());
            ui.add_space(12.0);
            ui.label(RichText::new("Scope").strong());
            ui.add_space(4.0);
            ui.add_enabled_ui(has_group, |ui| {
                ui.radio_value(
                    &mut scope,
                    ExportScopeChoice::SelectedGroup,
                    "Selected group",
                );
            });
            if !has_group {
                ui.label(
                    RichText::new("Select a group (or Ungrouped) on the left")
                        .small()
                        .color(CHROME_MUTED()),
                );
            }
            ui.radio_value(&mut scope, ExportScopeChoice::Everything, "Everything");
            ui.add_space(14.0);
            ui.separator();
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    let can = has_group || scope == ExportScopeChoice::Everything;
                    let export_btn = if let Some(tex) = icons.get("export") {
                        ui.add_enabled(
                            can,
                            egui::Button::image_and_text(
                                egui::Image::new(tex)
                                    .fit_to_exact_size(egui::vec2(TINY_ICON, TINY_ICON)),
                                RichText::new("Export…").strong(),
                            )
                            .fill(CHROME_ACCENT_SOFT())
                            .stroke(egui::Stroke::new(1.0_f32, CHROME_ACCENT_BORDER()))
                            .min_size(egui::vec2(120.0, 32.0)),
                        )
                    } else {
                        ui.add_enabled(
                            can,
                            egui::Button::new(RichText::new("Export…").strong())
                                .fill(CHROME_ACCENT_SOFT())
                                .stroke(egui::Stroke::new(1.0_f32, CHROME_ACCENT_BORDER()))
                                .min_size(egui::vec2(120.0, 32.0)),
                        )
                    };
                    if export_btn.clicked() {
                        do_export = true;
                    }
                    let cancel = if let Some(tex) = icons.get("close_red") {
                        ui.add(
                            egui::Button::image_and_text(
                                egui::Image::new(tex)
                                    .fit_to_exact_size(egui::vec2(TINY_ICON, TINY_ICON)),
                                "Cancel",
                            )
                            .min_size(egui::vec2(104.0, 32.0)),
                        )
                    } else {
                        ui.add(egui::Button::new("Cancel").min_size(egui::vec2(104.0, 32.0)))
                    };
                    if cancel.clicked() {
                        close = true;
                    }
                });
            });
        });
        ui.ctx().data_mut(|d| d.insert_temp(export_scope_key, scope));
        if do_export {
            let fetched = match scope {
                ExportScopeChoice::Everything => client.export_all(),
                ExportScopeChoice::SelectedGroup => {
                    let gid = client.selected_group_id.clone().unwrap_or_default();
                    client.export_group(&gid)
                }
            };
            match fetched {
                Ok(v) => match serde_json::to_string_pretty(&v) {
                    Ok(text) => {
                        ui.ctx().data_mut(|d| d.insert_temp(export_text_key, text));
                        crate::ui::native_dialog::begin_path(
                            ui.ctx(),
                            export_dlg,
                            crate::ui::native_dialog::PathOp::SaveFile,
                            |d| {
                                d.set_title("Export scenarios")
                                    .set_file_name("scenarios.json")
                                    .add_filter("JSON", &["json"])
                            },
                        );
                        close = true;
                    }
                    Err(e) => *status = format!("json: {e}"),
                },
                Err(e) => *status = e,
            }
        }
        if close {
            ui.ctx()
                .data_mut(|d| d.insert_temp(export_modal_key, false));
        }
    }
    if let Some(path_opt) = crate::ui::native_dialog::poll_path(ui.ctx(), export_dlg) {
        if let Some(path) = path_opt {
            if let Some(text) = ui
                .ctx()
                .data_mut(|d| d.remove_temp::<String>(export_text_key))
            {
                match std::fs::write(&path, format!("{text}\n")) {
                    Ok(()) => *status = format!("Exported scenarios → {}", path.display()),
                    Err(e) => *status = format!("Export write failed: {e}"),
                }
            }
        } else {
            let _ = ui
                .ctx()
                .data_mut(|d| d.remove_temp::<String>(export_text_key));
        }
    }

    if icons
        .tiny_header(ui, "import", false, "Import scenarios")
        .clicked()
        && armed
    {
        crate::ui::native_dialog::begin_path(
            ui.ctx(),
            import_dlg,
            crate::ui::native_dialog::PathOp::PickFile,
            |d| {
                d.set_title("Import scenarios")
                    .add_filter("JSON", &["json"])
            },
        );
    }
    if let Some(Some(path)) = crate::ui::native_dialog::poll_path(ui.ctx(), import_dlg) {
        let default = ImportModeChoice::Merge;
        ui.ctx().data_mut(|d| {
            d.insert_temp(import_path_key, path);
            d.insert_temp(import_modal_key, true);
            d.insert_temp(import_choice_key, default);
        });
    }

    // Import modal (replace / merge / into group).
    let import_open = ui
        .ctx()
        .data(|d| d.get_temp::<bool>(import_modal_key))
        .unwrap_or(false);
    if import_open {
        let groups = client.groups.clone();
        let mut choice = ui
            .ctx()
            .data(|d| d.get_temp::<ImportModeChoice>(import_choice_key))
            .unwrap_or(ImportModeChoice::Merge);
        let mut close = false;
        let mut do_import = false;
        egui::Modal::new(egui::Id::new("topicbench_scn_import")).show(ui.ctx(), |ui| {
            ui.set_min_width(400.0);
            ui.set_max_width(440.0);
            icons::section_header_bar(ui, |ui| {
                icons::section_icon(ui, icons, "import");
                icons::section_title(ui, "Import scenarios");
            });
            ui.add_space(6.0);
            let bar_w = ui.available_width();
            let (bar, _) = ui.allocate_exact_size(egui::vec2(bar_w, 3.0), egui::Sense::hover());
            ui.painter().rect_filled(bar, 1.5, CHROME_ACCENT_BORDER());
            ui.add_space(12.0);
            ui.label(RichText::new("Mode").strong());
            ui.add_space(4.0);
            if ui
                .radio(
                    matches!(choice, ImportModeChoice::Replace),
                    "Replace everything",
                )
                .clicked()
            {
                choice = ImportModeChoice::Replace;
            }
            if ui
                .radio(
                    matches!(choice, ImportModeChoice::Merge),
                    "Merge everything",
                )
                .clicked()
            {
                choice = ImportModeChoice::Merge;
            }
            let into_sel = matches!(choice, ImportModeChoice::IntoGroup { .. });
            if ui.radio(into_sel, "Into a group").clicked() {
                choice = ImportModeChoice::IntoGroup {
                    group_id: String::new(),
                    new_name: String::new(),
                };
            }
            if let ImportModeChoice::IntoGroup {
                group_id,
                new_name,
            } = &mut choice
            {
                ui.add_space(6.0);
                ui.indent("scn_into_group", |ui| {
                    let label = if group_id.is_empty() {
                        "New group…".to_string()
                    } else {
                        groups
                            .iter()
                            .find(|g| g.id == *group_id)
                            .map(|g| g.name.clone())
                            .unwrap_or_else(|| group_id.clone())
                    };
                    egui::ComboBox::from_id_salt("scn_import_into_group")
                        .selected_text(label)
                        .width(ui.available_width().min(280.0))
                        .show_ui(ui, |ui| {
                            if ui
                                .selectable_label(group_id.is_empty(), "New group…")
                                .clicked()
                            {
                                group_id.clear();
                            }
                            for g in &groups {
                                if ui.selectable_label(group_id == &g.id, &g.name).clicked() {
                                    *group_id = g.id.clone();
                                }
                            }
                        });
                    if group_id.is_empty() {
                        ui.add_space(4.0);
                        ui.add(
                            egui::TextEdit::singleline(new_name)
                                .desired_width(ui.available_width())
                                .hint_text("New group name"),
                        );
                    }
                });
            }
            ui.add_space(14.0);
            ui.separator();
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    let into_ok = match &choice {
                        ImportModeChoice::IntoGroup {
                            group_id,
                            new_name,
                        } => !group_id.is_empty() || !new_name.trim().is_empty(),
                        _ => true,
                    };
                    let import_btn = if let Some(tex) = icons.get("import") {
                        ui.add_enabled(
                            into_ok,
                            egui::Button::image_and_text(
                                egui::Image::new(tex)
                                    .fit_to_exact_size(egui::vec2(TINY_ICON, TINY_ICON)),
                                RichText::new("Import").strong(),
                            )
                            .fill(CHROME_ACCENT_SOFT())
                            .stroke(egui::Stroke::new(1.0_f32, CHROME_ACCENT_BORDER()))
                            .min_size(egui::vec2(120.0, 32.0)),
                        )
                    } else {
                        ui.add_enabled(
                            into_ok,
                            egui::Button::new(RichText::new("Import").strong())
                                .fill(CHROME_ACCENT_SOFT())
                                .stroke(egui::Stroke::new(1.0_f32, CHROME_ACCENT_BORDER()))
                                .min_size(egui::vec2(120.0, 32.0)),
                        )
                    };
                    if import_btn.clicked() {
                        do_import = true;
                    }
                    let cancel = if let Some(tex) = icons.get("close_red") {
                        ui.add(
                            egui::Button::image_and_text(
                                egui::Image::new(tex)
                                    .fit_to_exact_size(egui::vec2(TINY_ICON, TINY_ICON)),
                                "Cancel",
                            )
                            .min_size(egui::vec2(104.0, 32.0)),
                        )
                    } else {
                        ui.add(egui::Button::new("Cancel").min_size(egui::vec2(104.0, 32.0)))
                    };
                    if cancel.clicked() {
                        close = true;
                    }
                });
            });
        });
        ui.ctx()
            .data_mut(|d| d.insert_temp(import_choice_key, choice.clone()));
        if do_import {
            if let Some(path) = ui
                .ctx()
                .data_mut(|d| d.remove_temp::<std::path::PathBuf>(import_path_key))
            {
                finish_scenario_import(client, status, &path, &choice);
            }
            close = true;
        }
        if close {
            ui.ctx().data_mut(|d| {
                d.insert_temp(import_modal_key, false);
                d.remove_temp::<std::path::PathBuf>(import_path_key);
            });
        }
    }
}

fn finish_scenario_import(
    client: &mut ScenarioClientState,
    status: &mut String,
    path: &std::path::Path,
    choice: &ImportModeChoice,
) {
    let (mode, gid, gname) = match choice {
        ImportModeChoice::Replace => ("replace", None, None),
        ImportModeChoice::Merge => ("merge", None, None),
        ImportModeChoice::IntoGroup {
            group_id,
            new_name,
        } => {
            if group_id.is_empty() {
                ("into_group", None, Some(new_name.as_str()))
            } else {
                ("into_group", Some(group_id.as_str()), None)
            }
        }
    };
    match std::fs::read_to_string(path) {
        Ok(text) => match serde_json::from_str::<serde_json::Value>(&text) {
            Ok(v) => match client.import_mode(&v, mode, gid, gname) {
                Ok(n) => *status = format!("Imported {n} scenarios ({mode})"),
                Err(e) => *status = e,
            },
            Err(e) => *status = format!("Import JSON: {e}"),
        },
        Err(e) => *status = format!("Import read: {e}"),
    }
}

/// Unused helper kept for docked layout symmetry with Simulation (future).
#[allow(dead_code)]
pub fn draw_docked(
    ui: &mut egui::Ui,
    client: &mut ScenarioClientState,
    profiles_names: &[(String, String)],
    icons: &IconSet,
    status: &mut String,
    left_width: &mut f32,
    geom_dirty: &mut bool,
) {
    let mut close = false;
    icons::apply_pro_panel_chrome(ui);
    paint_body(
        ui,
        client,
        profiles_names,
        icons,
        status,
        left_width,
        geom_dirty,
        &mut close,
    );
    let _ = close;
}
