//! Compiles the StoreKit shim on Apple platforms, and nothing anywhere else.
//!
//! `swift-rs` is here for its linker: a Swift static library needs the
//! runtime's search paths and rpaths per target — device, simulator and Mac
//! each differ — and that is the whole of what it does for us. The shim
//! itself depends on no Swift package, so this stays an offline build.

fn main() {
    println!("cargo:rerun-if-changed=swift");
    let target = std::env::var("CARGO_CFG_TARGET_VENDOR").unwrap_or_default();
    if target != "apple" {
        return;
    }
    swift_rs::SwiftLinker::new("12.0")
        .with_ios("15.0")
        .with_package("balaur-storekit", "swift")
        .link();
}
