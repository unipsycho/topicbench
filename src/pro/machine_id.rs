//! Stable TopicBench machine ID (honest-user license binding).
//!
//! Scheme `tb1:` = SHA-256 (lowercase hex) of one OS identity string:
//! - Linux: `/etc/machine-id`, else `/var/lib/dbus/machine-id`
//! - Windows: `HKLM\SOFTWARE\Microsoft\Cryptography` → `MachineGuid`
//!
//! Not used: MAC, disk serial, SMB/NetBIOS name, motherboard serial.

use sha2::{Digest, Sha256};
use std::io;

/// Prefix marking machine-id scheme version 1.
pub const MACHINE_ID_PREFIX: &str = "tb1:";

/// Compute this host's TopicBench machine ID (`tb1:` + 64 hex).
pub fn current_machine_id() -> Result<String, String> {
    let raw = raw_os_machine_identity().map_err(|e| e.to_string())?;
    Ok(format_machine_id(&raw))
}

/// Hash raw OS identity → canonical `tb1:…` string.
pub fn format_machine_id(raw: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(raw.trim().as_bytes());
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(64);
    for b in digest {
        hex.push_str(&format!("{b:02x}"));
    }
    format!("{MACHINE_ID_PREFIX}{hex}")
}

fn raw_os_machine_identity() -> io::Result<String> {
    #[cfg(target_os = "linux")]
    {
        linux_machine_identity()
    }
    #[cfg(target_os = "windows")]
    {
        windows_machine_identity()
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "machine ID only supported on Linux and Windows",
        ))
    }
}

#[cfg(target_os = "linux")]
fn linux_machine_identity() -> io::Result<String> {
    for path in ["/etc/machine-id", "/var/lib/dbus/machine-id"] {
        match std::fs::read_to_string(path) {
            Ok(s) => {
                let t = s.trim();
                if !t.is_empty() {
                    return Ok(t.to_string());
                }
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::NotFound,
        "no /etc/machine-id or /var/lib/dbus/machine-id",
    ))
}

#[cfg(target_os = "windows")]
fn windows_machine_identity() -> io::Result<String> {
    use winreg::enums::HKEY_LOCAL_MACHINE;
    use winreg::RegKey;
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let key = hklm
        .open_subkey(r"SOFTWARE\Microsoft\Cryptography")
        .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("Cryptography key: {e}")))?;
    let guid: String = key
        .get_value("MachineGuid")
        .map_err(|e| io::Error::new(io::ErrorKind::NotFound, format!("MachineGuid: {e}")))?;
    let t = guid.trim();
    if t.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "MachineGuid empty",
        ));
    }
    Ok(t.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_is_stable_and_prefixed() {
        let a = format_machine_id("abc\n");
        let b = format_machine_id("abc");
        assert_eq!(a, b);
        assert!(a.starts_with(MACHINE_ID_PREFIX));
        assert_eq!(a.len(), MACHINE_ID_PREFIX.len() + 64);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_current_succeeds() {
        let id = current_machine_id().expect("machine id");
        assert!(id.starts_with("tb1:"));
        assert_eq!(id.len(), 4 + 64);
    }
}
