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
use crate::update::{self, ReleaseInfo, UpdateService, UpdateStatus};
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
    /// Background GitHub Releases check / install.
    update_service: UpdateService,
    update_status: UpdateStatus,
    /// Offer shown in the update modal (also kept while Remind/Skip hides the modal).
    update_offer: Option<ReleaseInfo>,
    update_modal_open: bool,
    /// Session-only Remind me later (also persisted ~24h via settings).
    update_remind_session: bool,
    /// Startup checks stay quiet on failure; manual Check now surfaces in Settings.
    update_check_quiet: bool,
    /// Startup auto-check fired once after splash.
    update_auto_check_done: bool,
    /// Extra note when install fell back to opening the browser.
    update_modal_fallback_note: Option<String>,
    /// Pro MCP localhost Unix-socket bridge (None when not entitled / failed to bind).
    /// Settings → MCP will start/stop this later; v1 auto-starts when `mcp_server` is licensed.
    pro_bridge: Option<crate::pro::bridge::BridgeHandle>,
    /// Pro Simulation panel (ribbon → Pro → Simulation).
    simulation_open: bool,
    /// True = floating Window; false = docked in the main middle column.
    simulation_floating: bool,
    /// HTTP client state for `topicbench-pro` `/sim`.
    sim_client: crate::pro::sim_client::SimClientState,
}

impl MqttMonitorApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // One-shot: ~/.config/mqtt-monitor → ~/.config/topicbench when needed.
        crate::mqtt::paths::migrate_legacy_config_dir();
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
        // Gate bridge listen behind mcp_server entitlement (Pro MCP sidecar talks here).
        let pro_bridge = if crate::pro::has(crate::pro::Entitlement::McpServer) {
            match crate::pro::bridge::BridgeHandle::start(&cc.egui_ctx) {
                Ok(h) => {
                    eprintln!(
                        "topicbench: Pro bridge listening on {}",
                        h.socket_path.display()
                    );
                    Some(h)
                }
                Err(e) => {
                    eprintln!("topicbench: Pro bridge not started: {e}");
                    None
                }
            }
        } else {
            None
        };
        let simulation_open = settings.show_simulation;
        let simulation_floating = settings.simulation_floating;
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
            update_service: UpdateService::new(),
            update_status: UpdateStatus::Idle,
            update_offer: None,
            update_modal_open: false,
            update_remind_session: false,
            update_check_quiet: true,
            update_auto_check_done: false,
            update_modal_fallback_note: None,
            pro_bridge,
            simulation_open,
            simulation_floating,
            sim_client: crate::pro::sim_client::SimClientState::new(),
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
        // Fan MQTT applies into the Pro bridge buffer so wait_for_message / GUI stay in sync.
        if let Some(bridge) = self.pro_bridge.as_ref() {
            bridge.bind_egui_ctx(ctx);
            for m in &poll.applied {
                bridge.on_message(
                    &m.profile_id,
                    &m.profile_name,
                    &m.topic,
                    &m.payload,
                    m.retain,
                );
            }
            let max_pkt =
                crate::mqtt::settings::AppSettings::clamp_max_packet(self.settings.max_packet_bytes);
            bridge.poll(
                &self.profiles,
                &mut self.sessions,
                max_pkt,
                ctx,
                &mut self.status,
            );
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
        // Combined live monitor: while open + entitled, keep ~1 Hz paints even when idle /
        // unfocused so series advance; rest of UI still follows the throttle below.
        let combined_live = self.settings.combined_graph_open
            && crate::pro::has(crate::pro::Entitlement::MultiGraph);
        let hz = crate::mqtt::session::BACKGROUND_UI_HZ_INTERVAL;
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
                if combined_live {
                    wait = Some(wait.map_or(hz, |w| w.min(hz)));
                }
                if let Some(w) = wait {
                    ctx.request_repaint_after(w);
                }
            }
        } else {
            // Unfocused / other workspace: ~1 Hz heartbeat — alive, not ANR-prone.
            // Combined open uses the same floor (already ~1 Hz).
            ctx.request_repaint_after(hz);
        }

        // Drain update-check / install results (never blocks UI).
        while let Some(ev) = self.update_service.try_recv() {
            self.handle_update_event(ev);
            ctx.request_repaint();
        }
        // Quiet startup check once after splash (24h cache).
        if !self.update_auto_check_done
            && update::should_auto_check(self.settings.last_update_check)
        {
            self.update_auto_check_done = true;
            self.request_update_check(true);
        } else if !self.update_auto_check_done {
            self.update_auto_check_done = true;
        }

        self.handle_shortcuts(ctx);
        // Suppress tips under Settings / Shortcuts / Update overlays.
        let block_tips =
            self.settings_open || self.shortcuts_open || self.update_modal_open;
        crate::ui::hover_tip::set_block_hover_tips(ctx, block_tips);
        if block_tips {
            crate::ui::topic_tree_panel::dismiss_hover_tips(ctx);
        }
        // Reserve top chrome, paint central/sides, then ribbon/status on Middle overlays
        // so sticky/tree scroll cannot draw over the navbar (same pattern as side panels).
        let top_chrome = self.reserve_top_chrome(ctx);
        self.draw_panels(ctx);
        self.draw_top_chrome_overlays(ctx, top_chrome);
        self.draw_undocked_topic_trees(ctx);
        self.draw_undocked_payloads(ctx);
        self.draw_simulation_panel(ctx);
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
