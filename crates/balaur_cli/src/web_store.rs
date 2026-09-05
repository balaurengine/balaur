//! Projects kept in the browser, in IndexedDB.
//!
//! The editor is a program whose whole job is reading and writing a project,
//! and a tab has no directory to keep one in. [`MemoryFs`] stays the
//! filesystem the engine reads through; every write is mirrored into
//! IndexedDB behind it, so a refresh finds the work still there.
//!
//! The mirror is write-behind because IndexedDB is asynchronous and
//! `FileBackend` is not: a write marks its path dirty and returns, and a
//! debounced task drains what accumulated. Nothing is ever read back through
//! the mirror while a tab runs — the memory filesystem is the truth, and
//! IndexedDB is only how the next tab starts where this one stopped.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use anyhow::Result;
use balaur::files::{FileBackend, MemoryFs, lexical};
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use web_sys::{IdbDatabase, IdbObjectStore, IdbRequest, IdbTransaction, IdbTransactionMode};

/// The database every project on this origin lives in.
const DB_NAME: &str = "balaur-projects";
const DB_VERSION: u32 = 1;

/// One record per file, keyed `<project id>\0<project-relative path>`.
const FILES: &str = "files";

/// One record per project: its id to `{ name, modified }`.
const META: &str = "meta";

/// How long a write waits for the next one before the mirror catches up. Long
/// enough that a burst of saves is one transaction, short enough that a tab
/// closed just after a save has already written it.
const FLUSH_DELAY_MS: i32 = 400;

/// A directory with nothing in it has no file to imply it, so it is kept
/// under a key ending in this and holding no bytes.
const DIR_MARK: char = '/';

thread_local! {
    /// The store this tab is editing through. The flush timer and the page's
    /// own calls arrive with no handle of their own.
    static LIVE: RefCell<Option<Rc<ProjectFs>>> = const { RefCell::new(None) };
}

/// What happened to a path since the last flush.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Change {
    Wrote,
    Removed,
}

/// A project's files in memory, mirrored into IndexedDB as they change.
pub(crate) struct ProjectFs {
    inner: MemoryFs,
    db: IdbDatabase,
    /// Every key of this project starts with this, so one origin holds many.
    prefix: String,
    /// Where the project is mounted; keys are stored relative to it.
    root: PathBuf,
    /// The same, as the absolute prefix `MemoryFs` stores its keys under.
    mounted: String,
    pending: RefCell<BTreeMap<String, Change>>,
    scheduled: Cell<bool>,
    flushing: Cell<bool>,
}

impl ProjectFs {
    /// Open `id`'s store and seed a filesystem at `root` with what it holds.
    pub(crate) async fn open(id: &str, root: &Path) -> Result<Rc<Self>, JsValue> {
        let db = open_db().await?;
        let fs = Rc::new(Self {
            inner: MemoryFs::new(),
            db,
            prefix: prefix_of(id),
            root: root.to_path_buf(),
            mounted: mounted_at(root),
            pending: RefCell::new(BTreeMap::new()),
            scheduled: Cell::new(false),
            flushing: Cell::new(false),
        });
        for (rel, bytes) in read_all(&fs.db, &fs.prefix).await? {
            let _ = match rel.strip_suffix(DIR_MARK) {
                Some(dir) => fs.inner.mkdir(&root.join(dir)),
                None => fs.inner.write(&root.join(&rel), &bytes),
            };
        }
        Ok(fs)
    }

    /// Whether anything was stored for this project. A project without a
    /// manifest is one the editor cannot open, so that is the test.
    pub(crate) fn is_empty(&self) -> bool {
        !self.inner.exists(&self.root.join("project.toml"))
    }

    /// Seed `root` from a pack's entries.
    ///
    /// Only what lands under the project's own root is mirrored: the editor
    /// is mounted here too, and its pack is fetched fresh every boot rather
    /// than kept in someone's browser.
    pub(crate) fn seed_at(
        &self,
        root: &Path,
        entries: impl IntoIterator<Item = (String, Vec<u8>)>,
    ) {
        for (rel, bytes) in entries {
            let path = root.join(rel.trim_start_matches('/'));
            if self.inner.write(&path, &bytes).is_ok() {
                self.mark(&path, Change::Wrote);
            }
        }
    }

    /// Hold this store as the tab's, so the flush timer and the page reach it.
    pub(crate) fn install(self: &Rc<Self>) {
        LIVE.with(|live| *live.borrow_mut() = Some(Rc::clone(self)));
    }

    /// The store this tab is editing through, if a project is open.
    pub(crate) fn live() -> Option<Rc<Self>> {
        LIVE.with(|live| live.borrow().clone())
    }

    /// How many paths are written but not yet mirrored. What a page asks
    /// before letting someone close the tab.
    pub(crate) fn unsaved(&self) -> usize {
        self.pending.borrow().len()
    }

    /// The whole project as it stands, project-relative, for a download.
    pub(crate) fn files(&self) -> Vec<(String, Vec<u8>)> {
        self.inner
            .snapshot()
            .into_iter()
            .filter_map(|(key, bytes)| {
                let rel = key.strip_prefix(&self.mounted)?;
                Some((rel.to_string(), bytes))
            })
            .collect()
    }

    /// The key `path` is mirrored under, or `None` for one outside the root
    /// or for the root itself, which is no file.
    fn key(&self, path: &Path) -> Option<String> {
        let rel = relative(&self.root, path)?;
        (!rel.is_empty()).then(|| format!("{}{rel}", self.prefix))
    }

    /// Note that `path` changed, and see that a flush is coming.
    fn mark(&self, path: &Path, change: Change) {
        let Some(key) = self.key(path) else {
            return;
        };
        self.pending.borrow_mut().insert(key, change);
        self.schedule();
    }

    /// Note the marker that keeps an empty directory.
    fn mark_dir(&self, path: &Path, change: Change) {
        let Some(key) = self.key(path) else {
            return;
        };
        self.pending
            .borrow_mut()
            .insert(format!("{key}{DIR_MARK}"), change);
        self.schedule();
    }

    /// Note `path` and, when it is a directory, everything under it.
    fn mark_tree(&self, path: &Path, change: Change) {
        if !self.inner.is_dir(path) {
            self.mark(path, change);
            return;
        }
        self.mark_dir(path, change);
        for child in walk(&self.inner, path) {
            self.mark(&child, change);
        }
    }

    /// Ask for a flush, unless one is already on its way.
    fn schedule(&self) {
        if self.scheduled.replace(true) {
            return;
        }
        let tick = Closure::once_into_js(move || {
            let Some(fs) = ProjectFs::live() else {
                return;
            };
            fs.scheduled.set(false);
            wasm_bindgen_futures::spawn_local(async move {
                if let Err(why) = fs.flush().await {
                    tracing::warn!("keeping the project in the browser failed: {why:?}");
                }
            });
        });
        let armed = web_sys::window().and_then(|w| {
            w.set_timeout_with_callback_and_timeout_and_arguments_0(
                tick.unchecked_ref(),
                FLUSH_DELAY_MS,
            )
            .ok()
        });
        if armed.is_none() {
            self.scheduled.set(false);
        }
    }

    /// Write everything that changed into IndexedDB, as one transaction.
    ///
    /// A write that lands while this is awaiting goes into a fresh batch and
    /// schedules its own flush, so nothing is lost by the swap.
    pub(crate) async fn flush(self: &Rc<Self>) -> Result<(), JsValue> {
        if self.flushing.replace(true) {
            self.schedule();
            return Ok(());
        }
        let batch = std::mem::take(&mut *self.pending.borrow_mut());
        let result = self.write_batch(&batch).await;
        self.flushing.set(false);
        if result.is_err() {
            // Put them back, so the next flush tries again rather than
            // dropping the work.
            let mut pending = self.pending.borrow_mut();
            for (key, change) in batch {
                pending.entry(key).or_insert(change);
            }
        }
        result
    }

    /// One transaction over `batch`, and the metadata that dates it.
    async fn write_batch(&self, batch: &BTreeMap<String, Change>) -> Result<(), JsValue> {
        if batch.is_empty() {
            return Ok(());
        }
        let names = js_sys::Array::of2(&FILES.into(), &META.into());
        let transaction = self
            .db
            .transaction_with_str_sequence_and_mode(&names, IdbTransactionMode::Readwrite)?;
        let files = transaction.object_store(FILES)?;
        for (key, change) in batch {
            match change {
                Change::Removed => {
                    files.delete(&JsValue::from_str(key))?;
                }
                Change::Wrote => {
                    files.put_with_key(&self.bytes_for(key), &JsValue::from_str(key))?;
                }
            }
        }
        touch(&transaction.object_store(META)?, self.id())?;
        settled(&transaction).await
    }

    /// What a key's record holds: the file's bytes, or none for the marker
    /// that stands in for an empty directory.
    fn bytes_for(&self, key: &str) -> JsValue {
        let rel = key.strip_prefix(&self.prefix).unwrap_or_default();
        if rel.is_empty() || rel.ends_with(DIR_MARK) {
            return js_sys::Uint8Array::new_with_length(0).into();
        }
        let bytes = self.inner.read(&self.root.join(rel)).unwrap_or_default();
        js_sys::Uint8Array::from(bytes.as_slice()).into()
    }

    /// The project id this store keeps, which its prefix was built from.
    fn id(&self) -> &str {
        self.prefix.trim_end_matches('\0')
    }
}

/// Every write goes to memory first and is mirrored after; a read never waits
/// on the mirror, which is what lets a synchronous script API keep files.
impl FileBackend for ProjectFs {
    fn read(&self, path: &Path) -> Result<Vec<u8>> {
        self.inner.read(path)
    }

    fn write(&self, path: &Path, bytes: &[u8]) -> Result<()> {
        self.inner.write(path, bytes)?;
        self.mark(path, Change::Wrote);
        Ok(())
    }

    fn exists(&self, path: &Path) -> bool {
        self.inner.exists(path)
    }

    fn is_dir(&self, path: &Path) -> bool {
        self.inner.is_dir(path)
    }

    fn remove(&self, path: &Path) -> Result<bool> {
        self.mark_tree(path, Change::Removed);
        self.inner.remove(path)
    }

    fn mkdir(&self, path: &Path) -> Result<()> {
        self.inner.mkdir(path)?;
        self.mark_dir(path, Change::Wrote);
        Ok(())
    }

    fn rename(&self, from: &Path, to: &Path) -> Result<()> {
        self.mark_tree(from, Change::Removed);
        self.inner.rename(from, to)?;
        if self.inner.is_dir(to) {
            self.mark_dir(to, Change::Wrote);
            for child in walk(&self.inner, to) {
                self.mark(&child, Change::Wrote);
            }
        } else {
            self.mark(to, Change::Wrote);
        }
        Ok(())
    }

    fn mtime(&self, path: &Path) -> Option<f64> {
        self.inner.mtime(path)
    }

    fn list(&self, path: &Path) -> Vec<(String, bool)> {
        self.inner.list(path)
    }

    fn canonicalize(&self, path: &Path) -> PathBuf {
        self.inner.canonicalize(path)
    }
}

/// Every file under `dir`, as absolute paths.
fn walk(fs: &MemoryFs, dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(next) = stack.pop() {
        for (name, is_dir) in fs.list(&next) {
            let child = next.join(name);
            if is_dir {
                stack.push(child);
            } else {
                out.push(child);
            }
        }
    }
    out
}

/// A project's key prefix. The separator cannot occur in a path, so one
/// project's range never runs into the next one's.
fn prefix_of(id: &str) -> String {
    format!("{id}\0")
}

/// The root as the absolute, forward-slashed prefix a memory filesystem
/// stores its keys under, trailing separator included.
fn mounted_at(root: &Path) -> String {
    let text = lexical(root).to_string_lossy().replace('\\', "/");
    format!("{}/", text.trim_end_matches('/'))
}

/// `path` relative to `root`, forward-slashed, or `None` for one outside it.
fn relative(root: &Path, path: &Path) -> Option<String> {
    Some(
        lexical(path)
            .strip_prefix(lexical(root))
            .ok()?
            .to_string_lossy()
            .replace('\\', "/"),
    )
}

/// Open the database, creating its stores the first time.
async fn open_db() -> Result<IdbDatabase, JsValue> {
    let factory = web_sys::window()
        .and_then(|w| w.indexed_db().ok().flatten())
        .ok_or_else(|| JsValue::from_str("this browser has no IndexedDB"))?;
    let request = factory.open_with_u32(DB_NAME, DB_VERSION)?;
    let upgrade = Closure::once_into_js(move |event: web_sys::Event| {
        let Some(db) = event
            .target()
            .and_then(|t| t.dyn_into::<web_sys::IdbOpenDbRequest>().ok())
            .and_then(|r| r.result().ok())
            .and_then(|r| r.dyn_into::<IdbDatabase>().ok())
        else {
            return;
        };
        let existing = db.object_store_names();
        for store in [FILES, META] {
            if !existing.contains(store) {
                let _ = db.create_object_store(store);
            }
        }
    });
    request.set_onupgradeneeded(Some(upgrade.unchecked_ref()));
    finished(request.as_ref())
        .await?
        .dyn_into::<IdbDatabase>()
        .map_err(|_| JsValue::from_str("indexedDB opened something that is not a database"))
}

/// Every file stored under `prefix`, project-relative.
async fn read_all(db: &IdbDatabase, prefix: &str) -> Result<Vec<(String, Vec<u8>)>, JsValue> {
    let files = read_only(db, FILES)?;
    let range = range_of(prefix)?;
    let keys = js_sys::Array::from(&finished(&files.get_all_keys_with_key(range.as_ref())?).await?);
    let values = js_sys::Array::from(&finished(&files.get_all_with_key(range.as_ref())?).await?);
    let mut out = Vec::with_capacity(keys.length() as usize);
    for index in 0..keys.length() {
        let Some(rel) = keys
            .get(index)
            .as_string()
            .and_then(|key| key.strip_prefix(prefix).map(str::to_string))
        else {
            continue;
        };
        out.push((rel, js_sys::Uint8Array::new(&values.get(index)).to_vec()));
    }
    Ok(out)
}

/// Write `entries` into a project that is not open, replacing what it held.
pub(crate) async fn import(
    id: &str,
    name: &str,
    entries: Vec<(String, Vec<u8>)>,
) -> Result<(), JsValue> {
    let db = open_db().await?;
    remove_files(&db, id).await?;
    let prefix = prefix_of(id);
    let names = js_sys::Array::of2(&FILES.into(), &META.into());
    let transaction =
        db.transaction_with_str_sequence_and_mode(&names, IdbTransactionMode::Readwrite)?;
    let files = transaction.object_store(FILES)?;
    for (rel, bytes) in entries {
        let key = format!("{prefix}{}", rel.trim_start_matches('/'));
        let value = js_sys::Uint8Array::from(bytes.as_slice());
        files.put_with_key(&value.into(), &JsValue::from_str(&key))?;
    }
    let record = js_sys::Object::new();
    js_sys::Reflect::set(&record, &"name".into(), &name.into())?;
    js_sys::Reflect::set(&record, &"modified".into(), &js_sys::Date::now().into())?;
    transaction
        .object_store(META)?
        .put_with_key(&record, &JsValue::from_str(id))?;
    settled(&transaction).await
}

/// Every project on this origin, newest first, as `{ id, name, modified }`.
pub(crate) async fn list() -> Result<js_sys::Array, JsValue> {
    let db = open_db().await?;
    let meta = read_only(&db, META)?;
    let keys = js_sys::Array::from(&finished(&meta.get_all_keys()?).await?);
    let values = js_sys::Array::from(&finished(&meta.get_all()?).await?);
    let mut rows = Vec::with_capacity(keys.length() as usize);
    for index in 0..keys.length() {
        let record = values.get(index);
        let entry = js_sys::Object::new();
        js_sys::Reflect::set(&entry, &"id".into(), &keys.get(index))?;
        for field in ["name", "modified"] {
            let value = js_sys::Reflect::get(&record, &field.into()).unwrap_or(JsValue::UNDEFINED);
            js_sys::Reflect::set(&entry, &field.into(), &value)?;
        }
        rows.push((number(&entry, "modified"), entry));
    }
    rows.sort_by(|a, b| b.0.total_cmp(&a.0));
    let out = js_sys::Array::new();
    for (_, entry) in rows {
        out.push(&entry);
    }
    Ok(out)
}

/// Forget a project: its files and the record that named it.
pub(crate) async fn delete(id: &str) -> Result<(), JsValue> {
    let db = open_db().await?;
    remove_files(&db, id).await?;
    let transaction = db.transaction_with_str_and_mode(META, IdbTransactionMode::Readwrite)?;
    transaction
        .object_store(META)?
        .delete(&JsValue::from_str(id))?;
    settled(&transaction).await
}

/// Drop every file record of one project, leaving its metadata alone.
async fn remove_files(db: &IdbDatabase, id: &str) -> Result<(), JsValue> {
    let transaction = db.transaction_with_str_and_mode(FILES, IdbTransactionMode::Readwrite)?;
    transaction
        .object_store(FILES)?
        .delete(range_of(&prefix_of(id))?.as_ref())?;
    settled(&transaction).await
}

/// Date a project's record without disturbing the name it was given.
fn touch(meta: &IdbObjectStore, id: &str) -> Result<(), JsValue> {
    let key = JsValue::from_str(id);
    let request = meta.get(&key)?;
    let store = meta.clone();
    let read = request.clone();
    let done = Closure::once_into_js(move |_: web_sys::Event| {
        let existing = read.result().unwrap_or(JsValue::UNDEFINED);
        let record = if existing.is_object() {
            js_sys::Object::from(existing)
        } else {
            js_sys::Object::new()
        };
        let _ = js_sys::Reflect::set(&record, &"modified".into(), &js_sys::Date::now().into());
        let _ = store.put_with_key(&record, &key);
    });
    request.set_onsuccess(Some(done.unchecked_ref()));
    Ok(())
}

/// A field of a JS record as a number, or zero when it is not one.
fn number(value: &JsValue, field: &str) -> f64 {
    js_sys::Reflect::get(value, &field.into())
        .ok()
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0)
}

/// Every key starting with `prefix`. The high sentinel sorts after every
/// character a path may hold, so the range is exactly one project.
fn range_of(prefix: &str) -> Result<web_sys::IdbKeyRange, JsValue> {
    web_sys::IdbKeyRange::bound(
        &JsValue::from_str(prefix),
        &JsValue::from_str(&format!("{prefix}\u{ffff}")),
    )
}

/// One store, for reading.
fn read_only(db: &IdbDatabase, name: &str) -> Result<IdbObjectStore, JsValue> {
    db.transaction_with_str(name)?.object_store(name)
}

/// Await one request, resolving to whatever it read or wrote.
async fn finished(request: &IdbRequest) -> Result<JsValue, JsValue> {
    let promise = js_sys::Promise::new(&mut |resolve, reject| {
        let ok = request.clone();
        let on_ok = Closure::once_into_js(move |_: web_sys::Event| {
            let _ = resolve.call1(&JsValue::NULL, &ok.result().unwrap_or(JsValue::UNDEFINED));
        });
        let failed = request.clone();
        let on_err = Closure::once_into_js(move |_: web_sys::Event| {
            let _ = reject.call1(&JsValue::NULL, &fault(failed.error().ok().flatten()));
        });
        request.set_onsuccess(Some(on_ok.unchecked_ref()));
        request.set_onerror(Some(on_err.unchecked_ref()));
    });
    JsFuture::from(promise).await
}

/// Await a whole transaction, which is what makes a batch one write.
async fn settled(transaction: &IdbTransaction) -> Result<(), JsValue> {
    let promise = js_sys::Promise::new(&mut |resolve, reject| {
        let on_done = Closure::once_into_js(move |_: web_sys::Event| {
            let _ = resolve.call0(&JsValue::NULL);
        });
        let failed = transaction.clone();
        let on_err = Closure::once_into_js(move |_: web_sys::Event| {
            let _ = reject.call1(&JsValue::NULL, &fault(failed.error()));
        });
        transaction.set_oncomplete(Some(on_done.unchecked_ref()));
        transaction.set_onerror(Some(on_err.unchecked_ref()));
    });
    JsFuture::from(promise).await.map(|_| ())
}

/// A DOM exception as something with a message, whatever the browser gave.
fn fault(error: Option<web_sys::DomException>) -> JsValue {
    error.map_or_else(
        || JsValue::from_str("indexedDB refused the request"),
        Into::into,
    )
}
