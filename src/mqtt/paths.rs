//! XDG-style config directory for TopicBench (`~/.config/topicbench`).

use std::fs;
use std::path::PathBuf;

/// Product config folder name (under XDG config / `~/.config`).
pub const CONFIG_DIR_NAME: &str = "topicbench";
/// Pre-rebrand folder; migrated once into [`CONFIG_DIR_NAME`].
const LEGACY_CONFIG_DIR_NAME: &str = "mqtt-monitor";

/// `$XDG_CONFIG_HOME` or `~/.config` (fallback `.`).
pub fn config_home() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// `…/topicbench` — creates nothing; call [`migrate_legacy_config_dir`] first at startup.
pub fn config_dir() -> PathBuf {
    config_home().join(CONFIG_DIR_NAME)
}

/// File under the TopicBench config dir.
pub fn config_file(name: &str) -> PathBuf {
    config_dir().join(name)
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
