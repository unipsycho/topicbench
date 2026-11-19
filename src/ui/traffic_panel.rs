//! Live traffic panel — own section; toggled from toolbar (off by default).
//!
//! One row per message: payload icon (click = copy payload) · time · topic (click = copy)
//! · payload filling the remaining width. Scroll back through the session buffer; while
//! scrolled away from the newest rows the list holds still so rows don't jump.

use crate::mqtt::session::{SessionManager, TrafficLine};
use crate::ui::hover_tip;
use crate::ui::icons::{
    self, IconSet, CHROME_ACCENT_BORDER, CHROME_ACCENT_SOFT, CHROME_HOVER, CHROME_MUTED, CHROME_TEXT,
};
use crate::ui::search_bar::{SearchFilter, SearchScope};
use eframe::egui::{self, text::LayoutJob, text::TextFormat, Color32, Key, RichText, Sense};

const ROW_H: f32 = 20.0;
const ICON_PX: f32 = 16.0;
const TIME_W: f32 = 62.0;
/// Fixed slots in the filter row so the ◀/▶ buttons never shift.
const COUNT_W: f32 = 64.0;
const TINY_SLOT: f32 = 24.0;

/// Pause / restart / filter state for the Live traffic panel.
#[derive(Clone, Debug, Default)]
pub struct TrafficViewState {
    /// When true, show [`frozen`] instead of the live session buffer.
    pub paused: bool,
    /// Snapshot taken when Pause was pressed (or while scrolled back).
    pub frozen: Vec<TrafficLine>,
    /// Local filter text (`a|b` = OR), applied on top of the global search.
    pub query: String,
    /// Auto-hold: list is frozen only because the user scrolled away from the newest rows.
    pub holding: bool,
    /// Scroll offset from the previous frame.
    pub scroll_y: f32,
    /// Row index (in the filtered list) of the current match for ◀/▶ navigation.
    pub current: usize,
    /// Scroll the current row into view on the next frame.
    pub scroll_to_current: bool,
    /// Filtered row count from the previous frame (for the n/N label).
    pub match_count: usize,
    /// Freeze the list on the next frame (set by ◀/▶ navigation).
    pub hold_request: bool,
}

fn snapshot(sessions: &SessionManager, id: &Option<String>) -> Vec<TrafficLine> {
    id.as_ref()
        .and_then(|id| sessions.open.get(id))
        .map(|s| s.traffic.iter().cloned().collect())
        .unwrap_or_default()
}

const HL_BG: Color32 = Color32::from_rgb(214, 170, 50);
const HL_BG_CURRENT: Color32 = Color32::from_rgb(240, 130, 40);
const HL_FG: Color32 = Color32::from_rgb(20, 20, 24);

/// Search terms (lowercased chars) that apply to the topic or the payload per filter scope.
fn terms_for(filters: &[&SearchFilter], topic: bool) -> Vec<Vec<char>> {
    let mut out = Vec::new();
    for f in filters {
        let applies = match f.scope {
            SearchScope::All => true,
            SearchScope::Topics => topic,
            SearchScope::Payloads => !topic,
        };
        if applies {
            out.extend(f.terms().into_iter().map(|t| t.chars().collect::<Vec<_>>()));
        }
    }
    out
}

fn lower(c: char) -> char {
    c.to_lowercase().next().unwrap_or(c)
}

/// Mark every case-insensitive occurrence of any term.
fn match_mask(chars: &[char], terms: &[Vec<char>]) -> Vec<bool> {
    let mut mask = vec![false; chars.len()];
    let low: Vec<char> = chars.iter().map(|c| lower(*c)).collect();
    for t in terms {
        if t.is_empty() || t.len() > low.len() {
            continue;
        }
        for i in 0..=(low.len() - t.len()) {
            if low[i..i + t.len()] == t[..] {
                for m in &mut mask[i..i + t.len()] {
                    *m = true;
                }
            }
        }
    }
    mask
}

/// Single-line, ellipsized layout that fills `width`; search matches get a highlight.
fn one_line(
    ui: &egui::Ui,
    text: &str,
    color: Color32,
    mono: bool,
    width: f32,
    terms: &[Vec<char>],
    hl_bg: Color32,
) -> std::sync::Arc<egui::Galley> {
    let chars: Vec<char> = text
        .chars()
        .take(600)
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let font = if mono {
        egui::TextStyle::Monospace.resolve(ui.style())
    } else {
        egui::TextStyle::Body.resolve(ui.style())
    };
    let mask = match_mask(&chars, terms);
    let mut job = LayoutJob::default();
    let mut i = 0;
    while i < chars.len() {
        let hit = mask[i];
        let mut j = i;
        while j < chars.len() && mask[j] == hit {
            j += 1;
        }
        let run: String = chars[i..j].iter().collect();
        let fmt = if hit {
            TextFormat {
                font_id: font.clone(),
                color: HL_FG,
                background: hl_bg,
                ..Default::default()
            }
        } else {
            TextFormat {
                font_id: font.clone(),
                color,
                ..Default::default()
            }
        };
        job.append(&run, 0.0, fmt);
        i = j;
    }
    job.wrap = egui::text::TextWrapping {
        max_width: width.max(1.0),
        max_rows: 1,
        break_anywhere: true,
        overflow_character: Some('…'),
    };
    ui.fonts(|f| f.layout_job(job))
}

pub fn draw(
    ui: &mut egui::Ui,
    sessions: &mut SessionManager,
    target_session_id: &Option<String>,
    filter: &SearchFilter,
    topic_color: Color32,
    icons: &IconSet,
    view: &mut TrafficViewState,
    height: &mut f32,
    status: &mut String,
) {
    let mut copy_all = false;
    icons::section_header_bar(ui, |ui| {
        icons::section_icon(ui, icons, "traffic");
        icons::section_title(ui, "Live traffic");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // Right-to-left → visual order: Copy all | Pause/Play | Reset
            if icons
                .tiny_header(
                    ui,
                    "playback",
                    false,
                    "Reset — clear traffic and start the list empty",
                )
                .clicked()
            {
                view.paused = false;
                view.holding = false;
                view.frozen.clear();
                if let Some(id) = target_session_id.as_ref() {
                    if let Some(session) = sessions.open.get_mut(id) {
                        session.traffic.clear();
                    }
                }
                *status = "Live traffic reset".into();
            }
            if view.paused {
                if icons
                    .tiny_header(ui, "play", false, "Play — resume live traffic")
                    .clicked()
                {
                    view.paused = false;
                    view.holding = false;
                    view.frozen.clear();
                    *status = "Live traffic playing".into();
                }
            } else if icons
                .tiny_header(
                    ui,
                    "pause",
                    false,
                    "Pause — freeze the stream so you can scroll and copy",
                )
                .clicked()
            {
                view.frozen = snapshot(sessions, target_session_id);
                view.holding = false;
                view.paused = true;
                *status = "Live traffic paused".into();
            }
            if icons
                .tiny_header(
                    ui,
                    "clipboard_checklist",
                    false,
                    "Copy all shown rows (topic and payload, tab-separated)",
                )
                .clicked()
            {
                copy_all = true;
            }
        });
    });
    ui.add_space(4.0);

    let Some(id) = target_session_id.as_ref() else {
        ui.weak("Select a connected profile to see traffic.");
        return;
    };
    if !sessions.open.contains_key(id) {
        ui.weak("No open session.");
        return;
    }

    // Local filter row: [search] [text …………] n/N [◀][▶] [x]
    let local = SearchFilter {
        query: view.query.clone(),
        ..SearchFilter::default()
    };
    let mut nav_next = false;
    let mut nav_prev = false;
    let any_filter = local.is_active() || filter.is_active();
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        icons::tiny_image(ui, icons, "search");
        // Trailing controls are anchored to the right edge with fixed-width slots, so the
        // buttons never move when the n/N count changes width (9 → 10 …).
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // Clear slot is always reserved (icon only when there's text).
            let (clear_rect, _) =
                ui.allocate_exact_size(egui::vec2(TINY_SLOT, ROW_H), Sense::hover());
            if !view.query.is_empty() {
                let mut child = ui.new_child(egui::UiBuilder::new().max_rect(clear_rect));
                if icons
                    .tiny(&mut child, "close_red", false, "Clear filter")
                    .clicked()
                {
                    view.query.clear();
                    view.current = 0;
                }
            }
            if hover_tip::attach(
                ui.add_enabled(any_filter && view.match_count > 0, egui::Button::new("▶").small()),
                "Next match (Enter)",
            )
            .clicked()
            {
                nav_next = true;
            }
            if hover_tip::attach(
                ui.add_enabled(any_filter && view.match_count > 0, egui::Button::new("◀").small()),
                "Previous match (Shift+Enter)",
            )
            .clicked()
            {
                nav_prev = true;
            }
            let n = view.match_count;
            let label = if !any_filter {
                "—".to_string()
            } else if n == 0 {
                "0".into()
            } else {
                format!("{}/{}", view.current.min(n - 1) + 1, n)
            };
            // Fixed-width, right-aligned count slot.
            ui.allocate_ui_with_layout(
                egui::vec2(COUNT_W, ROW_H),
                egui::Layout::right_to_left(egui::Align::Center),
                |ui| {
                    ui.add(egui::Label::new(RichText::new(label).small().monospace().weak()).truncate());
                },
            );
            // Text box takes whatever is left.
            let resp = ui.add(
                egui::TextEdit::singleline(&mut view.query)
                    .id_salt("traffic_filter")
                    .desired_width(ui.available_width())
                    .hint_text("Filter traffic (a|b = either)"),
            );
            let resp = hover_tip::attach(
                resp,
                "Show only rows whose topic or payload matches. Use | for OR. Enter / Shift+Enter = next / previous match.",
            );
            if resp.changed() {
                view.current = 0;
                view.scroll_to_current = true;
            }
            if resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                if ui.input(|i| i.modifiers.shift) {
                    nav_prev = true;
                } else {
                    nav_next = true;
                }
                resp.request_focus();
            }
        });
    });
    ui.add_space(2.0);

    // Hold the list still while the user is scrolled back in live mode.
    if !view.paused {
        if (view.scroll_y > ROW_H * 0.5 || view.hold_request) && !view.holding {
            view.frozen = snapshot(sessions, target_session_id);
            view.holding = true;
            view.hold_request = false;
        } else if view.scroll_y <= ROW_H * 0.5 && view.holding && !view.scroll_to_current {
            view.holding = false;
            view.frozen.clear();
        }
    }
    let use_frozen = view.paused || view.holding;

    let Some(session) = sessions.open.get(id) else {
        return;
    };
    let source: Vec<&TrafficLine> = if use_frozen {
        view.frozen.iter().collect()
    } else {
        session.traffic.iter().collect()
    };
    let total = source.len();
    let lines: Vec<&TrafficLine> = source
        .into_iter()
        .filter(|l| filter.matches(&l.topic, Some(&l.payload)))
        .filter(|l| local.matches(&l.topic, Some(&l.payload)))
        .collect();

    // ◀/▶ walk the matching rows (newest first); freeze the list so rows don't shift.
    view.match_count = lines.len();
    if lines.is_empty() {
        view.current = 0;
    } else {
        view.current = view.current.min(lines.len() - 1);
        if (nav_next || nav_prev) && any_filter {
            let n = lines.len();
            view.current = if nav_next {
                (view.current + 1) % n
            } else {
                (view.current + n - 1) % n
            };
            view.scroll_to_current = true;
            view.hold_request = true;
        }
    }

    // Count + state strip.
    ui.horizontal(|ui| {
        let counts = if lines.len() == total {
            format!("{total} messages")
        } else {
            format!("{} of {total} messages", lines.len())
        };
        ui.label(RichText::new(counts).small().color(CHROME_MUTED()));
        if view.paused {
            ui.label(RichText::new("· paused").small().color(Color32::from_rgb(220, 180, 80)));
        } else if view.holding {
            ui.label(
                RichText::new("· holding while scrolled — scroll to top to resume")
                    .small()
                    .color(Color32::from_rgb(220, 180, 80)),
            );
        }
    });

    if copy_all {
        let text = lines
            .iter()
            .map(|l| format!("{}\t{}", l.topic, l.payload))
            .collect::<Vec<_>>()
            .join("\n");
        ui.ctx().copy_text(text);
        *status = format!("Copied {} traffic rows", lines.len());
    }

    if lines.is_empty() {
        ui.weak(if total > 0 {
            "No messages match the filter."
        } else if view.paused {
            "Paused — no messages in snapshot."
        } else {
            "Waiting for messages…"
        });
        view.scroll_y = 0.0;
        return;
    }

    // Highlight terms from the global search and the local filter (per scope).
    let both = [filter, &local];
    let topic_terms = terms_for(&both, true);
    let pay_terms = terms_for(&both, false);
    let current = view.current;
    let step = ROW_H + ui.spacing().item_spacing.y;

    // Drag to resize height (persisted by caller).
    let h = (*height).clamp(80.0, 480.0);
    let mut area = egui::ScrollArea::vertical()
        .id_salt("live_traffic_panel")
        .max_height(h)
        // Fill the panel width; only shrink vertically for short lists.
        .auto_shrink([false, true]);
    if view.scroll_to_current && any_filter {
        area = area.vertical_scroll_offset((current as f32 * step - (h - step) * 0.5).max(0.0));
    }
    view.scroll_to_current = false;
    let out = area.show_rows(ui, ROW_H, lines.len(), |ui, range| {
            for i in range {
                let line = lines[i];
                let w = ui.available_width();
                let (rect, _) = ui.allocate_exact_size(egui::vec2(w, ROW_H), Sense::hover());
                let hovered = ui.rect_contains_pointer(rect);
                let is_current = any_filter && i == current;
                if is_current {
                    ui.painter().rect_filled(rect, 2.0, CHROME_ACCENT_SOFT());
                    ui.painter().rect_stroke(
                        rect,
                        2.0,
                        egui::Stroke::new(1.0_f32, CHROME_ACCENT_BORDER()),
                        egui::StrokeKind::Inside,
                    );
                } else if hovered {
                    ui.painter().rect_filled(rect, 2.0, CHROME_HOVER());
                }
                let hl_bg = if is_current { HL_BG_CURRENT } else { HL_BG };
                let painter = ui.painter().with_clip_rect(rect);
                let cy = rect.center().y;
                let mut x = rect.left() + 2.0;

                // Payload icon = copy payload.
                let icon_rect = egui::Rect::from_min_size(
                    egui::pos2(x, cy - ICON_PX * 0.5),
                    egui::vec2(ICON_PX, ICON_PX),
                );
                if let Some(tex) = icons.get("json_payload") {
                    egui::Image::new(tex)
                        .fit_to_exact_size(egui::vec2(ICON_PX, ICON_PX))
                        .paint_at(ui, icon_rect);
                }
                let icon_resp = hover_tip::attach(
                    ui.interact(
                        icon_rect.expand(2.0),
                        ui.id().with(("tr_copy", i)),
                        Sense::click(),
                    ),
                    "Copy payload",
                );
                if icon_resp.clicked() {
                    ui.ctx().copy_text(line.payload.clone());
                    *status = "Copied payload".into();
                }
                x += ICON_PX + 6.0;

                // Arrival time.
                let time = line.at.format("%H:%M:%S").to_string();
                let tg = ui.fonts(|f| {
                    f.layout_no_wrap(
                        time,
                        egui::TextStyle::Small.resolve(ui.style()),
                        CHROME_MUTED(),
                    )
                });
                painter.galley(egui::pos2(x, cy - tg.size().y * 0.5), tg, CHROME_MUTED());
                x += TIME_W;

                // Topic (≤ 45% of the row), click = copy topic.
                let topic_max = ((rect.right() - x) * 0.45).max(60.0);
                let topic_g = one_line(ui, &line.topic, topic_color, true, topic_max, &topic_terms, hl_bg);
                let topic_rect = egui::Rect::from_min_size(
                    egui::pos2(x, rect.top()),
                    egui::vec2(topic_g.size().x, ROW_H),
                );
                painter.galley(
                    egui::pos2(x, cy - topic_g.size().y * 0.5),
                    topic_g,
                    topic_color,
                );
                let topic_resp = hover_tip::attach(
                    ui.interact(topic_rect, ui.id().with(("tr_topic", i)), Sense::click()),
                    "Click to copy topic",
                );
                if topic_resp.clicked() {
                    ui.ctx().copy_text(line.topic.clone());
                    *status = format!("Copied topic: {}", line.topic);
                }
                x = topic_rect.right() + 10.0;

                // Payload fills everything that's left.
                let pay_w = rect.right() - x - 4.0;
                if pay_w > 8.0 {
                    let pg = one_line(ui, &line.payload, CHROME_TEXT(), false, pay_w, &pay_terms, hl_bg);
                    painter.galley(egui::pos2(x, cy - pg.size().y * 0.5), pg, CHROME_TEXT());
                    let pay_rect = egui::Rect::from_min_max(
                        egui::pos2(x, rect.top()),
                        egui::pos2(rect.right(), rect.bottom()),
                    );
                    let preview: String = line.payload.chars().take(800).collect();
                    let pay_resp = hover_tip::attach(
                        ui.interact(pay_rect, ui.id().with(("tr_pay", i)), Sense::click()),
                        format!("{preview}\n\n(click to copy)"),
                    );
                    if pay_resp.clicked() {
                        ui.ctx().copy_text(line.payload.clone());
                        *status = "Copied payload".into();
                    }
                }
            }
        });
    view.scroll_y = out.state.offset.y;

    let resp = ui.allocate_response(
        egui::vec2(ui.available_width(), 6.0),
        egui::Sense::click_and_drag(),
    );
    if resp.hovered() || resp.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
    }
    if resp.dragged() {
        *height = (*height + resp.drag_delta().y).clamp(80.0, 480.0);
    }
}
