//! Download release asset, verify SHA-256, replace running binary, relaunch.

use super::github::{self, ReleaseInfo, ASSET_NAME, SHA256SUMS_NAME};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone)]
pub enum InstallOutcome {
    /// Binary replaced; caller should exit so the new process can take over (or already relaunched).
    ReplacedAndRelaunched,
    /// Assets/checksum missing or apply failed — opened the release page instead.
    OpenedBrowser { reason: String },
}

/// Prefer in-place install; fall back to opening the release HTML URL.
pub fn apply_update(info: &ReleaseInfo) -> InstallOutcome {
    match try_replace(info) {
        Ok(()) => InstallOutcome::ReplacedAndRelaunched,
        Err(reason) => {
            let open_err = github::open_release_page(&info.html_url)
                .err()
                .map(|e| format!(" (also couldn't open browser: {e})"))
                .unwrap_or_default();
            InstallOutcome::OpenedBrowser {
                reason: format!("{reason}{open_err}"),
            }
        }
    }
}

fn try_replace(info: &ReleaseInfo) -> Result<(), String> {
    let asset_url = info
        .asset_url
        .as_deref()
        .ok_or_else(|| format!("release has no asset `{ASSET_NAME}` — open page to download"))?;
    let sums_url = info.sha256sums_url.as_deref().ok_or_else(|| {
        format!("release has no `{SHA256SUMS_NAME}` — open page to download manually")
    })?;

    let bytes = github::download_bytes(asset_url)?;
    let sums_text = String::from_utf8(github::download_bytes(sums_url)?)
        .map_err(|e| format!("SHA256SUMS not UTF-8: {e}"))?;
    let expected = parse_sha256_for_asset(&sums_text, ASSET_NAME)
        .ok_or_else(|| format!("no SHA-256 line for `{ASSET_NAME}` in SHA256SUMS"))?;
    let actual = hex_sha256(&bytes);
    if !actual.eq_ignore_ascii_case(&expected) {
        return Err(format!(
            "SHA-256 mismatch (got {actual}, expected {expected})"
        ));
    }

    let exe = std::env::current_exe().map_err(|e| format!("current_exe: {e}"))?;
    let exe = fs::canonicalize(&exe).unwrap_or(exe);
    replace_and_relaunch(&exe, &bytes)
}

fn hex_sha256(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    let dig = hasher.finalize();
    dig.iter().map(|b| format!("{b:02x}")).collect()
}

/// Parse `SHA256SUMS` / `sha256sum` style lines for `asset_name`.
fn parse_sha256_for_asset(text: &str, asset_name: &str) -> Option<String> {
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        // "hash  filename" or "hash *filename"
        let mut parts = line.split_whitespace();
        let hash = parts.next()?;
        let name = parts.next()?.trim_start_matches('*');
        let base = Path::new(name)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(name);
        if base == asset_name || name == asset_name {
            if hash.len() == 64 && hash.chars().all(|c| c.is_ascii_hexdigit()) {
                return Some(hash.to_string());
            }
        }
    }
    None
}

#[cfg(target_os = "windows")]
fn replace_and_relaunch(exe: &Path, new_bytes: &[u8]) -> Result<(), String> {
    let dir = exe.parent().unwrap_or_else(|| Path::new("."));
    let staging = dir.join(format!(
        "{}.new",
        exe.file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("mqtt-master")
    ));
    let backup = backup_path(exe);
    write_bytes(&staging, new_bytes)?;

    // Rename dance: current → .old, staging → current.
    if backup.exists() {
        let _ = fs::remove_file(&backup);
    }
    fs::rename(exe, &backup).map_err(|e| format!("rename current→backup: {e}"))?;
    if let Err(e) = fs::rename(&staging, exe) {
        let _ = fs::rename(&backup, exe);
        return Err(format!("rename staging→current: {e}"));
    }

    spawn_relaunch(exe)?;
    // Exit so Windows can unlock; caller receives ReplacedAndRelaunched then quits.
    std::process::exit(0);
}

#[cfg(not(target_os = "windows"))]
fn replace_and_relaunch(exe: &Path, new_bytes: &[u8]) -> Result<(), String> {
    let dir = exe.parent().unwrap_or_else(|| Path::new("."));
    let staging = dir.join(format!(
        "{}.new",
        exe.file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("mqtt-master")
    ));
    let backup = backup_path(exe);
    write_bytes(&staging, new_bytes)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&staging)
            .map_err(|e| format!("stat staging: {e}"))?
            .permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&staging, perms).map_err(|e| format!("chmod staging: {e}"))?;
    }

    if backup.exists() {
        let _ = fs::remove_file(&backup);
    }
    // On Linux, renaming over a running binary works; keep a `.bak` for rollback.
    fs::rename(exe, &backup).map_err(|e| format!("rename current→backup: {e}"))?;
    if let Err(e) = fs::rename(&staging, exe) {
        let _ = fs::rename(&backup, exe);
        return Err(format!("rename staging→current: {e}"));
    }

    spawn_relaunch(exe)?;
    std::process::exit(0);
}

fn backup_path(exe: &Path) -> PathBuf {
    let mut p = exe.as_os_str().to_os_string();
    p.push(".bak");
    PathBuf::from(p)
}

fn write_bytes(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut f = fs::File::create(path).map_err(|e| format!("create {}: {e}", path.display()))?;
    f.write_all(bytes)
        .map_err(|e| format!("write {}: {e}", path.display()))?;
    f.sync_all()
        .map_err(|e| format!("sync {}: {e}", path.display()))?;
    Ok(())
}

fn spawn_relaunch(exe: &Path) -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    Command::new(exe)
        .args(&args)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("relaunch {}: {e}", exe.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_sha_sums() {
        let text = "\
# comment
aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa  mqtt-master-linux-x64
bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb *mqtt-master-windows-x64.exe
";
        assert_eq!(
            parse_sha256_for_asset(text, "mqtt-master-linux-x64").as_deref(),
            Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
        );
        assert_eq!(
            parse_sha256_for_asset(text, "mqtt-master-windows-x64.exe").as_deref(),
            Some("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb")
        );
        assert!(parse_sha256_for_asset(text, "missing").is_none());
    }
}
