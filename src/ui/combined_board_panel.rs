//! Combined graph boards — Pro MultiGraph OS pop-outs (Simulation undock pattern).

use crate::mqtt::pins::HistorySample;
use crate::mqtt::session::SessionState;
use crate::mqtt::window_geom::WinGeom;
use crate::pro::combined_boards::{
    clamp_height, CombinedBoardsStore, CombinedTimeRange,
};
use crate::ui::flash;
use crate::ui::icons::{
    self, IconSet, CHROME_ACCENT_BORDER, CHROME_ACCENT_SOFT, CHROME_BG, CHROME_CAPTION,
    CHROME_COMBO_BG, CHROME_MUTED, CHROME_SEP, CHROME_TEXT, TINY_ICON,
};
use eframe::egui::{
    self, Align, Color32, Pos2, RichText, Sense, Stroke, ViewportBuilder,
    ViewportClass,
};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::LazyLock;
use std::time::{Duration, Instant, SystemTime};

const SERIES_PALETTE: [Color32; 8] = [
    Color32::from_rgb(80, 180, 120),
    Color32::from_rgb(100, 160, 220),
    Color32::from_rgb(220, 160, 80),
    Color32::from_rgb(200, 100, 160),
    Color32::from_rgb(120, 200, 200),
    Color32::from_rgb(180, 140, 220),
    Color32::from_rgb(220, 120, 100),
    Color32::from_rgb(160, 180, 100),
];

const Y_LABEL_W: f32 = 56.0;
const X_LABEL_H: f32 = 18.0;
const Y_TICKS: usize = 4;
const MIN_VP: [f32; 2] = [480.0, 260.0];

/// OS / section title: board name alone when it is already "Combined";
/// otherwise `Combined: {name}` so custom library names stay labeled.
fn chrome_title(board_title: &str) -> String {
    let t = board_title.trim();
    if t.eq_ignore_ascii_case("combined") {
        "Combined".into()
    } else {
        format!("Combined: {t}")
    }
}

/// One plotted series (board-owned buffer; sticky optional).
struct SeriesPaint<'a> {
    profile_id: &'a str,
    topic: &'a str,
    profile_name: &'a str,
    history: &'a VecDeque<HistorySample>,
    color: Color32,
    offline: bool,
}

/// Draw every open Combined board as its own OS viewport.
#[allow(clippy::too_many_arguments)]
pub fn draw_viewports(
    ctx: &egui::Context,
    store: &mut CombinedBoardsStore,
    sessions: &crate::mqtt::session::SessionManager,
    profiles: &crate::mqtt::profile::ProfileStore,
    icons: &IconSet,
    status: &mut String,
    builder_seeds: &mut HashMap<String, WinGeom>,
    geom_dirty: &mut bool,
) {
    let open_ids = store.open_board_ids();
    if open_ids.is_empty() {
        builder_seeds.clear();
        return;
    }

    let connected: HashSet<String> = sessions
        .open
        .iter()
        .filter(|(_, s)| s.state == SessionState::Connected)
        .map(|(id, _)| id.clone())
        .collect();

    // Profile display names for offline legend tooltips.
    let mut name_by_id: HashMap<String, String> = HashMap::new();
    for (id, p) in &profiles.profiles {
        name_by_id.insert(id.clone(), p.name.clone());
    }

    let mut closed: Vec<String> = Vec::new();
    let mut persist = false;

    for board_id in open_ids {
        let Some(board) = store.get(&board_id) else {
            continue;
        };
        let title = chrome_title(&board.display_title());
        let saved = board.window_geom.unwrap_or_else(|| WinGeom::size(720.0, 360.0));
        let seed = builder_seeds.entry(board_id.clone()).or_insert(saved);
        let mut close = false;
        let mut focused = false;
        let mut local_persist = false;
        let mut local_status: Option<String> = None;

        // Snapshot mutable fields we need inside the viewport closure via temps.
        let vp_id = egui::ViewportId::from_hash_of(format!("topicbench_combined_{board_id}"));

        ctx.show_viewport_immediate(
            vp_id,
            seed.apply(
                ViewportBuilder::default().with_title(title.clone()),
                MIN_VP,
            )
            .with_min_inner_size(MIN_VP),
            |ctx, class| {
                if ctx.input(|i| i.focused) {
                    focused = true;
                }
                let mut paint = |ui: &mut egui::Ui| {
                    icons::apply_pro_panel_chrome(ui);
                    paint_board(
                        ui,
                        store,
                        &board_id,
                        &connected,
                        &name_by_id,
                        icons,
                        &mut close,
                        &mut local_persist,
                        &mut local_status,
                    );
                };
                match class {
                    ViewportClass::Embedded => {
                        egui::Window::new(title.clone())
                            .id(egui::Id::new(("combined_win", &board_id)))
                            .title_bar(false)
                            .collapsible(false)
                            .default_size([720.0, 360.0])
                            .min_width(MIN_VP[0])
                            .min_height(MIN_VP[1])
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
                    if let Some(board) = store.get_mut(&board_id) {
                        if crate::mqtt::window_geom::sync_slot(
                            &mut board.window_geom,
                            g,
                            MIN_VP,
                        ) {
                            *geom_dirty = true;
                            local_persist = true;
                        }
                    }
                }
            },
        );

        if focused {
            store.mark_focused(&board_id);
        }
        if let Some(s) = local_status {
            *status = s;
        }
        if local_persist {
            persist = true;
        }
        if close {
            closed.push(board_id);
        }
    }

    for id in closed {
        store.close_board(&id);
        builder_seeds.remove(&id);
        persist = true;
    }
    if persist {
        store.mark_dirty();
        let _ = store.save_session();
    }
}

#[allow(clippy::too_many_arguments)]
fn paint_board(
    ui: &mut egui::Ui,
    store: &mut CombinedBoardsStore,
    board_id: &str,
    connected: &HashSet<String>,
    name_by_id: &HashMap<String, String>,
    icons: &IconSet,
    close: &mut bool,
    persist: &mut bool,
    status: &mut Option<String>,
) {
    // Snapshot chrome fields so we can mutate the store freely afterward.
    let (title, plot_h, time_range, series_count, y_min, y_max) = {
        let Some(board) = store.get(board_id) else {
            return;
        };
        (
            board.display_title(),
            clamp_height(board.plot_height),
            board.time_range,
            board.series.len(),
            board.y_min,
            board.y_max,
        )
    };

    // Section chrome header (matches Simulation / Export panels).
    let save_id = egui::Id::new(("combined_save_open", board_id));
    let open_id = egui::Id::new(("combined_lib_open", board_id));
    let header_title = chrome_title(&title);
    let png_dlg = egui::Id::new(("dlg_combined_png", board_id));
    let png_h_key = egui::Id::new(("dlg_combined_png_h", board_id));
    // Capture before the header strip shrinks available height.
    let export_plot_h = ui.available_height().max(120.0);
    icons::section_header_bar(ui, |ui| {
        // Right icons first (fixed strip) so they stay one horizontal row when
        // the title/combo would otherwise consume the remaining width.
        ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
            let icon_gap = 4.0;
            let icon_cell = TINY_ICON + 4.0; // tiny_header pad 2px each side
            let n_icons = if series_count > 0 { 5.0 } else { 4.0 };
            let strip_w = n_icons * icon_cell + (n_icons - 1.0) * icon_gap;
            ui.allocate_ui_with_layout(
                egui::vec2(strip_w, icon_cell),
                egui::Layout::right_to_left(Align::Center),
                |ui| {
                    ui.set_max_size(egui::vec2(strip_w, icon_cell));
                    ui.spacing_mut().item_spacing = egui::vec2(icon_gap, 0.0);
                    // RTL paint order → visual LTR: save | delete | import | export | close
                    if icons
                        .tiny_header(ui, "close_red", false, "Close board")
                        .clicked()
                    {
                        *close = true;
                    }
                    if series_count > 0
                        && icons
                            .tiny_header(ui, "export", false, "Save Combined graph as PNG")
                            .clicked()
                    {
                        ui.ctx().data_mut(|d| d.insert_temp(png_h_key, export_plot_h));
                        let name = combined_export_filename();
                        crate::ui::native_dialog::begin_path(
                            ui.ctx(),
                            png_dlg,
                            crate::ui::native_dialog::PathOp::SaveFile,
                            move |d| {
                                d.set_title("Save Combined graph")
                                    .set_file_name(&name)
                                    .add_filter("PNG image", &["png"])
                            },
                        );
                    }
                    if icons
                        .tiny_header(ui, "import", false, "Recall a saved board")
                        .clicked()
                    {
                        ui.ctx().data_mut(|d| d.insert_temp(open_id, true));
                    }
                    if icons
                        .tiny_header(ui, "delete", false, "Clear display buffer (keep series)")
                        .clicked()
                    {
                        store.clear_display_buffer(board_id);
                        *status = Some("Combined display buffer cleared".into());
                    }
                    if icons
                        .tiny_header(ui, "save", false, "Save board to library")
                        .clicked()
                    {
                        let draft = store
                            .get(board_id)
                            .map(|b| b.display_title())
                            .unwrap_or_else(|| "Combined".into());
                        ui.ctx().data_mut(|d| d.insert_temp(save_id, draft));
                    }
                },
            );

            // Left chrome fills remaining width.
            ui.with_layout(egui::Layout::left_to_right(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                icons::section_icon(ui, icons, "node_graph");
                icons::section_title(ui, &header_title);
                if series_count > 0 {
                    ui.label(
                        RichText::new(format!("{series_count} series"))
                            .small()
                            .color(CHROME_CAPTION()),
                    );
                }
                ui.add_space(8.0);
                let mut tr = time_range;
                egui::ComboBox::from_id_salt(("combined_tr", board_id))
                    .selected_text(tr.label())
                    .width(100.0)
                    .show_ui(ui, |ui| {
                        for opt in CombinedTimeRange::ALL {
                            ui.selectable_value(&mut tr, opt, opt.label());
                        }
                    });
                if tr != time_range {
                    if let Some(b) = store.get_mut(board_id) {
                        b.time_range = tr;
                        *persist = true;
                    }
                }
            });
        });
    });

    // Save-name popup — Export / Playback section chrome.
    if let Some(mut name) = ui.ctx().data(|d| d.get_temp::<String>(save_id)) {
        let mut dismiss = false;
        let mut do_save = false;
        egui::Window::new("combined_save_chrome")
            .id(egui::Id::new(("combined_save_win", board_id)))
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .frame(
                egui::Frame::NONE
                    .fill(CHROME_BG())
                    .stroke(egui::Stroke::new(1.0_f32, CHROME_SEP()))
                    .corner_radius(8.0)
                    .inner_margin(egui::Margin::symmetric(16, 14)),
            )
            .show(ui.ctx(), |ui| {
                ui.set_min_width(360.0);
                ui.set_max_width(420.0);
                icons::section_header_bar(ui, |ui| {
                    icons::section_icon(ui, icons, "save");
                    icons::section_title(ui, "Save Combined board");
                });
                ui.add_space(6.0);
                let bar_w = ui.available_width();
                let (bar, _) =
                    ui.allocate_exact_size(egui::vec2(bar_w, 3.0), Sense::hover());
                ui.painter().rect_filled(bar, 1.5, CHROME_ACCENT_BORDER());
                ui.add_space(12.0);
                ui.label(RichText::new("Library name").strong().color(CHROME_TEXT()));
                ui.add_space(4.0);
                ui.add(
                    egui::TextEdit::singleline(&mut name)
                        .desired_width(ui.available_width())
                        .text_color(CHROME_TEXT()),
                );
                ui.add_space(14.0);
                ui.separator();
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 8.0;
                        let save_btn = if let Some(tex) = icons.get("save") {
                            ui.add(
                                egui::Button::image_and_text(
                                    egui::Image::new(tex)
                                        .fit_to_exact_size(egui::vec2(TINY_ICON, TINY_ICON)),
                                    RichText::new("Save").strong(),
                                )
                                .fill(CHROME_ACCENT_SOFT())
                                .stroke(egui::Stroke::new(1.0_f32, CHROME_ACCENT_BORDER()))
                                .min_size(egui::vec2(120.0, 32.0)),
                            )
                        } else {
                            ui.add(
                                egui::Button::new(RichText::new("Save").strong())
                                    .fill(CHROME_ACCENT_SOFT())
                                    .stroke(egui::Stroke::new(1.0_f32, CHROME_ACCENT_BORDER()))
                                    .min_size(egui::vec2(120.0, 32.0)),
                            )
                        };
                        if save_btn.clicked() {
                            do_save = true;
                        }
                        let cancel_btn = if let Some(tex) = icons.get("close_red") {
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
                        if cancel_btn.clicked() {
                            dismiss = true;
                        }
                    });
                });
            });
        if do_save {
            match store.save_board_as(board_id, &name) {
                Ok(()) => {
                    *status = Some(format!("Saved Combined board “{name}”"));
                    dismiss = true;
                }
                Err(e) => *status = Some(format!("Save failed: {e}")),
            }
        }
        if dismiss {
            ui.ctx().data_mut(|d| d.remove_temp::<String>(save_id));
        } else {
            ui.ctx().data_mut(|d| d.insert_temp(save_id, name));
        }
    }

    // Open / recall popup — same chrome as Save.
    let show_open = ui
        .ctx()
        .data(|d| d.get_temp::<bool>(open_id))
        .unwrap_or(false);
    if show_open {
        let names: Vec<String> = store.library.iter().map(|e| e.name.clone()).collect();
        let pick_id = egui::Id::new(("combined_lib_pick", board_id));
        let mut pick = ui
            .ctx()
            .data(|d| d.get_temp::<String>(pick_id))
            .unwrap_or_else(|| names.first().cloned().unwrap_or_default());
        let mut dismiss = false;
        let mut do_open = false;
        let mut do_delete = false;
        egui::Window::new("combined_open_chrome")
            .id(egui::Id::new(("combined_open_win", board_id)))
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .frame(
                egui::Frame::NONE
                    .fill(CHROME_BG())
                    .stroke(egui::Stroke::new(1.0_f32, CHROME_SEP()))
                    .corner_radius(8.0)
                    .inner_margin(egui::Margin::symmetric(16, 14)),
            )
            .show(ui.ctx(), |ui| {
                ui.set_min_width(360.0);
                ui.set_max_width(420.0);
                icons::section_header_bar(ui, |ui| {
                    icons::section_icon(ui, icons, "import");
                    icons::section_title(ui, "Open Combined board");
                });
                ui.add_space(6.0);
                let bar_w = ui.available_width();
                let (bar, _) =
                    ui.allocate_exact_size(egui::vec2(bar_w, 3.0), Sense::hover());
                ui.painter().rect_filled(bar, 1.5, CHROME_ACCENT_BORDER());
                ui.add_space(12.0);
                if names.is_empty() {
                    ui.label(
                        RichText::new("No saved boards yet — use Save first.")
                            .color(CHROME_MUTED()),
                    );
                } else {
                    ui.label(RichText::new("Saved board").strong().color(CHROME_TEXT()));
                    ui.add_space(4.0);
                    egui::ComboBox::from_id_salt(("combined_lib_cb", board_id))
                        .selected_text(&pick)
                        .width(ui.available_width().min(280.0))
                        .show_ui(ui, |ui| {
                            for n in &names {
                                ui.selectable_value(&mut pick, n.clone(), n);
                            }
                        });
                }
                ui.add_space(14.0);
                ui.separator();
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 8.0;
                        let can = !names.is_empty();
                        let open_btn = if let Some(tex) = icons.get("import") {
                            ui.add_enabled(
                                can,
                                egui::Button::image_and_text(
                                    egui::Image::new(tex)
                                        .fit_to_exact_size(egui::vec2(TINY_ICON, TINY_ICON)),
                                    RichText::new("Open").strong(),
                                )
                                .fill(CHROME_ACCENT_SOFT())
                                .stroke(egui::Stroke::new(1.0_f32, CHROME_ACCENT_BORDER()))
                                .min_size(egui::vec2(120.0, 32.0)),
                            )
                        } else {
                            ui.add_enabled(
                                can,
                                egui::Button::new(RichText::new("Open").strong())
                                    .fill(CHROME_ACCENT_SOFT())
                                    .stroke(egui::Stroke::new(1.0_f32, CHROME_ACCENT_BORDER()))
                                    .min_size(egui::vec2(120.0, 32.0)),
                            )
                        };
                        if open_btn.clicked() {
                            do_open = true;
                        }
                        if can
                            && icons
                                .get("delete_red")
                                .map(|tex| {
                                    ui.add(
                                        egui::Button::image_and_text(
                                            egui::Image::new(tex).fit_to_exact_size(
                                                egui::vec2(TINY_ICON, TINY_ICON),
                                            ),
                                            "Delete",
                                        )
                                        .min_size(egui::vec2(104.0, 32.0)),
                                    )
                                    .clicked()
                                })
                                .unwrap_or_else(|| {
                                    ui.add(
                                        egui::Button::new("Delete")
                                            .min_size(egui::vec2(104.0, 32.0)),
                                    )
                                    .clicked()
                                })
                        {
                            do_delete = true;
                        }
                        let cancel_btn = if let Some(tex) = icons.get("close_red") {
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
                        if cancel_btn.clicked() {
                            dismiss = true;
                        }
                    });
                });
            });
        if do_open {
            if store.recall_library(&pick).is_some() {
                *status = Some(format!("Opened Combined board “{pick}”"));
                *persist = true;
            }
            dismiss = true;
        }
        if do_delete {
            let _ = store.delete_library_entry(&pick);
            *status = Some(format!("Deleted library entry “{pick}”"));
            pick = store
                .library
                .first()
                .map(|e| e.name.clone())
                .unwrap_or_default();
        }
        if dismiss {
            ui.ctx().data_mut(|d| {
                d.remove_temp::<bool>(open_id);
                d.remove_temp::<String>(pick_id);
            });
        } else {
            ui.ctx().data_mut(|d| {
                d.insert_temp(open_id, true);
                d.insert_temp(pick_id, pick);
            });
        }
    }

    ui.add_space(4.0);

    // Owned paint snapshot — avoids holding store borrows across mutations.
    let time_range = store
        .get(board_id)
        .map(|b| b.time_range)
        .unwrap_or(time_range);
    let paint_series: Vec<SeriesPaintOwned> = {
        let Some(board) = store.get(board_id) else {
            return;
        };
        board
            .series
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let offline = !connected.contains(&s.profile_id);
                let profile_name = if s.profile_name.is_empty() {
                    name_by_id
                        .get(&s.profile_id)
                        .cloned()
                        .unwrap_or_else(|| s.profile_id.clone())
                } else {
                    s.profile_name.clone()
                };
                let key = s.series_key();
                let history = board
                    .buffers
                    .get(&key)
                    .cloned()
                    .unwrap_or_default();
                SeriesPaintOwned {
                    profile_id: s.profile_id.clone(),
                    topic: s.topic.clone(),
                    profile_name,
                    history,
                    color: SERIES_PALETTE[i % SERIES_PALETTE.len()],
                    offline,
                }
            })
            .collect()
    };
    let series_refs: Vec<SeriesPaint<'_>> = paint_series
        .iter()
        .map(|s| SeriesPaint {
            profile_id: &s.profile_id,
            topic: &s.topic,
            profile_name: &s.profile_name,
            history: &s.history,
            color: s.color,
            offline: s.offline,
        })
        .collect();

    let png_dlg = egui::Id::new(("dlg_combined_png", board_id));
    let png_h_key = egui::Id::new(("dlg_combined_png_h", board_id));
    if let Some(path_opt) = crate::ui::native_dialog::poll_path(ui.ctx(), png_dlg) {
        let plot_h_png = ui
            .ctx()
            .data_mut(|d| d.remove_temp::<f32>(png_h_key))
            .unwrap_or(plot_h);
        match path_opt {
            Some(path) => match write_combined_png(&path, &series_refs, plot_h_png, time_range)
            {
                Ok(p) => *status = Some(format!("Saved Combined graph → {p}")),
                Err(e) => *status = Some(format!("Combined graph export failed: {e}")),
            },
            None => {}
        }
    }

    let mut remove: Option<(String, String)> = None;

    // Plot fills the pop-out: reserve legend + x-labels, use the rest (no short resize strip).
    let avail_h = ui.available_height().max(80.0);
    let legend_reserve = if series_refs.is_empty() {
        0.0
    } else {
        36.0 + (series_refs.len() as f32 * 4.0).min(48.0)
    };
    let fill_plot_h = (avail_h - legend_reserve - X_LABEL_H - 16.0).max(80.0);

    let border = Stroke::new(1.0_f32, CHROME_ACCENT_BORDER());
    let frame = egui::Frame::NONE
        .fill(CHROME_COMBO_BG())
        .corner_radius(4.0)
        .inner_margin(egui::Margin::symmetric(10, 8))
        .show(ui, |ui| {
            let w = (ui.available_width() - flash::ITEM_CLIP_CLEAR).max(48.0);
            ui.set_min_width(w);
            ui.set_max_width(w);
            ui.set_min_height((avail_h - 4.0).max(72.0));

            if series_refs.is_empty() {
                ui.label(RichText::new("No series yet.").color(CHROME_MUTED()));
                ui.label(
                    RichText::new("Use “Add to Combined” on a Payload field or Graph sticky.")
                        .color(CHROME_MUTED()),
                );
                return;
            }

            if let Some(edit) = paint_multi_series_plot(
                ui,
                icons,
                &series_refs,
                fill_plot_h,
                time_range,
                board_id,
                y_min,
                y_max,
            ) {
                if let Some(b) = store.get_mut(board_id) {
                    b.y_min = edit.y_min;
                    b.y_max = edit.y_max;
                    *persist = true;
                }
            }
            ui.add_space(6.0);
            draw_combined_legend(ui, &series_refs, &mut remove);
        });
    ui.painter().rect_stroke(
        frame.response.rect.shrink(1.0),
        4.0,
        border,
        egui::StrokeKind::Inside,
    );

    drop(series_refs);
    drop(paint_series);

    if let Some((pid, topic)) = remove {
        store.remove_series(board_id, &pid, &topic);
        *persist = true;
        *status = Some(format!("Removed series · {topic}"));
    }
    // Persist last on-screen height for PNG export defaults / session restore.
    if let Some(b) = store.get_mut(board_id) {
        let h = clamp_height(fill_plot_h);
        if (b.plot_height - h).abs() > 0.5 {
            b.plot_height = h;
            *persist = true;
        }
    }
    let _ = plot_h;
}

struct SeriesPaintOwned {
    profile_id: String,
    topic: String,
    profile_name: String,
    history: VecDeque<HistorySample>,
    color: Color32,
    offline: bool,
}

/// Applied Y limits from the axis popup (`None` = Auto).
struct YLimitEdit {
    y_min: Option<f64>,
    y_max: Option<f64>,
}

/// Draft while the Y-scale popup is open (per board).
#[derive(Clone, Default)]
struct YScaleDraft {
    max_auto: bool,
    max_text: String,
    min_auto: bool,
    min_text: String,
    /// Parse / invert error shown under the fields.
    err: Option<String>,
}

fn paint_multi_series_plot(
    ui: &mut egui::Ui,
    icons: &IconSet,
    series: &[SeriesPaint<'_>],
    height: f32,
    time_range: CombinedTimeRange,
    board_id: &str,
    y_min: Option<f64>,
    y_max: Option<f64>,
) -> Option<YLimitEdit> {
    let Some(domain) = series_time_value_domain(series, time_range, y_min, y_max) else {
        ui.weak("No numeric samples in this timeline.");
        return None;
    };
    let SeriesDomain {
        t0,
        t1,
        y_lo,
        y_hi,
        min_v,
        max_v,
    } = domain;
    let y_span = (y_hi - y_lo).max(1e-9);
    let dt = t1.saturating_duration_since(t0).as_secs_f32().max(1e-3);
    let cutoff = time_range.cutoff(Instant::now());

    let plot_h = height.max(48.0);
    let avail = ui.available_width().max(Y_LABEL_W + 48.0);
    let plot_w = (avail - Y_LABEL_W).max(48.0);

    let mut y_edit: Option<YLimitEdit> = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        // Whole Y-label column is the hit target (opens min/max popup).
        let (label_rect, label_resp) =
            ui.allocate_exact_size(egui::vec2(Y_LABEL_W, plot_h), Sense::click());
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(plot_w, plot_h), egui::Sense::hover());

        let font = egui::TextStyle::Small.resolve(ui.style());
        let label_h = ui.fonts(|f| f.row_height(&font));
        let y_inset = (label_h * 0.5 + 1.0).min(plot_h * 0.2);
        let plot = egui::Rect::from_min_max(
            Pos2::new(rect.left(), rect.top() + y_inset),
            Pos2::new(rect.right(), rect.bottom() - y_inset),
        );

        let painter = ui.painter_at(rect.union(label_rect));
        // Plot well tracks chrome status (Latte-safe — not a baked charcoal).
        painter.rect_filled(rect, 2.0, CHROME_COMBO_BG());

        let grid = Stroke::new(1.0_f32, CHROME_SEP());
        let y_popup_id = egui::Id::new(("combined_y_scale", board_id));
        let any_manual = y_min.is_some() || y_max.is_some();
        // Soft accent on the label column when either limit is manual.
        if any_manual {
            painter.rect_filled(
                label_rect,
                2.0,
                Color32::from_rgba_unmultiplied(
                    CHROME_ACCENT_SOFT().r(),
                    CHROME_ACCENT_SOFT().g(),
                    CHROME_ACCENT_SOFT().b(),
                    40,
                ),
            );
        }
        for i in 0..Y_TICKS {
            let frac = i as f32 / (Y_TICKS - 1) as f32;
            let v = y_hi - f64::from(frac) * (y_hi - y_lo);
            let y = plot.top() + frac * plot.height();
            painter.hline(plot.x_range(), y, grid);
            let is_max = i == 0;
            let is_min = i + 1 == Y_TICKS;
            let manual = (is_max && y_max.is_some()) || (is_min && y_min.is_some());
            let color = if manual {
                CHROME_ACCENT_BORDER()
            } else {
                CHROME_MUTED()
            };
            let text = fmt_scale(v);
            let galley = ui.fonts(|f| f.layout_no_wrap(text, font.clone(), color));
            let mut ty = y - galley.size().y * 0.5;
            ty = ty.clamp(label_rect.top(), label_rect.bottom() - galley.size().y);
            let tx = (label_rect.right() - galley.size().x - 2.0).max(label_rect.left());
            painter.galley(Pos2::new(tx, ty), galley, color);
        }

        let tip = if any_manual {
            "Y scale (manual) — click to edit · right-click Auto"
        } else {
            "Y scale (auto) — click to set min/max"
        };
        label_resp.clone().on_hover_text(tip);
        if label_resp.clicked() {
            // Seed from current limits; empty Auto fields use data suggestion.
            let suggest = series_time_value_domain(series, time_range, None, None);
            let suggest_hi = suggest.as_ref().map(|d| d.y_hi).unwrap_or(y_hi);
            let suggest_lo = suggest.as_ref().map(|d| d.y_lo).unwrap_or(y_lo);
            let draft = YScaleDraft {
                max_auto: y_max.is_none(),
                max_text: fmt_scale(y_max.unwrap_or(suggest_hi)),
                min_auto: y_min.is_none(),
                min_text: fmt_scale(y_min.unwrap_or(suggest_lo)),
                err: None,
            };
            ui.ctx().data_mut(|d| d.insert_temp(y_popup_id, draft));
        }
        if label_resp.secondary_clicked() {
            y_edit = Some(YLimitEdit {
                y_min: None,
                y_max: None,
            });
        }

        // Styled Y-scale popup (matches Combined Save / Open chrome).
        if let Some(mut draft) = ui.ctx().data(|d| d.get_temp::<YScaleDraft>(y_popup_id)) {
            let mut dismiss = false;
            let suggest = series_time_value_domain(series, time_range, None, None);
            let suggest_hi = suggest.as_ref().map(|d| d.y_hi).unwrap_or(y_hi);
            let suggest_lo = suggest.as_ref().map(|d| d.y_lo).unwrap_or(y_lo);
            egui::Window::new(format!("combined_y_scale_{board_id}"))
                .id(y_popup_id.with("win"))
                .title_bar(false)
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .frame(
                    egui::Frame::NONE
                        .fill(CHROME_BG())
                        .stroke(egui::Stroke::new(1.0_f32, CHROME_SEP()))
                        .corner_radius(8.0)
                        .inner_margin(egui::Margin::symmetric(16, 14)),
                )
                .show(ui.ctx(), |ui| {
                    ui.set_min_width(320.0);
                    ui.set_max_width(380.0);
                    icons::section_header_bar(ui, |ui| {
                        icons::section_icon(ui, icons, "line_graph");
                        icons::section_title(ui, "Y scale");
                    });
                    ui.add_space(6.0);
                    let bar_w = ui.available_width();
                    let (bar, _) =
                        ui.allocate_exact_size(egui::vec2(bar_w, 3.0), Sense::hover());
                    ui.painter().rect_filled(bar, 1.5, CHROME_ACCENT_BORDER());
                    ui.add_space(12.0);

                    // Max row
                    ui.label(RichText::new("Max").strong().color(CHROME_TEXT()));
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 8.0;
                        ui.add_enabled_ui(!draft.max_auto, |ui| {
                            ui.add(
                                egui::TextEdit::singleline(&mut draft.max_text)
                                    .desired_width(140.0)
                                    .text_color(CHROME_TEXT()),
                            );
                        });
                        ui.checkbox(&mut draft.max_auto, "Auto")
                            .on_hover_text(format!("Auto suggests {}", fmt_scale(suggest_hi)));
                    });
                    ui.add_space(10.0);

                    // Min row
                    ui.label(RichText::new("Min").strong().color(CHROME_TEXT()));
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 8.0;
                        ui.add_enabled_ui(!draft.min_auto, |ui| {
                            ui.add(
                                egui::TextEdit::singleline(&mut draft.min_text)
                                    .desired_width(140.0)
                                    .text_color(CHROME_TEXT()),
                            );
                        });
                        ui.checkbox(&mut draft.min_auto, "Auto")
                            .on_hover_text(format!("Auto suggests {}", fmt_scale(suggest_lo)));
                    });

                    if let Some(err) = &draft.err {
                        ui.add_space(8.0);
                        ui.label(RichText::new(err).color(Color32::from_rgb(220, 120, 100)));
                    }

                    ui.add_space(14.0);
                    ui.separator();
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                            ui.spacing_mut().item_spacing.x = 8.0;
                            let apply_btn = if let Some(tex) = icons.get("line_graph") {
                                ui.add(
                                    egui::Button::image_and_text(
                                        egui::Image::new(tex)
                                            .fit_to_exact_size(egui::vec2(TINY_ICON, TINY_ICON)),
                                        RichText::new("Apply").strong(),
                                    )
                                    .fill(CHROME_ACCENT_SOFT())
                                    .stroke(egui::Stroke::new(1.0_f32, CHROME_ACCENT_BORDER()))
                                    .min_size(egui::vec2(120.0, 32.0)),
                                )
                            } else {
                                ui.add(
                                    egui::Button::new(RichText::new("Apply").strong())
                                        .fill(CHROME_ACCENT_SOFT())
                                        .stroke(egui::Stroke::new(
                                            1.0_f32,
                                            CHROME_ACCENT_BORDER(),
                                        ))
                                        .min_size(egui::vec2(120.0, 32.0)),
                                )
                            };
                            if apply_btn.clicked() {
                                match parse_y_scale_draft(&draft) {
                                    Ok(edit) => {
                                        y_edit = Some(edit);
                                        dismiss = true;
                                    }
                                    Err(msg) => draft.err = Some(msg),
                                }
                            }
                            let cancel_btn = if let Some(tex) = icons.get("close_red") {
                                ui.add(
                                    egui::Button::image_and_text(
                                        egui::Image::new(tex)
                                            .fit_to_exact_size(egui::vec2(TINY_ICON, TINY_ICON)),
                                        "Cancel",
                                    )
                                    .min_size(egui::vec2(104.0, 32.0)),
                                )
                            } else {
                                ui.add(
                                    egui::Button::new("Cancel").min_size(egui::vec2(104.0, 32.0)),
                                )
                            };
                            if cancel_btn.clicked() {
                                dismiss = true;
                            }
                        });
                    });
                });
            if dismiss {
                ui.ctx().data_mut(|d| d.remove_temp::<YScaleDraft>(y_popup_id));
            } else {
                ui.ctx().data_mut(|d| d.insert_temp(y_popup_id, draft));
            }
        }

        for s in series {
            let mut points = Vec::new();
            for sample in s.history {
                if let Some(c) = cutoff {
                    if sample.at < c {
                        continue;
                    }
                }
                let Some(v) = sample.numeric else { continue };
                if !v.is_finite() {
                    continue;
                }
                let x_t = sample.at.saturating_duration_since(t0).as_secs_f32() / dt;
                let x = plot.left() + x_t.clamp(0.0, 1.0) * plot.width();
                let y = plot.bottom() - (((v - y_lo) / y_span) as f32) * plot.height();
                points.push(Pos2::new(x, y));
            }
            let stroke = if s.offline {
                Stroke::new(1.2_f32, s.color.gamma_multiply(0.45))
            } else {
                Stroke::new(1.6_f32, s.color)
            };
            if points.len() >= 2 {
                painter.add(egui::Shape::line(points, stroke));
            } else if let Some(p) = points.first() {
                painter.circle_filled(*p, 3.0, s.color);
            }
        }

        if let Some(pos) = response.hover_pos() {
            if plot.contains(pos) {
                let x_t = ((pos.x - plot.left()) / plot.width()).clamp(0.0, 1.0);
                let at = t0 + Duration::from_secs_f32(dt * x_t);
                painter.vline(
                    pos.x,
                    plot.y_range(),
                    Stroke::new(1.0_f32, Color32::from_rgb(90, 96, 110)),
                );
                let mut lines = vec![(instant_hhmmss(at), CHROME_TEXT())];
                for s in series {
                    if let Some(v) = value_at_time(s.history, at, cutoff) {
                        let short = sticky_short_label(s.topic);
                        let tag = if s.offline { " (offline)" } else { "" };
                        lines.push((
                            format!("{short}{tag}  {}", fmt_scale(v)),
                            s.color,
                        ));
                    }
                }
                let font = egui::TextStyle::Small.resolve(ui.style());
                let mut max_w = 0.0_f32;
                let mut galleys = Vec::new();
                for (text, color) in &lines {
                    let g = ui.fonts(|f| f.layout_no_wrap(text.clone(), font.clone(), *color));
                    max_w = max_w.max(g.size().x);
                    galleys.push(g);
                }
                let pad = 6.0;
                let line_h = ui.fonts(|f| f.row_height(&font));
                let box_h = line_h * galleys.len() as f32 + pad * 2.0;
                let box_w = max_w + pad * 2.0;
                let mut bx = pos.x + 10.0;
                let mut by = pos.y + 10.0;
                if bx + box_w > rect.right() {
                    bx = pos.x - box_w - 10.0;
                }
                if by + box_h > rect.bottom() {
                    by = pos.y - box_h - 10.0;
                }
                let tip = egui::Rect::from_min_size(Pos2::new(bx, by), egui::vec2(box_w, box_h));
                painter.rect_filled(tip, 3.0, Color32::from_rgba_unmultiplied(20, 22, 28, 230));
                painter.rect_stroke(
                    tip,
                    3.0,
                    Stroke::new(1.0_f32, Color32::from_rgb(70, 74, 84)),
                    egui::StrokeKind::Inside,
                );
                for (i, g) in galleys.into_iter().enumerate() {
                    painter.galley(
                        Pos2::new(tip.left() + pad, tip.top() + pad + i as f32 * line_h),
                        g,
                        CHROME_TEXT(),
                    );
                }
            }
        }

        let _ = (min_v, max_v);
    });

    // X labels.
    let mid = t0 + Duration::from_secs_f32(dt * 0.5);
    let left_l = instant_hhmm(t0);
    let mid_l = instant_hhmm(mid);
    let right_l = instant_hhmm(t1);
    ui.horizontal(|ui| {
        ui.add_space(Y_LABEL_W + 4.0);
        let w = ui.available_width().max(1.0);
        let (xr, _) = ui.allocate_exact_size(egui::vec2(w, X_LABEL_H), Sense::hover());
        let font = egui::TextStyle::Small.resolve(ui.style());
        let painter = ui.painter_at(xr);
        for (frac, text) in [(0.0_f32, left_l), (0.5, mid_l), (1.0, right_l)] {
            let galley = ui.fonts(|f| f.layout_no_wrap(text, font.clone(), CHROME_MUTED()));
            let align = if frac <= 0.0 {
                0.0
            } else if frac >= 1.0 {
                galley.size().x
            } else {
                galley.size().x * 0.5
            };
            let x = xr.left() + frac * xr.width() - align;
            let y = xr.center().y - galley.size().y * 0.5;
            painter.galley(Pos2::new(x, y), galley, CHROME_MUTED());
        }
    });
    y_edit
}

fn draw_combined_legend(
    ui: &mut egui::Ui,
    series: &[SeriesPaint<'_>],
    remove: &mut Option<(String, String)>,
) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 12.0;
        ui.spacing_mut().item_spacing.y = 4.0;
        for s in series {
            let short = sticky_short_label(s.topic);
            let cur = s
                .history
                .iter()
                .rev()
                .find_map(|h| h.numeric)
                .map(|v| format!("{v}"))
                .unwrap_or_else(|| "—".into());
            let tip = if s.profile_name.is_empty() {
                s.topic.to_string()
            } else {
                format!("{} · {}", s.topic, s.profile_name)
            };
            let tip = if s.offline {
                format!("{tip} (offline)")
            } else {
                tip
            };
            let r = ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                let (sw, _) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), Sense::hover());
                ui.painter().circle_filled(sw.center(), 3.5, s.color);
                let name = if short.len() > 18 {
                    format!("{}…", &short[..17])
                } else {
                    short
                };
                let name_c = if s.offline {
                    CHROME_MUTED()
                } else {
                    CHROME_TEXT()
                };
                ui.label(RichText::new(name).small().color(name_c));
                ui.label(
                    RichText::new(cur)
                        .small()
                        .strong()
                        .monospace()
                        .color(s.color),
                );
                if ui
                    .small_button("×")
                    .on_hover_text("Remove series from this board")
                    .clicked()
                {
                    *remove = Some((s.profile_id.to_string(), s.topic.to_string()));
                }
            });
            r.response.on_hover_text(tip);
        }
    });
}

struct SeriesDomain {
    t0: Instant,
    t1: Instant,
    y_lo: f64,
    y_hi: f64,
    min_v: f64,
    max_v: f64,
}

fn series_time_value_domain(
    series: &[SeriesPaint<'_>],
    time_range: CombinedTimeRange,
    y_min: Option<f64>,
    y_max: Option<f64>,
) -> Option<SeriesDomain> {
    let cutoff = time_range.cutoff(Instant::now());
    let mut t0: Option<Instant> = None;
    let mut t1: Option<Instant> = None;
    let mut min_v = f64::INFINITY;
    let mut max_v = f64::NEG_INFINITY;
    for s in series {
        for sample in s.history {
            if let Some(c) = cutoff {
                if sample.at < c {
                    continue;
                }
            }
            let Some(v) = sample.numeric else { continue };
            if !v.is_finite() {
                continue;
            }
            min_v = min_v.min(v);
            max_v = max_v.max(v);
            t0 = Some(t0.map_or(sample.at, |t| t.min(sample.at)));
            t1 = Some(t1.map_or(sample.at, |t| t.max(sample.at)));
        }
    }
    if !min_v.is_finite() || !max_v.is_finite() {
        return None;
    }
    // Tip = last sample time (not Instant::now) — plot only advances when data arrives.
    // Idle Combined boards do not get a timer; X stays put between series samples.
    let data_t0 = t0?;
    let data_t1 = t1?;
    let (t0, t1) = match time_range.max_span() {
        Some(max_span) => {
            let span = data_t1.saturating_duration_since(data_t0);
            if span < max_span {
                // Autofit to data so far; expand as samples grow until the selected max.
                (data_t0, data_t1)
            } else {
                let capped_t0 = data_t1.checked_sub(max_span).unwrap_or(data_t0);
                (capped_t0, data_t1)
            }
        }
        None => (data_t0, data_t1),
    };
    let _ = cutoff;
    let raw_span = (max_v - min_v).abs();
    let (auto_lo, auto_hi) = if raw_span < 1e-12 {
        let pad = (min_v.abs() * 0.05).max(1.0);
        (min_v - pad, max_v + pad)
    } else {
        let pad = (raw_span * 0.04).max(raw_span * 1e-6);
        (min_v - pad, max_v + pad)
    };
    // Manual Y limits (axis popup); default auto from data.
    let mut y_lo = y_min.unwrap_or(auto_lo);
    let mut y_hi = y_max.unwrap_or(auto_hi);
    if y_hi <= y_lo {
        // Keep a usable span if the user set inverted/equal limits.
        let pad = ((y_lo.abs() + y_hi.abs()) * 0.05).max(1.0);
        y_lo = y_lo.min(y_hi) - pad;
        y_hi = y_lo + pad * 2.0;
    }
    Some(SeriesDomain {
        t0,
        t1,
        y_lo,
        y_hi,
        min_v,
        max_v,
    })
}

fn value_at_time(
    history: &VecDeque<HistorySample>,
    at: Instant,
    cutoff: Option<Instant>,
) -> Option<f64> {
    let mut before: Option<(Instant, f64)> = None;
    let mut after: Option<(Instant, f64)> = None;
    for s in history {
        if let Some(c) = cutoff {
            if s.at < c {
                continue;
            }
        }
        let Some(v) = s.numeric else { continue };
        if !v.is_finite() {
            continue;
        }
        if s.at <= at {
            if before.map(|(t, _)| s.at >= t).unwrap_or(true) {
                before = Some((s.at, v));
            }
        } else if after.map(|(t, _)| s.at < t).unwrap_or(true) {
            after = Some((s.at, v));
        }
    }
    match (before, after) {
        (Some((tb, vb)), Some((ta, va))) => {
            let span = ta.saturating_duration_since(tb).as_secs_f64().max(1e-9);
            let u = at.saturating_duration_since(tb).as_secs_f64() / span;
            Some(vb + (va - vb) * u)
        }
        (Some((_, v)), None) | (None, Some((_, v))) => Some(v),
        (None, None) => None,
    }
}

fn sticky_short_label(topic: &str) -> String {
    let (base, path) = crate::mqtt::value_path::split_sticky_id(topic);
    if let Some(p) = path {
        p.rsplit('.').next().unwrap_or(p).to_string()
    } else {
        base.rsplit('/').next().unwrap_or(base).to_string()
    }
}

fn instant_clock(at: Instant, with_secs: bool) -> String {
    let age = Instant::now().saturating_duration_since(at);
    let wall = SystemTime::now()
        .checked_sub(age)
        .unwrap_or_else(SystemTime::now);
    let dt: chrono::DateTime<chrono::Local> = wall.into();
    if with_secs {
        dt.format("%H:%M:%S").to_string()
    } else {
        dt.format("%H:%M").to_string()
    }
}

fn instant_hhmm(at: Instant) -> String {
    instant_clock(at, false)
}

fn instant_hhmmss(at: Instant) -> String {
    instant_clock(at, true)
}

fn combined_export_filename() -> String {
    let now: chrono::DateTime<chrono::Local> = chrono::Local::now();
    format!("topicbench-combined-{}.png", now.format("%Y%m%d-%H%M"))
}

fn fmt_scale(v: f64) -> String {
    if !v.is_finite() {
        return "—".into();
    }
    let a = v.abs();
    if a >= 1000.0 || (v.fract().abs() < 1e-9 && a < 1e12) {
        format!("{v:.0}")
    } else if a >= 10.0 {
        format!("{v:.1}")
    } else if a >= 1.0 {
        format!("{v:.2}")
    } else {
        format!("{v:.3}")
    }
}

/// Parse popup draft into board limits; `None` means Auto.
fn parse_y_scale_draft(draft: &YScaleDraft) -> Result<YLimitEdit, String> {
    let y_max = if draft.max_auto {
        None
    } else {
        let v = draft
            .max_text
            .trim()
            .parse::<f64>()
            .map_err(|_| "Max must be a number".to_string())?;
        if !v.is_finite() {
            return Err("Max must be finite".into());
        }
        Some(v)
    };
    let y_min = if draft.min_auto {
        None
    } else {
        let v = draft
            .min_text
            .trim()
            .parse::<f64>()
            .map_err(|_| "Min must be a number".to_string())?;
        if !v.is_finite() {
            return Err("Min must be finite".into());
        }
        Some(v)
    };
    if let (Some(lo), Some(hi)) = (y_min, y_max) {
        if hi <= lo {
            return Err("Max must be greater than Min".into());
        }
    }
    Ok(YLimitEdit { y_min, y_max })
}

#[cfg(test)]
mod y_scale_tests {
    use super::*;

    #[test]
    fn parse_auto_both() {
        let d = YScaleDraft {
            max_auto: true,
            max_text: "x".into(),
            min_auto: true,
            min_text: "y".into(),
            err: None,
        };
        let e = parse_y_scale_draft(&d).unwrap();
        assert!(e.y_min.is_none() && e.y_max.is_none());
    }

    #[test]
    fn parse_manual_range() {
        let d = YScaleDraft {
            max_auto: false,
            max_text: "100".into(),
            min_auto: false,
            min_text: "0".into(),
            err: None,
        };
        let e = parse_y_scale_draft(&d).unwrap();
        assert_eq!(e.y_min, Some(0.0));
        assert_eq!(e.y_max, Some(100.0));
    }

    #[test]
    fn parse_rejects_inverted() {
        let d = YScaleDraft {
            max_auto: false,
            max_text: "1".into(),
            min_auto: false,
            min_text: "10".into(),
            err: None,
        };
        assert!(parse_y_scale_draft(&d).is_err());
    }
}

fn write_combined_png(
    path: &std::path::Path,
    series: &[SeriesPaint<'_>],
    plot_h: f32,
    time_range: CombinedTimeRange,
) -> Result<String, String> {
    let domain = series_time_value_domain(series, time_range, None, None)
        .ok_or_else(|| "no numeric samples".to_string())?;
    let cutoff = time_range.cutoff(Instant::now());
    let w = 900u32;
    let label_w = 70u32;
    let x_h = 28u32;
    let legend_row_h = 16u32;
    let legend_pad = 10u32;
    let legend_h = legend_pad * 2 + (series.len() as u32) * legend_row_h;
    let plot_w = w.saturating_sub(label_w);
    let plot_body = (plot_h.round() as u32).clamp(80, 800);
    let h = plot_body + x_h + legend_h + 8;
    let mut img = image::RgbaImage::from_pixel(w, h, image::Rgba([32, 34, 40, 255]));
    let plot_top = 4u32;
    let plot_bot = plot_top + plot_body;
    let y_inset = 10u32;
    let inner_top = plot_top + y_inset;
    let inner_bot = plot_bot.saturating_sub(y_inset);
    let inner_h = inner_bot.saturating_sub(inner_top).max(1);

    for y in plot_top..plot_bot {
        for x in label_w..w {
            img.put_pixel(x, y, image::Rgba([22, 24, 28, 255]));
        }
    }

    let y_span = (domain.y_hi - domain.y_lo).max(1e-9);
    let dt = domain
        .t1
        .saturating_duration_since(domain.t0)
        .as_secs_f32()
        .max(1e-3);

    for i in 0..Y_TICKS {
        let frac = i as f32 / (Y_TICKS - 1) as f32;
        let y = inner_top + ((frac * inner_h as f32) as u32).min(inner_h);
        for x in label_w..w {
            let p = img.get_pixel_mut(x, y);
            p.0 = [48, 52, 60, 255];
        }
        let v = domain.y_hi - f64::from(frac) * (domain.y_hi - domain.y_lo);
        blit_text(
            &mut img,
            4,
            y.saturating_sub(4),
            &fmt_scale(v),
            [175, 182, 196],
        );
    }

    for s in series {
        let rgb = [s.color.r(), s.color.g(), s.color.b()];
        let mut prev: Option<(u32, u32)> = None;
        for sample in s.history {
            if let Some(c) = cutoff {
                if sample.at < c {
                    continue;
                }
            }
            let Some(v) = sample.numeric else { continue };
            if !v.is_finite() {
                continue;
            }
            let x_t = sample.at.saturating_duration_since(domain.t0).as_secs_f32() / dt;
            let x = label_w + ((x_t.clamp(0.0, 1.0) * (plot_w.saturating_sub(1) as f32)) as u32);
            let yf = 1.0 - (((v - domain.y_lo) / y_span) as f32).clamp(0.0, 1.0);
            let y = inner_top + ((yf * inner_h as f32) as u32).min(inner_h);
            if let Some((px, py)) = prev {
                draw_line(&mut img, px, py, x, y, rgb);
            }
            prev = Some((x, y));
        }
    }

    let mid = domain.t0 + Duration::from_secs_f32(dt * 0.5);
    let y_lab = plot_bot + 6;
    blit_text(
        &mut img,
        label_w + 2,
        y_lab,
        &instant_hhmm(domain.t0),
        [175, 182, 196],
    );
    blit_text(
        &mut img,
        label_w + plot_w / 2 - 12,
        y_lab,
        &instant_hhmm(mid),
        [175, 182, 196],
    );
    blit_text(
        &mut img,
        w.saturating_sub(40),
        y_lab,
        &instant_hhmm(domain.t1),
        [175, 182, 196],
    );

    let legend_top = plot_bot + x_h;
    blit_text(&mut img, 12, legend_top, "Legend", [175, 182, 196]);
    for (i, s) in series.iter().enumerate() {
        let y = legend_top + legend_pad + (i as u32) * legend_row_h;
        let rgb = [s.color.r(), s.color.g(), s.color.b()];
        fill_circle(&mut img, 16, y + 4, 4, rgb);
        let short = sticky_short_label(s.topic);
        let cur = s
            .history
            .iter()
            .rev()
            .find_map(|h| h.numeric)
            .map(fmt_scale)
            .unwrap_or_else(|| "-".into());
        blit_text(&mut img, 28, y, &format!("{short}  {cur}"), rgb);
    }

    img.save(&path)
        .map_err(|e| format!("write {}: {e}", path.display()))?;
    Ok(path.display().to_string())
}

fn fill_circle(img: &mut image::RgbaImage, cx: u32, cy: u32, r: i32, rgb: [u8; 3]) {
    let (w, h) = (img.width() as i32, img.height() as i32);
    let (cx, cy) = (cx as i32, cy as i32);
    for dy in -r..=r {
        for dx in -r..=r {
            if dx * dx + dy * dy <= r * r {
                let x = cx + dx;
                let y = cy + dy;
                if x >= 0 && y >= 0 && x < w && y < h {
                    img.put_pixel(
                        x as u32,
                        y as u32,
                        image::Rgba([rgb[0], rgb[1], rgb[2], 255]),
                    );
                }
            }
        }
    }
}

fn draw_line(img: &mut image::RgbaImage, x0: u32, y0: u32, x1: u32, y1: u32, rgb: [u8; 3]) {
    let (w, h) = (img.width() as i32, img.height() as i32);
    let (mut x0, mut y0, x1, y1) = (x0 as i32, y0 as i32, x1 as i32, y1 as i32);
    let dx = (x1 - x0).abs();
    let dy = -(y1 - y0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx + dy;
    loop {
        if x0 >= 0 && y0 >= 0 && x0 < w && y0 < h {
            img.put_pixel(x0 as u32, y0 as u32, image::Rgba([rgb[0], rgb[1], rgb[2], 255]));
        }
        if x0 == x1 && y0 == y1 {
            break;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x0 += sx;
        }
        if e2 <= dx {
            err += dx;
            y0 += sy;
        }
    }
}

fn blit_text(img: &mut image::RgbaImage, x: u32, y: u32, text: &str, rgb: [u8; 3]) {
    let mut cx = x;
    for ch in text.chars() {
        if let Some(glyph) = TINY_GLYPH.get(&ch) {
            for (row, bits) in glyph.iter().enumerate() {
                for col in 0..5 {
                    if bits & (1 << (4 - col)) != 0 {
                        let px = cx + col;
                        let py = y + row as u32;
                        if px < img.width() && py < img.height() {
                            img.put_pixel(px, py, image::Rgba([rgb[0], rgb[1], rgb[2], 255]));
                        }
                    }
                }
            }
            cx += 6;
        } else {
            cx += 4;
        }
    }
}

fn tiny_glyph_map() -> HashMap<char, [u8; 7]> {
    let mut m = HashMap::new();
    let put = |m: &mut HashMap<char, [u8; 7]>, c: char, g: [u8; 7]| {
        m.insert(c, g);
    };
    put(&mut m, '0', [0x0E, 0x11, 0x13, 0x15, 0x19, 0x11, 0x0E]);
    put(&mut m, '1', [0x04, 0x0C, 0x04, 0x04, 0x04, 0x04, 0x0E]);
    put(&mut m, '2', [0x0E, 0x11, 0x01, 0x06, 0x08, 0x10, 0x1F]);
    put(&mut m, '3', [0x0E, 0x11, 0x01, 0x06, 0x01, 0x11, 0x0E]);
    put(&mut m, '4', [0x02, 0x06, 0x0A, 0x12, 0x1F, 0x02, 0x02]);
    put(&mut m, '5', [0x1F, 0x10, 0x1E, 0x01, 0x01, 0x11, 0x0E]);
    put(&mut m, '6', [0x06, 0x08, 0x10, 0x1E, 0x11, 0x11, 0x0E]);
    put(&mut m, '7', [0x1F, 0x01, 0x02, 0x04, 0x08, 0x08, 0x08]);
    put(&mut m, '8', [0x0E, 0x11, 0x11, 0x0E, 0x11, 0x11, 0x0E]);
    put(&mut m, '9', [0x0E, 0x11, 0x11, 0x0F, 0x01, 0x02, 0x0C]);
    put(&mut m, ':', [0x00, 0x04, 0x00, 0x00, 0x04, 0x00, 0x00]);
    put(&mut m, '.', [0x00, 0x00, 0x00, 0x00, 0x00, 0x0C, 0x0C]);
    put(&mut m, '-', [0x00, 0x00, 0x00, 0x1F, 0x00, 0x00, 0x00]);
    put(&mut m, '_', [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x1F]);
    put(&mut m, '/', [0x01, 0x02, 0x04, 0x04, 0x08, 0x10, 0x10]);
    put(&mut m, ' ', [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]);
    put(&mut m, '+', [0x00, 0x04, 0x04, 0x1F, 0x04, 0x04, 0x00]);
    put(&mut m, 'A', [0x0E, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11]);
    put(&mut m, 'B', [0x1E, 0x11, 0x11, 0x1E, 0x11, 0x11, 0x1E]);
    put(&mut m, 'C', [0x0E, 0x11, 0x10, 0x10, 0x10, 0x11, 0x0E]);
    put(&mut m, 'D', [0x1E, 0x11, 0x11, 0x11, 0x11, 0x11, 0x1E]);
    put(&mut m, 'E', [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x1F]);
    put(&mut m, 'F', [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x10]);
    put(&mut m, 'G', [0x0E, 0x11, 0x10, 0x17, 0x11, 0x11, 0x0E]);
    put(&mut m, 'H', [0x11, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11]);
    put(&mut m, 'I', [0x0E, 0x04, 0x04, 0x04, 0x04, 0x04, 0x0E]);
    put(&mut m, 'J', [0x01, 0x01, 0x01, 0x01, 0x11, 0x11, 0x0E]);
    put(&mut m, 'K', [0x11, 0x12, 0x14, 0x18, 0x14, 0x12, 0x11]);
    put(&mut m, 'L', [0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x1F]);
    put(&mut m, 'M', [0x11, 0x1B, 0x15, 0x15, 0x11, 0x11, 0x11]);
    put(&mut m, 'N', [0x11, 0x19, 0x15, 0x13, 0x11, 0x11, 0x11]);
    put(&mut m, 'O', [0x0E, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E]);
    put(&mut m, 'P', [0x1E, 0x11, 0x11, 0x1E, 0x10, 0x10, 0x10]);
    put(&mut m, 'Q', [0x0E, 0x11, 0x11, 0x11, 0x15, 0x12, 0x0D]);
    put(&mut m, 'R', [0x1E, 0x11, 0x11, 0x1E, 0x14, 0x12, 0x11]);
    put(&mut m, 'S', [0x0E, 0x11, 0x10, 0x0E, 0x01, 0x11, 0x0E]);
    put(&mut m, 'T', [0x1F, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04]);
    put(&mut m, 'U', [0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E]);
    put(&mut m, 'V', [0x11, 0x11, 0x11, 0x11, 0x11, 0x0A, 0x04]);
    put(&mut m, 'W', [0x11, 0x11, 0x11, 0x15, 0x15, 0x1B, 0x11]);
    put(&mut m, 'X', [0x11, 0x11, 0x0A, 0x04, 0x0A, 0x11, 0x11]);
    put(&mut m, 'Y', [0x11, 0x11, 0x0A, 0x04, 0x04, 0x04, 0x04]);
    put(&mut m, 'Z', [0x1F, 0x01, 0x02, 0x04, 0x08, 0x10, 0x1F]);
    for c in b'a'..=b'z' {
        let up = (c - b'a' + b'A') as char;
        if let Some(g) = m.get(&up).copied() {
            m.insert(c as char, g);
        }
    }
    m
}

static TINY_GLYPH: LazyLock<HashMap<char, [u8; 7]>> = LazyLock::new(tiny_glyph_map);

/// Helper used by Payload / Sticky “Add to Combined” buttons.
pub fn add_series_from_ui(
    store: &mut CombinedBoardsStore,
    profile_id: &str,
    profile_name: &str,
    topic: &str,
    seed: Option<&VecDeque<HistorySample>>,
    status: &mut String,
) {
    let (board_id, added) = store.add_series(profile_id, profile_name, topic, seed);
    let _ = store.save_session();
    let title = store
        .get(&board_id)
        .map(|b| b.display_title())
        .unwrap_or_else(|| "Combined".into());
    if added {
        *status = format!("Added to Combined · {title}");
    } else {
        *status = format!("Already on Combined · {title}");
    }
}
