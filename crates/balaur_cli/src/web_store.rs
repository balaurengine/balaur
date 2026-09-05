//! Files kept in the browser, in IndexedDB.
//!
//! A tab has no directory. [`MemoryFs`] is the filesystem the engine reads
//! through, and everything written under one root — the project the editor
//! is editing, or a running game's user directory — is mirrored into
//! IndexedDB behind it, so the next visit starts where this one stopped.
//!
//! The contract is the desktop's. `std::fs::write` returns once the kernel
//! has the bytes, and the disk gets them when the kernel writes back; here a
//! write returns once memory has them, and the transaction that commits them
//! is *issued* before control leaves the task that wrote — a microtask
//! gathers one tick's writes into one transaction — so the browser commits
//! them whether or not the page runs again. What the desktop calls `fsync`
//! is [`FileBackend::sync`]: the next transaction asks for a durable commit,
//! which Chromium and Firefox honour and Safari treats as ordinary.
//!
//! Nothing is read back through the mirror while a tab runs. Memory is the
//! truth; IndexedDB is how the next tab is seeded.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::rc::{Rc, Weak};

use anyhow::Result;
use balaur::files::{FileBackend, MemoryFs, lexical};
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use web_sys::{IdbDatabase, IdbObjectStore, IdbRequest, IdbTransaction, IdbTransactionMode};

/// The database everything on this origin lives in.
const DB_NAME: &str = "balaur-projects";
const DB_VERSION: u32 = 1;

/// One record per file, keyed `<id>\0<root-relative path>`, holding
/// `{ b: bytes, m: mtime }`. A bare byte array is an older record and reads
/// back stamped with the moment it was loaded.
const FILES: &str = "files";

/// One record per id: `{ name, modified }`.
const META: &str = "meta";

/// A directory with nothing in it has no file to imply it, so it is kept
/// under a key ending in this and holding no bytes.
const DIR_MARK: char = '/';

thread_local! {
    /// The store this tab writes through, for the page's own calls, which
    /// arrive with no handle of their own.
    static LIVE: RefCell<Option<Rc<ProjectFs>>> = const { RefCell::new(None) };
}

/// What happened to a path since the last transaction was issued.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Change {
    Wrote,
    Removed,
}

/// Files in memory under one root, mirrored into IndexedDB as they change.
pub(crate) struct ProjectFs {
    inner: MemoryFs,
    /// Absent where the browser refuses IndexedDB, in a private window or
    /// with storage switched off. The engine runs; nothing is kept.
    db: Option<IdbDatabase>,
    /// Every key of this store starts with this, so one origin holds many.
    prefix: String,
    /// The root that is mirrored; keys are stored relative to it.
    root: PathBuf,
    /// The same, as the prefix `MemoryFs` stores its keys under.
    mounted: String,
    /// So a callback scheduled from a `&self` method can find the store
    /// again without going through the tab-wide slot.
    me: Weak<ProjectFs>,
    pending: RefCell<BTreeMap<String, Change>>,
    /// A name to record with the next transaction, given when a project is
    /// made from a pack rather than imported.
    pending_name: RefCell<Option<String>>,
    /// Paths a transaction has taken and not yet committed. Counted apart
    /// from `pending` so what is in flight still reads as unsaved.
    in_flight: Cell<usize>,
    /// Whether a microtask is already queued to issue what is pending.
    queued: Cell<bool>,
    /// Whether the next transaction asks for a durable commit.
    strict: Cell<bool>,
}

impl ProjectFs {
    /// Open `id`'s store and seed a filesystem at `root` with what it holds.
    pub(crate) async fn open(id: &str, root: &Path) -> Result<Rc<Self>, JsValue> {
        let db = match open_db().await {
            Ok(db) => Some(db),
            Err(why) => {
                tracing::warn!("this browser keeps no files: {why:?}");
                None
            }
        };
        let stored = match &db {
            Some(db) => read_all(db, &prefix_of(id)).await?,
            None => Vec::new(),
        };
        let fs = Rc::new_cyclic(|me| Self {
            inner: MemoryFs::with_clock(|| js_sys::Date::now() / 1000.0),
            db,
            prefix: prefix_of(id),
            root: root.to_path_buf(),
            mounted: mounted_at(root),
            me: me.clone(),
            pending: RefCell::new(BTreeMap::new()),
            pending_name: RefCell::new(None),
            in_flight: Cell::new(0),
            queued: Cell::new(false),
            strict: Cell::new(false),
        });
        for (rel, bytes, mtime) in stored {
            match rel.strip_suffix(DIR_MARK) {
                Some(dir) => {
                    let _ = fs.inner.mkdir(&root.join(dir));
                }
                None => fs.inner.restore(&root.join(&rel), &bytes, mtime),
            }
        }
        Ok(fs)
    }

    /// Whether the store held a project. One without a manifest is one the
    /// editor cannot open, so that is the test.
    pub(crate) fn is_empty(&self) -> bool {
        !self.inner.exists(&self.root.join("project.toml"))
    }

    /// Seed `root` from a pack's entries.
    ///
    /// Only what lands under the mirrored root is kept: the editor's own
    /// project is mounted beside the one it edits and is fetched fresh every
    /// boot rather than kept in someone's browser.
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

    /// Give the store's record a name, with the next transaction.
    pub(crate) fn name(&self, name: &str) {
        *self.pending_name.borrow_mut() = Some(name.to_string());
        self.queue();
    }

    /// Hold this store as the tab's, and see that what is pending when the
    /// page is hidden or left is issued at once rather than with the next
    /// tick, which may never come.
    pub(crate) fn install(self: &Rc<Self>) {
        LIVE.with(|live| *live.borrow_mut() = Some(Rc::clone(self)));
        let Some(window) = web_sys::window() else {
            return;
        };
        let leaving = Closure::<dyn Fn()>::new(|| {
            if let Some(fs) = ProjectFs::live() {
                fs.issue();
            }
        });
        for event in ["pagehide", "visibilitychange"] {
            let _ =
                window.add_event_listener_with_callback(event, leaving.as_ref().unchecked_ref());
        }
        // One store per tab, so the listener lives as long as the page.
        leaving.forget();
    }

    /// The store this tab writes through, if one is installed.
    pub(crate) fn live() -> Option<Rc<Self>> {
        LIVE.with(|live| live.borrow().clone())
    }

    /// How many paths are written but not yet committed. What a page asks
    /// before letting someone close the tab.
    pub(crate) fn unsaved(&self) -> usize {
        self.pending.borrow().len() + self.in_flight.get()
    }

    /// Everything under the root as it stands, root-relative, for a download.
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

    /// Issue what is pending and resolve once the browser has committed it.
    /// What the page calls before it lets someone leave.
    pub(crate) async fn flush(&self) -> Result<(), JsValue> {
        match self.issue() {
            Some(transaction) => settled(&transaction).await,
            None => Ok(()),
        }
    }

    /// The key `path` is mirrored under, or `None` for one outside the root
    /// or for the root itself, which is no file.
    fn key(&self, path: &Path) -> Option<String> {
        let rel = relative(&self.root, path)?;
        (!rel.is_empty()).then(|| format!("{}{rel}", self.prefix))
    }

    /// Note that `path` changed, and see that a transaction is coming.
    fn mark(&self, path: &Path, change: Change) {
        let Some(key) = self.key(path) else {
            return;
        };
        self.pending.borrow_mut().insert(key, change);
        self.queue();
    }

    /// Note the marker that keeps an empty directory.
    fn mark_dir(&self, path: &Path, change: Change) {
        let Some(key) = self.key(path) else {
            return;
        };
        self.pending
            .borrow_mut()
            .insert(format!("{key}{DIR_MARK}"), change);
        self.queue();
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

    /// See that what is pending is issued before this task ends. A microtask
    /// runs after the current callback and before the browser does anything
    /// else, so one tick's writes become one transaction and none of them
    /// wait on a timer.
    fn queue(&self) {
        if self.db.is_none() || self.queued.replace(true) {
            return;
        }
        let me = self.me.clone();
        let task = Closure::once_into_js(move || {
            if let Some(fs) = me.upgrade() {
                fs.issue();
            }
        });
        match web_sys::window() {
            Some(window) => window.queue_microtask(task.unchecked_ref()),
            None => self.queued.set(false),
        }
    }

    /// Open one transaction over everything pending and issue every request,
    /// synchronously. Commit is the browser's from here; the callbacks only
    /// keep the count honest and put a failed batch back.
    fn issue(&self) -> Option<IdbTransaction> {
        self.queued.set(false);
        let db = self.db.as_ref()?;
        let batch = std::mem::take(&mut *self.pending.borrow_mut());
        let name = self.pending_name.borrow_mut().take();
        if batch.is_empty() && name.is_none() {
            return None;
        }
        let strict = self.strict.replace(false);
        let transaction = match transaction(db, strict) {
            Ok(transaction) => transaction,
            Err(why) => {
                tracing::warn!("indexedDB refused a transaction: {why:?}");
                self.put_back(batch, name);
                return None;
            }
        };
        if let Err(why) = self.request_all(&transaction, &batch, name.as_deref()) {
            tracing::warn!("indexedDB refused a write: {why:?}");
            self.put_back(batch, name);
            return None;
        }
        let count = batch.len();
        self.in_flight.set(self.in_flight.get() + count);
        let me = self.me.clone();
        let on_done = Closure::once_into_js(move |_: web_sys::Event| {
            if let Some(fs) = me.upgrade() {
                fs.in_flight.set(fs.in_flight.get().saturating_sub(count));
            }
        });
        let me = self.me.clone();
        let on_err = Closure::once_into_js(move |_: web_sys::Event| {
            let Some(fs) = me.upgrade() else {
                return;
            };
            fs.in_flight.set(fs.in_flight.get().saturating_sub(count));
            tracing::warn!("keeping files in the browser failed; trying again");
            fs.put_back(batch, name);
        });
        transaction.set_oncomplete(Some(on_done.unchecked_ref()));
        transaction.set_onerror(Some(on_err.unchecked_ref()));
        transaction.set_onabort(Some(on_err.unchecked_ref()));
        Some(transaction)
    }

    /// Every put and delete of one batch, and the record that dates it.
    fn request_all(
        &self,
        transaction: &IdbTransaction,
        batch: &BTreeMap<String, Change>,
        name: Option<&str>,
    ) -> Result<(), JsValue> {
        let files = transaction.object_store(FILES)?;
        for (key, change) in batch {
            match change {
                Change::Removed => {
                    files.delete(&JsValue::from_str(key))?;
                }
                Change::Wrote => {
                    files.put_with_key(&self.record_for(key)?, &JsValue::from_str(key))?;
                }
            }
        }
        touch(&transaction.object_store(META)?, self.id(), name)
    }

    /// Return a batch the browser refused to what is pending, without
    /// overwriting anything written since, and try again.
    fn put_back(&self, batch: BTreeMap<String, Change>, name: Option<String>) {
        {
            let mut pending = self.pending.borrow_mut();
            for (key, change) in batch {
                pending.entry(key).or_insert(change);
            }
        }
        if let Some(name) = name {
            self.pending_name.borrow_mut().get_or_insert(name);
        }
        self.queue();
    }

    /// What a key's record holds: the file's bytes and stamp, or nothing for
    /// the marker that stands in for an empty directory.
    fn record_for(&self, key: &str) -> Result<JsValue, JsValue> {
        let rel = key.strip_prefix(&self.prefix).unwrap_or_default();
        if rel.is_empty() || rel.ends_with(DIR_MARK) {
            return Ok(js_sys::Uint8Array::new_with_length(0).into());
        }
        let path = self.root.join(rel);
        let bytes = self.inner.read(&path).unwrap_or_default();
        let mtime = self.inner.mtime(&path).unwrap_or(0.0);
        record(&bytes, mtime)
    }

    /// The id this store keeps, which its prefix was built from.
    fn id(&self) -> &str {
        self.prefix.trim_end_matches('\0')
    }
}

/// Every write goes to memory first and is issued to the mirror before the
/// task ends; a read never waits on the mirror, which is what lets a
/// synchronous script API keep files.
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

    /// The browser's `fsync`: the transaction carrying this write asks for a
    /// durable commit rather than the default one.
    fn sync(&self, _path: &Path) {
        self.strict.set(true);
        self.queue();
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

/// A store's key prefix. The separator cannot occur in a path, so one
/// store's range never runs into the next one's.
fn prefix_of(id: &str) -> String {
    format!("{id}\0")
}

/// The root as the forward-slashed prefix a memory filesystem stores its keys
/// under, trailing separator included.
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

/// One file's record: its bytes and when they were written.
fn record(bytes: &[u8], mtime: f64) -> Result<JsValue, JsValue> {
    let out = js_sys::Object::new();
    js_sys::Reflect::set(&out, &"b".into(), &js_sys::Uint8Array::from(bytes).into())?;
    js_sys::Reflect::set(&out, &"m".into(), &mtime.into())?;
    Ok(out.into())
}

/// A record's bytes and stamp, whichever shape it was written in.
fn unrecord(value: &JsValue) -> (Vec<u8>, f64) {
    if value.is_instance_of::<js_sys::Uint8Array>() {
        let bytes = js_sys::Uint8Array::new(value).to_vec();
        return (bytes, js_sys::Date::now() / 1000.0);
    }
    let bytes = js_sys::Reflect::get(value, &"b".into())
        .map(|b| js_sys::Uint8Array::new(&b).to_vec())
        .unwrap_or_default();
    let mtime = js_sys::Reflect::get(value, &"m".into())
        .ok()
        .and_then(|m| m.as_f64())
        .unwrap_or_else(|| js_sys::Date::now() / 1000.0);
    (bytes, mtime)
}

/// A read-write transaction over both stores, durable when asked.
///
/// The durable form is called as a page would call it: web-sys keeps
/// `durability` behind its unstable cfg, and a browser without the option
/// ignores the third argument, which leaves the ordinary transaction.
fn transaction(db: &IdbDatabase, strict: bool) -> Result<IdbTransaction, JsValue> {
    let names = js_sys::Array::of2(&FILES.into(), &META.into());
    if !strict {
        return db.transaction_with_str_sequence_and_mode(&names, IdbTransactionMode::Readwrite);
    }
    let options = js_sys::Object::new();
    js_sys::Reflect::set(&options, &"durability".into(), &"strict".into())?;
    let method =
        js_sys::Reflect::get(db.as_ref(), &"transaction".into())?.dyn_into::<js_sys::Function>()?;
    method
        .call3(db.as_ref(), &names, &"readwrite".into(), &options)?
        .dyn_into::<IdbTransaction>()
        .map_err(|_| JsValue::from_str("transaction() answered with no transaction"))
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

/// Every file stored under `prefix`: root-relative path, bytes, stamp.
async fn read_all(db: &IdbDatabase, prefix: &str) -> Result<Vec<(String, Vec<u8>, f64)>, JsValue> {
    let files = read_only(db, FILES)?;
    let range = range_of(prefix)?;
    // Both issued before either is awaited: a transaction commits once its
    // requests are done and control returns to the loop, so a request made
    // after an await can arrive at one that has already finished.
    let wanted_keys = files.get_all_keys_with_key(range.as_ref())?;
    let wanted_values = files.get_all_with_key(range.as_ref())?;
    let keys = js_sys::Array::from(&finished(&wanted_keys).await?);
    let values = js_sys::Array::from(&finished(&wanted_values).await?);
    let mut out = Vec::with_capacity(keys.length() as usize);
    for index in 0..keys.length() {
        let Some(rel) = keys
            .get(index)
            .as_string()
            .and_then(|key| key.strip_prefix(prefix).map(str::to_string))
        else {
            continue;
        };
        let (bytes, mtime) = unrecord(&values.get(index));
        out.push((rel, bytes, mtime));
    }
    Ok(out)
}

/// Write `entries` under `id`, replacing what it held, for a store that is
/// not open: what a folder someone chose becomes before the editor boots on
/// it.
pub(crate) async fn import(
    id: &str,
    name: &str,
    entries: Vec<(String, Vec<u8>)>,
) -> Result<(), JsValue> {
    let db = open_db().await?;
    remove_files(&db, id).await?;
    let prefix = prefix_of(id);
    let now = js_sys::Date::now() / 1000.0;
    let transaction = transaction(&db, false)?;
    let files = transaction.object_store(FILES)?;
    for (rel, bytes) in entries {
        let key = format!("{prefix}{}", rel.trim_start_matches('/'));
        files.put_with_key(&record(&bytes, now)?, &JsValue::from_str(&key))?;
    }
    touch(&transaction.object_store(META)?, id, Some(name))?;
    settled(&transaction).await
}

/// Every store on this origin, newest first, as `{ id, name, modified }`.
pub(crate) async fn list() -> Result<js_sys::Array, JsValue> {
    let db = open_db().await?;
    let meta = read_only(&db, META)?;
    let wanted_keys = meta.get_all_keys()?;
    let wanted_values = meta.get_all()?;
    let keys = js_sys::Array::from(&finished(&wanted_keys).await?);
    let values = js_sys::Array::from(&finished(&wanted_values).await?);
    let mut rows = Vec::with_capacity(keys.length() as usize);
    for index in 0..keys.length() {
        let entry = js_sys::Object::new();
        js_sys::Reflect::set(&entry, &"id".into(), &keys.get(index))?;
        let source = values.get(index);
        for field in ["name", "modified"] {
            let value = js_sys::Reflect::get(&source, &field.into()).unwrap_or(JsValue::UNDEFINED);
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

/// Forget a store: its files and the record that named it.
pub(crate) async fn delete(id: &str) -> Result<(), JsValue> {
    let db = open_db().await?;
    remove_files(&db, id).await?;
    let transaction = db.transaction_with_str_and_mode(META, IdbTransactionMode::Readwrite)?;
    transaction
        .object_store(META)?
        .delete(&JsValue::from_str(id))?;
    settled(&transaction).await
}

/// Drop every file record of one store, leaving its metadata alone.
async fn remove_files(db: &IdbDatabase, id: &str) -> Result<(), JsValue> {
    let transaction = db.transaction_with_str_and_mode(FILES, IdbTransactionMode::Readwrite)?;
    transaction
        .object_store(FILES)?
        .delete(range_of(&prefix_of(id))?.as_ref())?;
    settled(&transaction).await
}

/// Date a store's record, and name it when a name is given, without losing
/// whatever else it says.
fn touch(meta: &IdbObjectStore, id: &str, name: Option<&str>) -> Result<(), JsValue> {
    let key = JsValue::from_str(id);
    let request = meta.get(&key)?;
    let store = meta.clone();
    let read = request.clone();
    let name = name.map(str::to_string);
    let done = Closure::once_into_js(move |_: web_sys::Event| {
        let existing = read.result().unwrap_or(JsValue::UNDEFINED);
        let record = if existing.is_object() {
            js_sys::Object::from(existing)
        } else {
            js_sys::Object::new()
        };
        if let Some(name) = name {
            let _ = js_sys::Reflect::set(&record, &"name".into(), &name.into());
        }
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
/// character a path may hold, so the range is exactly one store.
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
