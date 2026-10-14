//! Embed Windows PE icon (taskbar / Explorer) from assets/icons/app.ico.

fn main() {
    let target = std::env::var("TARGET").unwrap_or_default();
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
    }
}
