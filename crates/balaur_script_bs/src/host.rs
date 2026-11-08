//! `.bs` scripts behind the seam: every file a unit of the project's one
//! instance, or of the one beside it for a file from outside the project,
//! one host-held object per node, its lifecycle methods called by name when
//! the script has them.

use crate::api::{self, Registry};
use crate::convert::{from_neutral, to_neutral};
use crate::vocabulary::words;
use anyhow::{Context, Result, anyhow};
use balaur_bs::runtime::{HostFns, Limits, Object, Reentry, Runtime, Value as L};
use balaur_bs::{Compiled, HostApi, Literal, Options, Ty};
use balaur_core::{Engine, Pack};
use balaur_script::{Module, NodeId, Pause, ScriptCompiler, ScriptHost, StepMode, Value};
use debug::Held;
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
use std::rc::{Rc, Weak};
use std::sync::Arc;
use tasks::{Pending, WaitingStart};

mod debug;
mod engine_facing;
mod holds;
mod profile;
mod tasks;
mod tools;
mod units;

pub const EXTENSION: &str = "bs";
/// Under the game's own data folder: what wasmtime compiled, kept across runs.
const CACHE_DIR: &str = "bs-cache";
/// An instance's memory: what grows past this traps rather than taking the
/// game with it.
const MEMORY_CAP: usize = 1 << 30;
/// A browser's engine meters no fuel, so a build for one counts its own.
const METER: bool = cfg!(all(target_arch = "wasm32", feature = "web"));
/// The project instance's own unit, which has no code: every file a unit
/// linked to it.
const PROJECT_KEY: &str = "#project";

/// Which instance a file is a unit of.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Place {
    /// The project's.
    Project,
    /// The one beside it: files from outside the project, as the editor reads
    /// a game's scripts, so stopping them holds nothing of the editor's; and
    /// a file a native build lacks, until the project has moved off it.
    Aside,
}

const PLACES: [Place; 2] = [Place::Project, Place::Aside];

/// Where scripts run: a runtime, and the way into it while it is busy in an
/// engine call.
struct Home {
    rt: RefCell<Runtime>,
    reentry: Reentry,
    /// A loaded native build runs it: only a unit the build has joins it.
    native: bool,
    /// It runs on the engine a debugger's stop can park a call on.
    parking: bool,
}

impl Home {
    fn new(rt: Runtime, parking: bool) -> Rc<Home> {
        Rc::new(Home {
            reentry: rt.reentry(),
            native: rt.runs_natively(),
            parking,
            rt: RefCell::new(rt),
        })
    }

    /// `direct` on the runtime, or `inside` on the way in while the runtime
    /// is busy in an engine call: a script reaching a node of its instance.
    fn either<R>(
        &self,
        direct: impl FnOnce(&mut Runtime) -> R,
        inside: impl FnOnce(&Reentry) -> R,
    ) -> R {
        match self.rt.try_borrow_mut() {
            Ok(mut rt) => direct(&mut rt),
            Err(_) => inside(&self.reentry),
        }
    }
}

struct File {
    home: Rc<Home>,
    /// The instance `home` is.
    place: Place,
    /// Its unit in `home`'s instance.
    unit: usize,
    /// Whether it runs from the native build an export made.
    native: bool,
    compiled: Compiled,
    ty: String,
}

impl File {
    fn either<R>(
        &self,
        direct: impl FnOnce(&mut Runtime) -> R,
        inside: impl FnOnce(&Reentry) -> R,
    ) -> R {
        self.home.either(direct, inside)
    }

    fn drop_object(&self, obj: Object) {
        let dropped = match self.home.rt.try_borrow_mut() {
            Ok(mut rt) => {
                rt.drop_object(obj);
                Ok(())
            }
            Err(_) => self.home.reentry.drop_object(obj),
        };
        if let Err(e) = dropped {
            tracing::error!("{e}");
        }
    }

    fn method(&self, name: &str) -> Option<&balaur_bs::FnInfo> {
        self.compiled.meta.method(&self.ty, name)
    }
    fn field_ty(&self, name: &str) -> Option<Ty> {
        self.compiled
            .meta
            .fields(&self.ty)?
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, t)| t.clone())
    }
}

struct Instance {
    node: u64,
    path: String,
    obj: RefCell<Option<Object>>,
    /// Where `obj` lives in its instance, for `node.script` to find while a
    /// call has it out.
    pointer: Cell<i32>,
    /// A second holder of `obj` while a call has it, which a call made into
    /// the same instance meanwhile runs on (`Runtime::share`).
    lent: RefCell<Option<Object>>,
    /// Detached: a pass over the instances that began before still holds
    /// it, and calls nothing on it.
    gone: Cell<bool>,
}

/// Where [`BsHost::of`] finds the host; weak, since the host holds the engine.
struct Registered(Weak<Inner>);

struct Inner {
    engine: Engine,
    root: PathBuf,
    pack: Option<Pack>,
    registry: Rc<Registry>,
    api: RefCell<Option<HostApi>>,
    files: RefCell<BTreeMap<String, Rc<File>>>,
    /// The instance the project's files share, made with the first.
    project: RefCell<Option<Rc<Home>>>,
    /// The instance beside it, made with the first file that needs it.
    aside: RefCell<Option<Rc<Home>>>,
    instances: RefCell<Vec<Rc<Instance>>>,
    /// Instances of a parked scene, and what they await: no pass or call
    /// finds them, and a wake waits in the runtime until they come back.
    parked: RefCell<Vec<Rc<Instance>>>,
    parked_tasks: RefCell<Vec<Pending>>,
    held: RefCell<Vec<u64>>,
    holds: Cell<u32>,
    /// What the file watcher heard, and the watcher itself; `None` in a
    /// run that does not watch (a pack, or `watch` off). Never a poll: the
    /// watcher wakes the loop when a file is written.
    events: Option<std::sync::mpsc::Receiver<notify::Result<notify::Event>>>,
    _watcher: Option<notify::RecommendedWatcher>,
    /// `root` as the watcher reports paths under it.
    watch_root: PathBuf,
    /// Each `path` dependency's folder as the watcher reports paths under
    /// it, with the `deps/<alias>` its scripts are keyed under.
    watch_mounts: Vec<(PathBuf, String)>,
    /// Every engine function a script compiled for export calls, as
    /// [`api::line`]: what the pack records.
    called: RefCell<std::collections::BTreeSet<String>>,
    tasks: RefCell<Vec<Pending>>,
    /// Async methods asked of an instance busy in the call that asked.
    waiting_starts: RefCell<Vec<WaitingStart>>,
    /// Files built with probes, their instance on an engine that can hold a
    /// call at a line.
    debugging: RefCell<HashSet<String>>,
    /// The lines a debugger asked to stop at, by file, as it asked for them.
    breaks: RefCell<BTreeMap<String, Vec<usize>>>,
    stopped: RefCell<Option<Held>>,
    /// A script of the stopped instance asked it to go on: between frames.
    resume_owed: Cell<Option<StepMode>>,
    /// Every loaded file stops at the next line: asked of the instance its
    /// probes are built into.
    pause_owed: Cell<bool>,
    /// Every file built with probes, so a trap stops at its line.
    break_on_error: Cell<bool>,
    /// Files are built with counters (`host/profile.rs`).
    profiling: Cell<bool>,
    /// Each script's calls from the engine and the statements they ran,
    /// since profiling started.
    tally: RefCell<HashMap<String, (u64, u64)>>,
    /// Files to rebuild between frames for another reason than an edit: a
    /// profiler switched on or off.
    rebuild: RefCell<Vec<String>>,
    /// Files whose code changed, and whose instances are owed `on_hot_reload`
    /// once the build lands.
    reload_owed: RefCell<Vec<String>>,
    /// Each file's unit, by key and how it was built: a script links the
    /// units of the modules it uses, each built once for every script
    /// ([`BsHost::unit`]).
    units: RefCell<HashMap<String, HashMap<units::Build, Arc<Compiled>>>>,
    /// The module files read for their declarations.
    parsed: balaur_bs::Parsed,
    /// Each file's source as last read, or `None` for no such file: a unit
    /// resolves every `use` of every module it reads. An edit drops it.
    sources: RefCell<HashMap<String, Option<Rc<str>>>>,
    /// Each file's text as last read, hashed: a watcher event for a file that
    /// still holds it is no edit.
    read: RefCell<HashMap<String, String>>,
    /// The engine's API, by hash: a unit kept on disk was built against one.
    api_hash: Cell<Option<String>>,
    /// A unit has no module in the loaded native build (it changed since the
    /// build was made), so the project's instance runs on its engine.
    skip_native: Cell<bool>,
}

/// The `.bs` backend. Cheap to clone; clones share the one host.
#[derive(Clone)]
pub struct BsHost {
    inner: Rc<Inner>,
}

pub(crate) fn key_of(path: &str) -> String {
    path.trim_start_matches("./").replace('\\', "/")
}

/// The instance a file belongs in: a file from outside the project the host
/// runs, as the editor reads a game's scripts, has an absolute path for a key.
fn place_of(key: &str) -> Place {
    if balaur_core::files::rooted(std::path::Path::new(key)) {
        Place::Aside
    } else {
        Place::Project
    }
}

impl BsHost {
    pub fn new(engine: Engine, root: &std::path::Path, pack: Option<Pack>, watch: bool) -> Self {
        use notify::Watcher as _;
        let watch_root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
        // A `path` dependency's folder is outside the root, so it is watched
        // on its own and its edits keyed back under `deps/<alias>`.
        let watch_mounts: Vec<(PathBuf, String)> = balaur_core::dependencies::installed(&engine)
            .iter()
            .filter(|dep| dep.game == root)
            .filter_map(|dep| {
                let to = dep.mount.as_ref()?;
                Some((
                    to.canonicalize().unwrap_or_else(|_| to.clone()),
                    balaur_core::dependencies::folder(&dep.alias),
                ))
            })
            .collect();
        let (watcher, events) = if watch && pack.is_none() {
            // The watcher's own thread hands file events in and nothing goes out:
            // the channel never leaves the process, so a replay has nothing to stop.
            let (tx, rx) = std::sync::mpsc::channel(); // in-process: file events only
            match notify::recommended_watcher(move |res| {
                let _ = tx.send(res);
                balaur_core::wake::wake();
            })
            .and_then(|mut watcher| {
                watcher.watch(&watch_root, notify::RecursiveMode::Recursive)?;
                for (folder, _) in &watch_mounts {
                    watcher.watch(folder, notify::RecursiveMode::Recursive)?;
                }
                Ok(watcher)
            }) {
                Ok(watcher) => (Some(watcher), Some(rx)),
                Err(why) => {
                    tracing::error!("watching {} for script edits: {why}", watch_root.display());
                    (None, None)
                }
            }
        } else {
            (None, None)
        };
        BsHost {
            inner: Rc::new(Inner {
                engine,
                root: root.to_path_buf(),
                pack,
                registry: Rc::default(),
                api: RefCell::new(None),
                files: RefCell::default(),
                project: RefCell::default(),
                aside: RefCell::default(),
                instances: RefCell::default(),
                parked: RefCell::default(),
                parked_tasks: RefCell::default(),
                reload_owed: RefCell::default(),
                held: RefCell::default(),
                holds: Cell::new(0),
                events,
                _watcher: watcher,
                watch_root,
                watch_mounts,
                called: RefCell::default(),
                tasks: RefCell::default(),
                waiting_starts: RefCell::default(),
                debugging: RefCell::default(),
                breaks: RefCell::default(),
                stopped: RefCell::new(None),
                resume_owed: Cell::new(None),
                pause_owed: Cell::new(false),
                break_on_error: Cell::new(false),
                profiling: Cell::new(false),
                tally: RefCell::default(),
                rebuild: RefCell::default(),
                units: RefCell::default(),
                parsed: balaur_bs::Parsed::default(),
                sources: RefCell::default(),
                read: RefCell::default(),
                api_hash: Cell::new(None),
                skip_native: Cell::new(false),
            }),
        }
    }

    /// Whether `path` is a script this backend runs.
    /// Whether the loaded script at `path` runs from a native build an
    /// export made; `None` when it is not loaded.
    #[must_use]
    pub fn runs_natively(&self, path: &str) -> Option<bool> {
        self.inner
            .files
            .borrow()
            .get(&key_of(path))
            .map(|f| f.native)
    }

    pub fn claims(path: &str) -> bool {
        std::path::Path::new(path)
            .extension()
            .is_some_and(|e| e == EXTENSION)
    }

    /// Make this host the one [`BsHost::of`] finds on `eng`.
    pub fn register(&self, eng: &Engine) {
        eng.insert_resource(Registered(Rc::downgrade(&self.inner)));
    }

    /// The host [`BsHost::register`] put on `eng`, while it lives.
    pub fn of(eng: &Engine) -> Option<BsHost> {
        let registered = eng.try_resource::<Registered>()?;
        let inner = registered.borrow().0.upgrade()?;
        Some(BsHost { inner })
    }

    // A pack holds each script's source; a key it lacks is read through `fs`,
    // as the browser editor plays a project's scripts from under the game root.
    fn source(&self, key: &str) -> Result<String> {
        let packed = self
            .inner
            .pack
            .as_ref()
            .and_then(|pack| pack.scripts.get(key).cloned());
        let bytes = match packed {
            Some(bytes) => bytes,
            None => balaur_core::files::backend(&self.inner.engine)
                .read(&self.inner.root.join(key))
                .with_context(|| format!("reading {key}"))?,
        };
        let hash = balaur_bs::module_hash(&bytes);
        self.inner.read.borrow_mut().insert(key.to_string(), hash);
        String::from_utf8(bytes).with_context(|| format!("{key} is not UTF-8"))
    }

    /// What the scripts of the instance at `place` call out to: the engine,
    /// and `node.script`, which finds a node's object only in that instance.
    fn host_fns(&self, place: Place) -> HostFns {
        let (engine, registry) = (self.inner.engine.clone(), self.inner.registry.clone());
        let host = Rc::downgrade(&self.inner);
        HostFns::default()
            .bind_engine(move |key, ret| crate::bind(&engine, &registry, key, ret))
            .scripts(move |node, file, class| {
                let inner = host.upgrade()?;
                let inst = inner
                    .instances
                    .borrow()
                    .iter()
                    .find(|inst| inst.node == node.cast_unsigned())
                    .cloned()?;
                if inst.path != file || inst.gone.get() || balaur_bs::script_class(file) != class {
                    return None;
                }
                let same = inner
                    .files
                    .borrow()
                    .get(file)
                    .is_some_and(|found| found.place == place);
                same.then(|| inst.pointer.get())
            })
    }

    // `source`, kept until an edit: a unit's build reads every module it
    // uses, and every script uses the same ones.
    fn kept_source(&self, key: &str) -> Option<Rc<str>> {
        if let Some(kept) = self.inner.sources.borrow().get(key) {
            return kept.clone();
        }
        let read: Option<Rc<str>> = self.source(key).ok().map(Into::into);
        self.inner
            .sources
            .borrow_mut()
            .insert(key.to_string(), read.clone());
        read
    }

    /// Where `use name;` in a file finds `name.bs`: beside it, then in each
    /// folder above it, from the pack or the project. A file in a dependency
    /// stops at the dependency's folder, so it never links a game's module
    /// of the same name.
    fn modules(&self) -> balaur_bs::Modules {
        let host = Rc::downgrade(&self.inner);
        balaur_bs::Modules(Rc::new(move |from: &str, name: &str| {
            let inner = host.upgrade()?;
            let me = BsHost { inner };
            // `use gamend::core;` is `gamend/core.bs`.
            let file = format!("{}.{EXTENSION}", name.replace("::", "/"));
            let stop = balaur_core::dependencies::boundary(&me.inner.engine, from);
            let mut dir = std::path::Path::new(from).parent();
            while let Some(d) = dir {
                let key = key_of(&d.join(&file).to_string_lossy());
                if let Some(source) = me.kept_source(&key) {
                    return Some((key, source.to_string()));
                }
                if stop.as_deref() == Some(d) {
                    break;
                }
                dir = d.parent();
            }
            None
        }))
    }

    /// The records a plugin declared whose fields a script cannot read,
    /// `module::Name`: empty when every declaration reads.
    pub fn unread_records(&self) -> Vec<String> {
        self.inner.registry.unread()
    }

    /// The script reference: each module's functions, what each takes and
    /// does, and its constants, as `script::api()` answers it.
    pub fn reference(&self) -> balaur_script::Value {
        crate::api::reference(&self.api(), &self.inner.registry)
    }

    /// What a script compiles against: the engine's functions, component
    /// properties, constants and records, typed. Built once every plugin has
    /// registered.
    pub fn api(&self) -> HostApi {
        self.inner
            .api
            .borrow_mut()
            .get_or_insert_with(|| {
                let built = api::build(&self.inner.engine, &self.inner.registry);
                if let Some(pack) = &self.inner.pack {
                    Self::compare_api(&built, &pack.api);
                }
                built
            })
            .clone()
    }

    /// What this runtime's API lacks, or types otherwise, of the one the
    /// pack's scripts were checked against: said once, before the first
    /// compile, so a script that fails names a known cause.
    fn compare_api(built: &HostApi, packed: &[String]) {
        if packed.is_empty() {
            return;
        }
        let lines = api::lines(built);
        let here: std::collections::BTreeSet<&str> = lines.iter().map(String::as_str).collect();
        let missing: Vec<&str> = packed
            .iter()
            .map(String::as_str)
            .filter(|line| !here.contains(line))
            .collect();
        if !missing.is_empty() {
            tracing::error!(
                "this runtime's script API differs from the one the pack was checked against ({} of {} lines): {}",
                missing.len(),
                packed.len(),
                missing.join("; ")
            );
        }
    }

    /// What is wrong with `source` as the script at `rel`, against the
    /// engine's API: the first error, where the compiler stopped.
    /// Every error in the file, one per function that does not compile;
    /// empty when it runs.
    /// [`Self::check`], for a script a scene attaches to a node: also that it
    /// declares the class its nodes are, which running it requires.
    pub fn check_attached(&self, rel: &str, source: &str) -> Vec<balaur_bs::Error> {
        let found = self.check(rel, source);
        if !found.is_empty() {
            return found;
        }
        let key = key_of(rel);
        match self.compile_units(&key) {
            Ok((_, ty)) if ty.is_empty() => vec![balaur_bs::Error {
                pos: balaur_bs::Pos { line: 1, col: 1 },
                msg: no_class(&key),
            }],
            _ => vec![],
        }
    }

    pub fn check(&self, rel: &str, source: &str) -> Vec<balaur_bs::Error> {
        let key = key_of(rel);
        let options = self.check_options(&key);
        let found = balaur_bs::errors(source, &options);
        if !found.is_empty() {
            return found;
        }
        self.unit_of(&key, source).map_or_else(
            |error| {
                vec![balaur_bs::Error {
                    pos: balaur_bs::Pos { line: 1, col: 1 },
                    msg: format!("{error:#}"),
                }]
            },
            |unit| self.module_errors(&key, &unit),
        )
    }

    /// What the file's own functions should say otherwise, though they compile.
    pub fn lints(&self, rel: &str, source: &str) -> Vec<balaur_bs::Error> {
        balaur_bs::lints(source, &self.check_options(&key_of(rel)))
    }

    // A file's own functions: each module answers for its own.
    fn check_options(&self, key: &str) -> Options {
        Options {
            debug: false,
            host: self.api(),
            modules: Some(self.modules()),
            path: key.to_string(),
            unit: true,
            parsed: Some(self.inner.parsed.clone()),
            ..Options::default()
        }
    }

    // Every error of every module `unit` uses, near or far, each named by its
    // file. Each module is built once a run.
    fn module_errors(&self, key: &str, unit: &Compiled) -> Vec<balaur_bs::Error> {
        let mut found = vec![];
        let mut seen: HashSet<String> = HashSet::from([key.to_string()]);
        let mut queue: Vec<String> = unit.meta.uses.clone();
        while let Some(k) = queue.pop() {
            if !seen.insert(k.clone()) {
                continue;
            }
            if let Ok(u) = self.unit(&k) {
                queue.extend(u.meta.uses.iter().cloned());
                continue;
            }
            let Some(source) = self.kept_source(&k) else {
                continue;
            };
            let options = self.unit_options(&k);
            found.extend(balaur_bs::errors(&source, &options).into_iter().map(|e| {
                balaur_bs::Error {
                    pos: e.pos,
                    msg: format!("in {k}: {}", e.msg),
                }
            }));
        }
        found
    }

    fn load(&self, key: &str) -> Result<Rc<File>> {
        let (compiled, ty) = self.compile_units(key)?;
        if ty.is_empty() {
            return Err(anyhow!("{key}: {}", no_class(key)));
        }
        balaur_bs::runtime::cache_compiled_in(
            &balaur_core::engine_api::user_data_dir_of(&self.inner.engine).join(CACHE_DIR),
        );
        let unit = self.unit(key)?;
        // A file the project cannot run yet runs beside it until it can.
        let place = if place_of(key) == Place::Project
            && !self.joins_project(key, &unit, &compiled.linked)
        {
            Place::Aside
        } else {
            place_of(key)
        };
        let joining: Vec<&Compiled> = std::iter::once(&*unit)
            .chain(compiled.linked.iter().map(|(_, linked)| &**linked))
            .collect();
        let home = self.home(place, &joining)?;
        let index = home
            .either(
                |rt| rt.link(key, &unit, &compiled.linked),
                |re| re.link(key, &unit, &compiled.linked),
            )
            .map_err(|e| anyhow!("{key}: {e}"))?;
        self.arm(&home);
        Ok(Rc::new(File {
            native: home.native,
            home,
            place,
            unit: index,
            compiled,
            ty,
        }))
    }

    // Every object of the files `unit_of` places, out of `home` and into
    // `into` at the unit it says, carried by field name, and each instance
    // pointed at its new object. On failure every object stays where it was.
    fn carry(
        &self,
        home: &Home,
        into: &mut Runtime,
        unit_of: impl Fn(&str) -> Option<usize>,
    ) -> Result<()> {
        let parked = self.inner.parked.borrow();
        let insts: Vec<(Rc<Instance>, usize)> = self
            .inner
            .instances
            .borrow()
            .iter()
            .chain(parked.iter())
            .filter(|inst| inst.obj.borrow().is_some())
            .filter_map(|inst| unit_of(&inst.path).map(|unit| (inst.clone(), unit)))
            .collect();
        let mut objs: Vec<Object> = insts
            .iter()
            .filter_map(|(inst, _)| inst.obj.borrow_mut().take())
            .collect();
        let units: Vec<usize> = insts.iter().map(|(_, unit)| *unit).collect();
        let mut refs: Vec<&mut Object> = objs.iter_mut().collect();
        let carried = home.rt.borrow_mut().carry_to(into, &mut refs, &units);
        for ((inst, _), obj) in insts.iter().zip(objs) {
            inst.pointer.set(Runtime::pointer(&obj));
            *inst.obj.borrow_mut() = Some(obj);
        }
        carried.map(|_| ()).map_err(|e| anyhow!("{e}"))
    }

    /// Every instance a loaded file runs in, each once.
    fn homes(&self) -> Vec<Rc<Home>> {
        let mut homes: Vec<Rc<Home>> = vec![];
        for file in self.inner.files.borrow().values() {
            if !homes.iter().any(|home| Rc::ptr_eq(home, &file.home)) {
                homes.push(file.home.clone());
            }
        }
        homes
    }

    fn place_slot(&self, place: Place) -> &RefCell<Option<Rc<Home>>> {
        match place {
            Place::Project => &self.inner.project,
            Place::Aside => &self.inner.aside,
        }
    }

    /// The instance at `place`, made with the first file that joins it:
    /// `joining`, its unit and the units it uses.
    fn home(&self, place: Place, joining: &[&Compiled]) -> Result<Rc<Home>> {
        if let Some(home) = self.place_slot(place).borrow().as_ref() {
            return Ok(home.clone());
        }
        let home = Home::new(
            self.instance_runtime(place, joining)?,
            self.debugged_in(place),
        );
        *self.place_slot(place).borrow_mut() = Some(home.clone());
        Ok(home)
    }

    /// The unit an instance starts from: an empty file.
    pub(crate) fn project_unit(&self) -> Result<Compiled> {
        balaur_bs::compile("", &self.options_of(PROJECT_KEY, units::Build::PLAIN))
            .map_err(|e| anyhow!("{PROJECT_KEY}:{e}"))
    }

    // An instance with no file in it yet, on the engine what it holds asks for;
    // native only when a loaded build has every unit of `joining`.
    fn instance_runtime(&self, place: Place, joining: &[&Compiled]) -> Result<Runtime> {
        let root = self.project_unit()?;
        let all_native = joining
            .iter()
            .all(|unit| crate::native::has_native(&unit.wasm));
        let limits = Limits {
            engine: self.engine_of(place),
            memory_bytes: Some(MEMORY_CAP),
            skip_native: !(self.may_run_native(place) && all_native),
            ..Limits::default()
        };
        Runtime::new(&root, self.host_fns(place), limits).map_err(|e| anyhow!("{PROJECT_KEY}: {e}"))
    }

    fn file(&self, path: &str) -> Result<Rc<File>> {
        let key = key_of(path);
        if let Some(f) = self.inner.files.borrow().get(&key) {
            return Ok(f.clone());
        }
        let f = self.load(&key)?;
        self.inner.files.borrow_mut().insert(key, f.clone());
        Ok(f)
    }

    /// The `.bs` files the watcher saw written since the last pump, as keys
    /// under the root; a file or module not loaded is dropped by the caller.
    /// Only a file whose text changed: macOS reports writes from just before
    /// the watch began, and a reload drops every task the file has parked.
    fn edited(&self) -> Vec<String> {
        let Some(events) = &self.inner.events else {
            return vec![];
        };
        let mut keys: Vec<String> = vec![];
        while let Ok(event) = events.try_recv() {
            let Ok(event) = event else { continue };
            // inotify reports every open and read; only a write changes what
            // a path holds.
            if matches!(event.kind, notify::EventKind::Access(_)) {
                continue;
            }
            for path in event.paths {
                if path.extension().and_then(|e| e.to_str()) != Some(EXTENSION) {
                    continue;
                }
                let key = if let Ok(rel) = path.strip_prefix(&self.inner.watch_root) {
                    key_of(&rel.to_string_lossy())
                } else if let Some((rel, folder)) = self
                    .inner
                    .watch_mounts
                    .iter()
                    .find_map(|(at, folder)| Some((path.strip_prefix(at).ok()?, folder)))
                {
                    format!("{folder}/{}", key_of(&rel.to_string_lossy()))
                } else {
                    continue;
                };
                if !keys.contains(&key) {
                    keys.push(key);
                }
            }
        }
        keys.retain(|key| {
            let before = self.inner.read.borrow().get(key).cloned();
            // A file gone is a change; `source` records what it read.
            self.source(key).is_err() || self.inner.read.borrow().get(key) != before.as_ref()
        });
        keys
    }

    fn instance(&self, node: NodeId) -> Option<(Rc<File>, Rc<Instance>)> {
        let inst = self
            .inner
            .instances
            .borrow()
            .iter()
            .find(|i| i.node == node.0)
            .cloned()?;
        let file = self.inner.files.borrow().get(&inst.path).cloned()?;
        Some((file, inst))
    }

    // A parked scene's: what freeing its node and a snapshot still reach.
    fn parked_instance(&self, node: NodeId) -> Option<(Rc<File>, Rc<Instance>)> {
        let inst = self
            .inner
            .parked
            .borrow()
            .iter()
            .find(|i| i.node == node.0)
            .cloned()?;
        let file = self.inner.files.borrow().get(&inst.path).cloned()?;
        Some((file, inst))
    }

    // `method` on every instance nothing holds.
    fn each(&self, method: &str, args: &[Value], sync_only: bool) {
        let holds = self.holds();
        let mut all: Vec<Rc<Instance>> = self.inner.instances.borrow().clone();
        all.retain(|inst| self.ticks(inst.node, holds) && !self.stopped_in(&inst.path));
        self.each_of(&all, method, args, sync_only);
    }

    // `method` on every instance but those a stop or a paused replay holds.
    fn tell(&self, method: &str, args: &[Value], about: impl Fn(&Instance) -> bool) {
        let holds = self.holds();
        let mut all: Vec<Rc<Instance>> = self.inner.instances.borrow().clone();
        all.retain(|inst| {
            about(inst) && !self.frozen(inst.node, holds) && !self.stopped_in(&inst.path)
        });
        self.each_of(&all, method, args, false);
    }

    // Every instance of a file just rebuilt, its fields carried across: the
    // one place a script whose fields moved can bring them forward. A parked
    // one catches up when it returns.
    fn announce_reloads(&self) {
        let owed: Vec<String> = {
            let rebuilding = self.inner.rebuild.borrow();
            let mut owed = self.inner.reload_owed.borrow_mut();
            let (waiting, ready) = std::mem::take(&mut *owed)
                .into_iter()
                .partition(|key| rebuilding.contains(key));
            *owed = waiting;
            ready
        };
        if !owed.is_empty() {
            self.tell(balaur_core::hooks::ON_HOT_RELOAD, &[], |inst| {
                owed.contains(&inst.path)
            });
        }
    }

    // `key` read again and its instance made again, with no word to its script.
    pub(crate) fn rebuild_key(&self, key: &str) -> Result<()> {
        self.invalidate(key);
        self.rebuild_instance(place_of(key), &[key.to_string()])
            .map(|_| ())
    }

    // `update` and `fixed_update` run every frame, so an async one would
    // start a task a frame: those two are called only when they do not await.
    fn each_of(&self, all: &[Rc<Instance>], method: &str, args: &[Value], sync_only: bool) {
        let mut start = 0;
        while start < all.len() {
            // A stop in this pass holds the rest of what it holds.
            let held = |inst: &Instance| {
                self.is_held()
                    && (self.frozen(inst.node, self.holds()) || self.stopped_in(&inst.path))
            };
            if held(&all[start]) {
                start += 1;
                continue;
            }
            // The nodes in a row that run one script, in scene order.
            let path = &all[start].path;
            let run = all[start..]
                .iter()
                .take_while(|inst| inst.path == *path && !held(inst))
                .count();
            let group = &all[start..start + run];
            start += run;
            let Some(file) = self.inner.files.borrow().get(path).cloned() else {
                continue;
            };
            if file
                .method(method)
                .is_none_or(|found| sync_only && found.is_async)
            {
                continue;
            }
            if run > 1 && self.batch(&file, group, method, args) {
                continue;
            }
            for inst in group {
                report(self.run(&file, inst, method, args, None));
            }
        }
    }

    /// `method` on every object of `group` in one call. False when it cannot
    /// be: a debug build steps each call, an object out in a call stays out,
    /// and a method that takes or gives a string, an array or an `any` has no
    /// batch entry.
    fn batch(&self, file: &File, group: &[Rc<Instance>], method: &str, args: &[Value]) -> bool {
        let Some(info) = file.method(method) else {
            return false;
        };
        let Ok(mut rt) = file.home.rt.try_borrow_mut() else {
            return false;
        };
        if rt.steppable() {
            return false;
        }
        let mut objs = vec![];
        for inst in group.iter().filter(|inst| !inst.gone.get()) {
            let shared = inst.obj.borrow().as_ref().and_then(|obj| rt.share(obj));
            let Some(shared) = shared else {
                for obj in objs {
                    rt.drop_object(obj);
                }
                return false;
            };
            objs.push(shared);
        }
        let args: Vec<L> = info
            .params
            .iter()
            .zip(args.iter().chain(std::iter::repeat(&Value::Nil)))
            .map(|(ty, value)| from_neutral(value, ty, &file.compiled.meta))
            .collect();
        let before = rt.steps();
        let out = rt.call_batch(&file.ty, &objs, method, &args);
        self.charge(&group[0].path, before, rt.steps());
        for obj in objs {
            rt.drop_object(obj);
        }
        match out {
            Ok(done) => done,
            Err(error) => {
                tracing::error!("[{}] {method}: {error}", group[0].path);
                true
            }
        }
    }

    fn init(&self, node: u64) {
        if let Some((file, inst)) = self.instance(NodeId(node)) {
            let r = self.run(&file, &inst, "init", &[], None);
            report(r);
        }
    }

    // `None` from an async method that suspended; `done` hears its result.
    fn run(
        &self,
        file: &File,
        inst: &Instance,
        method: &str,
        args: &[Value],
        done: Option<u64>,
    ) -> Result<Option<Value>> {
        let Some(info) = file.method(method).filter(|_| !inst.gone.get()) else {
            return Ok(None);
        };
        // A path the script builds reads from its own project's folder.
        let _turn = balaur_core::dependencies::calling(&self.inner.engine, &inst.path);
        let args: Vec<L> = info
            .params
            .iter()
            .zip(args.iter().chain(std::iter::repeat(&Value::Nil)))
            .map(|(t, v)| from_neutral(v, t, &file.compiled.meta))
            .collect();
        let Ok(mut rt) = file.home.rt.try_borrow_mut() else {
            return self.reenter(file, inst, method, &args, info.is_async, done);
        };
        if info.is_async {
            drop(rt);
            return self.start(file, inst, method, &args, done);
        }
        if rt.steppable() && !self.is_held() {
            drop(rt);
            return self.run_steppable(file, inst, method, &args);
        }
        let Some(obj) = inst.obj.borrow_mut().take() else {
            drop(rt);
            return Self::nested(file, inst, method, &args);
        };
        *inst.lent.borrow_mut() = rt.share(&obj);
        let before = rt.steps();
        let out = rt.call_info(&obj, info, &args);
        self.charge(&inst.path, before, rt.steps());
        *inst.obj.borrow_mut() = Some(obj);
        if let Some(lent) = inst.lent.borrow_mut().take() {
            rt.drop_object(lent);
        }
        let v = out.map_err(|e| anyhow!("[{}] {method}: {e}", inst.path))?;
        Ok(Some(to_neutral(&v)))
    }

    // A call into an instance while another call into it runs, the plugin
    // that answers the Editor calling the Editor back: on a second holder of
    // the object, so the class it keeps its state in is the one both see.
    fn nested(file: &File, inst: &Instance, method: &str, args: &[L]) -> Result<Option<Value>> {
        let busy = || {
            anyhow!(
                "[{}] {method}: the instance is busy, and holds a string, list or any outside a class",
                inst.path
            )
        };
        let copy = {
            let lent = inst.lent.borrow();
            let lent = lent.as_ref().ok_or_else(busy)?;
            file.either(|rt| rt.share(lent), |re| re.share(lent))
                .ok_or_else(busy)?
        };
        let out = match file.home.rt.try_borrow_mut() {
            Ok(mut rt) => rt.call_method(&copy, method, args),
            Err(_) => file.home.reentry.call_method(&copy, method, args),
        };
        file.drop_object(copy);
        let v = out.map_err(|e| anyhow!("[{}] {method}: {e}", inst.path))?;
        Ok(Some(to_neutral(&v)))
    }

    // A call into a file that is itself inside an engine call: a script
    // calling a method of its own file, on another node, through the engine.
    // It runs on the same module, and a debugger stop inside it is passed by.
    fn reenter(
        &self,
        file: &File,
        inst: &Instance,
        method: &str,
        args: &[L],
        is_async: bool,
        done: Option<u64>,
    ) -> Result<Option<Value>> {
        let Some(obj) = inst.obj.borrow_mut().take() else {
            if is_async {
                self.inner.waiting_starts.borrow_mut().push(WaitingStart {
                    node: inst.node,
                    method: method.to_string(),
                    args: args.iter().map(to_neutral).collect(),
                    done,
                });
                return Ok(None);
            }
            return Self::nested(file, inst, method, args);
        };
        *inst.lent.borrow_mut() = file.home.reentry.share(&obj);
        let out = if is_async {
            let key = format!("{}.{method}", file.ty);
            file.home
                .reentry
                .start(&key, args, &obj)
                .map(|(task, out)| {
                    if out.is_none() {
                        self.inner.tasks.borrow_mut().push(Pending {
                            node: inst.node,
                            path: inst.path.clone(),
                            method: method.to_string(),
                            task,
                            done,
                        });
                    }
                    out
                })
        } else {
            file.home.reentry.call_method(&obj, method, args).map(Some)
        };
        *inst.obj.borrow_mut() = Some(obj);
        if let Some(lent) = inst.lent.borrow_mut().take() {
            file.drop_object(lent);
        }
        let out = out.map_err(|e| anyhow!("[{}] {method}: {e}", inst.path))?;
        Ok(out.map(|v| to_neutral(&v)))
    }
}

/// What a node's script lacks when it declares no class named after its file.
fn no_class(key: &str) -> String {
    let name = balaur_bs::script_class(key);
    format!(
        "a script declares `pub class {name}`, named after its file; a file without one is a module other scripts `use`"
    )
}

/// The class a script's nodes are: the one named after its file,
/// `spinner.bs`'s `Spinner`; "" for a file without it, a module others `use`.
pub(crate) fn script_type(key: &str, meta: &balaur_bs::Meta) -> String {
    let name = balaur_bs::script_class(key);
    if meta.classes.contains(&name) {
        name
    } else {
        String::new()
    }
}

fn report(r: Result<Option<Value>>) -> Option<Value> {
    r.unwrap_or_else(|e| {
        tracing::error!("{e:#}");
        None
    })
}

fn spec(lit: &Literal, ty: &Ty) -> Value {
    let (kind, default) = match lit {
        Literal::Int(v) => (words::doc_types::INT, Value::Int(*v)),
        Literal::Float(v) => (words::doc_types::FLOAT, Value::Num(*v)),
        Literal::Bool(v) => (words::doc_types::BOOL, Value::Bool(*v)),
        Literal::Str(v) => (words::doc_types::STRING, Value::Str(v.clone())),
    };
    let kind = if *ty == Ty::Float {
        words::doc_types::FLOAT
    } else {
        kind
    };
    Value::Map(vec![
        ("type".into(), Value::text(kind)),
        ("default".into(), default),
    ])
}

// An export compiles every `.bs` as a check and packs the source it checked.
