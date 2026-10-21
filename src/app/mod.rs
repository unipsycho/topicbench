//! Top-level eframe app: Office-style ribbon chrome, optional panels, settings viewport.

mod actions;
mod layout;
mod settings_modal;

use crate::mqtt::pins::PinBoard;
use crate::mqtt::profile::ProfileStore;
use crate::mqtt::publish::{PublishDraft, PublishHistory};
use crate::mqtt::session::SessionManager;
use crate::mqtt::settings::AppSettings;
use crate::ui::icons::{IconSet, CHROME_BG};
use crate::ui::profiles::ProfileListFilter;
use crate::ui::search_bar::SearchFilter;
use crate::ui::payload_panel::{PayloadSearch, PoppedPayload};
use crate::ui::flash;
use crate::ui::splash::SplashIntro;
use eframe::egui::{self, TextureHandle};
use std::collections::HashSet;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

/// Profiles left panel: resize handle cannot drag narrower than this (no see-through gap).
const PROFILES_PANEL_MIN_W: f32 = 240.0;

/// Which optional panels are visible (ribbon toggles).
#[derive(Clone, Debug)]
struct PanelVisibility {
    /// Left profiles side panel (toggle from Profiles ribbon group).
    profiles: bool,
    /// Off by default — doesn't dominate the UI.
    live_traffic: bool,
    stickies: bool,
    topic_tree: bool,
    /// Off by default — only when publishing.
    publish: bool,
    /// Off by default — Value view for the selected tree topic.
    payload: bool,
}

impl Default for PanelVisibility {
    fn default() -> Self {
        Self {
            profiles: true,
            live_traffic: false,
            stickies: true,
            topic_tree: true,
            publish: false,
            payload: false,
        }
    }
}

/// Root UI state for the MQTT Explorer–style monitor.
pub struct MqttMonitorApp {
    pub profiles: ProfileStore,
    pub sessions: SessionManager,
    pub pins: PinBoard,
    pub publish: PublishDraft,
    pub publish_history: PublishHistory,
    pub settings: AppSettings,
    pub profile_filter: ProfileListFilter,
    pub search: SearchFilter,
    /// Draft topic filter while editing a profile’s subscriptions.
    pub sub_draft: String,
    pub status: String,
    /// When `status` last changed — right-side strip fades after 1 minute idle.
    status_changed_at: Instant,
    /// Shadow of `status` so we detect mutations from any panel/action.
    status_snap: String,
    logo: Option<TextureHandle>,
    icons: IconSet,
    panels: PanelVisibility,
    settings_open: bool,
    shortcuts_open: bool,
    focus_search: bool,
    focus_profile_editor: bool,
    /// Profiles panel was opened by New (auto-close on next Save).
    profiles_auto_opened: bool,
    auto_reconnect_done: bool,
    /// Find-in-payload query / match cursor for the Payload side panel.
    payload_search: PayloadSearch,
    /// Profile password field: plain text when true (session UI only; resets on profile switch).
    show_profile_password: bool,
    /// Settings modal draft — applied on Save, discarded on Cancel / Escape / backdrop.
    settings_draft: Option<AppSettings>,
    /// Topic-tree brokers popped out to their own OS windows (profile ids).
    topic_tree_undocked: HashSet<String>,
    /// Payload topics locked into floating windows (each keeps its own topic).
    payload_popped: Vec<PoppedPayload>,
    /// Startup AA logo intro; dropped when finished / skipped.
    splash: Option<SplashIntro>,
}

impl MqttMonitorApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let profiles = ProfileStore::load_or_sample();
        let settings = AppSettings::load();
        let status = profiles.last_io.clone();
        layout::apply_font_zoom(&cc.egui_ctx, settings.font_zoom_factor());
        // Use our font-zoom chords; disable egui’s pixels_per_point zoom.
        cc.egui_ctx.options_mut(|o| o.zoom_with_keyboard = false);
        let logo = layout::load_logo(&cc.egui_ctx);
        let splash_logo = layout::load_splash_logo(&cc.egui_ctx);
        let icons = IconSet::load(&cc.egui_ctx);
        // Restore ribbon panel open/closed (incl. Payload) from settings.json.
        let panels = PanelVisibility {
            profiles: settings.show_profiles,
            live_traffic: settings.show_live_traffic,
            publish: settings.show_publish,
            payload: settings.show_payload,
            stickies: settings.show_stickies,
            topic_tree: settings.show_topic_tree,
        };
        // Restore search text + scope so the filter is active as the tree paints.
        let search = SearchFilter::from_settings(
            settings.search_query.clone(),
            settings.search_scope,
            settings.search_apply_to_stickies,
        );
        let sessions = SessionManager::default();
        sessions.bind_egui_ctx(&cc.egui_ctx);
        Self {
            profiles,
            sessions,
            pins: PinBoard::default(),
            publish: PublishDraft::default(),
            publish_history: PublishHistory::load(),
            settings,
            profile_filter: ProfileListFilter::All,
            search,
            sub_draft: String::new(),
            status: status.clone(),
            status_changed_at: Instant::now(),
            status_snap: status,
            logo,
            icons,
            panels,
            settings_open: false,
            shortcuts_open: false,
            focus_search: false,
            focus_profile_editor: false,
            profiles_auto_opened: false,
            auto_reconnect_done: false,
            payload_search: PayloadSearch::default(),
            show_profile_password: false,
            settings_draft: None,
            topic_tree_undocked: HashSet::new(),
            payload_popped: Vec::new(),
            splash: Some(SplashIntro::new(splash_logo)),
        }
    }
}

impl eframe::App for MqttMonitorApp {
    /// Transparent during splash so the AA logo can float; opaque chrome after.
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        if self.splash.is_some() {
            [0.0, 0.0, 0.0, 0.0]
        } else {
            CHROME_BG.to_normalized_gamma_f32()
        }
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.run_auto_reconnect(ctx);

        // Hyprland ANR = missed xdg_wm_base pings (misc:anr_missed_pings=3).
        // `ui_visible` comes from a Hyprland poller — clears before egui Focused(false)
        // so inbox drops to 1 Hz even if keyboard leave is delayed.
        let compositor_visible = self.sessions.ui_visible.load(Ordering::Relaxed);
        let focused = ctx.input(|i| i.focused) && compositor_visible;
        self.sessions
            .ui_focused
            .store(focused, Ordering::Relaxed);

        // Always apply a capped inbox batch so Stats/counts stay alive (even off-workspace).
        let poll = self.sessions.poll_events(&mut self.pins);
        let need_repaint = poll.need_repaint();
        if let Some(msg) = poll.status {
            self.status = msg;
        }
        // Stats ribbon labels: rebuild frozen snapshot at most once per second.
        let _ = self.sessions.traffic_stats.refresh_display_if_due();

        // Splash owns the first frames — skip ribbon/panels (MQTT poll still runs above).
        let splash_active = if let Some(splash) = self.splash.as_mut() {
            splash.tick_and_paint(ctx)
        } else {
            false
        };
        if !splash_active {
            self.splash = None;
        }
        if splash_active {
            return;
        }

        // egui idles without input — keep frames flowing while any fade is live.
        // Session/pin flash_until already covers tree pulses (set on the same apply).
        // Do not walk the whole topic tree here — that alone ANRs under large trees.
        let now = Instant::now();
        let flashing = self
            .sessions
            .open
            .values()
            .any(|s| flash::flash_active(s.flash_until, now))
            || self
                .pins
                .items
                .values()
                .any(|p| flash::flash_active(p.flash_until, now))
            || self.payload_search.change_flash_active(now)
            || self
                .payload_popped
                .iter()
                .any(|p| p.search.change_flash_active(now));
        let status_fade = status_strip_fade_alpha(self.status_changed_at, now);
        if focused {
            if need_repaint || flashing || (status_fade > 0.0 && status_fade < 1.0) {
                ctx.request_repaint();
            } else {
                // 1 Hz Stats refresh and/or wake when status fade is due.
                let mut wait = if self.sessions.traffic_stats.wants_periodic_refresh() {
                    Some(self.sessions.traffic_stats.time_until_display_refresh())
                } else {
                    None
                };
                if let Some(status_wait) = status_strip_repaint_after(self.status_changed_at, now) {
                    wait = Some(wait.map_or(status_wait, |w| w.min(status_wait)));
                }
                if let Some(w) = wait {
                    ctx.request_repaint_after(w);
                }
            }
        } else {
            // Unfocused / other workspace: ~1 Hz heartbeat — alive, not ANR-prone.
            ctx.request_repaint_after(crate::mqtt::session::BACKGROUND_UI_HZ_INTERVAL);
        }

        self.handle_shortcuts(ctx);
        // Suppress tips from panels under Settings / Keyboard Shortcuts overlays
        // (including custom tree payload/topic popups — not only egui on_hover_text).
        let block_tips = self.settings_open || self.shortcuts_open;
        crate::ui::hover_tip::set_block_hover_tips(ctx, block_tips);
        if block_tips {
            crate::ui::topic_tree_panel::dismiss_hover_tips(ctx);
        }
        self.draw_ribbon(ctx);
        self.draw_panels(ctx);
        self.draw_undocked_topic_trees(ctx);
        self.draw_undocked_payloads(ctx);
        self.draw_overlays(ctx);

        // Panels/actions may have rewritten `status` — refresh fade clock.
        if self.status != self.status_snap {
            self.status_snap = self.status.clone();
            self.status_changed_at = Instant::now();
        }
    }
}

/// Hold ephemeral status 60s, then fade over 2s.
const STATUS_HOLD: Duration = Duration::from_secs(60);
const STATUS_FADE: Duration = Duration::from_secs(2);

pub(super) fn status_strip_fade_alpha(changed_at: Instant, now: Instant) -> f32 {
    let age = now.saturating_duration_since(changed_at);
    if age < STATUS_HOLD {
        1.0
    } else {
        let into = age.saturating_sub(STATUS_HOLD).as_secs_f32();
        (1.0 - into / STATUS_FADE.as_secs_f32()).clamp(0.0, 1.0)
    }
}

/// When to wake for the next status fade step (or hold expiry).
fn status_strip_repaint_after(changed_at: Instant, now: Instant) -> Option<Duration> {
    let age = now.saturating_duration_since(changed_at);
    if age < STATUS_HOLD {
        Some(STATUS_HOLD.saturating_sub(age))
    } else if age < STATUS_HOLD + STATUS_FADE {
        Some(Duration::from_millis(50))
    } else {
        None
    }
}
