// `jit`: wasmtime is in this build. The `wasmtime` feature asks for it, and
// it is granted only where a JIT may run: not in a browser, not on iOS.
fn main() {
    println!("cargo::rustc-check-cfg=cfg(jit)");
    println!("cargo::rerun-if-env-changed=CARGO_FEATURE_WASMTIME");
    let asked = std::env::var_os("CARGO_FEATURE_WASMTIME").is_some();
    let arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if asked && arch != "wasm32" && os != "ios" {
        println!("cargo::rustc-cfg=jit");
    }
}
