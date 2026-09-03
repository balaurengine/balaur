//! Compiles the emscripten websocket shim for web builds. Native targets have
//! no build step at all.
//!
//! The final web binary must link with `-lwebsocket.js`. Cargo cannot
//! propagate link args out of a library, so `.cargo/config.toml` carries them
//! for the emscripten target.

fn main() {
    let target = std::env::var("TARGET").unwrap_or_default();
    if target.contains("emscripten") {
        cc::Build::new()
            .file("shim/emscripten_websocket.c")
            .compile("balaur_websocket_shim");
    }
    println!("cargo:rerun-if-changed=shim/emscripten_websocket.c");
}
