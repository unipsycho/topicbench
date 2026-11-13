//! Download release asset, verify SHA-256, replace running binary, relaunch.
//!
//! On Unix, if the executable directory is not writable (e.g. `/usr/bin` from a
//! system-wide copy), stage the payload in a temp dir and elevate via `pkexec`
//! (GUI polkit prompt) or non-interactive `sudo -n` to perform the atomic
//! rename dance while preserving owner/mode. Release-page fallback remains when
//! elevation is refused or unavailable. `.deb`/`apt` is not published on
//! Releases, so there is no package-aware automatic path — elevated binary
//! replace is the intended fix for root-owned install locations.

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

/// How the Unix replace path will apply the new binary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplaceMethod {
    /// Executable directory is writable — rename dance in place.
    Direct,
    /// Need root to write beside the binary (e.g. `/usr/bin`).
    Elevated,
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

/// True when `dir` allows creating a new file (probe + remove).
pub fn dir_is_writable(dir: &Path) -> bool {
    let probe = dir.join(format!(".topicbench-write-test-{}", std::process::id()));
    match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe)
    {
        Ok(_) => {
            let _ = fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

/// Choose direct vs elevated replace from the executable's parent directory.
pub fn replace_method_for_exe(exe: &Path) -> ReplaceMethod {
    let dir = exe.parent().unwrap_or_else(|| Path::new("."));
    if dir_is_writable(dir) {
        ReplaceMethod::Direct
    } else {
        ReplaceMethod::Elevated
    }
}

#[cfg(target_os = "windows")]
fn replace_and_relaunch(exe: &Path, new_bytes: &[u8]) -> Result<(), String> {
    let dir = exe.parent().unwrap_or_else(|| Path::new("."));
    let staging = dir.join(format!(
        "{}.new",
        exe.file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("topicbench")
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
    match replace_method_for_exe(exe) {
        ReplaceMethod::Direct => {
            if let Err(e) = replace_direct(exe, new_bytes) {
                // Probe can race with permissions; retry via elevation on EACCES-style errors.
                if looks_like_permission_denied(&e) {
                    replace_elevated(exe, new_bytes)?;
                } else {
                    return Err(e);
                }
            }
        }
        ReplaceMethod::Elevated => replace_elevated(exe, new_bytes)?,
    }
    spawn_relaunch(exe)?;
    std::process::exit(0);
}

#[cfg(not(target_os = "windows"))]
fn replace_direct(exe: &Path, new_bytes: &[u8]) -> Result<(), String> {
    let dir = exe.parent().unwrap_or_else(|| Path::new("."));
    let staging = dir.join(format!(
        "{}.new",
        exe.file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("topicbench")
    ));
    let backup = backup_path(exe);
    write_bytes(&staging, new_bytes)?;
    set_executable(&staging)?;

    if backup.exists() {
        let _ = fs::remove_file(&backup);
    }
    // On Linux, renaming over a running binary works; keep a `.bak` for rollback.
    fs::rename(exe, &backup).map_err(|e| format!("rename current→backup: {e}"))?;
    if let Err(e) = fs::rename(&staging, exe) {
        let _ = fs::rename(&backup, exe);
        return Err(format!("rename staging→current: {e}"));
    }
    Ok(())
}

/// Stage in `$TMPDIR`, then `pkexec` / `sudo -n` to swap into a root-owned path.
#[cfg(not(target_os = "windows"))]
fn replace_elevated(exe: &Path, new_bytes: &[u8]) -> Result<(), String> {
    let tmp = std::env::temp_dir().join(format!(
        "topicbench-update-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    write_bytes(&tmp, new_bytes)?;
    set_executable(&tmp)?;

    let exe_s = exe.to_string_lossy().into_owned();
    let tmp_s = tmp.to_string_lossy().into_owned();
    // GNU coreutils `stat` (Debian/Ubuntu). Preserve owner/mode across the swap.
    let script = r#"
set -e
SRC="$1"
DST="$2"
BAK="${DST}.bak"
NEW="${DST}.new"
OWNER=$(stat -c '%u:%g' "$DST")
MODE=$(stat -c '%a' "$DST")
cp -f "$SRC" "$NEW"
chmod "$MODE" "$NEW"
chown "$OWNER" "$NEW"
mv -f "$DST" "$BAK"
mv -f "$NEW" "$DST"
rm -f "$SRC"
"#;

    run_elevated(script, &tmp_s, &exe_s).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        e
    })?;
    Ok(())
}

/// Match os/fs permission failures (and our `create …: Permission denied` strings).
pub fn looks_like_permission_denied(err: &str) -> bool {
    let lower = err.to_ascii_lowercase();
    lower.contains("permission denied")
        || lower.contains("os error 13")
        || lower.contains("read-only file system")
}

/// Run `script` as root with `$1=src`, `$2=dst`. Prefer pkexec (polkit GUI).
#[cfg(not(target_os = "windows"))]
fn run_elevated(script: &str, src: &str, dst: &str) -> Result<(), String> {
    if command_exists("pkexec") {
        let out = Command::new("pkexec")
            .arg("/bin/sh")
            .arg("-c")
            .arg(script)
            .arg("topicbench-elevate")
            .arg(src)
            .arg(dst)
            .output()
            .map_err(|e| format!("pkexec: {e}"))?;
        if out.status.success() {
            return Ok(());
        }
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        let code = out.status.code().unwrap_or(-1);
        // User dismissed the polkit dialog, or policy denied — don't try sudo noise.
        if code == 126 || code == 127 || stderr.to_lowercase().contains("dismissed") {
            return Err(format!(
                "elevation refused or unavailable (pkexec exit {code}{}) — authorize the prompt or install manually",
                if stderr.is_empty() {
                    String::new()
                } else {
                    format!(": {stderr}")
                }
            ));
        }
        // Fall through to sudo -n when pkexec exists but failed for another reason.
        if !stderr.is_empty() {
            // Keep trying sudo below; remember pkexec detail if sudo also fails.
            if let Err(sudo_err) = run_sudo_n(script, src, dst) {
                return Err(format!(
                    "elevated install failed (pkexec: {stderr}; {sudo_err})"
                ));
            }
            return Ok(());
        }
    }

    run_sudo_n(script, src, dst)
}

#[cfg(not(target_os = "windows"))]
fn run_sudo_n(script: &str, src: &str, dst: &str) -> Result<(), String> {
    if !command_exists("sudo") {
        return Err(
            "install directory not writable and neither pkexec nor sudo is available — open the release page to update manually"
                .into(),
        );
    }
    let out = Command::new("sudo")
        .arg("-n")
        .arg("/bin/sh")
        .arg("-c")
        .arg(script)
        .arg("topicbench-elevate")
        .arg(src)
        .arg(dst)
        .output()
        .map_err(|e| format!("sudo: {e}"))?;
    if out.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
    Err(format!(
        "elevation refused or unavailable (sudo -n{}) — authorize pkexec/sudo or install manually",
        if stderr.is_empty() {
            String::new()
        } else {
            format!(": {stderr}")
        }
    ))
}

#[cfg(not(target_os = "windows"))]
fn command_exists(name: &str) -> bool {
    Command::new("sh")
        .arg("-c")
        .arg(format!("command -v {name} >/dev/null 2>&1"))
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[cfg(unix)]
fn set_executable(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = fs::metadata(path)
        .map_err(|e| format!("stat staging: {e}"))?
        .permissions();
    perms.set_mode(0o755);
    fs::set_permissions(path, perms).map_err(|e| format!("chmod staging: {e}"))
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
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn parse_sha_sums() {
        let text = "\
# comment
aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa  topicbench-linux-x64
bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb *topicbench-windows-x64.exe
";
        assert_eq!(
            parse_sha256_for_asset(text, "topicbench-linux-x64").as_deref(),
            Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
        );
        assert_eq!(
            parse_sha256_for_asset(text, "topicbench-windows-x64.exe").as_deref(),
            Some("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb")
        );
        assert!(parse_sha256_for_asset(text, "missing").is_none());
    }

    #[test]
    fn writable_temp_dir_uses_direct_replace() {
        let dir = std::env::temp_dir().join(format!(
            "topicbench-writetest-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("topicbench");
        fs::write(&exe, b"old").unwrap();
        assert!(dir_is_writable(&dir));
        assert_eq!(replace_method_for_exe(&exe), ReplaceMethod::Direct);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn nonwritable_dir_selects_elevated() {
        // `/usr/bin` is root-owned on typical Linux CI/dev images; skip if somehow writable.
        let usr_bin = Path::new("/usr/bin");
        if !usr_bin.is_dir() || dir_is_writable(usr_bin) {
            return;
        }
        let exe = usr_bin.join("topicbench");
        assert_eq!(replace_method_for_exe(&exe), ReplaceMethod::Elevated);
    }

    #[test]
    fn permission_denied_detection() {
        assert!(looks_like_permission_denied(
            "create /usr/bin/topicbench.new: Permission denied (os error 13)"
        ));
        assert!(looks_like_permission_denied(
            "rename current→backup: os error 13"
        ));
        assert!(!looks_like_permission_denied(
            "SHA-256 mismatch (got aaa, expected bbb)"
        ));
    }
}
