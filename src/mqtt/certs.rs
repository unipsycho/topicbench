//! Discover CA + client cert + private key in a profile certs folder (AWS IoT–style).

use std::fs;
use std::path::{Path, PathBuf};

/// Paths resolved from a certs folder (display names + absolute paths).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedCerts {
    pub ca: PathBuf,
    pub client_cert: PathBuf,
    pub private_key: PathBuf,
}

impl ResolvedCerts {
    pub fn ca_name(&self) -> String {
        file_name(&self.ca)
    }
    pub fn cert_name(&self) -> String {
        file_name(&self.client_cert)
    }
    pub fn key_name(&self) -> String {
        file_name(&self.private_key)
    }

    /// Load PEM bytes for rumqttc `Transport::tls`.
    pub fn load_pem(&self) -> Result<(Vec<u8>, Vec<u8>, Vec<u8>), String> {
        let ca = fs::read(&self.ca).map_err(|e| format!("read CA {}: {e}", self.ca.display()))?;
        let cert = fs::read(&self.client_cert)
            .map_err(|e| format!("read client cert {}: {e}", self.client_cert.display()))?;
        let key = fs::read(&self.private_key)
            .map_err(|e| format!("read private key {}: {e}", self.private_key.display()))?;
        Ok((ca, cert, key))
    }
}

fn file_name(p: &Path) -> String {
    p.file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| p.display().to_string())
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PemKind {
    PrivateKey,
    Certificate,
    Unknown,
}

fn classify_pem(bytes: &[u8]) -> PemKind {
    let s = String::from_utf8_lossy(bytes);
    if s.contains("PRIVATE KEY") {
        PemKind::PrivateKey
    } else if s.contains("BEGIN CERTIFICATE") {
        PemKind::Certificate
    } else {
        PemKind::Unknown
    }
}

fn looks_like_cert_file(name: &str) -> bool {
    let n = name.to_lowercase();
    n.ends_with(".pem")
        || n.ends_with(".crt")
        || n.ends_with(".cer")
        || n.ends_with(".key")
        || n.ends_with(".pem.crt")
        || n.ends_with(".pem.key")
}

fn score_ca(name: &str) -> i32 {
    let n = name.to_lowercase();
    if n.contains("amazonrootca") || n.contains("root-ca") || n.contains("root_ca") {
        100
    } else if n.contains("rootca") || n == "ca.pem" || n == "root.pem" || n.starts_with("ca.") {
        90
    } else if n.contains("ca")
        && !n.contains("certificate")
        && !n.contains("private")
        && (n.ends_with(".pem") || n.ends_with(".crt") || n.ends_with(".cer"))
    {
        50
    } else {
        0
    }
}

fn score_client_cert(name: &str) -> i32 {
    let n = name.to_lowercase();
    if n.contains("certificate") && (n.ends_with(".crt") || n.ends_with(".pem") || n.contains(".pem."))
    {
        100
    } else if n.ends_with(".crt") || n.ends_with(".cer") {
        80
    } else if n.contains("cert")
        && n.ends_with(".pem")
        && !n.contains("ca")
        && !n.contains("key")
        && !n.contains("private")
    {
        60
    } else if n.ends_with(".pem") && !n.contains("ca") && !n.contains("key") && !n.contains("private")
    {
        20
    } else {
        0
    }
}

fn score_private_key(name: &str) -> i32 {
    let n = name.to_lowercase();
    if n.contains("public") {
        return -100;
    }
    if n.contains("private") && (n.ends_with(".key") || n.ends_with(".pem") || n.contains(".pem.")) {
        100
    } else if n.ends_with(".key") {
        80
    } else if n.contains("private") {
        70
    } else {
        0
    }
}

/// Pick CA, device certificate, and private key from `dir`.
///
/// Naming rules (AWS IoT–friendly):
/// - **CA:** `AmazonRootCA1.pem`, `*RootCA*`, `ca.pem`, or other `*ca*.pem` / `.crt`
/// - **Client cert:** `*-certificate.pem.crt`, `*.crt`, `*certificate*`
/// - **Private key:** `*-private.pem.key`, `*.key`, `*private*` (never `*public*`)
///
/// Content is used to separate private keys from certificates when names collide.
pub fn discover(dir: &Path) -> Result<ResolvedCerts, String> {
    if dir.as_os_str().is_empty() {
        return Err("Certs folder path is empty".into());
    }
    if !dir.is_dir() {
        return Err(format!("Certs folder not found: {}", dir.display()));
    }

    let mut entries: Vec<(PathBuf, String, PemKind)> = Vec::new();
    let rd = fs::read_dir(dir).map_err(|e| format!("read {}: {e}", dir.display()))?;
    for ent in rd {
        let ent = ent.map_err(|e| format!("read {}: {e}", dir.display()))?;
        let path = ent.path();
        if !path.is_file() {
            continue;
        }
        let name = file_name(&path);
        if !looks_like_cert_file(&name) {
            continue;
        }
        let bytes = fs::read(&path).unwrap_or_default();
        let kind = classify_pem(&bytes);
        entries.push((path, name, kind));
    }

    if entries.is_empty() {
        return Err(format!(
            "No .pem/.crt/.key files in {}",
            dir.display()
        ));
    }

    // Private key: prefer content PRIVATE KEY, then name score.
    let mut keys: Vec<&(PathBuf, String, PemKind)> = entries
        .iter()
        .filter(|(_, name, k)| *k == PemKind::PrivateKey || score_private_key(name) > 0)
        .collect();
    keys.sort_by_key(|(_, name, kind)| {
        let content_bonus = if *kind == PemKind::PrivateKey { 1000 } else { 0 };
        -(content_bonus + score_private_key(name))
    });
    let private_key = keys
        .first()
        .filter(|(_, name, kind)| *kind == PemKind::PrivateKey || score_private_key(name) > 0)
        .map(|(p, _, _)| p.clone())
        .ok_or_else(|| {
            format!(
                "No private key found in {} (need *-private.pem.key / *.key)",
                dir.display()
            )
        })?;

    // Certificates only (exclude chosen key path).
    let certs: Vec<&(PathBuf, String, PemKind)> = entries
        .iter()
        .filter(|(p, name, kind)| {
            p != &private_key
                && (*kind == PemKind::Certificate
                    || score_client_cert(name) > 0
                    || score_ca(name) > 0)
        })
        .collect();

    if certs.is_empty() {
        return Err(format!(
            "No certificates found in {} (need AmazonRootCA*.pem + *-certificate.pem.crt)",
            dir.display()
        ));
    }

    // CA: highest CA score among certs.
    let mut ca_cands: Vec<&(PathBuf, String, PemKind)> = certs.clone();
    ca_cands.sort_by_key(|(_, name, _)| -score_ca(name));
    let ca = if score_ca(&ca_cands[0].1) > 0 {
        ca_cands[0].0.clone()
    } else if certs.len() >= 2 {
        // Ambiguous: pick lowest client-cert score as CA (often the leftover).
        let mut by_client: Vec<_> = certs.clone();
        by_client.sort_by_key(|(_, name, _)| score_client_cert(name));
        by_client[0].0.clone()
    } else {
        return Err(format!(
            "Could not identify CA cert in {} (expected AmazonRootCA1.pem or *RootCA*)",
            dir.display()
        ));
    };

    // Client cert: highest client score, not the CA.
    let mut client_cands: Vec<&(PathBuf, String, PemKind)> = certs
        .iter()
        .copied()
        .filter(|(p, _, _)| p != &ca)
        .collect();
    client_cands.sort_by_key(|(_, name, _)| -score_client_cert(name));
    let client_cert = client_cands
        .first()
        .filter(|(_, name, _)| score_client_cert(name) > 0 || score_ca(name) == 0)
        .map(|(p, _, _)| p.clone())
        .ok_or_else(|| {
            format!(
                "Could not identify client certificate in {} (expected *-certificate.pem.crt)",
                dir.display()
            )
        })?;

    if ca == client_cert || ca == private_key || client_cert == private_key {
        return Err(format!(
            "Ambiguous cert files in {} — rename to AmazonRootCA1.pem, *-certificate.pem.crt, *-private.pem.key",
            dir.display()
        ));
    }

    Ok(ResolvedCerts {
        ca,
        client_cert,
        private_key,
    })
}

/// Discover when path is non-empty; `Ok(None)` if unset.
pub fn discover_optional(dir: &str) -> Result<Option<ResolvedCerts>, String> {
    let dir = dir.trim();
    if dir.is_empty() {
        return Ok(None);
    }
    discover(Path::new(dir)).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovers_aws_iot_layout() {
        let dir = std::env::temp_dir().join(format!("mqtt-certs-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        fs::write(
            dir.join("AmazonRootCA1.pem"),
            b"-----BEGIN CERTIFICATE-----\nCA\n-----END CERTIFICATE-----\n",
        )
        .unwrap();
        fs::write(
            dir.join("thing-certificate.pem.crt"),
            b"-----BEGIN CERTIFICATE-----\nDEVICE\n-----END CERTIFICATE-----\n",
        )
        .unwrap();
        fs::write(
            dir.join("thing-private.pem.key"),
            b"-----BEGIN RSA PRIVATE KEY-----\nKEY\n-----END RSA PRIVATE KEY-----\n",
        )
        .unwrap();
        // Public key should be ignored.
        fs::write(
            dir.join("thing-public.pem.key"),
            b"-----BEGIN PUBLIC KEY-----\nPUB\n-----END PUBLIC KEY-----\n",
        )
        .unwrap();

        let r = discover(&dir).unwrap();
        assert_eq!(r.ca_name(), "AmazonRootCA1.pem");
        assert_eq!(r.cert_name(), "thing-certificate.pem.crt");
        assert_eq!(r.key_name(), "thing-private.pem.key");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn empty_dir_errors() {
        let dir = std::env::temp_dir().join(format!("mqtt-certs-empty-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        assert!(discover(&dir).is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn optional_empty_ok() {
        assert!(discover_optional("").unwrap().is_none());
        assert!(discover_optional("   ").unwrap().is_none());
    }
}
