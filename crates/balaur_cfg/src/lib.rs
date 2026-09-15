//! The two target facts the workspace gates on, named once.
//!
//! Three targets decide both, and the code spelled them out at each use in
//! both polarities. They are not complements: a browser tab is neither a
//! desktop nor a phone, so `not(desktop)` catches wasm where `mobile` does
//! not, and that difference is the whole reason to name them.
//!
//! A build script is the only place this can live. Cargo resolves the
//! `[target.'cfg(...)']` manifest keys before one runs, so a dependency
//! gated on a platform still names its targets itself.

/// Emits `desktop` and `mobile` for the target being compiled.
///
/// Both are registered whether or not they are set, so a misspelled gate is
/// an error rather than a condition that is quietly false everywhere.
pub fn emit() {
    println!("cargo::rustc-check-cfg=cfg(desktop)");
    println!("cargo::rustc-check-cfg=cfg(mobile)");

    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    // A list, not a word: wasm32-wasip1 reports `wasm,unix`.
    let families = std::env::var("CARGO_CFG_TARGET_FAMILY").unwrap_or_default();
    let wasm = families.split(',').any(|family| family == "wasm");

    let mobile = os == "ios" || os == "android";
    if mobile {
        println!("cargo:rustc-cfg=mobile");
    }
    if !mobile && !wasm {
        println!("cargo:rustc-cfg=desktop");
    }
}
