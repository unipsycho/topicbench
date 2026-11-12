//! Settings + keyboard-shortcuts + About + update-available overlay modals.

use super::MqttMonitorApp;
use crate::mqtt::settings::AppSettings;
use crate::ui::icons::{
    CHROME_ACCENT_BORDER, CHROME_ACCENT_SOFT, CHROME_MUTED, CHROME_SEP, CHROME_TEXT, TINY_ICON,
};
use crate::ui::{settings_panel, shortcuts};
use crate::update::{self, InstallOutcome, UpdateEvent, UpdateStatus};
use eframe::egui::{self, FontId, RichText};

/// Company site opened from the About dialog (Modbus / Protocol Bench parity).
const COMPANY_URL: &str = "https://www.agileautomation.ca";
const COMPANY_LINK: &str = "www.agileautomation.ca";
const GITHUB_URL: &str = "https://github.com/unipsycho/topicbench";
/// Mike’s wording; sentence case + period to match Chart / Modbus slogan presentation.
const ABOUT_SLOGAN: &str = "Your bench for every imaginable topic.";

impl MqttMonitorApp {
    /// Open Settings with a draft snapshot of current settings.
    pub(super) fn open_settings(&mut self) {
        self.settings_draft = Some(self.settings.clone());
        self.settings_open = true;
        // One-shot Pro version probe on open — never every frame (stalls MQTT stats).
        self.refresh_pro_update_info();
    }

    /// Commit draft to live settings + disk, then close.
    fn save_settings_modal(&mut self, ctx: &egui::Context) {
        if let Some(draft) = self.settings_draft.take() {
            // Keep update prefs from live settings (edited outside the draft).
            let last_update_check = self.settings.last_update_check;
            let skipped_update_version = self.settings.skipped_update_version.clone();
            let remind_later_until = self.settings.remind_later_until;
            self.settings = draft;
            self.settings.last_update_check = last_update_check;
            self.settings.skipped_update_version = skipped_update_version;
            self.settings.remind_later_until = remind_later_until;
            if let Some(tc) = self.theme_client.as_mut() {
                // Persist + ensure chrome matches (preview may already have applied).
                tc.preview(ctx, &self.settings.theme_id);
            } else {
                // Unlicensed installs only persist Classic.
                self.settings.theme_id = crate::ui::theme::CLASSIC_ID.to_string();
                crate::ui::theme::apply_theme(ctx, &crate::ui::theme::classic());
            }
            match self.settings.save() {
                Ok(()) => {
                    self.status = format!("Saved {}", AppSettings::path().display());
                }
                Err(e) => self.status = e,
            }
        }
        self.settings_open = false;
    }

    /// Discard draft, revert theme preview to saved id, then close.
    fn cancel_settings_modal(&mut self, ctx: &egui::Context) {
        let revert = self.settings.theme_id.clone();
        if let Some(tc) = self.theme_client.as_mut() {
            tc.preview(ctx, &revert);
        } else {
            crate::ui::theme::apply_theme(ctx, &crate::ui::theme::classic());
        }
        self.settings_draft = None;
        self.settings_open = false;
    }

    /// Kick a manual check from Settings → Check now.
    pub(super) fn request_update_check(&mut self, quiet: bool) {
        if matches!(
            self.update_status,
            UpdateStatus::Checking | UpdateStatus::Installing
        ) {
            return;
        }
        self.update_check_quiet = quiet;
        self.update_status = UpdateStatus::Checking;
        self.update_service.start_check();
        // Delegate Pro channel check to the sidecar when licensed (no Pro URLs in OSS).
        if crate::pro::is_licensed() {
            crate::pro::update_client::request_check();
        }
        self.refresh_pro_update_info();
    }

    /// Probe localhost Pro sidecar for version / update status (hide lines if down).
    pub(super) fn refresh_pro_update_info(&mut self) {
        self.pro_update_info = crate::pro::update_client::probe();
        self.last_pro_update_probe = Some(std::time::Instant::now());
    }

    /// Apply a background update event on the UI thread.
    pub(super) fn handle_update_event(&mut self, ev: UpdateEvent) {
        match ev {
            UpdateEvent::CheckOk {
                info,
                update_available,
            } => {
                self.settings.last_update_check = Some(update::now_unix());
                let _ = self.settings.save();
                match info {
                    Some(info) if update_available => {
                        let skipped = self
                            .settings
                            .skipped_update_version
                            .as_deref()
                            .map(|s| s == info.tag || s == info.version)
                            .unwrap_or(false);
                        let remind = update::remind_later_active(self.settings.remind_later_until)
                            || self.update_remind_session;
                        self.update_status = UpdateStatus::Available(info.clone());
                        if !skipped && !remind {
                            self.update_modal_open = true;
                            self.update_offer = Some(info);
                        } else {
                            self.update_offer = Some(info);
                        }
                    }
                    Some(info) => {
                        self.update_status = UpdateStatus::UpToDate {
                            latest: info.tag.clone(),
                        };
                        self.update_offer = None;
                        self.update_modal_open = false;
                    }
                    None => {
                        self.update_status = UpdateStatus::UpToDate {
                            latest: update::current_version().into(),
                        };
                    }
                }
                // Free check finished first — re-read Pro (may still be checking).
                if crate::pro::is_licensed() {
                    self.refresh_pro_update_info();
                }
            }
            UpdateEvent::CheckFailed(msg) => {
                self.settings.last_update_check = Some(update::now_unix());
                let _ = self.settings.save();
                self.update_status = UpdateStatus::Error(msg);
                // Quiet on startup — Settings still shows “Couldn't check”.
                if !self.update_check_quiet {
                    // Manual Check now already surfaces via status line.
                }
                if crate::pro::is_licensed() {
                    self.refresh_pro_update_info();
                }
            }
            UpdateEvent::InstallDone(outcome) => match outcome {
                InstallOutcome::ReplacedAndRelaunched => {
                    // install.rs exits the process after relaunch; defensive fallback:
                    self.update_status = UpdateStatus::Installing;
                    self.update_modal_open = false;
                }
                InstallOutcome::OpenedBrowser { reason } => {
                    self.update_status = UpdateStatus::InstallFailed(format!(
                        "Opened release page ({reason})"
                    ));
                    self.update_modal_fallback_note = Some(reason);
                }
            },
        }
    }

    /// Overlays last so they own pointer/hover above Payload and other panels.
    pub(super) fn draw_overlays(&mut self, ctx: &egui::Context) {
        if self.about_open {
            self.draw_about_modal(ctx);
        }

        if self.shortcuts_open {
            let mut close_shortcuts = false;
            let modal = egui::Modal::new(egui::Id::new("mqtt_master_shortcuts")).show(ctx, |ui| {
                ui.set_min_width(480.0);
                ui.set_max_width(560.0);
                egui::ScrollArea::vertical()
                    .max_height(640.0)
                    .show(ui, |ui| {
                        shortcuts::draw_help(ui);
                    });
                ui.add_space(8.0);
                ui.separator();
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Close").clicked() {
                            close_shortcuts = true;
                        }
                    });
                });
            });
            if close_shortcuts || modal.should_close() {
                self.shortcuts_open = false;
            }
        }

        // Update available — ask before install (no auto-install toggle in v1).
        if self.update_modal_open {
            self.draw_update_modal(ctx);
        }

        // Settings — centered modal (Save commits; Cancel / Escape / outside discards).
        if self.settings_open {
            if self.settings_draft.is_none() {
                self.settings_draft = Some(self.settings.clone());
            }
            let mut open_shortcuts = false;
            let mut save_clicked = false;
            let mut cancel_clicked = false;
            let mut check_now = false;
            let mut update_pro = false;
            // Do NOT probe Pro here — sync HTTP every frame tanks MQTT rates.
            let checking = matches!(self.update_status, UpdateStatus::Checking);
            let status_snapshot = self.update_status.clone();
            let pro_snapshot = self.pro_update_info.clone();
            let pro_catalog = self
                .theme_client
                .as_ref()
                .map(|t| t.catalog())
                .unwrap_or_default();
            let pro_up = self
                .theme_client
                .as_ref()
                .map(|t| t.sidecar_up())
                .unwrap_or(false);
            let licensed = crate::pro::is_licensed();
            let mut theme_changed: Option<String> = None;
            let modal = egui::Modal::new(egui::Id::new("mqtt_master_settings")).show(ctx, |ui| {
                // Tabbed Settings — General / Pro / Updates.
                ui.set_min_width(560.0);
                ui.set_max_width(640.0);
                egui::ScrollArea::vertical()
                    .max_height(620.0)
                    .show(ui, |ui| {
                        if let Some(draft) = self.settings_draft.as_mut() {
                            settings_panel::draw(
                                ui,
                                draft,
                                &self.icons,
                                &mut open_shortcuts,
                                settings_panel::UpdatesSection {
                                    status: &status_snapshot,
                                    checking,
                                    check_now: &mut check_now,
                                    pro: pro_snapshot.as_ref(),
                                    pro_enabled: licensed,
                                    update_pro: &mut update_pro,
                                },
                                settings_panel::ThemeSection {
                                    pro_catalog: &pro_catalog,
                                    pro_up,
                                    licensed,
                                    changed_id: &mut theme_changed,
                                },
                            );
                        }
                    });
                ui.add_space(8.0);
                ui.separator();
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        // Save — accent, larger, with icon (same spot Close used).
                        let save_btn = if let Some(tex) = self.icons.get("save") {
                            ui.add(
                                egui::Button::image_and_text(
                                    egui::Image::new(tex)
                                        .fit_to_exact_size(egui::vec2(
                                            crate::ui::icons::TINY_ICON,
                                            crate::ui::icons::TINY_ICON,
                                        )),
                                    RichText::new("Save").strong(),
                                )
                                .fill(CHROME_ACCENT_SOFT())
                                .stroke(egui::Stroke::new(1.0_f32, CHROME_ACCENT_BORDER()))
                                .min_size(egui::vec2(96.0, 32.0)),
                            )
                        } else {
                            ui.add(
                                egui::Button::new(RichText::new("Save").strong())
                                    .fill(CHROME_ACCENT_SOFT())
                                    .stroke(egui::Stroke::new(1.0_f32, CHROME_ACCENT_BORDER()))
                                    .min_size(egui::vec2(96.0, 32.0)),
                            )
                        };
                        if save_btn.clicked() {
                            save_clicked = true;
                        }
                        if ui
                            .add(egui::Button::new("Cancel").min_size(egui::vec2(72.0, 28.0)))
                            .clicked()
                        {
                            cancel_clicked = true;
                        }
                    });
                });
            });
            // Live theme preview while Settings is open (try / switch back without Save).
            if let Some(id) = theme_changed {
                if let Some(tc) = self.theme_client.as_mut() {
                    tc.preview(ctx, &id);
                } else {
                    crate::ui::theme::apply_theme(ctx, &crate::ui::theme::classic());
                }
            }
            if open_shortcuts {
                self.shortcuts_open = true;
            }
            if check_now {
                self.request_update_check(false);
            }
            if update_pro {
                match crate::pro::update_client::request_install() {
                    Ok(()) => {
                        self.status = "Pro update installing…".into();
                        self.refresh_pro_update_info();
                    }
                    Err(e) => self.status = format!("Pro update failed: {e}"),
                }
            }
            if save_clicked {
                self.save_settings_modal(ctx);
            } else if cancel_clicked || modal.should_close() {
                self.cancel_settings_modal(ctx);
            }
        }
    }

    /// About — Modbus Bench layout adapted to TopicBench modal chrome.
    fn draw_about_modal(&mut self, ctx: &egui::Context) {
        let mut open = true;
        let mut check_now = false;
        let checking = matches!(self.update_status, UpdateStatus::Checking);
        let status_snapshot = self.update_status.clone();
        let app_mark = self.app_mark.clone();
        let aa_logo = self.aa_logo.clone();
        let close_tex = self.icons.get("close_red").cloned();

        let modal = egui::Modal::new(egui::Id::new("topicbench_about")).show(ctx, |ui| {
            ui.set_min_width(420.0);
            ui.set_max_width(460.0);

            // Close X on the corner — does not take a layout row (keeps mark centered).
            {
                let area = ui.available_rect_before_wrap();
                let btn = 26.0;
                let btn_rect = egui::Rect::from_min_size(
                    egui::pos2(area.right() - btn, area.top()),
                    egui::vec2(btn, btn),
                );
                let mut btn_ui = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(btn_rect)
                        .layout(egui::Layout::left_to_right(egui::Align::Center)),
                );
                let close = if let Some(tex) = &close_tex {
                    btn_ui
                        .add(
                            egui::ImageButton::new(
                                egui::Image::new(tex).fit_to_exact_size(egui::vec2(TINY_ICON, TINY_ICON)),
                            )
                            .frame(false),
                        )
                        .on_hover_text("Close")
                } else {
                    btn_ui
                        .add(egui::Button::new(RichText::new("×").size(18.0).color(CHROME_MUTED())))
                        .on_hover_text("Close")
                };
                if close.clicked() {
                    open = false;
                }
            }

            ui.vertical_centered(|ui| {
                // App mark — same art as window / taskbar icon.
                if let Some(mark) = &app_mark {
                    let size = egui::vec2(72.0, 72.0);
                    ui.add(
                        egui::Image::new(mark)
                            .fit_to_exact_size(size)
                            .maintain_aspect_ratio(true)
                            .sense(egui::Sense::hover()),
                    );
                } else {
                    ui.allocate_exact_size(egui::vec2(72.0, 72.0), egui::Sense::hover());
                }
                ui.add_space(6.0);
                paint_about_wordmark(ui);
                ui.add_space(6.0);
                ui.label(
                    RichText::new(ABOUT_SLOGAN)
                        .size(13.0)
                        .color(CHROME_TEXT()),
                );
                ui.add_space(4.0);
                ui.label(
                    RichText::new(format!("Version {}", env!("CARGO_PKG_VERSION")))
                        .size(13.0)
                        .color(CHROME_MUTED()),
                );
                ui.add_space(18.0);
                // Company logo — static (no About hover tip over the artwork).
                if let Some(logo) = &aa_logo {
                    let max_w = ui.available_width().min(280.0);
                    let aspect = logo.size_vec2().x / logo.size_vec2().y.max(1.0);
                    let h = 64.0_f32;
                    let w = (h * aspect).min(max_w);
                    ui.add(
                        egui::Image::new(logo)
                            .fit_to_exact_size(egui::vec2(w, h))
                            .maintain_aspect_ratio(true)
                            .sense(egui::Sense::hover()),
                    );
                }
                ui.add_space(14.0);
                // Normal web-link blue (not theme accent) — Modbus About cue.
                let link_blue = egui::Color32::from_rgb(74, 144, 226);
                let site = ui
                    .link(
                        RichText::new(COMPANY_LINK)
                            .size(13.0)
                            .color(link_blue)
                            .underline(),
                    )
                    .on_hover_text(COMPANY_URL)
                    .on_hover_cursor(egui::CursorIcon::PointingHand);
                if site.clicked() {
                    let _ = update::open_url(COMPANY_URL);
                }
            });

            ui.add_space(14.0);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);
                for tag in [
                    "MQTT",
                    "Multi-broker",
                    "Stickies",
                    "Publish",
                    "Payload",
                    "Search",
                ] {
                    about_tag_chip(ui, tag);
                }
            });

            ui.add_space(14.0);
            // Free GitHub Releases check (Settings → Updates already exposes this).
            ui.vertical_centered(|ui| {
                let btn = ui.add_enabled(
                    !checking,
                    egui::Button::new(RichText::new(if checking {
                        "Checking…"
                    } else {
                        "Check for Updates"
                    }).size(13.0))
                    .min_size(egui::vec2(200.0, 34.0)),
                );
                if btn.clicked() {
                    check_now = true;
                }
                ui.add_space(6.0);
                match &status_snapshot {
                    UpdateStatus::Idle => {}
                    UpdateStatus::Checking => {
                        ui.label(RichText::new("Checking for updates…").size(12.0).color(CHROME_MUTED()));
                    }
                    UpdateStatus::UpToDate { latest } => {
                        ui.label(
                            RichText::new(format!("You're up to date ({latest})."))
                                .size(12.0)
                                .color(egui::Color32::from_rgb(140, 210, 160)),
                        );
                    }
                    UpdateStatus::Available(info) => {
                        ui.label(
                            RichText::new(format!("Update available: {}", info.tag))
                                .size(12.0)
                                .color(egui::Color32::from_rgb(240, 190, 120)),
                        );
                    }
                    UpdateStatus::Installing => {
                        ui.label(
                            RichText::new("Installing…")
                                .size(12.0)
                                .color(CHROME_MUTED()),
                        );
                    }
                    UpdateStatus::Error(msg) | UpdateStatus::InstallFailed(msg) => {
                        ui.label(
                            RichText::new(format!("Couldn't check: {msg}"))
                                .size(12.0)
                                .color(egui::Color32::from_rgb(230, 140, 140)),
                        );
                    }
                }
            });

            ui.add_space(12.0);
            ui.vertical_centered(|ui| {
                if ui
                    .link(
                        RichText::new("github.com/unipsycho/topicbench")
                            .size(12.0)
                            .color(egui::Color32::from_rgb(160, 190, 255)),
                    )
                    .on_hover_text(GITHUB_URL)
                    .clicked()
                {
                    let _ = update::open_url(GITHUB_URL);
                }
                ui.add_space(4.0);
                ui.label(
                    RichText::new("MIT OR Apache-2.0")
                        .size(11.0)
                        .color(CHROME_MUTED()),
                );
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    ui.label(RichText::new("©").size(11.0).color(CHROME_MUTED()));
                    if ui
                        .link(
                            RichText::new("Agile Automation")
                                .size(11.0)
                                .color(egui::Color32::from_rgb(160, 190, 255)),
                        )
                        .on_hover_text(COMPANY_URL)
                        .clicked()
                    {
                        let _ = update::open_url(COMPANY_URL);
                    }
                });
            });
        });

        if check_now {
            self.request_update_check(false);
        }
        if !open || modal.should_close() {
            self.about_open = false;
        }
    }

    fn draw_update_modal(&mut self, ctx: &egui::Context) {
        let Some(info) = self.update_offer.clone() else {
            self.update_modal_open = false;
            return;
        };
        let installing = matches!(self.update_status, UpdateStatus::Installing);
        let notes = info.body.clone();
        let fallback = self.update_modal_fallback_note.clone();
        let mut update_clicked = false;
        let mut remind_clicked = false;
        let mut skip_clicked = false;
        let mut dismiss = false;

        let modal = egui::Modal::new(egui::Id::new("mqtt_master_update")).show(ctx, |ui| {
            ui.set_min_width(440.0);
            ui.set_max_width(520.0);

            ui.label(
                RichText::new("Update available")
                    .strong()
                    .size(18.0)
                    .color(CHROME_TEXT()),
            );
            ui.add_space(4.0);
            // ASCII "->" — egui default fonts lack Unicode arrows (→ shows as a box).
            ui.label(
                RichText::new(format!(
                    "{} -> {}",
                    update::current_version(),
                    info.tag
                ))
                .color(CHROME_ACCENT_BORDER()),
            );
            if !info.name.is_empty() && info.name != info.tag {
                ui.label(RichText::new(&info.name).color(CHROME_MUTED()));
            }
            ui.add_space(8.0);
            let bar_w = ui.available_width();
            let (bar, _) = ui.allocate_exact_size(egui::vec2(bar_w, 2.0), egui::Sense::hover());
            ui.painter().rect_filled(bar, 1.0, CHROME_SEP());
            ui.add_space(8.0);

            ui.label(RichText::new("Release notes").strong().color(CHROME_TEXT()));
            ui.add_space(4.0);
            egui::ScrollArea::vertical()
                .max_height(220.0)
                .show(ui, |ui| {
                    update::notes_ui::draw_release_notes(ui, &notes);
                });

            if let Some(note) = &fallback {
                ui.add_space(8.0);
                ui.label(
                    RichText::new(format!(
                        "Couldn't install in-app — opened the release page.\n{note}"
                    ))
                    .color(CHROME_MUTED()),
                );
            } else if info.asset_url.is_none() || info.sha256sums_url.is_none() {
                ui.add_space(8.0);
                ui.label(
                    RichText::new(
                        "Release assets or SHA256SUMS missing — Update will open the release page in your browser.",
                    )
                    .color(CHROME_MUTED()),
                );
            }

            ui.add_space(12.0);
            ui.separator();
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let update_btn = ui.add_enabled(
                        !installing,
                        egui::Button::new(RichText::new(if installing {
                            "Installing…"
                        } else {
                            "Update"
                        }).strong())
                        .fill(CHROME_ACCENT_SOFT())
                        .stroke(egui::Stroke::new(1.0_f32, CHROME_ACCENT_BORDER()))
                        .min_size(egui::vec2(96.0, 32.0)),
                    );
                    if update_btn.clicked() {
                        update_clicked = true;
                    }
                    if ui
                        .add_enabled(
                            !installing,
                            egui::Button::new("Remind me later").min_size(egui::vec2(120.0, 28.0)),
                        )
                        .clicked()
                    {
                        remind_clicked = true;
                    }
                    if ui
                        .add_enabled(
                            !installing,
                            egui::Button::new("Skip this version").min_size(egui::vec2(120.0, 28.0)),
                        )
                        .clicked()
                    {
                        skip_clicked = true;
                    }
                });
            });
        });

        if update_clicked {
            self.update_modal_fallback_note = None;
            self.update_status = UpdateStatus::Installing;
            self.update_service.start_install(info);
        } else if remind_clicked {
            self.update_remind_session = true;
            self.settings.remind_later_until =
                Some(update::now_unix() + update::REMIND_LATER.as_secs());
            let _ = self.settings.save();
            self.update_modal_open = false;
            dismiss = true;
        } else if skip_clicked {
            self.settings.skipped_update_version = Some(info.tag.clone());
            self.settings.remind_later_until = None;
            let _ = self.settings.save();
            self.update_modal_open = false;
            dismiss = true;
        } else if modal.should_close() && !installing {
            // Escape / backdrop → same as Remind me later for this session.
            self.update_remind_session = true;
            self.update_modal_open = false;
            dismiss = true;
        }
        let _ = dismiss;
    }
}

/// "Topic Bench" centered — Bench in accent (Modbus / Protocol Bench wordmark pattern).
fn paint_about_wordmark(ui: &mut egui::Ui) {
    let gap = 4.0_f32;
    let font = FontId::proportional(24.0);
    let topic = ui.fonts(|f| f.layout_no_wrap("Topic".to_owned(), font.clone(), CHROME_TEXT()));
    let bench = ui.fonts(|f| {
        f.layout_no_wrap("Bench".to_owned(), font, CHROME_ACCENT_BORDER())
    });
    let width = topic.size().x + gap + bench.size().x;
    let height = topic.size().y.max(bench.size().y);
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), height), egui::Sense::hover());
    let origin = egui::pos2(
        rect.center().x - width * 0.5,
        rect.center().y - height * 0.5,
    );
    let painter = ui.painter();
    painter.galley(origin, topic, CHROME_TEXT());
    painter.galley(
        egui::pos2(origin.x + width - bench.size().x, origin.y),
        bench,
        CHROME_ACCENT_BORDER(),
    );
}

fn about_tag_chip(ui: &mut egui::Ui, tag: &str) {
    let accent = CHROME_ACCENT_BORDER();
    let galley = ui.fonts(|f| {
        f.layout_no_wrap(tag.to_owned(), FontId::proportional(12.0), accent)
    });
    let pad = egui::vec2(10.0, 5.0);
    let size = galley.size() + pad * 2.0;
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    let chrome = rect.shrink(0.5);
    let painter = ui.painter();
    let wash = egui::Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 40);
    painter.rect_filled(chrome, 6.0, wash);
    painter.rect_stroke(
        chrome,
        6.0,
        egui::Stroke::new(1.0_f32, accent),
        egui::StrokeKind::Middle,
    );
    painter.galley(
        egui::pos2(rect.left() + pad.x, rect.top() + pad.y),
        galley,
        accent,
    );
}
