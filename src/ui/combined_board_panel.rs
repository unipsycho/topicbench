//! Combined graph boards — Pro MultiGraph OS pop-outs (Simulation undock pattern).

use crate::mqtt::pins::HistorySample;
use crate::mqtt::session::SessionState;
use crate::mqtt::window_geom::WinGeom;
use crate::pro::combined_boards::{
    clamp_height, CombinedBoardsStore, CombinedTimeRange,
};
use crate::ui::flash;
use crate::ui::icons::{
    self, IconSet, CHROME_ACCENT_BORDER, CHROME_BG, CHROME_MUTED, CHROME_TEXT,
};
use eframe::egui::{
    self, Align, Color32, CursorIcon, Pos2, RichText, Sense, Stroke, ViewportBuilder,
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
        let title = board.display_title();
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
                ViewportBuilder::default().with_title(format!("Combined: {title}")),
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
                        egui::Window::new(format!("Combined: {title}"))
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
    let (title, plot_h, time_range, series_count) = {
        let Some(board) = store.get(board_id) else {
            return;
        };
        (
            board.display_title(),
            clamp_height(board.plot_height),
            board.time_range,
            board.series.len(),
        )
    };

    // Chrome header.
    ui.horizontal(|ui| {
        ui.label(RichText::new(format!("Combined: {title}")).strong().color(CHROME_TEXT()));
        if series_count > 0 {
            ui.label(
                RichText::new(format!("{series_count} series"))
                    .small()
                    .color(CHROME_MUTED()),
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

        ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
            if icons
                .tiny_header(ui, "close_red", false, "Close board")
                .clicked()
            {
                *close = true;
            }
            let png_dlg = egui::Id::new(("dlg_combined_png", board_id));
            let png_h_key = egui::Id::new(("dlg_combined_png_h", board_id));
            if series_count > 0
                && icons
                    .tiny_header(ui, "export", false, "Save Combined graph as PNG")
                    .clicked()
            {
                ui.ctx().data_mut(|d| d.insert_temp(png_h_key, plot_h));
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
            if ui
                .small_button("Clear")
                .on_hover_text("Clear display buffer (keep series)")
                .clicked()
            {
                store.clear_display_buffer(board_id);
                *status = Some("Combined display buffer cleared".into());
            }
            let save_id = egui::Id::new(("combined_save_open", board_id));
            let open_id = egui::Id::new(("combined_lib_open", board_id));
            if ui
                .small_button("Open")
                .on_hover_text("Recall a saved board")
                .clicked()
            {
                ui.ctx().data_mut(|d| d.insert_temp(open_id, true));
            }
            if ui
                .small_button("Save")
                .on_hover_text("Save board to library")
                .clicked()
            {
                let draft = store
                    .get(board_id)
                    .map(|b| b.display_title())
                    .unwrap_or_else(|| "Combined".into());
                ui.ctx().data_mut(|d| d.insert_temp(save_id, draft));
            }
        });
    });

    // Save-name popup.
    let save_id = egui::Id::new(("combined_save_open", board_id));
    if let Some(mut name) = ui.ctx().data(|d| d.get_temp::<String>(save_id)) {
        let mut dismiss = false;
        egui::Window::new("Save Combined board")
            .id(egui::Id::new(("combined_save_win", board_id)))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_TOP, [0.0, 40.0])
            .show(ui.ctx(), |ui| {
                ui.label("Library name:");
                ui.text_edit_singleline(&mut name);
                ui.horizontal(|ui| {
                    if ui.button("Save").clicked() {
                        match store.save_board_as(board_id, &name) {
                            Ok(()) => {
                                *status = Some(format!("Saved Combined board “{name}”"));
                                dismiss = true;
                            }
                            Err(e) => *status = Some(format!("Save failed: {e}")),
                        }
                    }
                    if ui.button("Cancel").clicked() {
                        dismiss = true;
                    }
                });
            });
        if dismiss {
            ui.ctx().data_mut(|d| d.remove_temp::<String>(save_id));
        } else {
            ui.ctx().data_mut(|d| d.insert_temp(save_id, name));
        }
    }

    // Open / recall popup.
    let open_id = egui::Id::new(("combined_lib_open", board_id));
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
        egui::Window::new("Open Combined board")
            .id(egui::Id::new(("combined_open_win", board_id)))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_TOP, [0.0, 40.0])
            .show(ui.ctx(), |ui| {
                if names.is_empty() {
                    ui.weak("No saved boards yet — use Save first.");
                } else {
                    egui::ComboBox::from_id_salt(("combined_lib_cb", board_id))
                        .selected_text(&pick)
                        .width(220.0)
                        .show_ui(ui, |ui| {
                            for n in &names {
                                ui.selectable_value(&mut pick, n.clone(), n);
                            }
                        });
                }
                ui.horizontal(|ui| {
                    if !names.is_empty() && ui.button("Open").clicked() {
                        if store.recall_library(&pick).is_some() {
                            *status = Some(format!("Opened Combined board “{pick}”"));
                            *persist = true;
                        }
                        dismiss = true;
                    }
                    if !names.is_empty() && ui.button("Delete").clicked() {
                        let _ = store.delete_library_entry(&pick);
                        *status = Some(format!("Deleted library entry “{pick}”"));
                        pick = store
                            .library
                            .first()
                            .map(|e| e.name.clone())
                            .unwrap_or_default();
                    }
                    if ui.button("Cancel").clicked() {
                        dismiss = true;
                    }
                });
            });
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
    let mut new_plot_h = plot_h;
    let mut height_persist = false;

    let border = Stroke::new(1.0_f32, CHROME_ACCENT_BORDER());
    let frame = egui::Frame::NONE
        .fill(Color32::from_rgb(32, 34, 40))
        .corner_radius(4.0)
        .inner_margin(egui::Margin::symmetric(10, 8))
        .show(ui, |ui| {
            let w = (ui.available_width() - flash::ITEM_CLIP_CLEAR).max(48.0);
            ui.set_min_width(w);
            ui.set_max_width(w);

            if series_refs.is_empty() {
                ui.label(RichText::new("No series yet.").color(CHROME_MUTED()));
                ui.weak("Use “Add to Combined” on a Payload field or Graph sticky.");
                return;
            }

            paint_multi_series_plot(ui, &series_refs, plot_h, time_range);
            ui.add_space(6.0);
            draw_combined_legend(ui, &series_refs, &mut remove);

            ui.add_space(2.0);
            let full_w = ui.available_width().max(24.0);
            let (sep, sep_r) =
                ui.allocate_exact_size(egui::vec2(full_w, 8.0), Sense::click_and_drag());
            let sep_r = sep_r.on_hover_cursor(CursorIcon::ResizeVertical);
            let mid_y = sep.center().y;
            ui.painter().hline(
                sep.x_range().shrink(8.0),
                mid_y,
                Stroke::new(2.0_f32, Color32::from_rgb(70, 74, 84)),
            );
            for dx in [-6.0_f32, 0.0, 6.0] {
                ui.painter().circle_filled(
                    Pos2::new(sep.center().x + dx, mid_y),
                    1.4,
                    CHROME_MUTED(),
                );
            }
            if sep_r.dragged() {
                new_plot_h = clamp_height(plot_h + sep_r.drag_delta().y);
                ui.ctx().request_repaint();
            }
            if sep_r.drag_stopped() {
                new_plot_h = clamp_height(new_plot_h);
                height_persist = true;
            }
            sep_r.on_hover_text("Drag to resize Combined graph");
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
    if new_plot_h != plot_h || height_persist {
        if let Some(b) = store.get_mut(board_id) {
            b.plot_height = clamp_height(new_plot_h);
        }
        if height_persist {
            *persist = true;
        }
    }
}

struct SeriesPaintOwned {
    profile_id: String,
    topic: String,
    profile_name: String,
    history: VecDeque<HistorySample>,
    color: Color32,
    offline: bool,
}

fn paint_multi_series_plot(
    ui: &mut egui::Ui,
    series: &[SeriesPaint<'_>],
    height: f32,
    time_range: CombinedTimeRange,
) {
    let Some(domain) = series_time_value_domain(series, time_range) else {
        ui.weak("No numeric samples in this timeline.");
        return;
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

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        let (label_rect, _) =
            ui.allocate_exact_size(egui::vec2(Y_LABEL_W, plot_h), egui::Sense::hover());
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
        painter.rect_filled(rect, 2.0, ui.visuals().extreme_bg_color);

        let grid = Stroke::new(1.0_f32, Color32::from_rgb(48, 52, 60));
        for i in 0..Y_TICKS {
            let frac = i as f32 / (Y_TICKS - 1) as f32;
            let v = y_hi - f64::from(frac) * (y_hi - y_lo);
            let y = plot.top() + frac * plot.height();
            painter.hline(plot.x_range(), y, grid);
            let text = fmt_scale(v);
            let galley = ui.fonts(|f| f.layout_no_wrap(text, font.clone(), CHROME_MUTED()));
            let mut ty = y - galley.size().y * 0.5;
            ty = ty.clamp(label_rect.top(), label_rect.bottom() - galley.size().y);
            let tx = (label_rect.right() - galley.size().x - 2.0).max(label_rect.left());
            painter.galley(Pos2::new(tx, ty), galley, CHROME_MUTED());
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
    // Live follow: when filtering, extend t1 to now so the window stays at the tip.
    if let Some(c) = cutoff {
        let now = Instant::now();
        t1 = Some(t1.map_or(now, |t| t.max(now)));
        t0 = Some(t0.map_or(c, |t| t.min(c).max(c)));
        // Prefer fixed window [now-span, now] when we have any samples.
        if min_v.is_finite() {
            t0 = Some(c);
            t1 = Some(now);
        }
    }
    if !min_v.is_finite() || !max_v.is_finite() {
        return None;
    }
    let (t0, t1) = (t0?, t1?);
    let raw_span = (max_v - min_v).abs();
    let (y_lo, y_hi) = if raw_span < 1e-12 {
        let pad = (min_v.abs() * 0.05).max(1.0);
        (min_v - pad, max_v + pad)
    } else {
        let pad = (raw_span * 0.04).max(raw_span * 1e-6);
        (min_v - pad, max_v + pad)
    };
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

fn write_combined_png(
    path: &std::path::Path,
    series: &[SeriesPaint<'_>],
    plot_h: f32,
    time_range: CombinedTimeRange,
) -> Result<String, String> {
    let domain = series_time_value_domain(series, time_range)
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
