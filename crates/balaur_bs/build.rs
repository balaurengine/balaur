// `jit`: wasmtime is in this build. The `wasmtime` feature asks for it, and
// it is granted only where a JIT may run: not in a browser, not on iOS.
fn main() {
    // What a build kept between runs was compiled by (`stored.rs`): any
    // change to the compiler's source makes a kept build stale.
    println!("cargo::rerun-if-changed=src");
    println!(
        "cargo::rustc-env=BALAUR_BS_SOURCE={:016x}",
        source_hash("src")
    );
    println!("cargo::rustc-check-cfg=cfg(jit)");
    println!("cargo::rerun-if-env-changed=CARGO_FEATURE_WASMTIME");
    let asked = std::env::var_os("CARGO_FEATURE_WASMTIME").is_some();
    let arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if asked && arch != "wasm32" && os != "ios" {
        println!("cargo::rustc-cfg=jit");
    }
}

// FNV-1a over every file under `dir`, by path, in a fixed order.
fn source_hash(dir: &str) -> u64 {
    fn walk(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, out);
            } else {
                out.push(path);
            }
        }
    }
    let mut files = vec![];
    walk(std::path::Path::new(dir), &mut files);
    files.sort();
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for file in files {
        let bytes = std::fs::read(&file).unwrap_or_default();
        for b in file.to_string_lossy().bytes().chain(bytes) {
            h = (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
        }
    }
    h
}
