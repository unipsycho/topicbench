//! TopicBench Pro license file format (v1) — signed offline blob.
//!
//! Shared by the OSS probe and the `topicbench-license` mint tool (`#[path]`).
//! Honest-user friction only; not unbreakable DRM.
//!
//! File (JSON), default path `~/.config/topicbench/license.json`:
//! ```json
//! {
//!   "v": 1,
//!   "payload": "{\"entitlements\":[\"multi_graph\"],\"expires\":\"2028-01-01\"}",
//!   "sig": "<base64 ed25519 signature over payload UTF-8 bytes>"
//! }
//! ```
//!
//! Private signing key never ships in the app tree. Settings → Activate (later)
//! will write this same file and re-probe; until then, install by copying the file.

use base64::Engine;
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Embedded ed25519 verify key (32 bytes). Pair lives outside the repo / gitignored
/// (`~/.config/topicbench/dev/ed25519.sk.hex`). Rotate via `topicbench-license keygen`
/// then paste the printed public hex here.
pub const LICENSE_VERIFY_KEY: [u8; 32] = [
    0xbc, 0xc4, 0x43, 0xd3, 0x6a, 0x75, 0x72, 0xc4, 0xdd, 0xc5, 0xf1, 0xcf, 0x91, 0x4d, 0xb0,
    0xda, 0xad, 0x07, 0x9a, 0xf3, 0xc4, 0x7f, 0x8f, 0x33, 0x84, 0x1c, 0x46, 0x51, 0x64, 0x35,
    0x86, 0xf3,
];

/// Default filename under `~/.config/topicbench/`.
pub const LICENSE_FILE_NAME: &str = "license.json";
/// Alternate extension (same JSON body).
pub const LICENSE_FILE_ALT: &str = "license.lic";

/// Outer envelope on disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LicenseFileV1 {
    pub v: u32,
    /// Exact UTF-8 JSON string that was signed (do not re-serialize before verify).
    pub payload: String,
    /// Standard base64 of the 64-byte ed25519 signature.
    pub sig: String,
}

/// Inner payload (parsed after signature checks out).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LicensePayload {
    pub entitlements: Vec<String>,
    /// Optional `YYYY-MM-DD` (UTC calendar day). Missing = no expiry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub licensee: Option<String>,
}

#[derive(Debug)]
pub enum LicenseError {
    Io(String),
    Json(String),
    Version,
    Signature,
    BadKey,
    Expired,
    Empty,
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
            Self::Empty => write!(f, "empty entitlements"),
        }
    }
}

/// Compact JSON for the payload (stable field order from struct derive).
pub fn payload_to_canonical_json(payload: &LicensePayload) -> Result<String, LicenseError> {
    serde_json::to_string(payload).map_err(|e| LicenseError::Json(e.to_string()))
}

/// Sign a payload with a signing key; return the on-disk JSON file text.
#[allow(dead_code)] // Used by `topicbench-license` mint bin (shared via #[path]).
pub fn mint_license_json(signing_key: &SigningKey, payload: &LicensePayload) -> Result<String, LicenseError> {
    let payload_str = payload_to_canonical_json(payload)?;
    let sig = signing_key.sign(payload_str.as_bytes());
    let file = LicenseFileV1 {
        v: 1,
        payload: payload_str,
        sig: base64::engine::general_purpose::STANDARD.encode(sig.to_bytes()),
    };
    serde_json::to_string_pretty(&file).map_err(|e| LicenseError::Json(e.to_string()))
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
    if payload.entitlements.is_empty() {
        return Err(LicenseError::Empty);
    }
    if let Some(ref exp) = payload.expires {
        if is_expired(exp)? {
            return Err(LicenseError::Expired);
        }
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

/// Hex-encode key bytes (lowercase).
#[allow(dead_code)] // Mint bin.
pub fn key_to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Decode a 32-byte key from hex (ignores whitespace).
#[allow(dead_code)] // Mint bin.
pub fn key_from_hex(hex: &str) -> Result<[u8; 32], LicenseError> {
    let clean: String = hex.chars().filter(|c| !c.is_whitespace()).collect();
    if clean.len() != 64 {
        return Err(LicenseError::BadKey);
    }
    let mut out = [0u8; 32];
    for i in 0..32 {
        out[i] = u8::from_str_radix(&clean[i * 2..i * 2 + 2], 16)
            .map_err(|_| LicenseError::BadKey)?;
    }
    Ok(out)
}

/// Load signing key from a hex file (64 hex chars).
#[allow(dead_code)] // Mint bin.
pub fn signing_key_from_hex_file(path: &Path) -> Result<SigningKey, LicenseError> {
    let text = std::fs::read_to_string(path).map_err(|e| LicenseError::Io(e.to_string()))?;
    let bytes = key_from_hex(text.trim())?;
    Ok(SigningKey::from_bytes(&bytes))
}

/// Generate a new keypair.
#[allow(dead_code)] // Mint bin + unit tests.
pub fn generate_keypair() -> SigningKey {
    let mut rng = rand::rngs::OsRng;
    SigningKey::generate(&mut rng)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_sign_verify() {
        let sk = generate_keypair();
        let vk = sk.verifying_key();
        let payload = LicensePayload {
            entitlements: vec!["multi_graph".into(), "mcp_server".into()],
            expires: Some("2099-01-01".into()),
            licensee: Some("dev".into()),
        };
        let json = mint_license_json(&sk, &payload).unwrap();
        let got = verify_license_bytes_with_key(json.as_bytes(), &vk.to_bytes()).unwrap();
        assert_eq!(got.entitlements, payload.entitlements);
        assert_eq!(got.expires, payload.expires);
    }

    #[test]
    fn rejects_tampered_payload() {
        let sk = generate_keypair();
        let vk = sk.verifying_key();
        let payload = LicensePayload {
            entitlements: vec!["multi_graph".into()],
            expires: None,
            licensee: None,
        };
        let mut json = mint_license_json(&sk, &payload).unwrap();
        json = json.replace("multi_graph", "mcp_server");
        assert!(verify_license_bytes_with_key(json.as_bytes(), &vk.to_bytes()).is_err());
    }
}
