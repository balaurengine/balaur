//! Asking the reader of a tab for a file to import.
//!
//! A desktop opens a dialog and answers with a path. A browser cannot: the
//! chooser is an element, the page raises an event when something is picked,
//! and reading each file is a promise. So nothing here returns the file --
//! `choose` starts the asking, and the import starts itself once the bytes
//! have arrived, reporting through the same events a drop does.
//!
//! **A model brings its images.** A `.gltf` names files beside itself, and in
//! a tab there is no "beside": the input takes several files, the first one an
//! importer claims is the model, and the rest are what it may name. A reader
//! who picks the `.gltf` alone gets told which image is missing.

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::mpsc::Sender;

use balaur_core::task;
use js_sys::Uint8Array;
use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;
use wasm_bindgen_futures::{JsFuture, spawn_local};
use web_sys::{Event, File, FileList, HtmlInputElement};

use crate::import_api::{ImportEvent, ImportJob, Source};

/// What the chooser offers, as an `accept` list from the one list of what an
/// importer reads.
fn accept() -> String {
    balaur_import::claimed()
        .iter()
        .map(|extension| format!(".{extension}"))
        .collect::<Vec<_>>()
        .join(",")
}

/// Open the chooser. The import follows when the reader has picked.
pub(crate) fn choose(
    project: PathBuf,
    report: Sender<ImportEvent>,
    running: Rc<Cell<usize>>,
    cancel: Rc<Cell<bool>>,
) -> bool {
    let Some(document) = web_sys::window().and_then(|w| w.document()) else {
        return false;
    };
    let Ok(input) = document
        .create_element("input")
        .and_then(|e| e.dyn_into::<HtmlInputElement>().map_err(Into::into))
    else {
        return false;
    };
    input.set_type("file");
    input.set_multiple(true);
    input.set_accept(&accept());

    let picker = input.clone();
    // `once`: a chooser is answered once, and the closure owns what it needs
    // to start the import without reaching back into the engine.
    let answered = Closure::once(move |_: Event| {
        let Some(files) = picker.files() else {
            return;
        };
        spawn_local(read_then_import(files, project, report, running, cancel));
    });
    input.set_onchange(Some(answered.as_ref().unchecked_ref()));
    // The page owns the closure now: the element outlives this call, and
    // dropping the closure here would leave its handler dangling.
    answered.forget();
    // Not added to the document: a detached input still opens its chooser,
    // and one in the page would need hiding and taking back out again.
    input.click();
    true
}

/// Read every picked file, then park the import.
async fn read_then_import(
    files: FileList,
    project: PathBuf,
    report: Sender<ImportEvent>,
    running: Rc<Cell<usize>>,
    cancel: Rc<Cell<bool>>,
) {
    let mut model: Option<(String, Vec<u8>)> = None;
    let mut with: Vec<(String, Vec<u8>)> = Vec::new();
    for index in 0..files.length() {
        let Some(file) = files.get(index) else {
            continue;
        };
        let name = file.name();
        let Some(bytes) = bytes_of(&file).await else {
            let _ = report.send(ImportEvent::Failed {
                source: name.clone(),
                message: format!("{name} could not be read from the page"),
            });
            return;
        };
        // The first file an importer claims is the one being imported; the
        // rest are what it may name beside itself.
        if model.is_none() && balaur_import::claims(&name) {
            model = Some((name, bytes));
        } else {
            with.push((name, bytes));
        }
    }
    let Some((name, bytes)) = model else {
        // Nothing an importer reads: say so against the first name picked,
        // since there is no path to name instead.
        let named = files.get(0).map(|file| file.name()).unwrap_or_default();
        let _ = report.send(ImportEvent::Failed {
            source: named.clone(),
            message: format!("nothing an importer reads was picked ({named})"),
        });
        return;
    };
    running.set(running.get() + 1);
    task::park(ImportJob::new(
        Source::Chosen { name, bytes, with },
        project,
        report,
        running,
        cancel,
    ));
}

/// One file's bytes, or `None` where the page refused to read it.
async fn bytes_of(file: &File) -> Option<Vec<u8>> {
    let buffer = JsFuture::from(file.array_buffer()).await.ok()?;
    Some(Uint8Array::new(&buffer).to_vec())
}
