//! The editor's browser entry points: the start screen, a project opened in
//! a tab, and the projects the tab keeps. A game template has none of them.
use std::path::Path;

use wasm_bindgen::prelude::*;

use crate::web::{err, fetch_bytes};

/// Where a project fetched into memory lives. A browser has no directory to
/// open, so the editor is handed a path that exists only in [`MemoryFs`] —
/// the same shape `balaur edit <game>` hands it on a desktop.
const PROJECT_ROOT: &str = "/project";

/// Where the editor's own project is unpacked. It reads its themes through
/// `fs`, so it needs a directory of its own even though its scripts and
/// scenes come from the pack.
const EDITOR_ROOT: &str = "/editor";

/// Open the editor on no project at all: the start screen, where a reader
/// says which of the projects this browser keeps to open.
///
/// Resolves when the screen picks one, which it does by quitting: the page
/// then reads [`crate::project_web::next_project`] and boots the editor
/// again through [`open_project`].
#[wasm_bindgen]
#[allow(
    unreachable_pub,
    reason = "exported to the page by wasm-bindgen, not to another crate"
)]
pub async fn start_manager(canvas_id: String, editor_pack_url: String) -> Result<(), JsValue> {
    let editor = fetch_bytes(&editor_pack_url).await?;
    let editor_pack = balaur::Pack::decode(&editor).map_err(err)?;
    // The screen writes nothing that outlives the tab, so the editor's own
    // root is memory rather than a store: what it edits comes later.
    let fs = balaur::files::MemoryFs::new();
    fs.seed(Path::new(EDITOR_ROOT), editor_pack.entries());
    balaur::files::set_default(std::rc::Rc::new(fs));
    crate::project_web::set_kept(&crate::web_store::list().await?);
    balaur::boot_editor_on_canvas(
        &editor,
        EDITOR_ROOT,
        "",
        &canvas_id,
        &mut editor_plugins(&editor_pack_url),
    )
    .await
    .map_err(err)
}

/// Open the editor on `project_pack_url`, drawing on the canvas with id
/// `canvas_id`.
///
/// The pack's URL names the project, so this is [`open_project`] with an id a
/// page need not have chosen: what an earlier visit kept under that URL comes
/// back, and the pack seeds it the first time.
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
    let id = format!("pack:{project_pack_url}");
    open_project(canvas_id, editor_pack_url, id, Some(project_pack_url)).await
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
    let editor = fetch_bytes(&editor_pack_url).await?;
    let editor_pack = balaur::Pack::decode(&editor).map_err(err)?;
    let fs = crate::web_store::ProjectFs::open(&project_id, Path::new(PROJECT_ROOT)).await?;
    fs.install();
    if fs.is_empty() {
        let url = seed_pack_url.ok_or_else(|| {
            JsValue::from_str("nothing is kept under that project id, and no pack to start it from")
        })?;
        let seed = balaur::Pack::decode(&fetch_bytes(&url).await?).map_err(err)?;
        fs.name(
            manifest_name(&seed.manifest)
                .as_deref()
                .unwrap_or(&project_id),
        );
        fs.seed_at(Path::new(PROJECT_ROOT), seed.entries());
    }
    fs.seed_at(Path::new(EDITOR_ROOT), editor_pack.entries());
    balaur::files::set_default(fs);
    // What the start screen lists, read here because the store is
    // asynchronous and the verb the screen calls is not.
    crate::project_web::set_kept(&crate::web_store::list().await?);
    balaur::boot_editor_on_canvas(
        &editor,
        EDITOR_ROOT,
        PROJECT_ROOT,
        &canvas_id,
        &mut editor_plugins(&editor_pack_url),
    )
    .await
    .map_err(err)
}

/// The editor's own verbs. `export` is told where the module a web bundle
/// ships is served from: beside the editor's own pack, which is how a page
/// serves the set. `import` is here so a drop answers the same way it does on
/// a desktop, with the error a tab has to give. `project` is the start
/// screen's: the editor's scripts name it on every platform, and the page
/// rather than the screen is what opens a project in a tab.
fn editor_plugins(editor_pack_url: &str) -> [Box<dyn balaur_plugin::Plugin>; 3] {
    [
        Box::new(crate::web_export::WebExportPlugin::new(
            std::path::PathBuf::from(PROJECT_ROOT),
            directory_of(editor_pack_url),
        )),
        Box::new(crate::import_api::ImportPlugin::new(
            std::path::PathBuf::from(PROJECT_ROOT),
        )),
        Box::new(crate::project_api::ProjectPlugin::new()),
    ]
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

/// The open project zipped, as `[name, bytes]`, for someone to take their
/// work out of the browser it is kept in.
#[wasm_bindgen]
#[allow(
    unreachable_pub,
    reason = "exported to the page by wasm-bindgen, not to another crate"
)]
pub fn download_project() -> Result<js_sys::Array, JsValue> {
    let fs = crate::web_store::ProjectFs::live()
        .ok_or_else(|| JsValue::from_str("no project is open"))?;
    let (name, bytes) =
        crate::web_export::archive(Path::new(PROJECT_ROOT), &fs.files()).map_err(err)?;
    Ok(js_sys::Array::of2(
        &JsValue::from_str(&name),
        &js_sys::Uint8Array::from(bytes.as_slice()).into(),
    ))
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

/// The name a manifest gives its project, for the record a store keeps.
fn manifest_name(manifest: &str) -> Option<String> {
    // `toml::Table`, not `toml::Value`: `Value`'s `FromStr` reads a value, and
    // a manifest's first `[table]` header ends it.
    manifest
        .parse::<toml::Table>()
        .ok()?
        .get("application")?
        .get("name")?
        .as_str()
        .filter(|name| !name.is_empty())
        .map(str::to_string)
}
