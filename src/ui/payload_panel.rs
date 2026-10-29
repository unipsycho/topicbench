//! Payload side panel — live Value view for the selected topic tree row + find.
//! Topics can also pop out to independent OS windows (snapshot focus; close to dismiss).

use crate::mqtt::pins::{DetectedValue, PinBoard};
use crate::mqtt::profile::{ProfileStore, StickyViewMode};
use crate::mqtt::publish::MqttMsgProps;
use crate::mqtt::session::SessionManager;
use crate::mqtt::value_path;
use crate::mqtt::topic_tree::FLASH_MS;
use crate::pro::history_client::{self, HistoryForwarder, PayloadScrub};
use crate::pro::{self, Entitlement};
use crate::ui::flash;
use crate::ui::hover_tip;
use crate::ui::icons::{self, IconSet, CHROME_MUTED, CHROME_ROW};
use crate::ui::json_view;
use eframe::egui::{
    self, Align, Color32, Key, RichText, Sense, Vec2, ViewportBuilder, ViewportClass,
};
use std::time::{Duration, Instant, SystemTime};

/// Graph pin sized to Payload line height (not full [`icons::TINY_ICON`] cell).
const PAYLOAD_GRAPH_ICON: f32 = 14.0;
/// Change-highlight linger — slightly longer than tree pulse so diffs stay readable.
const PAYLOAD_CHANGE_FLASH_MS: u64 = 1000;

fn match_line_fill() -> Color32 {
    Color32::from_rgba_unmultiplied(90, 80, 30, 90)
}
fn match_current_fill() -> Color32 {
    Color32::from_rgba_unmultiplied(140, 120, 40, 140)
}
/// Top→bottom fade wash behind a scrubbed (non-Live) payload body.
fn paint_historical_payload_wash(painter: &egui::Painter, rect: egui::Rect) {
    const STEPS: i32 = 28;
    let rgb = CHROME_ROW;
    for i in 0..STEPS {
        let t0 = i as f32 / STEPS as f32;
        let t1 = (i + 1) as f32 / STEPS as f32;
        // Stronger at top, transparent at bottom.
        let a = ((1.0 - t0) * 150.0) as u8;
        if a == 0 {
            continue;
        }
        let y0 = egui::lerp(rect.top()..=rect.bottom(), t0);
        let y1 = egui::lerp(rect.top()..=rect.bottom(), t1);
        painter.rect_filled(
            egui::Rect::from_min_max(
                egui::pos2(rect.left(), y0),
                egui::pos2(rect.right(), y1),
            ),
            0.0,
            Color32::from_rgba_unmultiplied(rgb.r(), rgb.g(), rgb.b(), a),
        );
    }
}

/// Dashed outline for historical payload (solid stroke reads too “live”).
fn paint_dashed_rect_border(painter: &egui::Painter, rect: egui::Rect) {
    let stroke = egui::Stroke::new(1.25_f32, CHROME_MUTED);
    let dash = 5.0;
    let gap = 4.0;
    let r = rect.shrink(0.5);
    dash_line_segment(painter, r.left_top(), r.right_top(), dash, gap, stroke);
    dash_line_segment(painter, r.right_top(), r.right_bottom(), dash, gap, stroke);
    dash_line_segment(painter, r.right_bottom(), r.left_bottom(), dash, gap, stroke);
    dash_line_segment(painter, r.left_bottom(), r.left_top(), dash, gap, stroke);
}

fn dash_line_segment(
    painter: &egui::Painter,
    a: egui::Pos2,
    b: egui::Pos2,
    dash: f32,
    gap: f32,
    stroke: egui::Stroke,
) {
    let delta = b - a;
    let len = delta.length();
    if len <= 0.5 {
        return;
    }
    let unit = delta / len;
    let mut d = 0.0;
    while d < len {
        let d1 = (d + dash).min(len);
        painter.line_segment([a + unit * d, a + unit * d1], stroke);
        d = d1 + gap;
    }
}

/// One payload topic locked into a floating OS window.
pub struct PoppedPayload {
    pub profile_id: String,
    pub profile_name: String,
    pub topic: String,
    pub search: PayloadSearch,
}

impl PoppedPayload {
    fn matches(&self, profile_id: &str, topic: &str) -> bool {
        self.profile_id == profile_id && self.topic == topic
    }
}

/// Find-in-payload + change-flash state (lives on the app so it survives redraws).
#[derive(Default)]
pub struct PayloadSearch {
    pub query: String,
    /// 0-based index into the current match list.
    pub current: usize,
    /// After Next/Prev / query change, scroll the active match into view.
    pub scroll_to_current: bool,
    /// Last `(profile_id, topic)` we rendered — topic switch resets diff history.
    focus_key: Option<(String, String)>,
    /// Previous display body (pretty JSON / value text) for change highlighting.
    prev_display: String,
    /// Byte ranges in the current display that differ from `prev_display`.
    change_ranges: Vec<(usize, usize)>,
    change_flash_until: Option<Instant>,
}

impl PayloadSearch {
    /// True while a payload change highlight is still fading (drives egui repaint).
    pub fn change_flash_active(&self, now: Instant) -> bool {
        flash::flash_active(self.change_flash_until, now)
    }

    /// Compare new display text to the previous frame; start a fade when it changes.
    fn note_display(&mut self, profile_id: &str, topic: &str, display: &str, now: Instant) {
        let key = (profile_id.to_string(), topic.to_string());
        if self.focus_key.as_ref() != Some(&key) {
            // Fresh topic selection — no flash, just seed the baseline.
            self.focus_key = Some(key);
            self.prev_display = display.to_string();
            self.change_ranges.clear();
            self.change_flash_until = None;
            return;
        }
        if display == self.prev_display {
            return;
        }
        self.change_ranges = changed_ranges(&self.prev_display, display);
        self.prev_display = display.to_string();
        if self.change_ranges.is_empty() {
            self.change_flash_until = None;
        } else {
            self.change_flash_until =
                Some(now + Duration::from_millis(PAYLOAD_CHANGE_FLASH_MS.max(FLASH_MS)));
        }
    }

    fn goto_next(&mut self, n: usize) {
        if n == 0 {
            return;
        }
        self.current = (self.current + 1) % n;
        self.scroll_to_current = true;
    }

    fn goto_prev(&mut self, n: usize) {
        if n == 0 {
            return;
        }
        self.current = if self.current == 0 { n - 1 } else { self.current - 1 };
        self.scroll_to_current = true;
    }
}

/// One substring hit in the displayed payload text.
#[derive(Clone, Copy, Debug)]
struct MatchHit {
    /// Line index in the displayed text (0-based).
    line: usize,
    /// Byte range within the full display string.
    start: usize,
    end: usize,
}

pub fn draw(
    ui: &mut egui::Ui,
    sessions: &mut SessionManager,
    profiles: &mut ProfileStore,
    pins: &mut PinBoard,
    icons: &IconSet,
    search: &mut PayloadSearch,
    open_stickies: &mut bool,
    status: &mut String,
    popped: &mut Vec<PoppedPayload>,
    history_fwd: Option<&HistoryForwarder>,
    payload_scrub: &mut PayloadScrub,
    bookmarks: &mut crate::pro::bookmarks::BookmarksStore,
) {
    // Stay within the side-panel width — long topic paths wrap; they must not widen the column.
    let panel_w = ui.available_width();
    ui.set_max_width(panel_w);

    icons::section_header_bar(ui, |ui| {
        icons::section_icon(ui, icons, "json_payload");
        icons::section_title(ui, "Payload");
    });

    ui.label("Live Value view for the selected topic.");
    ui.add_space(4.0);

    let Some((profile_id, profile_name, topic)) = pins.last_tree_focus.clone() else {
        ui.weak("Select a topic in the tree.");
        return;
    };

    let mut pop_out = false;
    let mut delete_hierarchy = false;
    draw_topic_body(
        ui,
        sessions,
        profiles,
        pins,
        icons,
        search,
        open_stickies,
        status,
        &profile_id,
        &profile_name,
        &topic,
        ("payload-panel", profile_id.as_str(), topic.as_str()),
        Some((&mut pop_out, &mut delete_hierarchy)),
        history_fwd,
        Some(payload_scrub),
        Some(bookmarks),
    );

    if pop_out {
        push_popped(popped, &profile_id, &profile_name, &topic);
        *status = format!("Payload popped out: {topic}");
    }

    // Drop selected topic + descendants from the in-memory display tree (not broker).
    if delete_hierarchy {
        pins.last_tree_focus = None;
        if let Some(session) = sessions.open.get_mut(&profile_id) {
            if session.topic_tree.remove_hierarchy(&topic) {
                *status = format!("Deleted topic hierarchy: {topic}");
            } else {
                *status = format!("Topic not in tree: {topic}");
            }
        } else {
            *status = format!("Broker not connected: {topic}");
        }
    }
}

/// Floating windows for each popped payload topic (close dismisses; no dock).
#[allow(clippy::too_many_arguments)]
pub fn draw_undocked_viewports(
    ctx: &egui::Context,
    sessions: &mut SessionManager,
    profiles: &mut ProfileStore,
    pins: &mut PinBoard,
    icons: &IconSet,
    open_stickies: &mut bool,
    status: &mut String,
    popped: &mut Vec<PoppedPayload>,
    builder_seeds: &mut std::collections::HashMap<String, crate::mqtt::window_geom::WinGeom>,
    saved_geoms: &mut std::collections::BTreeMap<String, crate::mqtt::window_geom::WinGeom>,
    geom_dirty: &mut bool,
) {
    let mut close_idxs: Vec<usize> = Vec::new();
    let n = popped.len();
    const MIN: [f32; 2] = [280.0, 200.0];
    const DEFAULT: [f32; 2] = [420.0, 520.0];
    let live_keys: std::collections::HashSet<String> = popped
        .iter()
        .map(|p| crate::mqtt::window_geom::payload_key(&p.profile_id, &p.topic))
        .collect();
    builder_seeds.retain(|k, _| live_keys.contains(k));
    for i in 0..n {
        let profile_id = popped[i].profile_id.clone();
        let profile_name = popped[i].profile_name.clone();
        let topic = popped[i].topic.clone();
        let title = format!("Payload — {profile_name} · {topic}");
        let key = crate::mqtt::window_geom::payload_key(&profile_id, &topic);
        let vp_id = egui::ViewportId::from_hash_of((
            "payload_pop",
            profile_id.as_str(),
            topic.as_str(),
        ));
        let mut close_me = false;
        let seed = *builder_seeds.entry(key.clone()).or_insert_with(|| {
            saved_geoms
                .get(&key)
                .copied()
                .unwrap_or_else(|| crate::mqtt::window_geom::WinGeom::size(DEFAULT[0], DEFAULT[1]))
        });

        ctx.show_viewport_immediate(
            vp_id,
            seed.apply(ViewportBuilder::default().with_title(title.clone()), MIN)
                .with_min_inner_size(MIN),
            |ctx, class| {
                let mut body = |ui: &mut egui::Ui| {
                    draw_topic_body(
                        ui,
                        sessions,
                        profiles,
                        pins,
                        icons,
                        &mut popped[i].search,
                        open_stickies,
                        status,
                        &profile_id,
                        &profile_name,
                        &topic,
                        ("payload-pop", profile_id.as_str(), topic.as_str()),
                        None, // already floated — no pop-out / trash chrome
                        None,
                        None, // scrub stays on the docked Payload panel
                        None, // bookmark chrome is on the docked Payload bar
                    );
                };
                match class {
                    ViewportClass::Embedded => {
                        egui::Window::new(&title)
                            .id(egui::Id::new((
                                "payload_pop_win",
                                profile_id.as_str(),
                                topic.as_str(),
                            )))
                            .default_size([420.0, 520.0])
                            .show(ctx, |ui| body(ui));
                    }
                    _ => {
                        egui::CentralPanel::default().show(ctx, |ui| body(ui));
                    }
                }
                if ctx.input(|i| i.viewport().close_requested()) {
                    close_me = true;
                }
                if let Some(g) = crate::mqtt::window_geom::capture(ctx) {
                    let mut slot = saved_geoms.get(&key).copied();
                    if crate::mqtt::window_geom::sync_slot(&mut slot, g, MIN) {
                        if let Some(g) = slot {
                            saved_geoms.insert(key.clone(), g);
                            *geom_dirty = true;
                        }
                    }
                }
            },
        );

        if close_me {
            close_idxs.push(i);
        }
    }
    for i in close_idxs.into_iter().rev() {
        let p = popped.remove(i);
        builder_seeds.remove(&crate::mqtt::window_geom::payload_key(&p.profile_id, &p.topic));
    }
}

fn push_popped(
    popped: &mut Vec<PoppedPayload>,
    profile_id: &str,
    profile_name: &str,
    topic: &str,
) {
    if popped.iter().any(|p| p.matches(profile_id, topic)) {
        return;
    }
    popped.push(PoppedPayload {
        profile_id: profile_id.to_string(),
        profile_name: profile_name.to_string(),
        topic: topic.to_string(),
        search: PayloadSearch::default(),
    });
}

/// Shared payload body for the side panel and floating windows.
/// `actions`: optional `(pop_out, delete)` flags — icons sit beside the broker name.
#[allow(clippy::too_many_arguments)]
fn draw_topic_body(
    ui: &mut egui::Ui,
    sessions: &mut SessionManager,
    profiles: &mut ProfileStore,
    pins: &mut PinBoard,
    icons: &IconSet,
    search: &mut PayloadSearch,
    open_stickies: &mut bool,
    status: &mut String,
    profile_id: &str,
    profile_name: &str,
    topic: &str,
    id_salt: impl std::hash::Hash,
    mut actions: Option<(&mut bool, &mut bool)>,
    history_fwd: Option<&HistoryForwarder>,
    mut payload_scrub: Option<&mut PayloadScrub>,
    bookmarks: Option<&mut crate::pro::bookmarks::BookmarksStore>,
) {
    let accent = profiles
        .profiles
        .get(profile_id)
        .map(|p| p.color())
        .unwrap_or(Color32::from_rgb(90, 190, 120));

    // Broker name left; bookmark + pop-out + trash on the right.
    ui.horizontal(|ui| {
        ui.label(RichText::new(profile_name).color(accent));
        if let Some((pop_out, delete_hierarchy)) = actions.as_mut() {
            ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                if icons
                    .tiny(ui, "delete_red", false, "Delete topic hierarchy")
                    .clicked()
                {
                    **delete_hierarchy = true;
                }
                if icons
                    .tiny(
                        ui,
                        "panel_undock",
                        false,
                        "Pop out this topic to its own window",
                    )
                    .clicked()
                {
                    **pop_out = true;
                }
                // Pro Bookmark for the selected tree node (parents allowed).
                if let Some(store) = bookmarks {
                    if pro::has(Entitlement::TopicBookmarks) {
                        crate::ui::bookmarks_panel::bookmark_toggle_icon(
                            ui, icons, store, profile_id, topic, status, false,
                        );
                    }
                }
            });
        }
    });
    ui.add(
        egui::Label::new(json_view::topic_path_text(topic))
            .wrap()
            .selectable(true),
    );
    ui.add_space(4.0);

    let node = sessions
        .open
        .get(profile_id)
        .and_then(|s| s.topic_tree.root.node_at(topic));

    match node {
        Some(n) if n.last_payload.as_deref().is_some_and(|p| !p.is_empty()) => {
            let live_payload = n.last_payload.as_deref().unwrap_or("").to_string();
            let message_count = n.message_count;
            let mqtt5_props = n.last_props.clone();
            // Merge short ring + Pro deep samples for scrub (oldest → newest).
            let history = merge_payload_history(
                profile_id,
                topic,
                &n.payload_history,
                history_fwd,
            );
            let (payload, scrubbing, sample_at) = if let Some(scrub) = payload_scrub.as_deref_mut()
            {
                if pro::has(Entitlement::HistoryScrubber) {
                    scrub.sync_topic(profile_id, topic);
                    // Keep −N label honest as the tip grows while pinned.
                    scrub.refresh_steps_back(&history);
                    let live = scrub.is_live();
                    let (p, at) = if live {
                        (live_payload.clone(), history.last().map(|(t, _)| *t))
                    } else {
                        scrub
                            .index_in(&history)
                            .and_then(|i| history.get(i).map(|(t, s)| (s.clone(), Some(*t))))
                            .unwrap_or_else(|| {
                                (live_payload.clone(), history.last().map(|(t, _)| *t))
                            })
                    };
                    (p, !live, at)
                } else {
                    (
                        live_payload.clone(),
                        false,
                        history.last().map(|(t, _)| *t),
                    )
                }
            } else {
                (
                    live_payload.clone(),
                    false,
                    history.last().map(|(t, _)| *t),
                )
            };
            let display = display_text(&payload);
            let is_json = json_view::try_pretty_json(&payload).is_some();
            let now = Instant::now();
            // Change flash only tracks live tip (avoid flashing while scrubbing).
            if !scrubbing {
                search.note_display(profile_id, topic, &display, now);
            }
            let change_bg = if scrubbing {
                Color32::TRANSPARENT
            } else {
                change_flash_bg(search.change_flash_until, now)
            };
            // MQTT 5 inbound metadata (user props / content type / expiry).
            if !mqtt5_props.is_empty() {
                draw_mqtt5_props(ui, &mqtt5_props);
                ui.add_space(4.0);
            }
            draw_search_bar(ui, icons, search, &display);
            ui.add_space(4.0);
            draw_searchable_body(
                ui,
                &display,
                is_json,
                id_salt,
                search,
                message_count,
                icons,
                profile_id,
                profile_name,
                topic,
                &payload,
                pins,
                profiles,
                open_stickies,
                status,
                change_bg,
                scrubbing,
                payload_scrub,
                &history,
                sample_at,
            );
        }
        _ => {
            ui.weak("No payload yet for this topic.");
        }
    }
}

/// Read-only MQTT 5 properties for the selected live tip.
fn draw_mqtt5_props(ui: &mut egui::Ui, props: &MqttMsgProps) {
    ui.weak("MQTT 5 properties");
    if let Some(ct) = props.content_type.as_ref().filter(|s| !s.is_empty()) {
        ui.horizontal(|ui| {
            ui.weak("Content-Type");
            ui.monospace(ct);
        });
    }
    if let Some(exp) = props.message_expiry_interval {
        ui.horizontal(|ui| {
            ui.weak("Message expiry");
            ui.monospace(format!("{exp}s"));
        });
    }
    if let Some(fmt) = props.payload_format_indicator {
        ui.horizontal(|ui| {
            ui.weak("Payload format");
            ui.monospace(if fmt == 1 { "UTF-8" } else { "unspecified" });
        });
    }
    for (k, v) in &props.user_properties {
        ui.horizontal(|ui| {
            ui.monospace(k);
            ui.weak("=");
            ui.monospace(v);
        });
    }
}

/// Short OSS ring ∪ Pro deep query payloads for one topic (oldest → newest).
fn merge_payload_history(
    profile_id: &str,
    topic: &str,
    short: &std::collections::VecDeque<(Instant, String)>,
    history_fwd: Option<&HistoryForwarder>,
) -> Vec<(Instant, String)> {
    let mut out: Vec<(Instant, String)> = short.iter().cloned().collect();
    if let Some(fwd) = history_fwd {
        if pro::has(Entitlement::HistoryScrubber) {
            let series_id = PinBoard::key(profile_id, topic);
            let (from_ms, to_ms) = if let (Some((t0, _)), Some((t1, _))) =
                (short.front(), short.back())
            {
                (
                    history_client::instant_to_t_ms(*t0).saturating_sub(86_400_000),
                    history_client::instant_to_t_ms(*t1).saturating_add(1_000),
                )
            } else {
                let now = history_client::instant_to_t_ms(Instant::now());
                (now.saturating_sub(86_400_000), now)
            };
            for s in fwd.query_cached(&series_id, from_ms, to_ms, 8000) {
                let payload = if s.payload.is_empty() {
                    s.v.map(|v| v.to_string()).unwrap_or_default()
                } else {
                    s.payload
                };
                if payload.is_empty() {
                    continue;
                }
                out.push((history_client::t_ms_to_instant(s.t_ms), payload));
            }
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out.dedup_by(|a, b| a.0 == b.0 && a.1 == b.1);
    out
}

/// Map sample Instant → local wall clock (age from Instant::now → SystemTime).
fn instant_hhmmss(at: Instant) -> String {
    let age = Instant::now().saturating_duration_since(at);
    let wall = SystemTime::now()
        .checked_sub(age)
        .unwrap_or_else(SystemTime::now);
    let dt: chrono::DateTime<chrono::Local> = wall.into();
    dt.format("%H:%M:%S").to_string()
}

/// Human relative age for scrubbed samples (“5 seconds ago”, “1 minute ago”).
fn instant_relative_ago(at: Instant) -> String {
    let secs = Instant::now().saturating_duration_since(at).as_secs();
    if secs < 2 {
        return "just now".into();
    }
    if secs < 60 {
        return if secs == 1 {
            "1 second ago".into()
        } else {
            format!("{secs} seconds ago")
        };
    }
    let mins = secs / 60;
    if mins < 60 {
        return if mins == 1 {
            "1 minute ago".into()
        } else {
            format!("{mins} minutes ago")
        };
    }
    let hours = mins / 60;
    if hours < 48 {
        return if hours == 1 {
            "1 hour ago".into()
        } else {
            format!("{hours} hours ago")
        };
    }
    let days = hours / 24;
    if days == 1 {
        "1 day ago".into()
    } else {
        format!("{days} days ago")
    }
}

/// Slider only (sits on the JSON/#count row).
fn draw_payload_scrub_slider(
    ui: &mut egui::Ui,
    scrub: &mut PayloadScrub,
    history: &[(Instant, String)],
) {
    let max_back = history.len().saturating_sub(1);
    if max_back == 0 {
        return;
    }
    let mut pos = (max_back - scrub.steps_back().min(max_back)) as f32;
    let max_f = max_back as f32;
    ui.scope(|ui| {
        ui.set_max_width(160.0);
        let resp = ui.add(
            egui::Slider::new(&mut pos, 0.0..=max_f)
                .show_value(false)
                .trailing_fill(true),
        );
        if resp.changed() || resp.dragged() {
            let idx = pos.round().clamp(0.0, max_f) as usize;
            scrub.scrub_to_index(history, idx);
        }
        resp.on_hover_text("Scrub previous payloads · tip = live · ←/→ step one");
    });
}

/// Row under the slider: ◀/▶ + Live/−N, timestamp on the right.
fn draw_payload_scrub_controls(
    ui: &mut egui::Ui,
    scrub: &mut PayloadScrub,
    history: &[(Instant, String)],
    sample_at: Option<Instant>,
) {
    if history.len() <= 1 {
        return;
    }
    // Left/Right when the find field does not own focus.
    let find_focused = ui.memory(|m| {
        m.has_focus(egui::Id::new("payload_find_edit"))
            || m.has_focus(egui::Id::new("payload_find_edit_dock"))
            || m.has_focus(egui::Id::new("payload_find_edit_float"))
    });
    if !find_focused {
        if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::ArrowLeft)) {
            scrub.step_by(history, -1);
        }
        if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::ArrowRight)) {
            scrub.step_by(history, 1);
        }
    }

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        let older = ui
            .add_enabled(
                scrub.index_in(history).unwrap_or(0) > 0,
                egui::Button::new("◀").small(),
            )
            .on_hover_text("Older sample (←)");
        if older.clicked() {
            scrub.step_by(history, -1);
        }
        let newer = ui
            .add_enabled(!scrub.is_live(), egui::Button::new("▶").small())
            .on_hover_text("Newer sample (→) · tip resumes Live");
        if newer.clicked() {
            scrub.step_by(history, 1);
        }
        if scrub.is_live() {
            ui.label(
                RichText::new("Live")
                    .small()
                    .color(Color32::from_rgb(90, 180, 110)),
            );
        } else if ui
            .add(egui::Button::new(
                RichText::new(format!("−{}", scrub.steps_back())).small(),
            ))
            .on_hover_text("Paused on history — click to resume live")
            .clicked()
        {
            scrub.resume_live();
        }
        if let Some(at) = sample_at {
            let historical = !scrub.is_live();
            ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                // right_to_left: clock on the far right; relative age just left of it.
                ui.label(
                    RichText::new(instant_hhmmss(at))
                        .small()
                        .monospace()
                        .weak(),
                );
                if historical {
                    ui.label(
                        RichText::new(instant_relative_ago(at))
                            .small()
                            .weak(),
                    );
                }
            });
        }
    });
}

/// Text shown in the Value body (pretty JSON when possible).
fn display_text(payload: &str) -> String {
    if let Some(pretty) = json_view::try_pretty_json(payload) {
        return pretty;
    }
    match DetectedValue::from_payload(payload) {
        DetectedValue::Bool(on) => {
            if on {
                "true".into()
            } else {
                "false".into()
            }
        }
        DetectedValue::Number(n) => format!("{n}"),
        DetectedValue::Text(s) => s,
    }
}

fn draw_search_bar(
    ui: &mut egui::Ui,
    icons: &IconSet,
    search: &mut PayloadSearch,
    display: &str,
) {
    let matches = find_matches(display, &search.query);
    let n = matches.len();
    if n > 0 {
        search.current = search.current.min(n - 1);
    } else {
        search.current = 0;
    }

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        icons.tiny(ui, "clipboard_checklist", false, "Find in payload");
        let edit = ui.add(
            egui::TextEdit::singleline(&mut search.query)
                .id(egui::Id::new("payload_find_edit"))
                .desired_width(ui.available_width().min(180.0).max(80.0))
                .hint_text("Find…")
                .font(egui::TextStyle::Monospace),
        );
        if edit.changed() {
            search.current = 0;
            search.scroll_to_current = !search.query.trim().is_empty();
        }
        // Enter / Shift+Enter while the field has focus.
        if edit.has_focus() {
            let enter = ui.input(|i| i.key_pressed(Key::Enter));
            let shift = ui.input(|i| i.modifiers.shift);
            if enter {
                if shift {
                    search.goto_prev(n);
                } else {
                    search.goto_next(n);
                }
            }
        }
        let label = if search.query.trim().is_empty() {
            "—".to_string()
        } else if n == 0 {
            "0".into()
        } else {
            format!("{}/{}", search.current + 1, n)
        };
        ui.label(RichText::new(label).small().monospace().weak());
        if hover_tip::attach(
            ui.add_enabled(n > 0, egui::Button::new("Prev").small()),
            "Previous match (Shift+Enter)",
        )
        .clicked()
        {
            search.goto_prev(n);
        }
        if hover_tip::attach(
            ui.add_enabled(n > 0, egui::Button::new("Next").small()),
            "Next match (Enter)",
        )
        .clicked()
        {
            search.goto_next(n);
        }
    });
}

#[allow(clippy::too_many_arguments)]
fn draw_searchable_body(
    ui: &mut egui::Ui,
    display: &str,
    is_json: bool,
    id_salt: impl std::hash::Hash,
    search: &mut PayloadSearch,
    message_count: u64,
    icons: &IconSet,
    profile_id: &str,
    profile_name: &str,
    topic: &str,
    raw_payload: &str,
    pins: &mut PinBoard,
    profiles: &mut ProfileStore,
    open_stickies: &mut bool,
    status: &mut String,
    change_bg: Color32,
    // True when scrub is pinned off Live — mute the payload body.
    scrubbing: bool,
    mut payload_scrub: Option<&mut PayloadScrub>,
    history: &[(Instant, String)],
    // Receive Instant for the viewed sample (live tip or scrubbed); None → hide clock.
    sample_at: Option<Instant>,
) {
    let history_len = history.len();
    let matches = find_matches(display, &search.query);
    let n = matches.len();
    if n > 0 {
        search.current = search.current.min(n - 1);
    }
    let current_line = matches.get(search.current).map(|m| m.line);
    let do_scroll = search.scroll_to_current && current_line.is_some();

    // Row 1: kind + #count + scrub slider. Row 2: ◀/▶ + Live/−N + timestamp.
    let scrub_ok =
        pro::has(Entitlement::HistoryScrubber) && history_len > 1 && payload_scrub.is_some();
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        if is_json {
            ui.label(RichText::new("JSON").small().weak());
        } else {
            ui.label(RichText::new("Value").small().weak());
        }
        if message_count > 0 {
            ui.label(
                RichText::new(format!("#{message_count}"))
                    .small()
                    .weak(),
            );
        }
        if scrub_ok {
            if let Some(scrub) = payload_scrub.as_deref_mut() {
                draw_payload_scrub_slider(ui, scrub, history);
            }
        }
    });
    if scrub_ok {
        if let Some(scrub) = payload_scrub {
            draw_payload_scrub_controls(ui, scrub, history, sample_at);
        }
    } else if let Some(at) = sample_at {
        if pro::has(Entitlement::HistoryScrubber) {
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                    ui.label(
                        RichText::new(instant_hhmmss(at))
                            .small()
                            .monospace()
                            .weak(),
                    );
                });
            });
        }
    }

    let height = ui.available_height().max(80.0);
    let font_size = ui.text_style_height(&egui::TextStyle::Monospace);
    let lines: Vec<&str> = display.split('\n').collect();
    // Per-line JSON field path when the leaf is number/bool (graphable).
    let graph_paths: Vec<Option<String>> = if is_json {
        value_path::graphable_paths_per_line(display)
    } else {
        // Whole-payload scalar Value: one graphable row, no field path.
        let graphable = matches!(
            DetectedValue::from_payload(raw_payload),
            DetectedValue::Bool(_) | DetectedValue::Number(_)
        );
        lines
            .iter()
            .enumerate()
            .map(|(i, _)| if graphable && i == 0 { Some(String::new()) } else { None })
            .collect()
    };
    // Byte offset of each line start in `display`.
    let mut line_starts = Vec::with_capacity(lines.len());
    let mut off = 0usize;
    for (i, line) in lines.iter().enumerate() {
        line_starts.push(off);
        off += line.len();
        if i + 1 < lines.len() {
            off += 1; // '\n'
        }
    }

    // Lines that contain any match (for row highlight).
    let mut matched_lines = vec![false; lines.len()];
    for m in &matches {
        if m.line < matched_lines.len() {
            matched_lines[m.line] = true;
        }
    }

    // Historical (non-Live): top→bottom fade + dashed border (painted around the frame).
    let body_frame = if scrubbing {
        egui::Frame::NONE
            .inner_margin(egui::Margin::symmetric(4, 4))
            .corner_radius(3.0)
    } else {
        egui::Frame::NONE
    };
    let scroll_h = if scrubbing {
        (height - 8.0).max(72.0)
    } else {
        height
    };
    let body = body_frame.show(ui, |ui| {
    // Paint wash first so JSON lines sit above it.
    if scrubbing {
        paint_historical_payload_wash(ui.painter(), ui.max_rect());
    }
    egui::ScrollArea::vertical()
        .id_salt(id_salt)
        .max_height(scroll_h)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.set_max_width(ui.available_width());
            // Dense stack — default item_spacing.y left a clear gap between JSON lines.
            ui.spacing_mut().item_spacing.y = 0.0;
            for (li, line) in lines.iter().enumerate() {
                let is_current = current_line == Some(li);
                let fill = if is_current {
                    match_current_fill()
                } else if matched_lines[li] {
                    match_line_fill()
                } else {
                    Color32::TRANSPARENT
                };

                let line_start = line_starts[li];
                // Match spans on this line, relative to line start.
                let line_hits: Vec<(usize, usize)> = matches
                    .iter()
                    .filter(|m| m.line == li)
                    .map(|m| (m.start - line_start, m.end - line_start))
                    .collect();
                let current_hit = matches.get(search.current).and_then(|m| {
                    if m.line == li {
                        Some((m.start - line_start, m.end - line_start))
                    } else {
                        None
                    }
                });
                // Change spans on this line (MQTT Explorer–style update flash).
                let line_end = line_start + line.len();
                let change_hits: Vec<(usize, usize)> = search
                    .change_ranges
                    .iter()
                    .filter(|&&(a, b)| a < line_end && b > line_start)
                    .map(|&(a, b)| {
                        (
                            a.saturating_sub(line_start).min(line.len()),
                            b.saturating_sub(line_start).min(line.len()),
                        )
                    })
                    .filter(|(a, b)| a < b)
                    .collect();

                let job = if is_json {
                    json_view::line_job_with_search(
                        line,
                        font_size,
                        &line_hits,
                        current_hit,
                        &change_hits,
                        change_bg,
                    )
                } else {
                    json_view::plain_line_job_with_search(
                        line,
                        font_size,
                        &line_hits,
                        current_hit,
                        &change_hits,
                        change_bg,
                    )
                };

                let graph_path = graph_paths.get(li).and_then(|p| p.as_ref());
                // Match monospace line height — tiny(18+pad) was stretching every row.
                let row_h = font_size + 2.0;
                let resp = egui::Frame::NONE
                    .fill(fill)
                    .inner_margin(egui::Margin::symmetric(2, 0))
                    .show(ui, |ui| {
                        ui.set_min_height(row_h);
                        ui.horizontal(|ui| {
                            ui.set_height(row_h);
                            ui.spacing_mut().item_spacing.x = 2.0;
                            ui.spacing_mut().item_spacing.y = 0.0;
                            // Always reserve the pin column so JSON indent stays aligned.
                            if let Some(path) = graph_path {
                                let path_opt = if path.is_empty() {
                                    None
                                } else {
                                    Some(path.as_str())
                                };
                                let sticky_topic = value_path::sticky_id(topic, path_opt);
                                let tip = if let Some(p) = path_opt {
                                    format!("Sticky Graph: {topic} · {p}")
                                } else {
                                    format!("Sticky Graph: {topic}")
                                };
                                if payload_graph_pin(ui, icons, &tip).clicked() {
                                    let current = if let Some(p) = path_opt {
                                        value_path::extract_path_value(raw_payload, p)
                                            .unwrap_or_else(|| line.trim().trim_end_matches(',').to_string())
                                    } else {
                                        display.trim().to_string()
                                    };
                                    pins.pin_graph(
                                        profile_id,
                                        profile_name,
                                        &sticky_topic,
                                        &current,
                                        Instant::now(),
                                    );
                                    profiles.set_sticky_and_save(profile_id, &sticky_topic, true);
                                    profiles.set_sticky_view_mode_and_save(
                                        profile_id,
                                        &sticky_topic,
                                        StickyViewMode::Graph,
                                    );
                                    *open_stickies = true;
                                    *status = format!("Sticky Graph · {sticky_topic}");
                                }
                            } else {
                                ui.allocate_exact_size(
                                    Vec2::splat(PAYLOAD_GRAPH_ICON),
                                    Sense::hover(),
                                );
                            }
                            ui.add(
                                egui::Label::new(job)
                                    .wrap()
                                    .selectable(true)
                                    .halign(Align::LEFT),
                            );
                        });
                    })
                    .response;

                if do_scroll && is_current {
                    resp.scroll_to_me(Some(Align::Center));
                }
            }
        });
    });
    if scrubbing {
        paint_dashed_rect_border(ui.painter(), body.response.rect);
    }

    if do_scroll {
        search.scroll_to_current = false;
    }
}

/// Compact left-rail graph pin — matches monospace row height.
fn payload_graph_pin(ui: &mut egui::Ui, icons: &IconSet, tip: &str) -> egui::Response {
    let size = Vec2::splat(PAYLOAD_GRAPH_ICON);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let response = hover_tip::attach(response, tip);
    if response.hovered() {
        ui.painter()
            .rect_filled(rect, 2.0, icons::CHROME_HOVER);
    }
    if let Some(tex) = icons.get("line_graph") {
        egui::Image::new(tex)
            .fit_to_exact_size(size)
            .maintain_aspect_ratio(true)
            .paint_at(ui, egui::Rect::from_center_size(rect.center(), size));
    }
    response
}

/// Fading amber fill for changed spans (t=1 at pulse start → 0).
fn change_flash_bg(until: Option<Instant>, now: Instant) -> Color32 {
    let Some(until) = until else {
        return Color32::TRANSPARENT;
    };
    if now >= until {
        return Color32::TRANSPARENT;
    }
    let left = until.saturating_duration_since(now).as_secs_f32();
    let t = (left / (PAYLOAD_CHANGE_FLASH_MS as f32 / 1000.0)).clamp(0.0, 1.0);
    let a = (160.0 * t) as u8;
    Color32::from_rgba_unmultiplied(210, 175, 50, a)
}

/// Byte ranges in `new` that differ from `old` (line-aligned, prefix/suffix within lines).
fn changed_ranges(old: &str, new: &str) -> Vec<(usize, usize)> {
    if old == new {
        return Vec::new();
    }
    if old.is_empty() {
        return if new.is_empty() {
            Vec::new()
        } else {
            vec![(0, new.len())]
        };
    }
    let old_lines: Vec<&str> = old.split('\n').collect();
    let new_lines: Vec<&str> = new.split('\n').collect();
    let mut spans = Vec::new();
    let mut offset = 0usize;
    for (i, new_line) in new_lines.iter().enumerate() {
        let old_line = old_lines.get(i).copied().unwrap_or("");
        if old_line != *new_line {
            let (a, b) = changed_span_in_line(old_line, new_line);
            if a < b {
                spans.push((offset + a, offset + b));
            }
        }
        offset += new_line.len();
        if i + 1 < new_lines.len() {
            offset += 1; // '\n'
        }
    }
    spans
}

/// Differing middle of a line after matching common prefix/suffix (char-safe).
fn changed_span_in_line(old: &str, new: &str) -> (usize, usize) {
    let ob = old.as_bytes();
    let nb = new.as_bytes();
    let mut pre = 0usize;
    while pre < ob.len() && pre < nb.len() && ob[pre] == nb[pre] {
        pre += 1;
    }
    while pre > 0 && !new.is_char_boundary(pre) {
        pre -= 1;
    }
    let mut suf = 0usize;
    while suf < ob.len().saturating_sub(pre)
        && suf < nb.len().saturating_sub(pre)
        && ob[ob.len() - 1 - suf] == nb[nb.len() - 1 - suf]
    {
        suf += 1;
    }
    while suf > 0 && !new.is_char_boundary(nb.len() - suf) {
        suf -= 1;
    }
    let end = nb.len() - suf;
    (pre.min(end), end)
}

/// Case-insensitive substring matches; each hit records its line + byte range.
fn find_matches(text: &str, query: &str) -> Vec<MatchHit> {
    let q = query.trim();
    if q.is_empty() {
        return Vec::new();
    }
    let lower_text = text.to_lowercase();
    let lower_q = q.to_lowercase();
    let q_len = lower_q.len();
    if q_len == 0 {
        return Vec::new();
    }

    let mut out = Vec::new();
    let mut search_from = 0usize;
    while let Some(rel) = lower_text[search_from..].find(&lower_q) {
        let start = search_from + rel;
        let end = start + q_len;
        // Line index = number of '\n' before start.
        let line = text[..start].bytes().filter(|&b| b == b'\n').count();
        out.push(MatchHit { line, start, end });
        search_from = start + q_len.max(1);
        if search_from >= lower_text.len() {
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_case_insensitive() {
        let t = "{\n  \"Name\": \"Citric Acid\"\n}";
        let m = find_matches(t, "citric");
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].line, 1);
        assert!(t[m[0].start..m[0].end].eq_ignore_ascii_case("citric"));
    }

    #[test]
    fn multiple_on_two_lines() {
        let t = "foo\nbar foo\nfoo";
        let m = find_matches(t, "foo");
        assert_eq!(m.len(), 3);
        assert_eq!(m[0].line, 0);
        assert_eq!(m[1].line, 1);
        assert_eq!(m[2].line, 2);
    }

    #[test]
    fn changed_ranges_highlights_value_edit() {
        let old = "{\n  \"temp\": 21.5,\n  \"ok\": true\n}";
        let new = "{\n  \"temp\": 33.0,\n  \"ok\": true\n}";
        let spans = changed_ranges(old, new);
        assert_eq!(spans.len(), 1);
        let (a, b) = spans[0];
        assert_eq!(&new[a..b], "33.0");
    }

    #[test]
    fn changed_ranges_identical_is_empty() {
        assert!(changed_ranges("abc", "abc").is_empty());
    }
}
