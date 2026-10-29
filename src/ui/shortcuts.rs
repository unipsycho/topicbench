//! Keyboard shortcut bindings + help listing.
//!
//! Cmd/Ctrl chords are consumed globally; plain keys (Delete, arrows, `?`) only
//! fire when a text field is not capturing keyboard input.
//! Pro panel rows appear in the help sheet only when licensed.

use eframe::egui::{self, Key, Modifiers};

/// One row in the Shortcuts help list.
#[derive(Clone, Copy)]
pub struct Binding {
    pub keys: &'static str,
    pub action: &'static str,
}

/// Hover tip with shortcut: `"Settings (Ctrl+,)"`.
pub fn tip(label: &str, keys: &str) -> String {
    format!("{label} ({keys})")
}

/// Free / always-available bindings (keep in sync with `consume`).
pub const BINDINGS: &[Binding] = &[
    Binding {
        keys: "Ctrl+Shift+P",
        action: "Toggle Profiles panel",
    },
    Binding {
        keys: "Ctrl+Shift+T",
        action: "Toggle Traffic panel",
    },
    Binding {
        keys: "Ctrl+Shift+U",
        action: "Toggle Publish panel",
    },
    Binding {
        keys: "Ctrl+Shift+L",
        action: "Toggle Payload panel",
    },
    Binding {
        keys: "← / →",
        action: "Payload history · older / newer sample (when not Live)",
    },
    Binding {
        keys: "Ctrl+Shift+K",
        action: "Toggle Stickies panel",
    },
    Binding {
        keys: "Ctrl+Shift+R",
        action: "Toggle Topic tree panel",
    },
    Binding {
        keys: "Ctrl+= / Ctrl++",
        action: "Zoom in",
    },
    Binding {
        keys: "Ctrl+-",
        action: "Zoom out",
    },
    Binding {
        keys: "Ctrl+0",
        action: "Reset zoom",
    },
    Binding {
        keys: "Ctrl+,",
        action: "Open Settings",
    },
    Binding {
        keys: "F1 / ?",
        action: "Open Keyboard shortcuts help",
    },
    Binding {
        keys: "Ctrl+F",
        action: "Focus search",
    },
    Binding {
        keys: "Ctrl+N",
        action: "New profile",
    },
    Binding {
        keys: "Ctrl+S",
        action: "Save profiles",
    },
    Binding {
        keys: "Delete / Ctrl+Backspace",
        action: "Delete selected sims (Simulate open) or profile",
    },
    Binding {
        keys: "Ctrl+Enter",
        action: "Connect / Disconnect selected profile",
    },
    Binding {
        keys: "Ctrl+1",
        action: "Show Profiles panel",
    },
    Binding {
        keys: "Ctrl+E / F2",
        action: "Focus profile editor",
    },
    Binding {
        keys: "↑ / ↓",
        action: "Move profile list selection (when not typing)",
    },
    Binding {
        keys: "Ctrl+Shift+S",
        action: "Toggle sticky on last tree topic",
    },
    Binding {
        keys: "Tab / Shift+Tab",
        action: "Move focus through interactive controls",
    },
];

/// Pro-only panel / topic actions — help rows shown only when licensed.
pub const PRO_BINDINGS: &[Binding] = &[
    Binding {
        keys: "Ctrl+Shift+M",
        action: "Toggle Simulate panel",
    },
    Binding {
        keys: "Ctrl+Shift+G",
        action: "Start all simulations / resume playback",
    },
    Binding {
        keys: "Ctrl+Shift+H",
        action: "Stop / pause simulations",
    },
    Binding {
        keys: "Click / Ctrl / Shift / drag",
        action: "Select sims (box-select from gutter; Shift range)",
    },
    Binding {
        keys: "Delete",
        action: "Delete selected simulations",
    },
    Binding {
        keys: "Ctrl+Shift+J",
        action: "Toggle Bookmarks panel",
    },
    Binding {
        keys: "↑ / ↓",
        action: "Select bookmark (Bookmarks panel)",
    },
    Binding {
        keys: "Enter",
        action: "Jump to selected bookmark (Bookmarks panel)",
    },
    Binding {
        keys: "Ctrl+Shift+C",
        action: "Toggle Color Rules panel",
    },
    Binding {
        keys: "Ctrl+Shift+B",
        action: "Toggle bookmark on selected tree topic",
    },
];

/// Actions produced from this frame’s key events.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    ToggleProfiles,
    ToggleTraffic,
    TogglePublish,
    TogglePayload,
    ToggleStickies,
    ToggleTopicTree,
    ZoomIn,
    ZoomOut,
    ZoomReset,
    OpenSettings,
    OpenShortcutsHelp,
    FocusSearch,
    NewProfile,
    SaveProfiles,
    DeleteProfile,
    ConnectToggle,
    ShowProfiles,
    FocusProfileEditor,
    ProfileSelectPrev,
    ProfileSelectNext,
    ToggleStickySelectedTopic,
    ToggleBookmarkSelectedTopic,
    OpenBookmarks,
    ToggleSimulation,
    ToggleColorRules,
    SimStart,
    SimStop,
}

fn chord(i: &mut egui::InputState, mods: Modifiers, key: Key) -> bool {
    i.consume_key(mods, key)
}

/// Consume matching chords; skip plain Delete/arrows/`?` while a text field has focus.
pub fn consume(ctx: &egui::Context) -> Vec<Action> {
    let typing = ctx.wants_keyboard_input();
    let mut out = Vec::new();
    ctx.input_mut(|i| {
        // COMMAND = Ctrl on Linux/Windows, Cmd on macOS.
        let cmd = Modifiers::COMMAND;
        let cmd_shift = Modifiers::COMMAND | Modifiers::SHIFT;

        if chord(i, cmd_shift, Key::P) {
            out.push(Action::ToggleProfiles);
        }
        if chord(i, cmd_shift, Key::T) {
            out.push(Action::ToggleTraffic);
        }
        if chord(i, cmd_shift, Key::U) {
            out.push(Action::TogglePublish);
        }
        if chord(i, cmd_shift, Key::L) {
            out.push(Action::TogglePayload);
        }
        if chord(i, cmd_shift, Key::K) {
            out.push(Action::ToggleStickies);
        }
        if chord(i, cmd_shift, Key::R) {
            out.push(Action::ToggleTopicTree);
        }
        if chord(i, cmd_shift, Key::S) {
            out.push(Action::ToggleStickySelectedTopic);
        }
        if chord(i, cmd_shift, Key::B) {
            out.push(Action::ToggleBookmarkSelectedTopic);
        }
        if chord(i, cmd_shift, Key::J) {
            out.push(Action::OpenBookmarks);
        }
        if chord(i, cmd_shift, Key::M) {
            out.push(Action::ToggleSimulation);
        }
        if chord(i, cmd_shift, Key::C) {
            out.push(Action::ToggleColorRules);
        }
        if chord(i, cmd_shift, Key::G) {
            out.push(Action::SimStart);
        }
        if chord(i, cmd_shift, Key::H) {
            out.push(Action::SimStop);
        }

        if chord(i, cmd, Key::Equals) || chord(i, cmd, Key::Plus) {
            out.push(Action::ZoomIn);
        }
        if chord(i, cmd, Key::Minus) {
            out.push(Action::ZoomOut);
        }
        if chord(i, cmd, Key::Num0) {
            out.push(Action::ZoomReset);
        }

        if chord(i, cmd, Key::Comma) {
            out.push(Action::OpenSettings);
        }
        if chord(i, Modifiers::NONE, Key::F1) {
            out.push(Action::OpenShortcutsHelp);
        }
        if chord(i, cmd, Key::F) {
            out.push(Action::FocusSearch);
        }
        if chord(i, cmd, Key::N) {
            out.push(Action::NewProfile);
        }
        if chord(i, cmd, Key::S) {
            out.push(Action::SaveProfiles);
        }
        if chord(i, cmd, Key::Enter) {
            out.push(Action::ConnectToggle);
        }
        if chord(i, cmd, Key::Num1) {
            out.push(Action::ShowProfiles);
        }
        if chord(i, cmd, Key::E) || chord(i, Modifiers::NONE, Key::F2) {
            out.push(Action::FocusProfileEditor);
        }

        if !typing {
            if chord(i, Modifiers::NONE, Key::Questionmark) {
                out.push(Action::OpenShortcutsHelp);
            }
            if chord(i, Modifiers::NONE, Key::Delete) || chord(i, cmd, Key::Backspace) {
                out.push(Action::DeleteProfile);
            }
            // NONE or SHIFT so Simulate can Shift-extend selection.
            if chord(i, Modifiers::NONE, Key::ArrowUp) || chord(i, Modifiers::SHIFT, Key::ArrowUp)
            {
                out.push(Action::ProfileSelectPrev);
            }
            if chord(i, Modifiers::NONE, Key::ArrowDown)
                || chord(i, Modifiers::SHIFT, Key::ArrowDown)
            {
                out.push(Action::ProfileSelectNext);
            }
        }
    });
    out
}

fn draw_binding_rows(ui: &mut egui::Ui, bindings: &[Binding]) {
    for b in bindings {
        ui.monospace(b.keys);
        ui.label(b.action);
        ui.end_row();
    }
}

/// Draw the Shortcuts help table (popup / Settings).
/// Pro panel rows appear only when TopicBench Pro is licensed.
pub fn draw_help(ui: &mut egui::Ui) {
    ui.heading("Keyboard shortcuts");
    ui.add_space(6.0);
    // Tall enough for free rows + Pro section without nested scroll fighting.
    egui::ScrollArea::vertical().max_height(560.0).show(ui, |ui| {
        egui::Grid::new("shortcuts_help_grid")
            .num_columns(2)
            .spacing([20.0, 5.0])
            .striped(true)
            .min_col_width(160.0)
            .show(ui, |ui| {
                ui.strong("Keys");
                ui.strong("Action");
                ui.end_row();
                // Pro panel toggles first when licensed (matches ribbon; stays above the fold).
                if crate::pro::is_licensed() {
                    ui.strong("Pro");
                    ui.strong("");
                    ui.end_row();
                    draw_binding_rows(ui, PRO_BINDINGS);
                }
                draw_binding_rows(ui, BINDINGS);
            });
    });
    ui.add_space(6.0);
    ui.weak("Ctrl chords work while typing; Delete / arrows / ? only when not in a text field.");
    ui.weak("Tab / Shift+Tab walks ribbon → profiles → Topic Tree search → panels in draw order.");
}
