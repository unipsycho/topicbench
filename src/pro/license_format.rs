//! Signed license file verify (ed25519) + machine-ID binding.
//!
//! Payload is all-or-nothing Pro: `licensed: true` + required `machine_id`
//! (+ optional expires/licensee). Legacy `entitlements` still mean full Pro.
//! Unbound licenses (missing/empty `machine_id`) are rejected.

use super::machine_id;
use base64::Engine;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Embedded ed25519 verify key (32 bytes).
pub const LICENSE_VERIFY_KEY: [u8; 32] = [
    0xbc, 0xc4, 0x43, 0xd3, 0x6a, 0x75, 0x72, 0xc4, 0xdd, 0xc5, 0xf1, 0xcf, 0x91, 0x4d, 0xb0,
    0xda, 0xad, 0x07, 0x9a, 0xf3, 0xc4, 0x7f, 0x8f, 0x33, 0x84, 0x1c, 0x46, 0x51, 0x64, 0x35,
    0x86, 0xf3,
];

pub const LICENSE_FILE_NAME: &str = "license.json";
pub const LICENSE_FILE_ALT: &str = "license.lic";
/// Unsigned customer request (same shape Pro CLI writes) — support / re-bind.
pub const LICENSE_REQUEST_FILE_NAME: &str = "license-request.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LicenseFileV1 {
    pub v: u32,
    /// Exact UTF-8 JSON string that was signed (do not re-serialize before verify).
    pub payload: String,
    /// Standard base64 of the 64-byte ed25519 signature.
    pub sig: String,
}

/// Inner payload after signature checks out.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LicensePayload {
    /// Full Pro unlock when true (current format).
    #[serde(default)]
    pub licensed: bool,
    /// Legacy per-feature list — presence of any entry means full Pro.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub entitlements: Vec<String>,
    /// Required machine binding (`tb1:` + sha256 hex). Missing = reject.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub machine_id: Option<String>,
    /// Optional `YYYY-MM-DD` (UTC). Missing = no expiry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub licensee: Option<String>,
}

impl LicensePayload {
    /// True when this payload grants full Pro (new or legacy).
    pub fn grants_pro(&self) -> bool {
        self.licensed || !self.entitlements.is_empty()
    }
}

#[derive(Debug)]
pub enum LicenseError {
    Io(String),
    Json(String),
    Version,
    Signature,
    BadKey,
    Expired,
    NotLicensed,
    /// License `machine_id` missing or does not match this host.
    MachineMismatch,
}

impl std::fmt::Display for LicenseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(s) => write!(f, "io: {s}"),
            Self::Json(s) => write!(f, "json: {s}"),
            Self::Version => write!(f, "unsupported license version"),
            Self::Signature => write!(f, "invalid signature"),
            Self::BadKey => write!(f, "bad verify/signing key"),
            Self::Expired => write!(f, "license expired"),
            Self::NotLicensed => write!(f, "payload does not grant Pro"),
            Self::MachineMismatch => write!(f, "license not valid for this machine"),
        }
    }
}

/// Verify bytes of a license file against the embedded public key.
pub fn verify_license_bytes(bytes: &[u8]) -> Result<LicensePayload, LicenseError> {
    verify_license_bytes_with_key(bytes, &LICENSE_VERIFY_KEY)
}

/// Verify with an explicit public key (tests / key rotation checks).
pub fn verify_license_bytes_with_key(
    bytes: &[u8],
    verify_key: &[u8; 32],
) -> Result<LicensePayload, LicenseError> {
    let file: LicenseFileV1 =
        serde_json::from_slice(bytes).map_err(|e| LicenseError::Json(e.to_string()))?;
    if file.v != 1 {
        return Err(LicenseError::Version);
    }
    let vk = VerifyingKey::from_bytes(verify_key).map_err(|_| LicenseError::BadKey)?;
    let sig_bytes = base64::engine::general_purpose::STANDARD
        .decode(file.sig.trim())
        .map_err(|_| LicenseError::Signature)?;
    let sig = Signature::from_slice(&sig_bytes).map_err(|_| LicenseError::Signature)?;
    vk.verify(file.payload.as_bytes(), &sig)
        .map_err(|_| LicenseError::Signature)?;
    let payload: LicensePayload =
        serde_json::from_str(&file.payload).map_err(|e| LicenseError::Json(e.to_string()))?;
    if !payload.grants_pro() {
        return Err(LicenseError::NotLicensed);
    }
    if let Some(ref exp) = payload.expires {
        if is_expired(exp)? {
            return Err(LicenseError::Expired);
        }
    }
    // Require machine binding — unbound (v1-without-id) licenses no longer unlock.
    let Some(ref licensed_mid) = payload.machine_id else {
        return Err(LicenseError::MachineMismatch);
    };
    if licensed_mid.trim().is_empty() {
        return Err(LicenseError::MachineMismatch);
    }
    let this_mid = machine_id::current_machine_id().map_err(|_| LicenseError::MachineMismatch)?;
    if licensed_mid.trim() != this_mid {
        return Err(LicenseError::MachineMismatch);
    }
    Ok(payload)
}

/// Load + verify from a path (missing file → None; invalid → Err).
pub fn load_license_path(path: &Path) -> Result<Option<LicensePayload>, LicenseError> {
    if !path.is_file() {
        return Ok(None);
    }
    let bytes = std::fs::read(path).map_err(|e| LicenseError::Io(e.to_string()))?;
    Ok(Some(verify_license_bytes(&bytes)?))
}

/// Unsigned license request (order-form paste is preferred; this is for support).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LicenseRequestV1 {
    pub v: u32,
    pub product: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub product_version: Option<String>,
    pub machine_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub licensee: Option<String>,
    pub requested_at: String,
}

impl LicenseRequestV1 {
    /// Build a request for this host (no private key — safe in OSS).
    pub fn for_this_machine(licensee: Option<String>) -> Result<Self, LicenseError> {
        let mid = machine_id::current_machine_id().map_err(LicenseError::Io)?;
        Ok(Self {
            v: 1,
            product: "topicbench-pro".into(),
            product_version: Some(env!("CARGO_PKG_VERSION").into()),
            machine_id: mid,
            licensee,
            requested_at: utc_rfc3339_now(),
        })
    }
}

/// Write pretty JSON request for support / re-bind (same shape as Pro CLI).
pub fn write_license_request(path: &Path, req: &LicenseRequestV1) -> Result<(), LicenseError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| LicenseError::Io(e.to_string()))?;
    }
    let json = serde_json::to_string_pretty(req).map_err(|e| LicenseError::Json(e.to_string()))?;
    std::fs::write(path, format!("{json}\n")).map_err(|e| LicenseError::Io(e.to_string()))
}

/// Parse `YYYY-MM-DD`; true if today (UTC) is after that day.
fn is_expired(expires: &str) -> Result<bool, LicenseError> {
    let parts: Vec<_> = expires.trim().split('-').collect();
    if parts.len() != 3 {
        return Err(LicenseError::Json(format!("bad expires date: {expires}")));
    }
    let y: i32 = parts[0]
        .parse()
        .map_err(|_| LicenseError::Json(format!("bad expires year: {expires}")))?;
    let m: u32 = parts[1]
        .parse()
        .map_err(|_| LicenseError::Json(format!("bad expires month: {expires}")))?;
    let d: u32 = parts[2]
        .parse()
        .map_err(|_| LicenseError::Json(format!("bad expires day: {expires}")))?;
    let (ty, tm, td) = utc_ymd_today();
    Ok((ty, tm, td) > (y, m, d))
}

fn utc_ymd_today() -> (i32, u32, u32) {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    // Civil from days since Unix epoch (Howard Hinnant algorithm).
    let days = secs.div_euclid(86_400) as i32;
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = (yoe as i32) + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

fn utc_rfc3339_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let (y, m, d) = utc_ymd_today();
    let day_secs = secs.rem_euclid(86_400) as u32;
    let hh = day_secs / 3600;
    let mm = (day_secs % 3600) / 60;
    let ss = day_secs % 60;
    format!("{y:04}-{m:02}-{d:02}T{hh:02}:{mm:02}:{ss:02}Z")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_bad_json() {
        assert!(verify_license_bytes(b"not json").is_err());
    }

    #[test]
    fn rejects_wrong_version() {
        let json = br#"{"v":99,"payload":"{}","sig":"AA=="}"#;
        assert!(matches!(
            verify_license_bytes(json),
            Err(LicenseError::Version)
        ));
    }

    #[test]
    fn grants_pro_new_and_legacy() {
        assert!(LicensePayload {
            licensed: true,
            entitlements: vec![],
            machine_id: Some("tb1:deadbeef".into()),
            expires: None,
            licensee: None,
        }
        .grants_pro());
        assert!(LicensePayload {
            licensed: false,
            entitlements: vec!["multi_graph".into()],
            machine_id: None,
            expires: None,
            licensee: None,
        }
        .grants_pro());
        assert!(!LicensePayload {
            licensed: false,
            entitlements: vec![],
            machine_id: None,
            expires: None,
            licensee: None,
        }
        .grants_pro());
    }
}
