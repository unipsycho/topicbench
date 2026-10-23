//! Dev-only TopicBench Pro license mint tool (keygen + sign).
//!
//! Private key stays outside the app tree by default:
//!   ~/.config/topicbench/dev/ed25519.sk.hex
//!
//! Usage:
//!   cargo run --bin topicbench-license -- keygen
//!   cargo run --bin topicbench-license -- sign \
//!     --entitlements multi_graph,mcp_server,simulation \
//!     [--expires 2028-01-01] [--licensee mike] \
//!     [--out ~/.config/topicbench/license.json]
//!
//! Env: TOPICBENCH_LICENSE_SIGNING_KEY = path to hex private key (overrides default).

#[allow(dead_code)] // Shared with OSS probe; mint tool only uses sign/keygen helpers.
#[path = "../pro/license_format.rs"]
mod license_format;

use ed25519_dalek::SigningKey;
use license_format::{
    generate_keypair, key_to_hex, mint_license_json, signing_key_from_hex_file, verify_license_bytes,
    LicensePayload, LICENSE_FILE_NAME, LICENSE_VERIFY_KEY,
};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut args: Vec<String> = env::args().skip(1).collect();
    if args.is_empty() {
        print_usage();
        return ExitCode::FAILURE;
    }
    let cmd = args.remove(0);
    match cmd.as_str() {
        "keygen" => cmd_keygen(&args),
        "sign" => cmd_sign(&args),
        "verify" => cmd_verify(&args),
        "pubkey" => cmd_pubkey(),
        "help" | "-h" | "--help" => {
            print_usage();
            ExitCode::SUCCESS
        }
        other => {
            eprintln!("unknown command: {other}");
            print_usage();
            ExitCode::FAILURE
        }
    }
}

fn print_usage() {
    eprintln!(
        "topicbench-license — mint signed Pro license files (dev)\n\n\
         Commands:\n\
           keygen [--out-dir DIR]     Generate ed25519 keypair (default DIR: ~/.config/topicbench/dev)\n\
           sign [options]            Sign entitlements → license.json\n\
           verify [PATH]             Verify a license file with the embedded OSS public key\n\
           pubkey                    Print embedded OSS verify key (hex)\n\n\
         sign options:\n\
           --entitlements LIST       Comma-separated flags (required)\n\
           --expires YYYY-MM-DD      Optional expiry\n\
           --licensee TEXT           Optional label\n\
           --key PATH                Private key hex file (or TOPICBENCH_LICENSE_SIGNING_KEY)\n\
           --out PATH                Output file (default: ~/.config/topicbench/license.json)\n"
    );
}

fn default_dev_dir() -> PathBuf {
    config_home().join("topicbench").join("dev")
}

fn default_license_path() -> PathBuf {
    config_home().join("topicbench").join(LICENSE_FILE_NAME)
}

fn config_home() -> PathBuf {
    env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| PathBuf::from("."))
}

fn cmd_keygen(args: &[String]) -> ExitCode {
    let mut out_dir = default_dev_dir();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--out-dir" if i + 1 < args.len() => {
                out_dir = PathBuf::from(&args[i + 1]);
                i += 2;
            }
            other => {
                eprintln!("unknown keygen arg: {other}");
                return ExitCode::FAILURE;
            }
        }
    }
    if let Err(e) = fs::create_dir_all(&out_dir) {
        eprintln!("mkdir {}: {e}", out_dir.display());
        return ExitCode::FAILURE;
    }
    let sk = generate_keypair();
    let vk = sk.verifying_key();
    let sk_path = out_dir.join("ed25519.sk.hex");
    let pk_path = out_dir.join("ed25519.pk.hex");
    if let Err(e) = fs::write(&sk_path, format!("{}\n", key_to_hex(&sk.to_bytes()))) {
        eprintln!("write {}: {e}", sk_path.display());
        return ExitCode::FAILURE;
    }
    // Restrict private key perms when possible (unix).
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&sk_path, fs::Permissions::from_mode(0o600));
    }
    if let Err(e) = fs::write(&pk_path, format!("{}\n", key_to_hex(&vk.to_bytes()))) {
        eprintln!("write {}: {e}", pk_path.display());
        return ExitCode::FAILURE;
    }
    println!("wrote {}", sk_path.display());
    println!("wrote {}", pk_path.display());
    println!();
    println!("Embed this public key in src/pro/license_format.rs as LICENSE_VERIFY_KEY:");
    println!("{}", key_to_hex(&vk.to_bytes()));
    println!();
    let embedded = key_to_hex(&LICENSE_VERIFY_KEY);
    if embedded.chars().all(|c| c == '0') {
        println!("(OSS still has the all-zero placeholder — paste the hex above into LICENSE_VERIFY_KEY.)");
    } else if embedded != key_to_hex(&vk.to_bytes()) {
        println!("NOTE: embedded OSS key differs from this new keypair. Update LICENSE_VERIFY_KEY or reuse the old private key.");
    } else {
        println!("Embedded OSS verify key matches this keypair.");
    }
    ExitCode::SUCCESS
}

fn cmd_pubkey() -> ExitCode {
    println!("{}", key_to_hex(&LICENSE_VERIFY_KEY));
    ExitCode::SUCCESS
}

fn cmd_verify(args: &[String]) -> ExitCode {
    let path = if args.is_empty() {
        default_license_path()
    } else {
        PathBuf::from(&args[0])
    };
    let bytes = match fs::read(&path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("read {}: {e}", path.display());
            return ExitCode::FAILURE;
        }
    };
    match verify_license_bytes(&bytes) {
        Ok(payload) => {
            println!("OK {}", path.display());
            println!("entitlements: {}", payload.entitlements.join(", "));
            if let Some(ref e) = payload.expires {
                println!("expires: {e}");
            }
            if let Some(ref l) = payload.licensee {
                println!("licensee: {l}");
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("FAIL {}: {e}", path.display());
            ExitCode::FAILURE
        }
    }
}

fn cmd_sign(args: &[String]) -> ExitCode {
    let mut entitlements: Option<String> = None;
    let mut expires: Option<String> = None;
    let mut licensee: Option<String> = None;
    let mut key_path: Option<PathBuf> = env::var_os("TOPICBENCH_LICENSE_SIGNING_KEY").map(PathBuf::from);
    let mut out_path = default_license_path();

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--entitlements" if i + 1 < args.len() => {
                entitlements = Some(args[i + 1].clone());
                i += 2;
            }
            "--expires" if i + 1 < args.len() => {
                expires = Some(args[i + 1].clone());
                i += 2;
            }
            "--licensee" if i + 1 < args.len() => {
                licensee = Some(args[i + 1].clone());
                i += 2;
            }
            "--key" if i + 1 < args.len() => {
                key_path = Some(PathBuf::from(&args[i + 1]));
                i += 2;
            }
            "--out" if i + 1 < args.len() => {
                out_path = PathBuf::from(&args[i + 1]);
                i += 2;
            }
            other => {
                eprintln!("unknown sign arg: {other}");
                return ExitCode::FAILURE;
            }
        }
    }

    let Some(ent_raw) = entitlements else {
        eprintln!("--entitlements is required");
        return ExitCode::FAILURE;
    };
    let ents: Vec<String> = ent_raw
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if ents.is_empty() {
        eprintln!("entitlements list is empty");
        return ExitCode::FAILURE;
    }

    let key_path = key_path.unwrap_or_else(|| default_dev_dir().join("ed25519.sk.hex"));
    let sk = match load_signing_key(&key_path) {
        Ok(k) => k,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };

    let payload = LicensePayload {
        entitlements: ents,
        expires,
        licensee,
    };
    let json = match mint_license_json(&sk, &payload) {
        Ok(j) => j,
        Err(e) => {
            eprintln!("mint failed: {e}");
            return ExitCode::FAILURE;
        }
    };

    if let Some(parent) = out_path.parent() {
        if let Err(e) = fs::create_dir_all(parent) {
            eprintln!("mkdir {}: {e}", parent.display());
            return ExitCode::FAILURE;
        }
    }
    if let Err(e) = fs::write(&out_path, format!("{json}\n")) {
        eprintln!("write {}: {e}", out_path.display());
        return ExitCode::FAILURE;
    }
    println!("wrote {}", out_path.display());
    println!("entitlements: {}", payload.entitlements.join(", "));
    if let Some(ref e) = payload.expires {
        println!("expires: {e}");
    }
    let vk_hex = key_to_hex(&sk.verifying_key().to_bytes());
    let embedded = key_to_hex(&LICENSE_VERIFY_KEY);
    if vk_hex != embedded {
        eprintln!();
        eprintln!("WARNING: signing key's public half does not match embedded LICENSE_VERIFY_KEY.");
        eprintln!("  signing pk:  {vk_hex}");
        eprintln!("  embedded:    {embedded}");
        eprintln!("OSS will reject this file until keys match.");
    }
    ExitCode::SUCCESS
}

fn load_signing_key(path: &Path) -> Result<SigningKey, String> {
    signing_key_from_hex_file(path).map_err(|e| format!("load key {}: {e}", path.display()))
}
