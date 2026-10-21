//! Profile passwords in the OS credential store (not in profiles.json).
//!
//! Linux: Secret Service · Windows: Credential Manager · macOS: Keychain.

use keyring::Entry;

const SERVICE: &str = "mqtt-monitor";

fn entry(profile_id: &str) -> Result<Entry, String> {
    Entry::new(SERVICE, &format!("profile:{profile_id}")).map_err(|e| e.to_string())
}

/// Read password for a profile, if present.
pub fn load_password(profile_id: &str) -> Result<Option<String>, String> {
    match entry(profile_id)?.get_password() {
        Ok(pw) => Ok(Some(pw)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

/// Persist or clear password for a profile.
/// Empty password deletes the keyring entry (if any).
pub fn store_password(profile_id: &str, password: &str) -> Result<(), String> {
    let e = entry(profile_id)?;
    if password.is_empty() {
        match e.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(err) => Err(err.to_string()),
        }
    } else {
        e.set_password(password).map_err(|err| err.to_string())
    }
}

/// Drop keyring entry when a profile is deleted.
pub fn delete_password(profile_id: &str) {
    let _ = store_password(profile_id, "");
}
