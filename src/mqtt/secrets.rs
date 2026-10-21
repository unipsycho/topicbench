//! Profile passwords in the OS credential store (not in profiles.json).
//!
//! Linux: Secret Service · Windows: Credential Manager · macOS: Keychain.

use keyring::Entry;

const SERVICE: &str = "topicbench";
/// Pre-rebrand keyring service; read + migrate into [`SERVICE`].
const LEGACY_SERVICE: &str = "mqtt-monitor";

fn entry_for(service: &str, profile_id: &str) -> Result<Entry, String> {
    Entry::new(service, &format!("profile:{profile_id}")).map_err(|e| e.to_string())
}

fn entry(profile_id: &str) -> Result<Entry, String> {
    entry_for(SERVICE, profile_id)
}

/// Read password for a profile, if present (migrates legacy service on hit).
pub fn load_password(profile_id: &str) -> Result<Option<String>, String> {
    match entry(profile_id)?.get_password() {
        Ok(pw) => Ok(Some(pw)),
        Err(keyring::Error::NoEntry) => load_and_migrate_legacy(profile_id),
        Err(e) => Err(e.to_string()),
    }
}

fn load_and_migrate_legacy(profile_id: &str) -> Result<Option<String>, String> {
    let legacy = entry_for(LEGACY_SERVICE, profile_id)?;
    match legacy.get_password() {
        Ok(pw) => {
            let _ = entry(profile_id)?.set_password(&pw);
            let _ = legacy.delete_credential();
            Ok(Some(pw))
        }
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

/// Persist or clear password for a profile.
/// Empty password deletes the keyring entry (if any).
pub fn store_password(profile_id: &str, password: &str) -> Result<(), String> {
    let e = entry(profile_id)?;
    if password.is_empty() {
        // Clear both new and legacy slots.
        let _ = entry_for(LEGACY_SERVICE, profile_id).and_then(|leg| {
            match leg.delete_credential() {
                Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
                Err(err) => Err(err.to_string()),
            }
        });
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
