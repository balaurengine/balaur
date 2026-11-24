//! The pickers, the relaunch and the importers all gate on a desktop.

fn main() {
    balaur_cfg::emit();
    // A Steam build loads `steam_api` from beside itself, which a Linux loader
    // reads only off a run path; Windows and Valve's macOS install name do anyway.
    if std::env::var_os("CARGO_FEATURE_STEAM").is_some()
        && std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("linux")
    {
        println!("cargo:rustc-link-arg-bins=-Wl,-rpath,$ORIGIN");
    }
}
