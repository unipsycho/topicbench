//! XDG-style config directory for TopicBench (`~/.config/topicbench`).
//!
//! On Windows the app is portable: default config root is the process **cwd**
//! so files live at `.\topicbench\` next to where you launch (same place
//! `license-request` writes). `%HOME%` is ignored on Windows (Git Bash often
//! sets it); use `$XDG_CONFIG_HOME` to override.

use std::fs;
use std::path::{Path, PathBuf};

/// Product config folder name (under XDG config / `~/.config`).
pub const CONFIG_DIR_NAME: &str = "topicbench";
/// Pre-rebrand folder; migrated once into [`CONFIG_DIR_NAME`].
const LEGACY_CONFIG_DIR_NAME: &str = "mqtt-monitor";

/// `$XDG_CONFIG_HOME`, else `$HOME/.config` (Unix), else `.` (Windows portable cwd).
pub fn config_home() -> PathBuf {
    if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME") {
        return PathBuf::from(xdg);
    }
    #[cfg(windows)]
    {
        let _ = std::env::var_os("HOME"); // intentionally unused — portable cwd wins
        return PathBuf::from(".");
    }
    #[cfg(not(windows))]
    {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(".config");
        }
        PathBuf::from(".")
    }
}

/// `…/topicbench` — creates nothing; call [`migrate_legacy_config_dir`] first at startup.
pub fn config_dir() -> PathBuf {
    config_home().join(CONFIG_DIR_NAME)
}

/// File under the TopicBench config dir.
pub fn config_file(name: &str) -> PathBuf {
    config_dir().join(name)
}

/// Absolute-ish display path (join cwd when relative; file need not exist).
pub fn abs_display(path: &Path) -> String {
    if path.is_absolute() {
        return path.display().to_string();
    }
    match std::env::current_dir() {
        Ok(cwd) => cwd.join(path).display().to_string(),
        Err(_) => path.display().to_string(),
    }
}

/// Directories to probe for `license.json` / `license.lic`.
///
/// Always includes cwd `.\topicbench\` first on Windows-style portable layouts,
/// then [`config_dir`], then `%USERPROFILE%\.config\topicbench`.
pub fn license_search_dirs() -> Vec<PathBuf> {
    let primary = config_dir();
    let mut dirs = vec![primary.clone()];
    let cwd_portable = PathBuf::from(".").join(CONFIG_DIR_NAME);
    if cwd_portable != primary {
        dirs.insert(0, cwd_portable);
    }
    if let Some(up) = std::env::var_os("USERPROFILE") {
        let legacy = PathBuf::from(up).join(".config").join(CONFIG_DIR_NAME);
        if !dirs.iter().any(|d| d == &legacy) {
            dirs.push(legacy);
        }
    }
    dirs
}

/// If `topicbench` is missing and legacy `mqtt-monitor` exists, rename (or copy) into place.
pub fn migrate_legacy_config_dir() {
    let home = config_home();
    let new_dir = home.join(CONFIG_DIR_NAME);
    let old_dir = home.join(LEGACY_CONFIG_DIR_NAME);
    if new_dir.exists() || !old_dir.exists() {
        return;
    }
    if fs::rename(&old_dir, &new_dir).is_ok() {
        return;
    }
    // Cross-device rename failed — copy then remove old.
    if copy_dir_recursive(&old_dir, &new_dir).is_ok() {
        let _ = fs::remove_dir_all(&old_dir);
    }
}

fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let to = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_recursive(&entry.path(), &to)?;
        } else {
            fs::copy(entry.path(), &to)?;
        }
    }
    Ok(())
}
