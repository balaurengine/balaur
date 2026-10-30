// `.bs` scripts behind the seam: every file a unit of the project's one
// instance (or, built whole for a debugger or a profiler, an instance of its
// own), one host-held object per node, its lifecycle methods called by name
// when the script has them.

use crate::api::{self, Module, Registry};
use crate::convert::{from_neutral, to_neutral};
use anyhow::{Context, Result, anyhow};
use balaur_bs::runtime::{HostFns, Limits, Object, Reentry, Runtime, Value as L};
use balaur_bs::{Compiled, HostApi, Literal, Options, Ty};
use balaur_core::{Engine, Pack};
use balaur_script::{
    Bindings, CallbackId, NodeId, Pause, ScriptCompiler, ScriptHost, StepMode, Value,
};
use debug::Held;
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
use std::rc::{Rc, Weak};
use std::sync::Arc;
use tasks::Pending;

mod debug;
mod profile;
mod tasks;
mod tools;
mod units;

pub const EXTENSION: &str = "bs";
/// Under the game's own data folder: what wasmtime compiled, kept across runs.
const CACHE_DIR: &str = "bs-cache";
/// The most memory one file's module may grow to: a runaway allocation traps
/// there rather than taking the game with it.
const MEMORY_CAP: usize = 256 << 20;
/// The project's instance, which every file built as units shares: what
/// grows past this traps rather than taking the game with it.
const PROJECT_MEMORY_CAP: usize = 1 << 30;
/// A browser's engine meters no fuel, so a build for one counts its own.
const METER: bool = cfg!(all(target_arch = "wasm32", feature = "web"));
const NODE_FIELD: &str = "node";
/// The project instance's own unit, which has no code: every file a unit
/// linked to it.
const PROJECT_KEY: &str = "#project";

/// Where scripts run: a runtime, and the way into it while it is busy in an
/// engine call. The project's, which every file built as units shares, or
/// a file's own.
struct Home {
    rt: RefCell<Runtime>,
    reentry: Reentry,
}

impl Home {
    fn new(rt: Runtime) -> Rc<Home> {
        Rc::new(Home {
            reentry: rt.reentry(),
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
    /// Its unit in `home`'s instance: 0 for a file with a home of its own.
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

    /// Whether it shares the project's instance.
    fn in_project(&self, project: Option<&Rc<Home>>) -> bool {
        project.is_some_and(|home| Rc::ptr_eq(home, &self.home))
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
    registry: Rc<RefCell<Registry>>,
    api: RefCell<Option<HostApi>>,
    files: RefCell<BTreeMap<String, Rc<File>>>,
    /// The instance every file built as units shares, made with the first.
    project: RefCell<Option<Rc<Home>>>,
    instances: RefCell<Vec<Rc<Instance>>>,
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
    /// Files built with probes, on an engine that can hold a call at a line.
    debugging: RefCell<HashSet<String>>,
    /// The lines a debugger asked to stop at, by file, as it asked for them.
    breaks: RefCell<BTreeMap<String, Vec<usize>>>,
    stopped: RefCell<Option<Held>>,
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
    /// Each file built as a unit, by key: a script links the units of the
    /// modules it uses, each built once for every script ([`BsHost::unit`]).
    units: RefCell<HashMap<String, Arc<Compiled>>>,
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
}

/// The `.bs` backend. Cheap to clone; clones share the one host.
#[derive(Clone)]
pub struct BsHost {
    inner: Rc<Inner>,
}

pub(crate) fn key_of(path: &str) -> String {
    path.trim_start_matches("./").replace('\\', "/")
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
                instances: RefCell::default(),
                held: RefCell::default(),
                holds: Cell::new(0),
                events,
                _watcher: watcher,
                watch_root,
                watch_mounts,
                called: RefCell::default(),
                tasks: RefCell::default(),
                debugging: RefCell::default(),
                breaks: RefCell::default(),
                stopped: RefCell::new(None),
                break_on_error: Cell::new(false),
                profiling: Cell::new(false),
                tally: RefCell::default(),
                rebuild: RefCell::default(),
                units: RefCell::default(),
                parsed: balaur_bs::Parsed::default(),
                sources: RefCell::default(),
                read: RefCell::default(),
                api_hash: Cell::new(None),
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

    /// What a runtime's scripts call out to: the engine, and `node.script`,
    /// which finds a node's object only in the same instance: the project's,
    /// or with `own` that file's.
    fn host_fns(&self, own: Option<&str>) -> HostFns {
        let (engine, registry) = (self.inner.engine.clone(), self.inner.registry.clone());
        let host = Rc::downgrade(&self.inner);
        let own = own.map(str::to_string);
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
                let same = match &own {
                    Some(key) => *key == inst.path,
                    None => inner
                        .files
                        .borrow()
                        .get(file)
                        .is_some_and(|found| found.in_project(inner.project.borrow().as_ref())),
                };
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

    // What runs here: with counters while a profiler wants them, and as
    // units unless something wants the whole file in one module.
    fn compile_module(&self, key: &str) -> Result<(Compiled, String)> {
        if self.as_units(key) {
            return self.compile_units(key);
        }
        self.compile_with(key, &self.source(key)?, self.inner.profiling.get())
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
        self.inner.registry.borrow().unread()
    }

    /// The script reference: each module's functions, what each takes and
    /// does, and its constants, as `script::api()` answers it.
    pub fn reference(&self) -> balaur_script::Value {
        crate::api::reference(&self.api(), &self.inner.registry.borrow())
    }

    /// What a script compiles against: the engine's functions, component
    /// properties, constants and records, typed. Built once every plugin has
    /// registered.
    pub fn api(&self) -> HostApi {
        self.inner
            .api
            .borrow_mut()
            .get_or_insert_with(|| {
                let built = api::build(&self.inner.engine, &self.inner.registry.borrow());
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
    pub fn check(&self, rel: &str, source: &str) -> Vec<balaur_bs::Error> {
        let key = key_of(rel);
        // As units, a file's own functions: each module answers for its own.
        let units = self.as_units(&key);
        let options = Options {
            debug: false,
            host: self.api(),
            modules: Some(self.modules()),
            path: key.clone(),
            unit: units,
            parsed: units.then(|| self.inner.parsed.clone()),
            ..Options::default()
        };
        let found = balaur_bs::errors(source, &options);
        if !found.is_empty() {
            return found;
        }
        let built = if units {
            self.unit_of(&key, source)
                .map(|unit| self.module_errors(&key, &unit))
        } else {
            self.compile_source(&key, source).map(|_| vec![])
        };
        built.unwrap_or_else(|e| {
            vec![balaur_bs::Error {
                pos: balaur_bs::Pos { line: 1, col: 1 },
                msg: format!("{e:#}"),
            }]
        })
    }

    // As the whole file would: every error of every module `unit` uses, near
    // or far, each named by its file. Each module is built once a run.
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

    /// `source` as the file at `key` ships: an export's check and its native
    /// build never carry a profiler's counters.
    pub(crate) fn compile_source(&self, key: &str, source: &str) -> Result<(Compiled, String)> {
        self.compile_with(key, source, false)
    }

    fn compile_with(&self, key: &str, source: &str, profile: bool) -> Result<(Compiled, String)> {
        let compiled = balaur_bs::compile(
            source,
            &Options {
                debug: self.probed(key),
                host: self.api(),
                meter: METER,
                profile,
                memory_max: Some(MEMORY_CAP),
                // A browser wires the script straight to the engine's exports.
                direct_imports: METER,
                modules: Some(self.modules()),
                path: key.to_string(),
                ..Options::default()
            },
        )
        .map_err(|e| anyhow!("{key}:{e}"))?;
        let ty = script_type(key, &compiled.meta);
        Ok((compiled, ty))
    }

    fn load(&self, key: &str) -> Result<Rc<File>> {
        let (compiled, ty) = self.compile_module(key)?;
        if ty.is_empty() {
            let name = balaur_bs::script_class(key);
            return Err(anyhow!(
                "{key}: a script declares `pub class {name}`, named after its file; a file without one is a module other scripts `use`"
            ));
        }
        balaur_bs::runtime::cache_compiled_in(
            &balaur_core::engine_api::user_data_dir_of(&self.inner.engine).join(CACHE_DIR),
        );
        // A unit joins the project's instance.
        if compiled.meta.unit.is_some() {
            let home = self.project()?;
            let unit = self.unit(key)?;
            let index = home
                .either(
                    |rt| rt.link(key, &unit, &compiled.linked),
                    |re| re.link(key, &unit, &compiled.linked),
                )
                .map_err(|e| anyhow!("{key}: {e}"))?;
            return Ok(Rc::new(File {
                home,
                unit: index,
                native: false,
                compiled,
                ty,
            }));
        }
        // The native build first, so this script's own module can be in it.
        #[cfg(all(feature = "native", not(target_arch = "wasm32")))]
        let built = crate::native::load_beside_executable();
        let native = crate::native::has_native(&compiled.wasm);
        #[cfg(all(feature = "native", not(target_arch = "wasm32")))]
        if built && !compiled.meta.debug && !native {
            tracing::warn!(
                "{key}: the native build has no module {}; it runs interpreted",
                balaur_bs::module_hash(&compiled.wasm)
            );
        }
        let mut rt = self.alone(key, &compiled)?;
        self.arm(key, &compiled, &mut rt);
        Ok(Rc::new(File {
            home: Home::new(rt),
            unit: 0,
            native,
            compiled,
            ty,
        }))
    }

    // A runtime of its own for `compiled`, the whole file at `key`.
    fn alone(&self, key: &str, compiled: &Compiled) -> Result<Runtime> {
        let limits = Limits {
            engine: self.engine_for(key),
            memory_bytes: Some(MEMORY_CAP),
            ..Limits::default()
        };
        Runtime::new(compiled, self.host_fns(Some(key)), limits).map_err(|e| anyhow!("{key}: {e}"))
    }

    /// `key` built whole into a runtime of its own, as a debugger or a
    /// profiler wants it, its instances carried across by field name from
    /// wherever they ran. Nothing changes when it does not build.
    fn reload_alone(&self, key: &str) -> Result<()> {
        let old = self
            .inner
            .files
            .borrow()
            .get(key)
            .cloned()
            .ok_or_else(|| anyhow!("{key} is not loaded"))?;
        if old.home.rt.try_borrow_mut().is_err() {
            return Err(anyhow!("{key} is running; reload it between frames"));
        }
        let (compiled, ty) = self.compile_module(key)?;
        if ty.is_empty() {
            return Err(anyhow!(
                "{key}: it declares no `pub class {}`",
                balaur_bs::script_class(key)
            ));
        }
        let mut fresh = self.alone(key, &compiled)?;
        self.carry(&old.home, &mut fresh, |path| (path == key).then_some(0))?;
        self.forget(key);
        self.arm(key, &compiled, &mut fresh);
        self.inner.files.borrow_mut().insert(
            key.to_string(),
            Rc::new(File {
                home: Home::new(fresh),
                unit: 0,
                native: crate::native::has_native(&compiled.wasm),
                compiled,
                ty,
            }),
        );
        Ok(())
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
        let insts: Vec<(Rc<Instance>, usize)> = self
            .inner
            .instances
            .borrow()
            .iter()
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

    /// The instance every file built as units shares, made with the first.
    fn project(&self) -> Result<Rc<Home>> {
        if let Some(home) = self.inner.project.borrow().as_ref() {
            return Ok(home.clone());
        }
        let home = Home::new(self.project_runtime()?);
        *self.inner.project.borrow_mut() = Some(home.clone());
        Ok(home)
    }

    // A project instance with no file in it yet.
    fn project_runtime(&self) -> Result<Runtime> {
        let root = balaur_bs::compile("", &self.unit_options(PROJECT_KEY))
            .map_err(|e| anyhow!("{PROJECT_KEY}:{e}"))?;
        let limits = Limits {
            memory_bytes: Some(PROJECT_MEMORY_CAP),
            ..Limits::default()
        };
        Runtime::new(&root, self.host_fns(None), limits).map_err(|e| anyhow!("{PROJECT_KEY}: {e}"))
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

    // `update` and `fixed_update` run every frame, so an async one would
    // start a task a frame: those two are called only when they do not await.
    fn each(&self, method: &str, args: &[Value], sync_only: bool) {
        let all: Vec<Rc<Instance>> = self.inner.instances.borrow().clone();
        let mut start = 0;
        while start < all.len() {
            // A stop freezes the game, the rest of this pass with it.
            if self.is_held() {
                break;
            }
            // The nodes in a row that run one script, in scene order.
            let path = &all[start].path;
            let run = all[start..]
                .iter()
                .take_while(|inst| inst.path == *path)
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

    // `method` on every object of `group` in one call. False when it cannot
    // be: a debug build steps each call, an object out in a call stays out,
    // and a method that takes or gives a string, an array or an `any` has no
    // batch entry.
    fn batch(&self, file: &File, group: &[Rc<Instance>], method: &str, args: &[Value]) -> bool {
        if file.compiled.meta.debug {
            return false;
        }
        let Some(info) = file.method(method) else {
            return false;
        };
        let Ok(mut rt) = file.home.rt.try_borrow_mut() else {
            return false;
        };
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
            .map(|(ty, value)| from_neutral(value, ty))
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
            .map(|(t, v)| from_neutral(v, t))
            .collect();
        let Ok(mut rt) = file.home.rt.try_borrow_mut() else {
            return self.reenter(file, inst, method, &args, info.is_async, done);
        };
        if info.is_async {
            drop(rt);
            return self.start(file, inst, method, &args, done);
        }
        if file.compiled.meta.debug {
            drop(rt);
            return self.run_steppable(file, inst, method, &args);
        }
        let Some(mut obj) = inst.obj.borrow_mut().take() else {
            drop(rt);
            return Self::nested(file, inst, method, &args);
        };
        *inst.lent.borrow_mut() = rt.share(&obj);
        let before = rt.steps();
        let out = rt.call_info(&mut obj, info, &args);
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
        let mut copy = {
            let lent = inst.lent.borrow();
            let lent = lent.as_ref().ok_or_else(busy)?;
            file.either(|rt| rt.share(lent), |re| re.share(lent))
                .ok_or_else(busy)?
        };
        let out = match file.home.rt.try_borrow_mut() {
            Ok(mut rt) => rt.call_method(&mut copy, method, args),
            Err(_) => file.home.reentry.call_method(&mut copy, method, args),
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
        let Some(mut obj) = inst.obj.borrow_mut().take() else {
            if is_async {
                return Err(anyhow!("[{}] {method}: the instance is busy", inst.path));
            }
            return Self::nested(file, inst, method, args);
        };
        *inst.lent.borrow_mut() = file.home.reentry.share(&obj);
        let out = if is_async {
            let key = format!("{}.{method}", file.ty);
            file.home
                .reentry
                .start(&key, args, &mut obj)
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
            file.home
                .reentry
                .call_method(&mut obj, method, args)
                .map(Some)
        };
        *inst.obj.borrow_mut() = Some(obj);
        if let Some(lent) = inst.lent.borrow_mut().take() {
            file.drop_object(lent);
        }
        let out = out.map_err(|e| anyhow!("[{}] {method}: {e}", inst.path))?;
        Ok(out.map(|v| to_neutral(&v)))
    }
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
        Literal::Int(v) => ("int", Value::Int(*v)),
        Literal::Float(v) => ("float", Value::Num(*v)),
        Literal::Bool(v) => ("bool", Value::Bool(*v)),
        Literal::Str(v) => ("string", Value::Str(v.clone())),
    };
    let kind = if *ty == Ty::Float { "float" } else { kind };
    Value::Map(vec![
        ("type".into(), Value::text(kind)),
        ("default".into(), default),
    ])
}

// An export compiles every `.bs` as a check and packs the source it checked.
impl ScriptCompiler for BsHost {
    fn extensions(&self) -> &[&str] {
        &[EXTENSION]
    }

    fn compile(&self, rel: &str, source: &str) -> Result<Vec<u8>> {
        let (compiled, _) = self.compile_source(&key_of(rel), source)?;
        self.inner
            .called
            .borrow_mut()
            .extend(compiled.meta.host_calls.iter().map(api::line));
        Ok(source.as_bytes().to_vec())
    }

    fn uses(&self, rel: &str, source: &str) -> Result<Vec<String>> {
        let key = key_of(rel);
        let options = Options {
            modules: Some(self.modules()),
            path: key.clone(),
            ..Options::default()
        };
        balaur_bs::uses(source, &options).map_err(|e| anyhow!("{key}:{e}"))
    }

    fn api(&self) -> Vec<String> {
        self.inner.called.borrow().iter().cloned().collect()
    }
}

impl ScriptHost<Engine> for BsHost {
    fn module(&self, name: &str) -> Result<Box<dyn Bindings<Engine>>> {
        Ok(Box::new(Module {
            name: name.to_string(),
            registry: self.inner.registry.clone(),
        }))
    }

    fn attach_with_props(&self, node: NodeId, path: &str, props: &[(String, Value)]) -> Result<()> {
        let file = self.file(path)?;
        let mut init: Vec<(String, L)> = vec![];
        for (name, v) in props {
            match file.field_ty(name) {
                Some(t) => init.push((name.clone(), from_neutral(v, &t))),
                None => {
                    tracing::warn!("[{path}] property '{name}' is set on a node but not exported");
                }
            }
        }
        if file.field_ty(NODE_FIELD) == Some(Ty::Node) {
            init.push((NODE_FIELD.into(), L::Node(node.0.cast_signed())));
        }
        let init_ref: Vec<(&str, L)> = init.iter().map(|(n, v)| (n.as_str(), v.clone())).collect();
        let obj = file
            .either(
                |rt| rt.in_unit(file.unit, |rt| rt.object(&file.ty, &init_ref)),
                |re| re.object(file.unit, &file.ty, &init_ref),
            )
            .map_err(|e| anyhow!("[{path}] {e}"))?;
        self.detach(node);
        self.inner.instances.borrow_mut().push(Rc::new(Instance {
            node: node.0,
            path: key_of(path),
            pointer: Cell::new(Runtime::pointer(&obj)),
            obj: RefCell::new(Some(obj)),
            lent: RefCell::new(None),
            gone: Cell::new(false),
        }));
        if self.inner.holds.get() > 0 {
            self.inner.held.borrow_mut().push(node.0);
        } else {
            self.init(node.0);
        }
        Ok(())
    }

    fn hold_inits(&self) {
        self.inner.holds.set(self.inner.holds.get() + 1);
    }

    fn release_inits(&self) {
        let depth = self.inner.holds.get().saturating_sub(1);
        self.inner.holds.set(depth);
        if depth == 0 {
            let held = std::mem::take(&mut *self.inner.held.borrow_mut());
            for node in held {
                self.init(node);
            }
        }
    }

    fn exports(&self, path: &str) -> Result<Vec<(String, Value)>> {
        let file = self.file(path)?;
        let meta = &file.compiled.meta;
        Ok(meta
            .defaults(&file.ty)
            .iter()
            .filter(|(_, public, _)| *public)
            .map(|(n, _, lit)| (n.clone(), spec(lit, &file.field_ty(n).unwrap_or(Ty::Void))))
            .collect())
    }

    fn detach(&self, node: NodeId) {
        let Some((file, inst)) = self.instance(node) else {
            return;
        };
        if file.method("on_free").is_some() {
            let r = self.run(&file, &inst, "on_free", &[], None);
            report(r);
        }
        self.cancel(node.0);
        self.inner
            .instances
            .borrow_mut()
            .retain(|i| i.node != node.0);
        inst.gone.set(true);
        if let Some(obj) = inst.obj.borrow_mut().take() {
            file.drop_object(obj);
        }
    }

    fn update(&self, dt: f32) {
        if self.is_held() {
            return;
        }
        // A task a busy instance could not take last time runs on here.
        if self.has_tasks() {
            self.run_tasks();
        }
        self.each("update", &[Value::Num(f64::from(dt))], true);
    }

    fn fixed_update(&self, dt: f32) {
        if self.is_held() {
            return;
        }
        self.each("fixed_update", &[Value::Num(f64::from(dt))], true);
    }

    fn save_state(&self) -> Vec<(NodeId, Value)> {
        let all: Vec<Rc<Instance>> = self.inner.instances.borrow().clone();
        let mut out = vec![];
        for inst in all {
            let Some(file) = self.inner.files.borrow().get(&inst.path).cloned() else {
                continue;
            };
            let fields = inst
                .obj
                .borrow()
                .as_ref()
                .map(|o| file.home.rt.borrow_mut().fields(o))
                .unwrap_or_default();
            out.push((
                NodeId(inst.node),
                Value::Map(
                    fields
                        .iter()
                        .map(|(k, v)| (k.clone(), to_neutral(v)))
                        .collect(),
                ),
            ));
        }
        out
    }

    fn load_state(&self, states: &[(NodeId, Value)]) {
        for (node, state) in states {
            let (Some((file, inst)), Value::Map(fields)) = (self.instance(*node), state) else {
                continue;
            };
            let mut guard = inst.obj.borrow_mut();
            let Some(obj) = guard.as_mut() else { continue };
            for (name, v) in fields {
                if let Some(t) = file.field_ty(name)
                    && let Err(e) = file
                        .home
                        .rt
                        .borrow_mut()
                        .set(obj, name, &from_neutral(v, &t))
                {
                    tracing::error!("[{}] load_state: {e}", inst.path);
                }
            }
        }
    }

    fn pump_reloads(&self) {
        let mut todo: Vec<String> = self.inner.rebuild.take();
        for key in self.edited() {
            self.invalidate(&key);
            // A module is no script of its own: what uses it reloads.
            let users: Vec<String> = self
                .inner
                .files
                .borrow()
                .iter()
                .filter(|(k, f)| {
                    **k == key
                        || f.compiled.meta.uses.contains(&key)
                        || f.compiled.linked.iter().any(|(l, _)| *l == key)
                })
                .map(|(k, _)| k.clone())
                .collect();
            for k in users {
                if !todo.contains(&k) {
                    todo.push(k);
                }
            }
        }
        // Every file built as units reloads with the project, once.
        let (units, alone): (Vec<String>, Vec<String>) =
            todo.into_iter().partition(|key| self.as_units(key));
        if !units.is_empty() {
            match self.rebuild_project(&units) {
                Ok(()) => tracing::info!("reloaded {}", units.join(", ")),
                Err(e) => tracing::error!("{e:#}"),
            }
        }
        for key in alone {
            match ScriptHost::reload(self, &key) {
                Ok(()) => tracing::info!("reloaded {key}"),
                Err(e) => tracing::error!("{e:#}"),
            }
        }
    }

    // A new module, with every instance of the file carried across by field
    // name: as a unit, the whole project's instance made again.
    fn reload(&self, key: &str) -> Result<()> {
        let key = key_of(key);
        if self.holds_file(&key) {
            return Err(anyhow!(
                "{key} is stopped at a breakpoint; it reloads once it goes on"
            ));
        }
        // Asked for by name, a reload reads the file again.
        self.invalidate(&key);
        if self.as_units(&key) {
            self.rebuild_project(&[key])
        } else {
            self.reload_alone(&key)
        }
    }

    fn call_on(&self, node: NodeId, method: &str, args: &[Value]) -> Option<Value> {
        let (file, inst) = self.instance(node)?;
        let r = self.run(&file, &inst, method, args, None);
        report(r)
    }

    fn call_on_async(
        &self,
        node: NodeId,
        method: &str,
        args: &[Value],
        done: u64,
    ) -> Option<Value> {
        let Some((file, inst)) = self.instance(node) else {
            return Some(Value::Nil);
        };
        if file.method(method).is_none() {
            return Some(Value::Nil);
        }
        match self.run(&file, &inst, method, args, Some(done)) {
            Ok(out) => out,
            Err(e) => {
                tracing::error!("{e:#}");
                Some(Value::Nil)
            }
        }
    }

    fn has_method(&self, node: NodeId, method: &str) -> bool {
        self.instance(node)
            .is_some_and(|(f, _)| f.method(method).is_some())
    }

    fn script_field(&self, node: NodeId, name: &str) -> Option<Value> {
        let (file, inst) = self.instance(node)?;
        let obj = inst.obj.borrow();
        let obj = obj.as_ref()?;
        file.either(|rt| rt.get(obj, name), |re| re.get(obj, name))
            .map(|v| to_neutral(&v))
    }

    fn call_all(&self, method: &str) {
        self.each(method, &[], false);
    }

    fn call_all_with(&self, method: &str, args: &[Value]) {
        self.each(method, args, false);
    }

    fn wake(&self, token: u64, payload: &Value) {
        self.deliver(token, payload);
    }

    fn scene_source(&self, rel: &str) -> Option<String> {
        let key = key_of(rel);
        if let Some(pack) = &self.inner.pack {
            return pack.scenes.get(&key).cloned();
        }
        let bytes = balaur_core::files::backend(&self.inner.engine)
            .read(&self.inner.root.join(&key))
            .ok()?;
        String::from_utf8(bytes).ok()
    }

    fn instance_count(&self) -> usize {
        self.inner.instances.borrow().len()
    }

    fn set_breakpoints(&self, path: &str, lines: &[usize]) -> Result<Vec<usize>> {
        self.debug_set_breakpoints(path, lines)
    }

    fn breakpoints(&self, path: &str) -> Vec<usize> {
        self.debug_breakpoints(path)
    }

    fn set_profiling(&self, on: bool) {
        self.profile_set(on);
    }

    fn script_costs(&self) -> Vec<(String, u64, u64)> {
        self.profile_scripts()
    }

    fn function_costs(&self) -> Vec<(String, u64, u64)> {
        self.profile_functions()
    }

    fn set_break_on_error(&self, on: bool) {
        self.debug_set_break_on_error(on);
    }

    fn break_on_error(&self) -> bool {
        self.debug_break_on_error()
    }

    fn request_break(&self) {
        self.debug_request_break();
    }

    fn paused(&self) -> Option<Pause> {
        self.debug_paused()
    }

    fn resume(&self, mode: StepMode) {
        self.debug_resume(mode);
    }

    fn invoke(&self, _callback: CallbackId, _args: &[Value]) -> Result<Value> {
        Err(anyhow!(
            "`.{EXTENSION}` scripts pass no functions to the engine"
        ))
    }

    fn tools_for(&self, path: &str) -> Option<&dyn balaur_script::LanguageTools> {
        BsHost::claims(path).then_some(self as &dyn balaur_script::LanguageTools)
    }

    fn as_any(&self) -> &dyn core::any::Any {
        self
    }
}
