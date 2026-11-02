//! Scenarios OS viewport (Pro) — left runners + transport; right step editor.

use crate::pro::scenario_client::{
    blank_scenario, blank_step, ScenarioClientState, ScenarioStep, WaitOp,
};
use crate::ui::icons::{
    self, IconSet, CHROME_ACCENT_BORDER, CHROME_ACCENT_SOFT, CHROME_BG, CHROME_HOVER, CHROME_MUTED,
    CHROME_ROW, CHROME_TEXT,
};
use eframe::egui::{self, Color32, RichText, Sense, ViewportBuilder, ViewportClass};

const LEFT_W: f32 = 260.0;
const ROW_H: f32 = 28.0;

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
                paint_body(ui, client, profiles_names, icons, status, &mut close);
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
    ui.horizontal(|ui| {
        // ── Left: runners + transport ──────────────────────────────────────
        ui.allocate_ui_with_layout(
            egui::vec2(LEFT_W.min(avail.x * 0.38), avail.y),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                paint_left(ui, client, profiles_names, icons, status);
            },
        );
        ui.separator();
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
    // Transport for selected runner
    let sel = client.selected_id.clone();
    let st = sel.as_ref().and_then(|id| client.status_for(id).cloned());
    ui.horizontal(|ui| {
        let can = sel.is_some() && client.sidecar_up;
        let running = st.as_ref().map(|s| s.is_running()).unwrap_or(false);
        let paused = st.as_ref().map(|s| s.is_paused()).unwrap_or(false);
        if icons
            .tiny_header(ui, "play", false, "Start")
            .on_disabled_hover_text("Start selected scenario")
            .clicked()
            && can
        {
            if let Some(id) = &sel {
                match client.start(id) {
                    Ok(()) => *status = format!("Scenario started: {id}"),
                    Err(e) => *status = e,
                }
            }
        }
        if icons
            .tiny_header(ui, "pause", false, if paused { "Resume" } else { "Pause" })
            .clicked()
            && can
        {
            if let Some(id) = &sel {
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
        if icons.tiny_header(ui, "close_red", false, "Stop").clicked() && can {
            if let Some(id) = &sel {
                match client.stop(id) {
                    Ok(()) => *status = "Scenario stopped".into(),
                    Err(e) => *status = e,
                }
            }
        }
        // Step-once uses playback icon
        if icons
            .tiny_header(ui, "playback", false, "Step once")
            .clicked()
            && can
        {
            if let Some(id) = &sel {
                match client.step(id) {
                    Ok(()) => *status = "Scenario stepped".into(),
                    Err(e) => *status = e,
                }
            }
        }
        let _ = (running,);
    });
    ui.add_space(4.0);

    // Add / Delete scenario
    ui.horizontal(|ui| {
        if icons
            .tiny_header(ui, "add_blue", false, "Add scenario")
            .clicked()
        {
            let profile = profiles_names
                .first()
                .map(|(id, _)| id.as_str())
                .unwrap_or("");
            let row = blank_scenario(profile);
            match client.upsert(&row) {
                Ok(r) => {
                    client.select(r.id.clone());
                    *status = format!("Added scenario {}", r.name);
                }
                Err(e) => *status = e,
            }
        }
        if icons
            .tiny_header(ui, "delete", false, "Delete scenario")
            .clicked()
        {
            if let Some(id) = client.selected_id.clone() {
                match client.delete(&id) {
                    Ok(()) => *status = "Scenario deleted".into(),
                    Err(e) => *status = e,
                }
            }
        }
    });
    ui.add_space(4.0);
    ui.label(RichText::new("Scenarios").color(CHROME_MUTED()).small());

    egui::ScrollArea::vertical()
        .id_salt("scenarios_runner_list")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let rows: Vec<(String, String, bool)> = client
                .rows
                .iter()
                .map(|r| (r.id.clone(), r.name.clone(), r.loop_))
                .collect();
            for (id, name, loop_on) in rows {
                let st = client.status_for(&id);
                let state = st.map(|s| s.state.as_str()).unwrap_or("stopped");
                let loop_n = st.map(|s| s.loop_n).unwrap_or(0);
                let selected = client.selected_id.as_deref() == Some(id.as_str());
                let fill = if selected {
                    CHROME_ACCENT_SOFT()
                } else {
                    CHROME_ROW()
                };
                let (rect, resp) =
                    ui.allocate_exact_size(egui::vec2(ui.available_width(), ROW_H), Sense::click());
                if ui.is_rect_visible(rect) {
                    ui.painter().rect_filled(rect, 4.0, fill);
                    if selected {
                        ui.painter().rect_stroke(
                            rect,
                            4.0,
                            egui::Stroke::new(1.0, CHROME_ACCENT_BORDER()),
                            egui::StrokeKind::Inside,
                        );
                    }
                    let state_color = match state {
                        "running" => Color32::from_rgb(90, 190, 120),
                        "paused" => Color32::from_rgb(220, 180, 80),
                        _ => CHROME_MUTED(),
                    };
                    let mut label = name;
                    if label.is_empty() {
                        label = id.clone();
                    }
                    let loop_bit = if loop_on {
                        format!(" · loop {loop_n}")
                    } else if loop_n > 0 {
                        format!(" · ×{loop_n}")
                    } else {
                        String::new()
                    };
                    ui.painter().text(
                        rect.left_center() + egui::vec2(8.0, 0.0),
                        egui::Align2::LEFT_CENTER,
                        format!("{label}  ·  {state}{loop_bit}"),
                        egui::FontId::proportional(13.0),
                        if selected { CHROME_TEXT() } else { state_color },
                    );
                }
                if resp.hovered() && !selected {
                    ui.painter()
                        .rect_filled(rect, 4.0, CHROME_HOVER().gamma_multiply(0.35));
                }
                if resp.clicked() {
                    client.select(id);
                }
            }
            if client.rows.is_empty() {
                ui.label(
                    RichText::new("No scenarios yet — Add to author a script.")
                        .color(CHROME_MUTED())
                        .italics(),
                );
            }
        });
}

fn paint_right(
    ui: &mut egui::Ui,
    client: &mut ScenarioClientState,
    profiles_names: &[(String, String)],
    icons: &IconSet,
    status: &mut String,
) {
    let Some(sel_id) = client.selected_id.clone() else {
        ui.label(
            RichText::new("Select a scenario on the left to edit its steps.")
                .color(CHROME_MUTED())
                .italics(),
        );
        return;
    };

    // Scenario meta (name / profile / loop) — still "editing", not transport.
    let mut dirty = false;
    let mut draft = client
        .selected_row()
        .cloned()
        .unwrap_or_else(|| blank_scenario(""));
    ui.horizontal(|ui| {
        ui.label(RichText::new("Name").color(CHROME_MUTED()).small());
        if ui
            .add(
                egui::TextEdit::singleline(&mut draft.name)
                    .desired_width(160.0)
                    .hint_text("Scenario name"),
            )
            .changed()
        {
            dirty = true;
        }
        ui.add_space(8.0);
        ui.label(RichText::new("Profile").color(CHROME_MUTED()).small());
        egui::ComboBox::from_id_salt(("scn_profile", &sel_id))
            .selected_text(if draft.profile.is_empty() {
                "(none)".into()
            } else {
                profiles_names
                    .iter()
                    .find(|(id, _)| id == &draft.profile)
                    .map(|(_, n)| n.clone())
                    .unwrap_or_else(|| draft.profile.clone())
            })
            .width(140.0)
            .show_ui(ui, |ui| {
                for (id, name) in profiles_names {
                    if ui.selectable_label(draft.profile == *id, name).clicked() {
                        draft.profile = id.clone();
                        dirty = true;
                    }
                }
            });
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

    // Step toolbar
    ui.horizontal(|ui| {
        ui.label(RichText::new("Steps").color(CHROME_MUTED()).small());
        ui.menu_button("Add ▾", |ui| {
            for (k, label) in [
                ("publish", "Publish"),
                ("wait_time", "Wait time"),
                ("wait_value", "Wait value"),
            ] {
                if ui.button(label).clicked() {
                    draft.steps.push(blank_step(k));
                    dirty = true;
                    client.selected_step = Some(draft.steps.len().saturating_sub(1));
                    ui.close_menu();
                }
            }
        });
        if icons
            .tiny_header(ui, "delete", false, "Delete step")
            .clicked()
        {
            if let Some(i) = client.selected_step {
                if i < draft.steps.len() {
                    draft.steps.remove(i);
                    dirty = true;
                    client.selected_step = None;
                }
            }
        }
        if ui.small_button("↑").on_hover_text("Move step up").clicked() {
            if let Some(i) = client.selected_step {
                if i > 0 && i < draft.steps.len() {
                    draft.steps.swap(i, i - 1);
                    client.selected_step = Some(i - 1);
                    dirty = true;
                }
            }
        }
        if ui
            .small_button("↓")
            .on_hover_text("Move step down")
            .clicked()
        {
            if let Some(i) = client.selected_step {
                if i + 1 < draft.steps.len() {
                    draft.steps.swap(i, i + 1);
                    client.selected_step = Some(i + 1);
                    dirty = true;
                }
            }
        }
    });

    // Step list + detail fields
    let current_step = client.status_for(&sel_id).and_then(|s| s.step_index);
    egui::ScrollArea::vertical()
        .id_salt("scenarios_steps")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for i in 0..draft.steps.len() {
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

                egui::Frame::NONE
                    .fill(fill)
                    .corner_radius(4.0)
                    .inner_margin(egui::Margin::symmetric(8, 6))
                    .stroke(egui::Stroke::new(
                        if selected { 1.0 } else { 0.0 },
                        CHROME_ACCENT_BORDER(),
                    ))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            let mut en = enabled;
                            if ui.checkbox(&mut en, "").changed() {
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
                            if ui
                                .add(
                                    egui::Label::new(RichText::new(label).color(CHROME_TEXT()))
                                        .sense(Sense::click()),
                                )
                                .clicked()
                            {
                                client.selected_step = Some(i);
                            }
                        });

                        if selected {
                            ui.add_space(4.0);
                            if edit_step_fields(ui, &mut draft.steps[i], profiles_names) {
                                dirty = true;
                            }
                        }
                    });
                ui.add_space(4.0);
            }
            if draft.steps.is_empty() {
                ui.label(
                    RichText::new("No steps — Add a Publish, Wait time, or Wait value step.")
                        .color(CHROME_MUTED())
                        .italics(),
                );
            }
        });

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
        if ui
            .add(egui::TextEdit::singleline(step.name_mut()).desired_width(140.0))
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
            ui.horizontal(|ui| {
                ui.label(RichText::new("Topic").color(CHROME_MUTED()).small());
                if ui
                    .add(egui::TextEdit::singleline(topic).desired_width(220.0))
                    .changed()
                {
                    dirty = true;
                }
            });
            ui.horizontal(|ui| {
                ui.label(RichText::new("Payload").color(CHROME_MUTED()).small());
                if ui
                    .add(
                        egui::TextEdit::multiline(payload)
                            .desired_width(ui.available_width().max(200.0))
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
                dirty |= edit_optional_profile(ui, profile, profiles_names);
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
            ui.horizontal(|ui| {
                ui.label(RichText::new("Topic").color(CHROME_MUTED()).small());
                if ui
                    .add(egui::TextEdit::singleline(topic).desired_width(200.0))
                    .changed()
                {
                    dirty = true;
                }
                ui.label(RichText::new("Field").color(CHROME_MUTED()).small());
                if ui
                    .add(
                        egui::TextEdit::singleline(field)
                            .desired_width(100.0)
                            .hint_text("optional"),
                    )
                    .changed()
                {
                    dirty = true;
                }
            });
            ui.horizontal(|ui| {
                ui.label(RichText::new("Op").color(CHROME_MUTED()).small());
                let ops = [
                    (WaitOp::Eq, "eq"),
                    (WaitOp::Neq, "neq"),
                    (WaitOp::Gt, "gt"),
                    (WaitOp::Gte, "gte"),
                    (WaitOp::Lt, "lt"),
                    (WaitOp::Lte, "lte"),
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
                if ui
                    .add(egui::TextEdit::singleline(value).desired_width(160.0))
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
                dirty |= edit_optional_profile(ui, profile, profiles_names);
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

fn edit_optional_profile(
    ui: &mut egui::Ui,
    profile: &mut Option<String>,
    profiles_names: &[(String, String)],
) -> bool {
    let mut dirty = false;
    ui.label(
        RichText::new("Profile override")
            .color(CHROME_MUTED())
            .small(),
    );
    let cur = profile.clone().unwrap_or_default();
    egui::ComboBox::from_id_salt(("step_profile", cur.as_str()))
        .selected_text(if cur.is_empty() {
            "(scenario default)".into()
        } else {
            profiles_names
                .iter()
                .find(|(id, _)| id == &cur)
                .map(|(_, n)| n.clone())
                .unwrap_or(cur.clone())
        })
        .width(120.0)
        .show_ui(ui, |ui| {
            if ui
                .selectable_label(cur.is_empty(), "(scenario default)")
                .clicked()
            {
                *profile = None;
                dirty = true;
            }
            for (id, name) in profiles_names {
                if ui.selectable_label(cur == *id, name).clicked() {
                    *profile = Some(id.clone());
                    dirty = true;
                }
            }
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
    let import_dlg = egui::Id::new("dlg_scn_import");
    let import_mode_dlg = egui::Id::new("dlg_scn_import_mode");
    let import_path_key = egui::Id::new("dlg_scn_import_path");
    let armed = client.toolbar_armed();

    if icons
        .tiny_header(ui, "export", false, "Export scenarios")
        .clicked()
        && armed
    {
        match client.export() {
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
                }
                Err(e) => *status = format!("json: {e}"),
            },
            Err(e) => *status = e,
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
        if client.rows.is_empty() {
            finish_scenario_import(client, status, &path, "replace");
        } else {
            ui.ctx().data_mut(|d| d.insert_temp(import_path_key, path));
            crate::ui::native_dialog::begin_message(ui.ctx(), import_mode_dlg, |d| {
                d.set_level(rfd::MessageLevel::Info)
                    .set_title("Import scenarios")
                    .set_description(
                        "Replace the current list, or Merge (upsert by id and append the rest)?\n\
                         Accepts bare [ … ] or { \"scenarios\": [ … ] }.",
                    )
                    .set_buttons(rfd::MessageButtons::YesNoCancelCustom(
                        "Replace".into(),
                        "Merge".into(),
                        "Cancel".into(),
                    ))
            });
        }
    }
    if let Some(result) = crate::ui::native_dialog::poll_message(ui.ctx(), import_mode_dlg) {
        if let Some(mode) = crate::ui::native_dialog::replace_merge_mode(result) {
            if let Some(path) = ui
                .ctx()
                .data_mut(|d| d.remove_temp::<std::path::PathBuf>(import_path_key))
            {
                finish_scenario_import(client, status, &path, mode);
            }
        } else {
            let _ = ui
                .ctx()
                .data_mut(|d| d.remove_temp::<std::path::PathBuf>(import_path_key));
        }
    }
}

fn finish_scenario_import(
    client: &mut ScenarioClientState,
    status: &mut String,
    path: &std::path::Path,
    mode: &str,
) {
    match std::fs::read_to_string(path) {
        Ok(text) => match serde_json::from_str::<serde_json::Value>(&text) {
            Ok(v) => match client.import_mode(&v, mode) {
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
) {
    let mut close = false;
    icons::apply_pro_panel_chrome(ui);
    paint_body(ui, client, profiles_names, icons, status, &mut close);
    let _ = close;
}
