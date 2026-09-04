//! Compiles the StoreKit shim on Apple platforms, and nothing anywhere else.
//!
//! `swift-rs` is here for its linker: a Swift static library needs the
//! runtime's search paths and rpaths per target — device, simulator and Mac
//! each differ — and that is the whole of what it does for us. The shim
//! itself depends on no Swift package, so this stays an offline build.
//!
//! A Mac with no Swift toolchain still builds the engine. It builds it
//! without purchases, which `apple.purchase` then says out loud, because a
//! link error naming a symbol nobody has heard of is a worse answer.

fn main() {
    println!("cargo::rustc-check-cfg=cfg(swift_shim)");
    println!("cargo:rerun-if-changed=swift");
    if std::env::var("CARGO_CFG_TARGET_VENDOR").unwrap_or_default() != "apple" {
        return;
    }
    if !swift_is_installed() {
        println!(
            "cargo:warning=no Swift toolchain found, so this build has no StoreKit: \
             in-app purchases will answer `unsupported`. Install Xcode's command line tools."
        );
        return;
    }
    swift_rs::SwiftLinker::new("12.0")
        .with_ios("15.0")
        .with_package("balaur-storekit", "swift")
        .link();
    println!("cargo:rustc-cfg=swift_shim");
}

fn swift_is_installed() -> bool {
    std::process::Command::new("xcrun")
        .args(["--find", "swift"])
        .output()
        .is_ok_and(|found| found.status.success())
}
