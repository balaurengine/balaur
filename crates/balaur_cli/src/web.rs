//! The browser entry point.
//!
//! `scripts/package_runtime.sh web` builds this binary for
//! `wasm32-unknown-unknown` and runs wasm-bindgen over it; the page it ships
//! with imports `balaur.js` and calls [`start`] with the id of a `<canvas>`
//! and the URL of a `.bpak`. The pack is fetched, decoded and booted exactly
//! as `boot_pack` boots an embedded one on desktop, on the windowed loop —
//! spawned rather than blocked on, since a page may never block.
use std::path::Path;

use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

/// The panic hook and the log capture, installed once when the page
/// instantiates the module.
///
/// Not per entry point: the page reaches `import_project_files` and
/// `delete_project` before it boots an engine, and a panic with no hook in
/// place surfaces as `RuntimeError: unreachable` carrying no message at all.
#[wasm_bindgen(start)]
fn on_load() {
    console_error_panic_hook::set_once();
    balaur::logbuf::capture(tracing::level_filters::LevelFilter::INFO);
}

/// Fetch `pack_url` and run it on the canvas with id `canvas_id`. Resolves
/// when the game quits; rejects with the error's message when it cannot
/// start — a bad URL, a pack the engine cannot decode, no GPU adapter.
#[wasm_bindgen]
#[allow(
    unreachable_pub,
    reason = "exported to the page by wasm-bindgen, not to another crate"
)]
pub async fn start(canvas_id: String, pack_url: String) -> Result<(), JsValue> {
    let bytes = fetch_bytes(&pack_url).await?;
    // The pack's URL names the game on this origin: the user directory an
    // earlier visit kept under it comes back before the first scene loads.
    let fs = crate::web_store::ProjectFs::open(&format!("game:{pack_url}"), Path::new(USER_DATA))
        .await?;
    fs.install();
    balaur::files::set_default(fs);
    let pack = balaur::Pack::decode(&bytes).map_err(err)?;
    drop(bytes);
    // The rest of a pack in pieces arrives while the game runs.
    if let Some(tail) = balaur::pack_tail::Tail::of(&pack) {
        let url = beside(&pack_url, &tail.file);
        wasm_bindgen_futures::spawn_local(async move {
            let fetched = fetch_bytes(&url).await.map_err(|e| format!("{e:?}"));
            match fetched
                .and_then(|bytes| balaur::Pack::decode(&bytes).map_err(|e| format!("{e:#}")))
            {
                Ok(tail) => balaur::pack_tail::deliver(tail),
                Err(why) => tracing::error!("fetching the pack's tail {url}: {why}"),
            }
        });
    }
    Box::pin(balaur::boot_packed_on_canvas(pack, &canvas_id))
        .await
        .map_err(err)
}

/// `file` in the directory of the URL `of` names.
fn beside(of: &str, file: &str) -> String {
    match of.rsplit_once('/') {
        Some((dir, _)) => format!("{dir}/{file}"),
        None => file.to_string(),
    }
}

/// Where a packed game's user directory lands on a platform with no data
/// directory: `user_data` under the project root, which for a pack is `.`.
/// The one directory a running game writes, and so the one that is kept.
const USER_DATA: &str = "user_data";

#[allow(
    clippy::needless_pass_by_value,
    reason = "the argument of `map_err`, which hands the error over"
)]
pub(crate) fn err(e: anyhow::Error) -> JsValue {
    JsValue::from_str(&format!("{e:#}"))
}

pub(crate) async fn fetch_bytes(url: &str) -> Result<Vec<u8>, JsValue> {
    let window = web_sys::window().ok_or_else(|| JsValue::from_str("no window"))?;
    let response: web_sys::Response = JsFuture::from(window.fetch_with_str(url))
        .await?
        .dyn_into()?;
    if !response.ok() {
        return Err(JsValue::from_str(&format!(
            "{url}: HTTP {}",
            response.status()
        )));
    }
    let buffer = JsFuture::from(response.array_buffer()?).await?;
    Ok(js_sys::Uint8Array::new(&buffer).to_vec())
}
