//! The browser entry point.
//!
//! `scripts/package_template.sh web` builds this binary for
//! `wasm32-unknown-unknown` and runs wasm-bindgen over it; the page it ships
//! with imports `balaur.js` and calls [`start`] with the id of a `<canvas>`
//! and the URL of a `.bpak`. The pack is fetched, decoded and booted exactly
//! as `boot_pack` boots an embedded one on desktop, on the windowed loop —
//! spawned rather than blocked on, since a page may never block.
use std::path::Path;

use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
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
    // The pack's URL names the game on this origin: what an earlier visit
    // saved under it comes back before the first scene loads.
    balaur::files::set_default(std::rc::Rc::new(crate::web_fs::StorageFs::open(&pack_url)));
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
/// Nothing is written back to the server, and what the editor saves is kept
/// in the browser under the project's own URL, so a refresh comes back to the
/// scene as it was left rather than to the one the pack shipped.
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
    // The editor's own project is not mirrored: it ships with the editor and
    // is fetched again every visit. The project under edit is, and the pack
    // is seeded first so a file an earlier visit kept lands on top of it.
    let fs = std::rc::Rc::new(crate::web_fs::StorageFs::mirroring(
        &project_pack_url,
        &[PROJECT_ROOT],
    ));
    fs.seed(Path::new(EDITOR_ROOT), editor_pack.entries());
    fs.seed(Path::new(PROJECT_ROOT), project.entries());
    fs.restore();
    balaur::files::set_default(fs);

    balaur::boot_editor_on_canvas(
        &editor,
        EDITOR_ROOT,
        PROJECT_ROOT,
        &canvas_id,
        &mut exporter(&editor_pack_url),
    )
    .await
    .map_err(err)
}

/// Open the editor on the project kept under `project_id`.
///
/// The project's files live in IndexedDB and are seeded into memory before
/// the editor boots; every save is mirrored back, so a refresh reopens the
/// work. `seed_pack_url` is fetched only when nothing is kept under that id
/// yet, which is how a bundled example becomes a project of one's own.
#[wasm_bindgen]
#[allow(
    unreachable_pub,
    reason = "exported to the page by wasm-bindgen, not to another crate"
)]
pub async fn open_project(
    canvas_id: String,
    editor_pack_url: String,
    project_id: String,
    seed_pack_url: Option<String>,
) -> Result<(), JsValue> {
    console_error_panic_hook::set_once();
    balaur::logbuf::capture(tracing::level_filters::LevelFilter::INFO);
    let editor = fetch_bytes(&editor_pack_url).await?;
    let editor_pack = balaur::Pack::decode(&editor).map_err(err)?;
    let fs = crate::web_store::ProjectFs::open(&project_id, Path::new(PROJECT_ROOT)).await?;
    if fs.is_empty() {
        let url = seed_pack_url.ok_or_else(|| {
            JsValue::from_str("nothing is kept under that project id, and no pack to start it from")
        })?;
        let seed = balaur::Pack::decode(&fetch_bytes(&url).await?).map_err(err)?;
        fs.seed_at(Path::new(PROJECT_ROOT), seed.entries());
    }
    fs.seed_at(Path::new(EDITOR_ROOT), editor_pack.entries());
    fs.install();
    balaur::files::set_default(fs);
    balaur::boot_editor_on_canvas(
        &editor,
        EDITOR_ROOT,
        PROJECT_ROOT,
        &canvas_id,
        &mut exporter(&editor_pack_url),
    )
    .await
    .map_err(err)
}

/// The editor's `export`, told where the module a web bundle ships is served
/// from: beside the editor's own pack, which is how a page serves the set.
fn exporter(editor_pack_url: &str) -> [Box<dyn balaur_plugin::Plugin>; 1] {
    [Box::new(crate::web_export::WebExportPlugin::new(
        std::path::PathBuf::from(PROJECT_ROOT),
        directory_of(editor_pack_url),
    ))]
}

/// The directory part of a URL: everything before the last slash, with any
/// query dropped.
fn directory_of(url: &str) -> String {
    let path = url.split(['?', '#']).next().unwrap_or(url);
    match path.rfind('/') {
        Some(at) => path[..at].to_string(),
        None => String::new(),
    }
}

/// What the last export produced, as `[name, bytes]`, or nothing when there
/// is none waiting. Taken: the page downloads it once.
#[wasm_bindgen]
#[allow(
    unreachable_pub,
    reason = "exported to the page by wasm-bindgen, not to another crate"
)]
pub fn take_export() -> Option<js_sys::Array> {
    let (name, bytes) = crate::web_export::take()?;
    Some(js_sys::Array::of2(
        &JsValue::from_str(&name),
        &js_sys::Uint8Array::from(bytes.as_slice()).into(),
    ))
}

/// Every project kept in this browser, newest first, as `{ id, name,
/// modified }`. What a page lists before anything is booted.
#[wasm_bindgen]
#[allow(
    unreachable_pub,
    reason = "exported to the page by wasm-bindgen, not to another crate"
)]
pub async fn list_projects() -> Result<js_sys::Array, JsValue> {
    crate::web_store::list().await
}

/// Forget a project and everything in it.
#[wasm_bindgen]
#[allow(
    unreachable_pub,
    reason = "exported to the page by wasm-bindgen, not to another crate"
)]
pub async fn delete_project(project_id: String) -> Result<(), JsValue> {
    crate::web_store::delete(&project_id).await
}

/// Keep a pack as a project of its own, replacing whatever that id held.
#[wasm_bindgen]
#[allow(
    unreachable_pub,
    reason = "exported to the page by wasm-bindgen, not to another crate"
)]
pub async fn import_project_pack(
    project_id: String,
    name: String,
    pack_url: String,
) -> Result<(), JsValue> {
    let pack = balaur::Pack::decode(&fetch_bytes(&pack_url).await?).map_err(err)?;
    crate::web_store::import(&project_id, &name, pack.entries()).await
}

/// Keep a directory someone chose as a project: `files` is one
/// `[path, bytes]` pair per file, project-relative.
#[wasm_bindgen]
#[allow(
    unreachable_pub,
    reason = "exported to the page by wasm-bindgen, not to another crate"
)]
pub async fn import_project_files(
    project_id: String,
    name: String,
    files: js_sys::Array,
) -> Result<(), JsValue> {
    let mut entries = Vec::with_capacity(files.length() as usize);
    for item in files.iter() {
        let pair = js_sys::Array::from(&item);
        let Some(path) = pair.get(0).as_string() else {
            continue;
        };
        entries.push((path, js_sys::Uint8Array::new(&pair.get(1)).to_vec()));
    }
    crate::web_store::import(&project_id, &name, entries).await
}

/// How many files are edited but not yet kept. What a page reads before it
/// lets someone close the tab.
#[wasm_bindgen]
#[allow(
    unreachable_pub,
    reason = "exported to the page by wasm-bindgen, not to another crate"
)]
pub fn unsaved_count() -> u32 {
    crate::web_store::ProjectFs::live()
        .map_or(0, |fs| u32::try_from(fs.unsaved()).unwrap_or(u32::MAX))
}

/// Mirror everything outstanding now, rather than when the timer comes round.
#[wasm_bindgen]
#[allow(
    unreachable_pub,
    reason = "exported to the page by wasm-bindgen, not to another crate"
)]
pub async fn save_project() -> Result<(), JsValue> {
    let Some(fs) = crate::web_store::ProjectFs::live() else {
        return Ok(());
    };
    fs.flush().await
}

fn err(e: anyhow::Error) -> JsValue {
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
