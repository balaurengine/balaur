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
#[allow(
    unreachable_pub,
    reason = "exported to the page by wasm-bindgen, not to another crate"
)]
pub async fn start(canvas_id: String, pack_url: String) -> Result<(), JsValue> {
    // A panic would otherwise surface as `RuntimeError: unreachable` with no
    // message; the hook prints the panic's own text on the console first.
    console_error_panic_hook::set_once();
    balaur::logbuf::capture(tracing::level_filters::LevelFilter::INFO);
    let bytes = fetch_bytes(&pack_url).await?;
    balaur::boot_pack_on_canvas(&bytes, &canvas_id)
        .await
        .map_err(err)
}

/// Where a project fetched into memory lives. A browser has no directory to
/// open, so the editor is handed a path that exists only in [`MemoryFs`] —
/// the same shape `balaur edit <game>` hands it on a desktop.
const PROJECT_ROOT: &str = "/project";

/// Where the editor's own project is unpacked. It reads its themes through
/// `fs`, so it needs a directory of its own even though its scripts and
/// scenes come from the pack.
const EDITOR_ROOT: &str = "/editor";

/// Open the editor on `project_pack_url`, drawing on the canvas with id
/// `canvas_id`.
///
/// Both packs are fetched: the editor's own project — its scripts, scenes,
/// fonts and themes — and the game to edit, which is unpacked into a virtual
/// filesystem the editor then reads and writes as if it were a directory.
/// Nothing is written back to the server; what the editor saves lives in the
/// tab until the page keeps it.
#[wasm_bindgen]
#[allow(
    unreachable_pub,
    reason = "exported to the page by wasm-bindgen, not to another crate"
)]
pub async fn start_editor(
    canvas_id: String,
    editor_pack_url: String,
    project_pack_url: String,
) -> Result<(), JsValue> {
    console_error_panic_hook::set_once();
    balaur::logbuf::capture(tracing::level_filters::LevelFilter::INFO);
    let editor = fetch_bytes(&editor_pack_url).await?;
    let project = fetch_bytes(&project_pack_url).await?;
    let project = balaur::Pack::decode(&project).map_err(err)?;

    let editor_pack = balaur::Pack::decode(&editor).map_err(err)?;
    let fs = std::rc::Rc::new(balaur::files::MemoryFs::new());
    fs.seed(std::path::Path::new(EDITOR_ROOT), editor_pack.entries());
    fs.seed(std::path::Path::new(PROJECT_ROOT), project.entries());
    balaur::files::set_default(fs);

    balaur::boot_editor_on_canvas(&editor, EDITOR_ROOT, PROJECT_ROOT, &canvas_id)
        .await
        .map_err(err)
}

fn err(e: anyhow::Error) -> JsValue {
    JsValue::from_str(&format!("{e:#}"))
}

async fn fetch_bytes(url: &str) -> Result<Vec<u8>, JsValue> {
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
