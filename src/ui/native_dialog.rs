//! Native OS file/message dialogs without blocking the egui/winit thread.
//!
//! On Linux, rfd's sync API (`FileDialog::pick_file`, `MessageDialog::show`, …)
//! runs `pollster::block_on` on the xdg-desktop-portal future. Calling that
//! inside `App::update` freezes Wayland/X11 dispatch, so compositors (Hyprland
//! `xdg_wm_base` ping) show “application is hung”.
//!
//! Pattern: spawn the blocking rfd call on a worker thread, wake egui when done,
//! and `poll_*` from the UI each frame.

use eframe::egui;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::{Mutex, OnceLock};

/// Which path dialog to open on the worker.
#[derive(Clone, Copy, Debug)]
pub enum PathOp {
    PickFile,
    PickFolder,
    SaveFile,
}

fn path_rx() -> &'static Mutex<HashMap<egui::Id, Receiver<Option<PathBuf>>>> {
    static M: OnceLock<Mutex<HashMap<egui::Id, Receiver<Option<PathBuf>>>>> = OnceLock::new();
    M.get_or_init(|| Mutex::new(HashMap::new()))
}

fn msg_rx() -> &'static Mutex<HashMap<egui::Id, Receiver<rfd::MessageDialogResult>>> {
    static M: OnceLock<Mutex<HashMap<egui::Id, Receiver<rfd::MessageDialogResult>>>> =
        OnceLock::new();
    M.get_or_init(|| Mutex::new(HashMap::new()))
}

/// True while a path dialog for `id` is still open.
pub fn path_pending(id: impl Into<egui::Id>) -> bool {
    path_rx()
        .lock()
        .map(|m| m.contains_key(&id.into()))
        .unwrap_or(false)
}

/// True while a message dialog for `id` is still open.
pub fn msg_pending(id: impl Into<egui::Id>) -> bool {
    msg_rx()
        .lock()
        .map(|m| m.contains_key(&id.into()))
        .unwrap_or(false)
}

/// Start a path dialog on a worker thread (no-op if `id` already pending).
pub fn begin_path(
    ctx: &egui::Context,
    id: impl Into<egui::Id>,
    op: PathOp,
    build: impl FnOnce(rfd::FileDialog) -> rfd::FileDialog + Send + 'static,
) {
    let id = id.into();
    {
        let Ok(map) = path_rx().lock() else {
            return;
        };
        if map.contains_key(&id) {
            return;
        }
    }
    let dialog = build(rfd::FileDialog::new());
    let (tx, rx) = mpsc::channel();
    let wake = ctx.clone();
    let _ = std::thread::Builder::new()
        .name("topicbench-rfd-path".into())
        .spawn(move || {
            let path = match op {
                PathOp::PickFile => dialog.pick_file(),
                PathOp::PickFolder => dialog.pick_folder(),
                PathOp::SaveFile => dialog.save_file(),
            };
            let _ = tx.send(path);
            wake.request_repaint();
        });
    if let Ok(mut map) = path_rx().lock() {
        map.insert(id, rx);
    }
}

/// Poll a path dialog started with [`begin_path`].
///
/// - `None` — idle, or still waiting
/// - `Some(None)` — cancelled / no selection
/// - `Some(Some(path))` — user chose a path
pub fn poll_path(ctx: &egui::Context, id: impl Into<egui::Id>) -> Option<Option<PathBuf>> {
    let id = id.into();
    let Ok(mut map) = path_rx().lock() else {
        return None;
    };
    let Some(rx) = map.get(&id) else {
        return None;
    };
    match rx.try_recv() {
        Ok(v) => {
            map.remove(&id);
            Some(v)
        }
        Err(TryRecvError::Empty) => {
            // Keep a light heartbeat so focus/throttle logic stays alive.
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
            None
        }
        Err(TryRecvError::Disconnected) => {
            map.remove(&id);
            Some(None)
        }
    }
}

/// Start a message dialog on a worker thread (no-op if `id` already pending).
pub fn begin_message(
    ctx: &egui::Context,
    id: impl Into<egui::Id>,
    build: impl FnOnce(rfd::MessageDialog) -> rfd::MessageDialog + Send + 'static,
) {
    let id = id.into();
    {
        let Ok(map) = msg_rx().lock() else {
            return;
        };
        if map.contains_key(&id) {
            return;
        }
    }
    let dialog = build(rfd::MessageDialog::new());
    let (tx, rx) = mpsc::channel();
    let wake = ctx.clone();
    let _ = std::thread::Builder::new()
        .name("topicbench-rfd-msg".into())
        .spawn(move || {
            let result = dialog.show();
            let _ = tx.send(result);
            wake.request_repaint();
        });
    if let Ok(mut map) = msg_rx().lock() {
        map.insert(id, rx);
    }
}

/// Poll a message dialog started with [`begin_message`].
pub fn poll_message(
    ctx: &egui::Context,
    id: impl Into<egui::Id>,
) -> Option<rfd::MessageDialogResult> {
    let id = id.into();
    let Ok(mut map) = msg_rx().lock() else {
        return None;
    };
    let Some(rx) = map.get(&id) else {
        return None;
    };
    match rx.try_recv() {
        Ok(v) => {
            map.remove(&id);
            Some(v)
        }
        Err(TryRecvError::Empty) => {
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
            None
        }
        Err(TryRecvError::Disconnected) => {
            map.remove(&id);
            None
        }
    }
}

/// Map Replace/Merge/Cancel custom buttons (Yes=Replace, No=Merge) to a mode tag.
pub fn replace_merge_mode(r: rfd::MessageDialogResult) -> Option<&'static str> {
    match r {
        rfd::MessageDialogResult::Yes => Some("replace"),
        rfd::MessageDialogResult::No => Some("merge"),
        rfd::MessageDialogResult::Custom(s) if s.eq_ignore_ascii_case("replace") => Some("replace"),
        rfd::MessageDialogResult::Custom(s) if s.eq_ignore_ascii_case("merge") => Some("merge"),
        _ => None,
    }
}
