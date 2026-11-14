//! Commit-on-blur text edits for MQTT topic fields.
//!
//! Typing must never mark a row dirty / upsert / publish. A one-frame pending
//! confirm after `lost_focus` ignores transient focus flicker between keystrokes
//! (the failure mode that let #41 still drip partial topic roots).

use eframe::egui;

/// Pure state for topic commit-on-blur (unit-tested without egui).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BlurCommitState {
    pub buffer: Option<String>,
    pub pending: Option<String>,
}

/// Result of one interaction step against a topic field.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlurCommitOutcome {
    pub state: BlurCommitState,
    /// When set, write into the committed topic and persist/upsert.
    pub commit: Option<String>,
}

/// Advance commit-on-blur state for one frame.
///
/// - Focused keystrokes update the buffer only (never commit).
/// - `lost_focus` arms a pending commit; a following focused frame cancels it
///   (focus flicker). A following stable unfocused frame commits once.
/// - Enter commits immediately.
pub fn step_blur_commit(
    state: &BlurCommitState,
    committed: &str,
    widget_text: &str,
    has_focus: bool,
    lost_focus: bool,
    enter: bool,
) -> BlurCommitOutcome {
    if has_focus {
        return BlurCommitOutcome {
            state: BlurCommitState {
                buffer: Some(widget_text.to_string()),
                pending: None,
            },
            commit: None,
        };
    }
    if enter {
        let commit = if widget_text != committed {
            Some(widget_text.to_string())
        } else {
            None
        };
        return BlurCommitOutcome {
            state: BlurCommitState {
                buffer: None,
                pending: None,
            },
            commit,
        };
    }
    if lost_focus {
        return BlurCommitOutcome {
            state: BlurCommitState {
                buffer: Some(widget_text.to_string()),
                pending: Some(widget_text.to_string()),
            },
            commit: None,
        };
    }
    if let Some(pending) = &state.pending {
        let commit = if pending.as_str() != committed {
            Some(pending.clone())
        } else {
            None
        };
        return BlurCommitOutcome {
            state: BlurCommitState {
                buffer: None,
                pending: None,
            },
            commit,
        };
    }
    BlurCommitOutcome {
        state: BlurCommitState {
            buffer: state.buffer.clone(),
            pending: None,
        },
        commit: None,
    }
}

/// Single-line topic TextEdit that commits only on confirmed blur or Enter.
///
/// Uses an absolute widget id so parent layout/clip remounts cannot change the
/// focus identity mid-edit. Returns `true` when `committed` changed.
pub fn commit_on_blur_text_edit(
    ui: &mut egui::Ui,
    id_key: impl std::hash::Hash,
    committed: &mut String,
    desired_width: f32,
    hint: &str,
) -> bool {
    let edit_id = egui::Id::new(("tb_commit_edit", &id_key));
    let buf_id = egui::Id::new(("tb_commit_buf", &id_key));
    let pending_id = egui::Id::new(("tb_commit_pending", &id_key));

    let prior = ui.ctx().data(|d| BlurCommitState {
        buffer: d.get_temp::<String>(buf_id),
        pending: d.get_temp::<String>(pending_id),
    });
    let mut text = prior
        .buffer
        .clone()
        .or_else(|| prior.pending.clone())
        .unwrap_or_else(|| committed.clone());

    let resp = ui.add(
        egui::TextEdit::singleline(&mut text)
            .id(edit_id)
            .desired_width(desired_width)
            .hint_text(hint),
    );

    // Enter on single-line surrenders focus; treat that as an immediate commit.
    let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
    let out = step_blur_commit(
        &prior,
        committed,
        &text,
        resp.has_focus(),
        resp.lost_focus(),
        enter,
    );

    ui.ctx().data_mut(|d| {
        match &out.state.buffer {
            Some(b) => d.insert_temp(buf_id, b.clone()),
            None => {
                d.remove_temp::<String>(buf_id);
            }
        }
        match &out.state.pending {
            Some(p) => d.insert_temp(pending_id, p.clone()),
            None => {
                d.remove_temp::<String>(pending_id);
            }
        }
    });

    if let Some(next) = out.commit {
        *committed = next;
        true
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keystrokes_while_focused_never_commit() {
        let mut state = BlurCommitState::default();
        let committed = "lab/temperature";
        for partial in ["tlab/temperature", "tolab/temperature", "toplab/temperature"] {
            let out = step_blur_commit(&state, committed, partial, true, false, false);
            assert!(out.commit.is_none(), "partial {partial} must not commit");
            assert_eq!(out.state.buffer.as_deref(), Some(partial));
            assert!(out.state.pending.is_none());
            state = out.state;
        }
    }

    #[test]
    fn focus_flicker_between_keys_does_not_commit() {
        let committed = "lab/temperature";
        let mut state = BlurCommitState::default();

        // Type one character.
        let out = step_blur_commit(&state, committed, "tlab/temperature", true, false, false);
        state = out.state;
        assert!(out.commit.is_none());

        // Spurious lost_focus (Wayland/compositor flicker).
        let out = step_blur_commit(&state, committed, "tlab/temperature", false, true, false);
        state = out.state;
        assert!(out.commit.is_none(), "lost_focus alone must only arm pending");
        assert!(state.pending.is_some());

        // Focus returns on the next keystroke frame — cancel pending.
        let out = step_blur_commit(&state, committed, "tolab/temperature", true, false, false);
        assert!(out.commit.is_none(), "flicker recovery must not commit");
        assert!(out.state.pending.is_none());
        assert_eq!(out.state.buffer.as_deref(), Some("tolab/temperature"));
    }

    #[test]
    fn confirmed_blur_commits_once() {
        let committed = "lab/temperature";
        let mut state = BlurCommitState::default();

        let out = step_blur_commit(&state, committed, "topicbench/lab/temperature", true, false, false);
        state = out.state;

        let out = step_blur_commit(
            &state,
            committed,
            "topicbench/lab/temperature",
            false,
            true,
            false,
        );
        state = out.state;
        assert!(out.commit.is_none());

        // Stable unfocused frame confirms the pending commit.
        let out = step_blur_commit(
            &state,
            committed,
            "topicbench/lab/temperature",
            false,
            false,
            false,
        );
        assert_eq!(
            out.commit.as_deref(),
            Some("topicbench/lab/temperature")
        );
        assert!(out.state.buffer.is_none());
        assert!(out.state.pending.is_none());
    }

    #[test]
    fn enter_commits_immediately() {
        let state = BlurCommitState {
            buffer: Some("topicbench/lab/temperature".into()),
            pending: None,
        };
        let out = step_blur_commit(
            &state,
            "lab/temperature",
            "topicbench/lab/temperature",
            false,
            true,
            true,
        );
        assert_eq!(
            out.commit.as_deref(),
            Some("topicbench/lab/temperature")
        );
        assert!(out.state.pending.is_none());
    }

    #[test]
    fn blur_to_same_value_does_not_signal_commit() {
        let state = BlurCommitState {
            buffer: Some("lab/temperature".into()),
            pending: Some("lab/temperature".into()),
        };
        let out = step_blur_commit(&state, "lab/temperature", "lab/temperature", false, false, false);
        assert!(out.commit.is_none());
    }
}
