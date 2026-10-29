//! Profile / panel / shortcut actions for the shell.

use super::layout;
use super::MqttMonitorApp;
use crate::mqtt::settings::AppSettings;
use crate::mqtt::subscribe;
use crate::ui::shortcuts::{self, Action as ShortcutAction};
use eframe::egui;

impl MqttMonitorApp {
    /// Persist search query + scope + apply-to-stickies to settings.json.
    pub(super) fn persist_search(&mut self) {
        self.settings.set_search(
            &self.search.query,
            self.search.scope,
            self.search.apply_to_stickies,
        );
        if let Err(e) = self.settings.save() {
            self.status = format!("Settings save failed: {e}");
        }
    }

    /// Persist current panel visibility (Payload + Simulation) to settings.json.
    pub(super) fn persist_panels(&mut self) {
        self.settings.set_panels(
            self.panels.profiles,
            self.panels.live_traffic,
            self.panels.publish,
            self.panels.payload,
            self.panels.stickies,
            self.panels.topic_tree,
        );
        self.settings
            .set_simulation_panel(self.simulation_open, self.simulation_floating);
        self.settings
            .set_pro_windows(self.bookmarks_open, self.color_rules_open);
        let _ = self.settings.save();
    }

    /// New profile: auto-show Profiles if hidden; remember for Save auto-close.
    pub(super) fn action_new_profile(&mut self) {
        if !self.panels.profiles {
            self.panels.profiles = true;
            self.profiles_auto_opened = true;
            self.persist_panels();
        }
        let id = self.profiles.add_new();
        self.show_profile_password = false;
        self.status = format!("Created profile {id}");
        self.focus_profile_editor = true;
    }

    /// Save profiles; if New auto-opened the panel, close it again.
    pub(super) fn action_save_profiles(&mut self) {
        match self.profiles.save() {
            Ok(()) => self.status = self.profiles.last_io.clone(),
            Err(e) => self.status = e,
        }
        if self.profiles_auto_opened {
            self.panels.profiles = false;
            self.profiles_auto_opened = false;
            self.persist_panels();
        }
    }

    pub(super) fn run_auto_reconnect(&mut self, ctx: &egui::Context) {
        if self.auto_reconnect_done {
            return;
        }
        self.auto_reconnect_done = true;
        if !self.settings.auto_reconnect_on_reload {
            return;
        }
        let ids = self.settings.last_connected.clone();
        let mut n = 0;
        for id in ids {
            if let Some(mut profile) = self.profiles.profiles.get(&id).cloned() {
                if subscribe::prune_redundant(&mut profile.subscriptions) {
                    if let Some(p) = self.profiles.profiles.get_mut(&id) {
                        p.subscriptions = profile.subscriptions.clone();
                    }
                    let _ = self.profiles.save();
                }
                let max_pkt = AppSettings::clamp_max_packet(self.settings.max_packet_bytes);
                if self.sessions.connect(&profile, max_pkt, ctx).is_ok() {
                    self.pins.restore_from_profile(&profile);
                    n += 1;
                }
            }
        }
        if n > 0 {
            self.status = format!("Auto-reconnect: connecting {n} profile(s)");
        }
    }

    pub(super) fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        for action in shortcuts::consume(ctx) {
            match action {
                ShortcutAction::ToggleProfiles => {
                    self.panels.profiles = !self.panels.profiles;
                    self.persist_panels();
                }
                ShortcutAction::ToggleTraffic => {
                    self.panels.live_traffic = !self.panels.live_traffic;
                    self.persist_panels();
                }
                ShortcutAction::TogglePublish => {
                    self.panels.publish = !self.panels.publish;
                    self.persist_panels();
                }
                ShortcutAction::TogglePayload => {
                    self.panels.payload = !self.panels.payload;
                    self.persist_panels();
                }
                ShortcutAction::ToggleStickies => {
                    self.panels.stickies = !self.panels.stickies;
                    self.persist_panels();
                }
                ShortcutAction::ToggleTopicTree => {
                    self.panels.topic_tree = !self.panels.topic_tree;
                    self.persist_panels();
                }
                ShortcutAction::ZoomIn => {
                    if self.settings.zoom_font_in() {
                        let _ = self.settings.save();
                        layout::apply_font_zoom(ctx, self.settings.font_zoom_factor());
                        self.status = format!("Font zoom {}", self.settings.font_zoom_step);
                    }
                }
                ShortcutAction::ZoomOut => {
                    if self.settings.zoom_font_out() {
                        let _ = self.settings.save();
                        layout::apply_font_zoom(ctx, self.settings.font_zoom_factor());
                        self.status = format!("Font zoom {}", self.settings.font_zoom_step);
                    }
                }
                ShortcutAction::ZoomReset => {
                    if self.settings.zoom_font_reset() {
                        let _ = self.settings.save();
                        layout::apply_font_zoom(ctx, self.settings.font_zoom_factor());
                        self.status = "Font zoom 0".into();
                    }
                }
                ShortcutAction::OpenSettings => {
                    self.open_settings();
                }
                ShortcutAction::OpenShortcutsHelp => {
                    self.shortcuts_open = true;
                }
                ShortcutAction::FocusSearch => {
                    self.focus_search = true;
                }
                ShortcutAction::NewProfile => {
                    self.action_new_profile();
                }
                ShortcutAction::SaveProfiles => {
                    self.action_save_profiles();
                }
                ShortcutAction::DeleteProfile => {
                    // Prefer bulk-deleting selected sims when Simulation is open with a selection.
                    if self.simulation_open && !self.sim_client.selected_ids.is_empty() {
                        match self.sim_client.delete_selected() {
                            Ok(n) => {
                                self.status = if n == 1 {
                                    "Deleted 1 simulation".into()
                                } else {
                                    format!("Deleted {n} simulations")
                                };
                            }
                            Err(e) => self.status = e,
                        }
                    } else if self.panels.profiles {
                        // Delete profile only when Profiles panel is visible.
                        if let Some(id) = self.profiles.delete_selected() {
                            self.sessions.close(&id);
                            self.status = format!("Deleted profile {id}");
                        }
                    }
                }
                ShortcutAction::ConnectToggle => {
                    self.shortcut_connect_toggle(ctx);
                }
                ShortcutAction::ShowProfiles => {
                    self.panels.profiles = true;
                    self.profiles_auto_opened = false;
                }
                ShortcutAction::FocusProfileEditor => {
                    self.panels.profiles = true;
                    self.profiles_auto_opened = false;
                    self.focus_profile_editor = true;
                }
                ShortcutAction::ProfileSelectPrev => {
                    // Prefer sim list when Simulate is open with a selection/focus.
                    if self.simulation_open
                        && (!self.sim_client.selected_ids.is_empty()
                            || self.sim_client.selection_focus.is_some())
                    {
                        let extend = ctx.input(|i| i.modifiers.shift);
                        self.sim_client.move_selection(-1, extend);
                    } else {
                        self.move_profile_selection(-1);
                    }
                }
                ShortcutAction::ProfileSelectNext => {
                    if self.simulation_open
                        && (!self.sim_client.selected_ids.is_empty()
                            || self.sim_client.selection_focus.is_some())
                    {
                        let extend = ctx.input(|i| i.modifiers.shift);
                        self.sim_client.move_selection(1, extend);
                    } else {
                        self.move_profile_selection(1);
                    }
                }
                ShortcutAction::ToggleStickySelectedTopic => {
                    self.shortcut_toggle_sticky();
                }
                ShortcutAction::ToggleBookmarkSelectedTopic => {
                    self.shortcut_toggle_bookmark();
                }
                ShortcutAction::OpenBookmarks => {
                    if crate::pro::has(crate::pro::Entitlement::TopicBookmarks) {
                        self.bookmarks_open = !self.bookmarks_open;
                        self.persist_panels();
                    } else {
                        self.status = "Bookmarks require TopicBench Pro".into();
                    }
                }
                ShortcutAction::ToggleSimulation => {
                    if crate::pro::has(crate::pro::Entitlement::Simulation) {
                        self.simulation_open = !self.simulation_open;
                        self.persist_panels();
                        if self.simulation_open {
                            self.sim_client.poll_if_due(true);
                        }
                    } else {
                        self.status = "Simulate requires TopicBench Pro".into();
                    }
                }
                ShortcutAction::ToggleColorRules => {
                    if crate::pro::has(crate::pro::Entitlement::ColorRules) {
                        self.color_rules_open = !self.color_rules_open;
                        self.persist_panels();
                    } else {
                        self.status = "Color Rules require TopicBench Pro".into();
                    }
                }
                ShortcutAction::SimStart => {
                    if crate::pro::has(crate::pro::Entitlement::Simulation) {
                        crate::ui::simulation_panel::action_start(
                            &mut self.sim_client,
                            &mut self.status,
                        );
                    } else {
                        self.status = "Simulate requires TopicBench Pro".into();
                    }
                }
                ShortcutAction::SimStop => {
                    if crate::pro::has(crate::pro::Entitlement::Simulation) {
                        crate::ui::simulation_panel::action_stop(
                            &mut self.sim_client,
                            &mut self.status,
                        );
                    } else {
                        self.status = "Simulate requires TopicBench Pro".into();
                    }
                }
            }
        }
    }

    fn move_profile_selection(&mut self, delta: i32) {
        let ids = self.profiles.ids_in_order();
        if ids.is_empty() {
            return;
        }
        let cur = self
            .profiles
            .selected_id
            .as_ref()
            .and_then(|id| ids.iter().position(|k| k == id))
            .unwrap_or(0);
        let next = (cur as i32 + delta).clamp(0, ids.len() as i32 - 1) as usize;
        if self.profiles.selected_id.as_deref() != Some(ids[next].as_str()) {
            self.show_profile_password = false;
        }
        self.profiles.selected_id = Some(ids[next].clone());
        self.panels.profiles = true;
    }

    fn shortcut_connect_toggle(&mut self, ctx: &egui::Context) {
        let Some(id) = self.profiles.selected_id.clone() else {
            self.status = "No profile selected".into();
            return;
        };
        let Some(profile) = self.profiles.profiles.get(&id).cloned() else {
            return;
        };
        let connecting = match self.sessions.open.get(&id).map(|s| s.state) {
            Some(crate::mqtt::session::SessionState::Connected)
            | Some(crate::mqtt::session::SessionState::Connecting) => false,
            _ => true,
        };
        let max_pkt = AppSettings::clamp_max_packet(self.settings.max_packet_bytes);
        match self.sessions.toggle(&profile, max_pkt, ctx, &mut self.pins) {
            Ok(msg) => {
                self.settings.remember_connected(&id, connecting);
                if connecting {
                    let pruned = self
                        .profiles
                        .profiles
                        .get_mut(&id)
                        .map(|p| subscribe::prune_redundant(&mut p.subscriptions))
                        .unwrap_or(false);
                    if pruned {
                        let _ = self.profiles.save();
                    }
                    if let Some(p) = self.profiles.profiles.get(&id) {
                        self.pins.restore_from_profile(p);
                    }
                }
                self.status = msg;
            }
            Err(e) => self.status = e,
        }
    }

    fn shortcut_toggle_sticky(&mut self) {
        let Some((profile_id, profile_name, topic)) = self.pins.last_tree_focus.clone() else {
            self.status = "Click a topic tree row first (Ctrl+Shift+S)".into();
            return;
        };
        if self.pins.is_sticky(&profile_id, &topic) {
            self.pins.unpin(&profile_id, &topic);
            self.profiles
                .set_sticky_and_save(&profile_id, &topic, false);
            self.status = format!("Unstuck {topic}");
        } else {
            self.pins.pin(&profile_id, &profile_name, &topic);
            self.profiles.set_sticky_and_save(&profile_id, &topic, true);
            self.status = format!("Sticky {topic}");
        }
    }

    fn shortcut_toggle_bookmark(&mut self) {
        if !crate::pro::has(crate::pro::Entitlement::TopicBookmarks) {
            self.status = "Bookmarks require TopicBench Pro".into();
            return;
        }
        let Some((profile_id, _, topic)) = self.pins.last_tree_focus.clone() else {
            self.status = "Click a topic tree row first (Ctrl+Shift+B)".into();
            return;
        };
        let on = self.bookmarks.toggle(&profile_id, &topic);
        self.status = if on {
            format!("Bookmarked {topic}")
        } else {
            format!("Removed bookmark {topic}")
        };
    }
}
