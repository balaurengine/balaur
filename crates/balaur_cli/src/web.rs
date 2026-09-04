//! The browser entry point.
//!
//! `scripts/package_template.sh web` builds this binary for
//! `wasm32-unknown-unknown` and runs wasm-bindgen over it; the page it ships
//! with imports `balaur.js` and calls [`start`] with the id of a `<canvas>`
//! and the URL of a `.bpak`. The pack is fetched, decoded and booted exactly
//! as `boot_pack` boots an embedded one on desktop, on the windowed loop —
//! spawned rather than blocked on, since a page may never block.
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;

/// Fetch `pack_url` and run it on the canvas with id `canvas_id`. Resolves
/// when the game quits; rejects with the error's message when it cannot
/// start — a bad URL, a pack the engine cannot decode, no GPU adapter.
#[wasm_bindgen]
pub async fn start(canvas_id: String, pack_url: String) -> Result<(), JsValue> {
    // A panic would otherwise surface as `RuntimeError: unreachable` with no
    // message; the hook prints the panic's own text on the console first.
    console_error_panic_hook::set_once();
    balaur::logbuf::capture(tracing::level_filters::LevelFilter::INFO);
    let bytes = fetch_bytes(&pack_url).await?;
    balaur::boot_pack_on_canvas(&bytes, &canvas_id)
        .await
        .map_err(|e| JsValue::from_str(&format!("{e:#}")))
}

async fn fetch_bytes(url: &str) -> Result<Vec<u8>, JsValue> {
    let window = web_sys::window().ok_or_else(|| JsValue::from_str("no window"))?;
    let response: web_sys::Response =
        JsFuture::from(window.fetch_with_str(url)).await?.dyn_into()?;
    if !response.ok() {
        return Err(JsValue::from_str(&format!(
            "{url}: HTTP {}",
            response.status()
        )));
    }
    let buffer = JsFuture::from(response.array_buffer()?).await?;
    Ok(js_sys::Uint8Array::new(&buffer).to_vec())
}
