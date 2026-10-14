//! Embed Windows PE icon (taskbar / Explorer) from assets/icons/app.ico.

use std::path::PathBuf;

fn main() {
    let target = std::env::var("TARGET").unwrap_or_default();
    println!("cargo:rerun-if-changed=assets/icons/app.ico");
    if !target.contains("windows") {
        return;
    }

    let mut res = winres::WindowsResource::new();
    res.set_icon("assets/icons/app.ico");
    // Cross-compile from Linux: use MinGW windres when available.
    if let Ok(windres) = std::env::var("WINDRES") {
        res.set_windres_path(&windres);
    } else if cfg!(target_os = "linux") {
        res.set_windres_path("x86_64-w64-mingw32-windres");
    }
    if let Err(e) = res.compile() {
        println!("cargo:warning=winres icon embed failed: {e}");
        return;
    }

    // GNU ld drops unused static libs — force-link resource.o so .rsrc (icon) survives.
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap_or_default());
    let obj = out.join("resource.o");
    if obj.is_file() {
        println!("cargo:rustc-link-arg={}", obj.display());
    } else {
        println!("cargo:warning=winres resource.o missing at {}", obj.display());
    }
}
