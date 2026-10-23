//! Ribbon chrome, side/central panels, logo + font helpers.

use super::{status_strip_fade_alpha, MqttMonitorApp, PROFILES_PANEL_MIN_W};
use crate::mqtt::publish::PublishFromTree;
use crate::mqtt::session::SessionState;
use crate::ui::icons::{
    self, ribbon_compact_columns, CHROME_BG, CHROME_CAPTION, CHROME_MUTED, CHROME_SEP, CHROME_TEXT,
    TINT_APP, TINT_PANELS, TINT_PROFILES,
};
use crate::ui::ribbon_layout::{self, SectionMode};
use crate::ui::shortcuts;
use crate::ui::{
    payload_panel, pin_board, profiles, publish_panel, stats_ribbon, topic_tree_panel,
    traffic_panel,
};
use eframe::egui::{
    self, Align, Color32, FontFamily, FontId, RichText, TextStyle, TextureHandle,
};
use image::RgbaImage;
use std::collections::BTreeMap;
use std::time::Instant;

/// Ribbon panel height: brand/group band (`brand_h` 70) + vertical frame margins (4+4).
const RIBBON_PANEL_H: f32 = 78.0;
const STATUS_STRIP_H: f32 = 24.0;

/// Rects reserved by top chrome panels (real UI paints later on Middle overlays).
pub(super) struct TopChromeRects {
    pub ribbon: egui::Rect,
    pub status: egui::Rect,
}

impl MqttMonitorApp {
    /// Profiles ribbon group (full captions or compact 2-high icons).
    fn draw_ribbon_profiles(&mut self, ui: &mut egui::Ui, mode: SectionMode) {
        icons::ribbon_group(ui, "Profiles", TINT_PROFILES, |ui| {
            let tip_new = shortcuts::tip("New profile", "Ctrl+N");
            let tip_save = shortcuts::tip("Save profiles", "Ctrl+S");
            let tip_del = shortcuts::tip("Delete selected profile", "Delete / Ctrl+Backspace");
            let tip_panel = shortcuts::tip("Show or hide the profiles side panel", "Ctrl+Shift+P");
            if mode == SectionMode::Full {
                if self
                    .icons
                    .command(ui, "profile_add", "New", false, &tip_new)
                    .clicked()
                {
                    self.action_new_profile();
                }
                if self
                    .icons
                    .command(ui, "save", "Save", false, &tip_save)
                    .clicked()
                {
                    self.action_save_profiles();
                }
                if self
                    .icons
                    .command_enabled(
                        ui,
                        "disconnect",
                        "Delete",
                        false,
                        &tip_del,
                        self.panels.profiles,
                    )
                    .clicked()
                {
                    if let Some(id) = self.profiles.delete_selected() {
                        self.sessions.close(&id);
                        self.status = format!("Deleted profile {id}");
                    }
                }
                if self
                    .icons
                    .command(ui, "panel_docked", "Profiles", self.panels.profiles, &tip_panel)
                    .clicked()
                {
                    self.panels.profiles = !self.panels.profiles;
                    self.profiles_auto_opened = false;
                    self.persist_panels();
                }
            } else {
                let mut clicks = [false; 4];
                ribbon_compact_columns(ui, 4, |i, ui| {
                    clicks[i] = match i {
                        0 => self
                            .icons
                            .command_compact(ui, "profile_add", false, &tip_new)
                            .clicked(),
                        1 => self
                            .icons
                            .command_compact(ui, "save", false, &tip_save)
                            .clicked(),
                        2 => self
                            .icons
                            .command_compact_enabled(
                                ui,
                                "disconnect",
                                false,
                                &tip_del,
                                self.panels.profiles,
                            )
                            .clicked(),
                        _ => self
                            .icons
                            .command_compact(
                                ui,
                                "panel_docked",
                                self.panels.profiles,
                                &tip_panel,
                            )
                            .clicked(),
                    };
                });
                if clicks[0] {
                    self.action_new_profile();
                }
                if clicks[1] {
                    self.action_save_profiles();
                }
                if clicks[2] {
                    if let Some(id) = self.profiles.delete_selected() {
                        self.sessions.close(&id);
                        self.status = format!("Deleted profile {id}");
                    }
                }
                if clicks[3] {
                    self.panels.profiles = !self.panels.profiles;
                    self.profiles_auto_opened = false;
                    self.persist_panels();
                }
            }
        });
    }

    /// Panels ribbon group (full or compact).
    fn draw_ribbon_panels(&mut self, ui: &mut egui::Ui, mode: SectionMode) {
        icons::ribbon_group(ui, "Panels", TINT_PANELS, |ui| {
            let tips = [
                shortcuts::tip("Live traffic", "Ctrl+Shift+T"),
                shortcuts::tip("Publish panel", "Ctrl+Shift+U"),
                shortcuts::tip("Payload panel", "Ctrl+Shift+L"),
                shortcuts::tip("Sticky topics", "Ctrl+Shift+K"),
                shortcuts::tip("Topic tree", "Ctrl+Shift+R"),
            ];
            let icons_names = [
                "incoming_messages",
                "publish_message",
                "panel_undock",
                "sticky_topic",
                "topic_tree",
            ];
            let labels = ["Traffic", "Publish", "Payload", "Stickies", "Tree"];
            let selected = [
                self.panels.live_traffic,
                self.panels.publish,
                self.panels.payload,
                self.panels.stickies,
                self.panels.topic_tree,
            ];
            let mut clicks = [false; 5];
            if mode == SectionMode::Full {
                for i in 0..5 {
                    clicks[i] = self
                        .icons
                        .command(ui, icons_names[i], labels[i], selected[i], &tips[i])
                        .clicked();
                }
            } else {
                ribbon_compact_columns(ui, 5, |i, ui| {
                    clicks[i] = self
                        .icons
                        .command_compact(ui, icons_names[i], selected[i], &tips[i])
                        .clicked();
                });
            }
            if clicks[0] {
                self.panels.live_traffic = !self.panels.live_traffic;
                self.persist_panels();
            }
            if clicks[1] {
                self.panels.publish = !self.panels.publish;
                self.persist_panels();
            }
            if clicks[2] {
                self.panels.payload = !self.panels.payload;
                self.persist_panels();
            }
            if clicks[3] {
                self.panels.stickies = !self.panels.stickies;
                self.persist_panels();
            }
            if clicks[4] {
                self.panels.topic_tree = !self.panels.topic_tree;
                self.persist_panels();
            }
        });
    }

    /// App ribbon group: Settings + zoom (full or compact).
    fn draw_ribbon_app(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, mode: SectionMode) {
        icons::ribbon_group(ui, "App", TINT_APP, |ui| {
            let tip_set = shortcuts::tip("Open settings window", "Ctrl+,");
            let tip_out = shortcuts::tip("Decrease font size (−4 steps from default)", "Ctrl+-");
            let tip_in =
                shortcuts::tip("Increase font size (+5 steps from default)", "Ctrl+= / Ctrl++");
            let mut clicks = [false; 3];
            if mode == SectionMode::Full {
                clicks[0] = self
                    .icons
                    .command(
                        ui,
                        "configuration_sliders",
                        "Settings",
                        self.settings_open,
                        &tip_set,
                    )
                    .clicked();
                clicks[1] = self
                    .icons
                    .command(ui, "zoom_out", "Zoom−", false, &tip_out)
                    .clicked();
                clicks[2] = self
                    .icons
                    .command(ui, "zoom_in", "Zoom+", false, &tip_in)
                    .clicked();
            } else {
                ribbon_compact_columns(ui, 3, |i, ui| {
                    clicks[i] = match i {
                        0 => self
                            .icons
                            .command_compact(
                                ui,
                                "configuration_sliders",
                                self.settings_open,
                                &tip_set,
                            )
                            .clicked(),
                        1 => self
                            .icons
                            .command_compact(ui, "zoom_out", false, &tip_out)
                            .clicked(),
                        _ => self
                            .icons
                            .command_compact(ui, "zoom_in", false, &tip_in)
                            .clicked(),
                    };
                });
            }
            if clicks[0] {
                self.open_settings();
            }
            if clicks[1] {
                if self.settings.zoom_font_out() {
                    let _ = self.settings.save();
                    apply_font_zoom(ctx, self.settings.font_zoom_factor());
                    self.status = format!("Font zoom {}", self.settings.font_zoom_step);
                }
            }
            if clicks[2] {
                if self.settings.zoom_font_in() {
                    let _ = self.settings.save();
                    apply_font_zoom(ctx, self.settings.font_zoom_factor());
                    self.status = format!("Font zoom {}", self.settings.font_zoom_step);
                }
            }
        });
    }

    /// Reserve top chrome space only — real ribbon/status paint after CentralPanel
    /// on Middle overlays so sticky/tree scroll cannot draw over the navbar.
    pub(super) fn reserve_top_chrome(&mut self, ctx: &egui::Context) -> TopChromeRects {
        let ribbon = egui::TopBottomPanel::top("ribbon")
            .resizable(false)
            .exact_height(RIBBON_PANEL_H)
            .show_separator_line(false)
            .frame(
                egui::Frame::NONE
                    .fill(CHROME_BG)
                    .stroke(egui::Stroke::new(1.0_f32, CHROME_SEP)),
            )
            .show(ctx, |ui| {
                reserve_side_panel_space(ui);
            });

        let status = egui::TopBottomPanel::top("status_strip")
            .exact_height(STATUS_STRIP_H)
            .show_separator_line(false)
            .frame(egui::Frame::NONE.fill(CHROME_BG))
            .show(ctx, |ui| {
                reserve_side_panel_space(ui);
            });

        TopChromeRects {
            ribbon: ribbon.response.rect,
            status: status.response.rect,
        }
    }

    /// Paint ribbon + status above central scroll (covers sticky/graph bleed).
    pub(super) fn draw_top_chrome_overlays(
        &mut self,
        ctx: &egui::Context,
        rects: TopChromeRects,
    ) {
        top_chrome_overlay(ctx, "ribbon_front", rects.ribbon, |ui| {
            self.paint_ribbon_bar(ui, ctx);
        });
        top_chrome_overlay(ctx, "status_front", rects.status, |ui| {
            self.paint_status_strip(ui);
        });
    }

    /// Ribbon command bar body (drawn inside the Middle overlay).
    fn paint_ribbon_bar(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.visuals_mut().override_text_color = Some(CHROME_TEXT);
        ui.spacing_mut().item_spacing.y = 0.0;

        // Top-align the ribbon row. Brand logo + two-line title centered in brand_h.
        ui.with_layout(egui::Layout::left_to_right(egui::Align::Min), |ui| {
            ui.spacing_mut().item_spacing.x = 4.0;

            let logo_size = self
                .logo
                .as_ref()
                .map(|l| l.size_vec2())
                .unwrap_or(egui::vec2(108.0, 54.0));
            let brand_h = 70.0; // match ribbon group / separator height
            let green = Color32::from_rgb(90, 190, 120);
            let title_size = 24.0;
            let line_gap = -2.0_f32;
            let strong_family = ui
                .style()
                .text_styles
                .get(&TextStyle::Name("Strong".into()))
                .map(|f| f.family.clone())
                .unwrap_or(FontFamily::Proportional);
            let font_id = FontId::new(title_size, strong_family);
            let (line1_g, line2_g) = ui.fonts(|f| {
                (
                    f.layout_no_wrap("Topic".into(), font_id.clone(), green),
                    f.layout_no_wrap("Bench".into(), font_id, green),
                )
            });
            let title_w = line1_g.size().x.max(line2_g.size().x).ceil();
            let stack_h = line1_g.size().y + line2_g.size().y + line_gap;
            let logo_w = logo_size.x;

            // Collapse: App → Profiles → Panels → hide title → hide Stats.
            let avail_w = ui.available_width();
            let plan = ribbon_layout::plan_for_width(
                avail_w,
                logo_w,
                title_w,
                stats_ribbon::estimate_width(),
                icons::font_zoom_factor(ui),
            );
            let brand_w = if plan.show_title {
                logo_w + 10.0 + title_w + 10.0
            } else {
                logo_w + 10.0
            };

            const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
            let ver_font = FontId::new(10.0, FontFamily::Proportional);
            let ver_g = ui.fonts(|f| {
                f.layout_no_wrap(format!("v{APP_VERSION}"), ver_font, CHROME_MUTED)
            });
            ui.allocate_ui_with_layout(
                egui::vec2(brand_w, brand_h),
                egui::Layout::left_to_right(Align::Center),
                |ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    let (logo_rect, _) =
                        ui.allocate_exact_size(egui::vec2(logo_w, brand_h), egui::Sense::hover());
                    if let Some(logo) = &self.logo {
                        let logo_pos = egui::pos2(
                            logo_rect.center().x - logo_size.x * 0.5,
                            logo_rect.center().y - logo_size.y * 0.5,
                        );
                        ui.put(
                            egui::Rect::from_min_size(logo_pos, logo_size),
                            egui::Image::new(logo)
                                .fit_to_exact_size(logo_size)
                                .maintain_aspect_ratio(true)
                                .sense(egui::Sense::hover()),
                        );
                    }
                    ui.painter().galley(
                        egui::pos2(
                            logo_rect.right() - ver_g.size().x,
                            logo_rect.bottom() - ver_g.size().y,
                        ),
                        ver_g,
                        CHROME_MUTED,
                    );
                    if plan.show_title {
                        ui.add_space(10.0);
                        let (title_rect, _) = ui.allocate_exact_size(
                            egui::vec2(title_w, brand_h),
                            egui::Sense::hover(),
                        );
                        let top = title_rect.center().y - stack_h * 0.5;
                        let line1_x = title_rect.left() + (title_w - line1_g.size().x) * 0.5;
                        let line2_x = title_rect.left() + (title_w - line2_g.size().x) * 0.5;
                        ui.painter()
                            .galley(egui::pos2(line1_x, top), line1_g, green);
                        ui.painter().galley(
                            egui::pos2(line2_x, top + stack_h - line2_g.size().y),
                            line2_g,
                            green,
                        );
                        ui.add_space(10.0);
                    }
                },
            );

            icons::ribbon_separator(ui);
            self.draw_ribbon_profiles(ui, plan.profiles);
            icons::ribbon_separator(ui);
            self.draw_ribbon_panels(ui, plan.panels);
            icons::ribbon_separator(ui);
            self.draw_ribbon_app(ui, ctx, plan.app);

            if plan.show_stats {
                let remaining = ui.available_width();
                if remaining > 8.0 {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                        ui.set_max_width(remaining);
                        stats_ribbon::draw_group(ui, &self.sessions.traffic_stats);
                        icons::ribbon_separator(ui);
                    });
                }
            }
        });
    }

    /// Status strip body (drawn inside the Middle overlay).
    fn paint_status_strip(&mut self, ui: &mut egui::Ui) {
        ui.visuals_mut().override_text_color = Some(CHROME_TEXT);
        let connected = self
            .sessions
            .open
            .values()
            .filter(|s| s.state == SessionState::Connected)
            .count();
        let broker_label = if connected == 1 {
            "1 broker connected".to_string()
        } else {
            format!("{connected} brokers connected")
        };
        let msgs = self.sessions.traffic_stats.format_msgs_per_min();
        let broker_green = Color32::from_rgb(90, 190, 120);

        ui.horizontal_centered(|ui| {
            ui.label(RichText::new(broker_label).color(broker_green));
            ui.add_space(8.0);
            ui.label(RichText::new("·").color(CHROME_MUTED));
            ui.add_space(8.0);
            ui.label(RichText::new(msgs).color(CHROME_CAPTION));
            ui.add_space(12.0);
            ui.label(RichText::new("·").color(CHROME_MUTED));
            ui.add_space(12.0);

            let alpha = status_strip_fade_alpha(self.status_changed_at, Instant::now());
            if alpha > 0.01 && !self.status.is_empty() {
                let a = (255.0 * alpha) as u8;
                ui.label(
                    RichText::new(&self.status).color(Color32::from_rgba_unmultiplied(
                        CHROME_TEXT.r(),
                        CHROME_TEXT.g(),
                        CHROME_TEXT.b(),
                        a,
                    )),
                );
            }
        });
    }

    /// Side panels + central scroll content (traffic / stickies / topic tree).
    ///
    /// Side panels only reserve space first; their real UI is painted afterward on a
    /// higher layer so sticky/tree content cannot bleed through when the window is
    /// narrow and panels overlap (egui paints CentralPanel last on the background layer).
    pub(super) fn draw_panels(&mut self, ctx: &egui::Context) {
        let mut profiles_rect = None;
        let mut publish_rect = None;
        let mut payload_rect = None;

        if self.panels.profiles {
            // Hard min width — resize handle cannot drag past this (avoids see-through gap).
            let r = egui::SidePanel::left("profiles")
                .default_width(320.0)
                .min_width(PROFILES_PANEL_MIN_W)
                .width_range(PROFILES_PANEL_MIN_W..=560.0)
                .resizable(true)
                .show(ctx, |ui| {
                    reserve_side_panel_space(ui);
                });
            profiles_rect = Some(r.response.rect);
        }

        if self.panels.publish {
            let r = egui::SidePanel::right("publish")
                .default_width(280.0)
                .min_width(200.0)
                .width_range(200.0..=480.0)
                .resizable(true)
                .show(ctx, |ui| {
                    reserve_side_panel_space(ui);
                });
            publish_rect = Some(r.response.rect);
        }

        if self.panels.payload {
            let r = egui::SidePanel::right("payload")
                .default_width(320.0)
                .min_width(220.0)
                .width_range(220.0..=560.0)
                .resizable(true)
                .show(ctx, |ui| {
                    reserve_side_panel_space(ui);
                });
            payload_rect = Some(r.response.rect);
        }

        // Match SidePanel vertical margin (2) — default CentralPanel uses 8 and drops the header.
        egui::CentralPanel::default()
            .frame(
                egui::Frame::NONE
                    .fill(CHROME_BG)
                    .inner_margin(egui::Margin::symmetric(8, 2)),
            )
            .show(ctx, |ui| {
                // Clip to central panel only (not full screen → no ribbon bleed).
                let panel_rect = ui.max_rect();
                ui.set_clip_rect(panel_rect);
                // AA room for strokes/#N; not a substitute for flash::ITEM_CLIP_CLEAR on items.
                ui.visuals_mut().clip_rect_margin = 4.0;
                // Scrollbar track reserve only — independent of per-item ITEM_CLIP_CLEAR.
                {
                    let scroll = &mut ui.spacing_mut().scroll;
                    scroll.floating_allocated_width = scroll.bar_width;
                }
                // Keep full panel width (default auto_shrink follows content → early ellipsis).
                egui::ScrollArea::vertical()
                    .id_salt("central_main")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        // Keep ScrollArea's viewport clip — do not widen to content max_rect.
                        if self.panels.live_traffic {
                            traffic_panel::draw(
                                ui,
                                &self.sessions,
                                &self.profiles.selected_id,
                                &self.search,
                                &mut self.status,
                            );
                            ui.add_space(8.0);
                            ui.separator();
                            ui.add_space(8.0);
                        }

                        if self.panels.stickies {
                            pin_board::draw(
                                ui,
                                &mut self.pins,
                                &mut self.profiles,
                                &self.sessions,
                                &mut self.settings,
                                &self.icons,
                                &self.search,
                                &mut self.status,
                            );
                            ui.add_space(8.0);
                            ui.separator();
                            ui.add_space(8.0);
                        }

                        if self.panels.topic_tree {
                            let mut publish_from_tree: Option<PublishFromTree> = None;
                            let search_before = (
                                self.search.query.clone(),
                                self.search.scope,
                                self.search.apply_to_stickies,
                            );
                            topic_tree_panel::draw(
                                ui,
                                &mut self.sessions,
                                &mut self.profiles,
                                &mut self.pins,
                                self.settings.topic_tree_expand,
                                &self.icons,
                                &mut self.search,
                                &mut self.focus_search,
                                &mut self.status,
                                &mut publish_from_tree,
                                &mut self.topic_tree_undocked,
                            );
                            if self.search.query != search_before.0
                                || self.search.scope != search_before.1
                                || self.search.apply_to_stickies != search_before.2
                            {
                                self.persist_search();
                            }
                            // Leaf double-click → open Publish and fill broker / topic / payload.
                            self.apply_publish_from_tree(publish_from_tree);
                        }
                        // Pad so the last tree row can scroll fully into the viewport.
                        ui.add_space(16.0);
                    });
            });

        // Paint side panel contents above the central column (covers overlap bleed).
        self.draw_side_panel_overlays(ctx, profiles_rect, publish_rect, payload_rect);
    }

    /// Real side-panel UI on [`Order::Middle`] so it stays above central stickies/tree.
    fn draw_side_panel_overlays(
        &mut self,
        ctx: &egui::Context,
        profiles_rect: Option<egui::Rect>,
        publish_rect: Option<egui::Rect>,
        payload_rect: Option<egui::Rect>,
    ) {
        if let Some(rect) = profiles_rect {
            side_panel_overlay(ctx, "profiles_front", rect, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    profiles::draw(
                        ui,
                        &mut self.profiles,
                        &mut self.sessions,
                        &mut self.pins,
                        &mut self.settings,
                        &mut self.profile_filter,
                        &self.icons,
                        &mut self.sub_draft,
                        &mut self.status,
                        ctx,
                        &mut self.focus_profile_editor,
                        &mut self.show_profile_password,
                    );
                });
            });
        }

        if let Some(rect) = publish_rect {
            side_panel_overlay(ctx, "publish_front", rect, |ui| {
                publish_panel::draw(
                    ui,
                    &mut self.publish,
                    &mut self.publish_history,
                    &mut self.sessions,
                    &self.profiles,
                    &self.icons,
                    &mut self.status,
                );
            });
        }

        if let Some(rect) = payload_rect {
            let stickies_before = self.panels.stickies;
            side_panel_overlay(ctx, "payload_front", rect, |ui| {
                payload_panel::draw(
                    ui,
                    &mut self.sessions,
                    &mut self.profiles,
                    &mut self.pins,
                    &self.icons,
                    &mut self.payload_search,
                    &mut self.panels.stickies,
                    &mut self.status,
                    &mut self.payload_popped,
                );
            });
            if self.panels.stickies != stickies_before {
                self.persist_panels();
            }
        }
    }

    /// Floating payload windows — each locked to the topic when it was popped.
    pub(super) fn draw_undocked_payloads(&mut self, ctx: &egui::Context) {
        if self.payload_popped.is_empty() {
            return;
        }
        let stickies_before = self.panels.stickies;
        payload_panel::draw_undocked_viewports(
            ctx,
            &mut self.sessions,
            &mut self.profiles,
            &mut self.pins,
            &self.icons,
            &mut self.panels.stickies,
            &mut self.status,
            &mut self.payload_popped,
        );
        if self.panels.stickies != stickies_before {
            self.persist_panels();
        }
    }

    /// Floating topic-tree windows for brokers that were popped out.
    pub(super) fn draw_undocked_topic_trees(&mut self, ctx: &egui::Context) {
        if self.topic_tree_undocked.is_empty() {
            return;
        }
        let mut publish_from_tree: Option<PublishFromTree> = None;
        topic_tree_panel::draw_undocked_viewports(
            ctx,
            &mut self.sessions,
            &mut self.profiles,
            &mut self.pins,
            self.settings.topic_tree_expand,
            &self.icons,
            &mut self.search,
            &mut self.status,
            &mut publish_from_tree,
            &mut self.topic_tree_undocked,
        );
        self.apply_publish_from_tree(publish_from_tree);
    }

    fn apply_publish_from_tree(&mut self, fill: Option<PublishFromTree>) {
        let Some(fill) = fill else {
            return;
        };
        self.publish.apply_from_tree(&fill);
        if !self.panels.publish {
            self.panels.publish = true;
            self.persist_panels();
        }
        self.status = format!(
            "Publish filled from tree: {} · {} (retain={})",
            fill.profile_name, fill.topic, fill.retain
        );
    }
}

/// Hold the side panel's allocated rect without drawing interactive content yet.
fn reserve_side_panel_space(ui: &mut egui::Ui) {
    ui.set_min_height(ui.available_height());
    ui.allocate_at_least(ui.available_size(), egui::Sense::hover());
}

/// Paint a side panel's real UI above the central column (fixes overlap z-order).
fn side_panel_overlay(
    ctx: &egui::Context,
    overlay_id: impl Into<egui::Id>,
    rect: egui::Rect,
    add_contents: impl FnOnce(&mut egui::Ui),
) {
    egui::Area::new(overlay_id.into())
        .order(egui::Order::Middle)
        .fixed_pos(rect.min)
        .default_size(rect.size())
        .constrain(false)
        .interactable(true)
        .show(ctx, |ui| {
            ui.set_min_size(rect.size());
            ui.set_max_size(rect.size());
            ui.set_clip_rect(rect);
            egui::Frame::side_top_panel(ui.style())
                .fill(CHROME_BG)
                .show(ui, |ui| {
                    ui.set_min_height(ui.available_height());
                    add_contents(ui);
                });
        });
}

/// Paint top chrome (ribbon / status) above central scroll (same z-order fix as sides).
fn top_chrome_overlay(
    ctx: &egui::Context,
    overlay_id: impl Into<egui::Id>,
    rect: egui::Rect,
    add_contents: impl FnOnce(&mut egui::Ui),
) {
    // Status strip is shorter — tighter margin, no box stroke (ribbon keeps chrome border).
    let is_status = rect.height() <= STATUS_STRIP_H + 1.0;
    let inner = if is_status {
        egui::Margin::symmetric(10, 3)
    } else {
        egui::Margin::symmetric(8, 4)
    };
    egui::Area::new(overlay_id.into())
        .order(egui::Order::Middle)
        .fixed_pos(rect.min)
        .default_size(rect.size())
        .constrain(false)
        .interactable(true)
        .show(ctx, |ui| {
            ui.set_min_size(rect.size());
            ui.set_max_size(rect.size());
            ui.set_clip_rect(rect);
            let mut frame = egui::Frame::NONE
                .fill(CHROME_BG)
                .inner_margin(inner);
            if !is_status {
                frame = frame.stroke(egui::Stroke::new(1.0_f32, CHROME_SEP));
            }
            frame.show(ui, |ui| {
                ui.set_min_height(ui.available_height());
                add_contents(ui);
            });
        });
}

/// Apply persisted font zoom to all egui text styles (relative to Style::default()).
pub(super) fn apply_font_zoom(ctx: &egui::Context, factor: f32) {
    let base = egui::Style::default().text_styles;
    let mut styles: BTreeMap<TextStyle, FontId> = BTreeMap::new();
    for (style, id) in base {
        styles.insert(
            style,
            FontId::new((id.size * factor).max(8.0), id.family),
        );
    }
    ctx.style_mut(|s| s.text_styles = styles);
}

/// Load AA logo at an **integer** 1/N of native pixels (box-average), painted 1:1 — no blurry fractional scale.
pub(super) fn load_logo(ctx: &egui::Context) -> Option<TextureHandle> {
    load_aa_logo(ctx, "aa-logo", 40, egui::TextureFilter::Nearest)
}

/// Larger AA mark for the startup splash (~half-window paint).
pub(super) fn load_splash_logo(ctx: &egui::Context) -> Option<TextureHandle> {
    load_aa_logo(ctx, "splash-aa-logo", 512, egui::TextureFilter::Linear)
}

fn load_aa_logo(
    ctx: &egui::Context,
    id: &str,
    max_h: u32,
    filter: egui::TextureFilter,
) -> Option<TextureHandle> {
    let bytes = include_bytes!("../../assets/agile-automation-logo.png");
    let raw = image::load_from_memory(bytes).ok()?.into_rgba8();
    let (w, h) = raw.dimensions();
    let factor = (h / max_h).max(1);
    let cw = (w / factor) * factor;
    let ch = (h / factor) * factor;
    let ox = (w - cw) / 2;
    let oy = (h - ch) / 2;
    let cropped = image::imageops::crop_imm(&raw, ox, oy, cw, ch).to_image();
    let logo = integer_box_downscale(&cropped, factor);
    let size = [logo.width() as usize, logo.height() as usize];
    let color = egui::ColorImage::from_rgba_unmultiplied(size, logo.as_raw());
    Some(ctx.load_texture(
        id,
        color,
        egui::TextureOptions {
            magnification: filter,
            minification: filter,
            ..egui::TextureOptions::LINEAR
        },
    ))
}

fn integer_box_downscale(src: &RgbaImage, factor: u32) -> RgbaImage {
    if factor <= 1 {
        return src.clone();
    }
    let w = src.width() / factor;
    let h = src.height() / factor;
    let mut out = RgbaImage::new(w, h);
    let area = (factor * factor) as u32;
    for y in 0..h {
        for x in 0..w {
            let mut r = 0u32;
            let mut g = 0u32;
            let mut b = 0u32;
            let mut a = 0u32;
            for dy in 0..factor {
                for dx in 0..factor {
                    let p = src.get_pixel(x * factor + dx, y * factor + dy).0;
                    r += p[0] as u32;
                    g += p[1] as u32;
                    b += p[2] as u32;
                    a += p[3] as u32;
                }
            }
            out.put_pixel(
                x,
                y,
                image::Rgba([
                    (r / area) as u8,
                    (g / area) as u8,
                    (b / area) as u8,
                    (a / area) as u8,
                ]),
            );
        }
    }
    out
}
