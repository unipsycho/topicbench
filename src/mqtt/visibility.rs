//! Compositor visibility hint — do not wait for egui `Focused` after a workspace switch.
//!
//! On Hyprland, leaving a workspace can leave egui `focused=true` until keyboard leave
//! is processed. Meanwhile `request_repaint` + vsync `swap_buffers` blocks the event
//! loop from ponging `xdg_wm_base` pings → ANR dialog (often noticed on the next desktop).

use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

/// Poll Hyprland (when present) and clear `ui_visible` while our window is off the
/// active workspace. Also clears `ui_focused` immediately so the inbox drops to 1 Hz
/// without waiting for egui `Focused(false)`. No-op outside Hyprland.
pub fn spawn_hyprland_visibility_watcher(
    ui_visible: Arc<AtomicBool>,
    ui_focused: Arc<AtomicBool>,
) {
    if std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_none() {
        return;
    }
    let pid = std::process::id();
    let _ = thread::Builder::new()
        .name("hypr-visible".into())
        .spawn(move || {
            // Start assuming visible; first poll corrects quickly.
            ui_visible.store(true, Ordering::Relaxed);
            loop {
                thread::sleep(Duration::from_millis(100));
                match window_on_active_workspace(pid) {
                    Some(on_active) => {
                        ui_visible.store(on_active, Ordering::Relaxed);
                        if !on_active {
                            // Stop 16ms inbox wakes before egui processes keyboard leave.
                            ui_focused.store(false, Ordering::Relaxed);
                        }
                    }
                    None => {
                        // hyprctl failed (reload etc.) — do not force-hide.
                    }
                }
            }
        });
}

/// `Some(true)` if our PID’s window is on the active workspace.
fn window_on_active_workspace(pid: u32) -> Option<bool> {
    let active = hyprctl_json(&["activeworkspace"])?;
    let active_id = active.get("id")?.as_i64()?;
    let clients = hyprctl_json(&["clients"])?;
    let clients = clients.as_array()?;
    let mut saw = false;
    let mut on_active = false;
    for c in clients {
        if c.get("pid")?.as_u64()? != u64::from(pid) {
            continue;
        }
        saw = true;
        let ws = c.get("workspace")?.get("id")?.as_i64()?;
        if ws == active_id {
            on_active = true;
            break;
        }
    }
    if !saw {
        // Window not mapped yet.
        return Some(true);
    }
    Some(on_active)
}

fn hyprctl_json(args: &[&str]) -> Option<serde_json::Value> {
    let mut cmd = Command::new("hyprctl");
    cmd.arg("-j");
    for a in args {
        cmd.arg(a);
    }
    let out = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    serde_json::from_slice(&out.stdout).ok()
}
