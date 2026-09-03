//! The Rune script host: loading, instancing, hot reload, precompiled packs.
//!
//! Scripting model: a `.rn` file declares free functions taking the instance
//! as their first argument. One instance object is created per node the
//! script is attached to, and the host hands it back on every call.
//!
//! ```rune
//! pub fn init(this) { this.angle = 0.0; }
//! pub fn update(this, dt) { this.angle += dt; }
//! ```
//!
//! A Rune object handed into a function is mutated in place, so what a script
//! writes to `this` is what the host sees on the next frame.

mod api;
mod bindings;
mod debugger;
mod inspect;
mod pause;
mod script_module;
mod value;

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::mpsc::Receiver;
use std::sync::Arc;

use anyhow::{anyhow, Context as _, Result};
use balaur_core::scene::ScriptAttachment;
use balaur_core::{Engine, Pack};
use balaur_script::{Pause, StepMode};
use hecs::Entity;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use std::future::Future as _;

use rune::alloc::clone::TryClone as _;
use rune::runtime::{Function, RuntimeContext, Unit, VmExecution, VmResult};
use rune::{Diagnostics, Source, Sources, TypeHash as _, Vm};

pub use api::{api_json, rune_of};
pub use inspect::Finding;
use inspect::{public_functions, render, PublicSignature};
use script_module::script_module;
pub use bindings::{ApiEntry, RuneModule};
pub use value::{Color, Node, Vec2, Vec3};

/// The Rune backend, as an `AppConfig::script_backend` factory.
pub fn factory() -> balaur_core::ScriptHostFactory {
    Box::new(|setup| {
        Ok(Rc::new(RuneHost::new(
            setup.engine.clone(),
            setup.project_root,
            setup.pack,
            setup.watch,
        )?))
    })
}

/// Compiles `.rn` sources for an export pack.
///
/// Rune has no stable serialised unit format, so a pack stores the source and
/// the shipped game compiles it at load. That is the honest tradeoff: startup
/// costs a compile, and a pack stays portable across Rune versions.
///
/// The compile still happens at export, purely to fail the build on a broken
/// script — and it runs through the live host, not a bare context. Rune
/// resolves `input::just_pressed` and friends at compile time, so a context
/// without the engine's modules rejects every script that touches the engine.
/// That is why the exporter boots an app (`AppConfig::export`) and compiles
/// through its host rather than constructing a compiler out of thin air.
impl balaur_script::ScriptCompiler for RuneHost {
    fn extensions(&self) -> &[&str] {
        &["rn"]
    }

    fn compile(&self, rel: &str, source: &str) -> Result<Vec<u8>> {
        self.compile_unit(rel, source)
            .map_err(|e| anyhow!("{rel}: {e}"))?;
        Ok(source.as_bytes().to_vec())
    }
}


struct Script {
    unit: Arc<Unit>,
    source: String,
    /// Resolved lifecycle and signal handlers. A miss is cached too: most
    /// scripts define none of `on_free`, and asking every frame is not free.
    methods: HashMap<String, Option<Function>>,
    lines: Rc<debugger::Lines>,
    functions: Vec<PublicSignature>,
    /// `exports()` evaluated once, since it is the same table for every node
    /// running this file. A reload replaces the whole `Script`, so a changed
    /// default reaches the next attach without an invalidation step.
    exports: Option<Vec<(String, balaur_script::Value)>>,
}

impl Script {
    fn new(unit: Arc<Unit>, source: String) -> Self {
        let lines = Rc::new(debugger::Lines::of(&unit, &source));
        let functions = public_functions(&source);
        Self {
            unit,
            source,
            methods: HashMap::new(),
            lines,
            functions,
            exports: None,
        }
    }
}

struct Instance {
    key: String,
    state: rune::Value,
}

/// One suspended async method: a VM future parked until `task::wait` finds
/// its wake, polled again on every wake.
struct RuneTask {
    /// The node whose script suspended; freeing it cancels the task.
    owner: Entity,
    /// The script key, so reloading the script cancels its tasks rather than
    /// resuming code that no longer exists.
    key: String,
    /// What to blame in an error report when the resumed code fails.
    label: String,
    future: std::pin::Pin<Box<rune::runtime::Future>>,
}

/// One file's breakpoints: the lines asked for, where they landed on the
/// unit currently loaded, and the instructions that stop it.
#[derive(Default)]
struct Breakpoints {
    requested: Vec<usize>,
    landed: Vec<usize>,
    ips: HashSet<usize>,
}

/// An execution the debugger parked, with the rest of the tick it cut short.
struct Paused {
    owner: Entity,
    key: String,
    label: String,
    exec: VmExecution<Vm>,
    /// The instruction it is parked on, run without breaking again on resume.
    ip: usize,
    lines: Rc<debugger::Lines>,
    pause: Pause,
    /// Instances the interrupted tick had not reached, run on resume.
    remaining: Vec<(Entity, String, rune::Value)>,
    method: Option<(String, f32)>,
}

thread_local! {
    /// Wake payloads not yet claimed by a `task::wait` future. Entries live
    /// only for the duration of one `wake` call: delivered or dropped.
    static WAKES: RefCell<Vec<(u64, balaur_script::Value)>> = const { RefCell::new(Vec::new()) };

    /// Hosts by the slot their context captured: Rune wants `Send + Sync`
    /// native functions, but a host is `Rc`-based, so `script::require`
    /// carries only an index here — the same trick the bindings use. Entries
    /// live as long as the thread; a test spawning many apps leaks a few
    /// handles, which the process outlives.
    static HOSTS: RefCell<Vec<RuneHost>> = const { RefCell::new(Vec::new()) };

    /// The functions behind `script::require`'s exports. A required module's
    /// entries are native trampolines holding only a slot here, because a
    /// unit-bound `Function` value called from *inside another unit's* VM
    /// execution dispatches into the wrong unit; a call routed through Rust
    /// lands correctly.
    static SHARED_FNS: RefCell<Vec<Function>> = const { RefCell::new(Vec::new()) };
}

/// A native function forwarding to `SHARED_FNS[slot]` with `arity` args.
/// Arity is fixed per wrapper because Rune native functions are typed;
/// script-model functions keep their whole signature on one line, which is
/// where the arity was read from.
fn trampoline(slot: usize, arity: usize) -> Option<Function> {
    fn relay(slot: usize, args: Vec<rune::Value>) -> rune::Value {
        // Cloned out before the call: the callee may itself require.
        let function = SHARED_FNS.with(|f| f.borrow()[slot].try_clone().ok());
        let outcome = function.map(|f| f.call::<rune::Value>(args).into_result());
        match outcome {
            Some(Ok(value)) => value,
            Some(Err(err)) => {
                tracing::error!("a required function failed: {err}");
                rune::to_value(()).expect("unit always converts")
            }
            None => rune::to_value(()).expect("unit always converts"),
        }
    }
    Some(match arity {
        0 => Function::new(move || relay(slot, Vec::new())),
        1 => Function::new(move |a: rune::Value| relay(slot, vec![a])),
        2 => Function::new(move |a: rune::Value, b: rune::Value| relay(slot, vec![a, b])),
        3 => Function::new(move |a: rune::Value, b: rune::Value, c: rune::Value| {
            relay(slot, vec![a, b, c])
        }),
        4 => Function::new(
            move |a: rune::Value, b: rune::Value, c: rune::Value, d: rune::Value| {
                relay(slot, vec![a, b, c, d])
            },
        ),
        5 => Function::new(
            move |a: rune::Value,
                  b: rune::Value,
                  c: rune::Value,
                  d: rune::Value,
                  e: rune::Value| { relay(slot, vec![a, b, c, d, e]) },
        ),
        _ => return None,
    })
}

/// `mod name;` in a packed script: `name.rn` or `name/mod.rn` beside the
/// requesting file, looked up in the pack.
struct PackSourceLoader {
    scripts: std::collections::BTreeMap<String, Vec<u8>>,
}

impl rune::compile::SourceLoader for PackSourceLoader {
    fn load(
        &mut self,
        root: &Path,
        item: &rune::Item,
        span: &dyn rune::ast::Spanned,
    ) -> rune::compile::Result<Source> {
        let not_found = |path: PathBuf| {
            rune::compile::Error::msg(
                span,
                format!("module {} is not in the pack", path.display()),
            )
        };
        let mut base = root.to_path_buf();
        base.pop();
        for component in item {
            match component {
                rune::item::ComponentRef::Str(name) => base.push(name),
                _ => return Err(not_found(base)),
            }
        }
        for candidate in [base.join("mod.rn"), base.with_extension("rn")] {
            let key = candidate.to_string_lossy().replace('\\', "/");
            if let Some(bytes) = self.scripts.get(&key) {
                let text = String::from_utf8_lossy(bytes);
                return Ok(Source::with_path(key.as_str(), text.as_ref(), &candidate)?);
            }
        }
        Err(not_found(base))
    }
}

/// The future behind `task::wait(token)`: pending until its token's wake
/// payload appears, ready with that payload converted for the script.
struct WaitFuture {
    token: u64,
}

impl std::future::Future for WaitFuture {
    type Output = rune::Value;

    fn poll(
        self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<rune::Value> {
        let taken = WAKES.with(|wakes| {
            let mut wakes = wakes.borrow_mut();
            let at = wakes.iter().position(|(t, _)| *t == self.token)?;
            Some(wakes.remove(at).1)
        });
        let Some(payload) = taken else {
            return std::task::Poll::Pending;
        };
        match value::from_neutral(&payload) {
            Ok(value) => std::task::Poll::Ready(value),
            Err(err) => {
                tracing::error!("task::wait({}): {err}", self.token);
                std::task::Poll::Ready(rune::to_value(()).expect("unit always converts"))
            }
        }
    }
}

struct State {
    /// Stop where a script threw instead of logging it past.
    break_on_error: bool,
    /// A debugger asked to stop; the next line a script runs is where.
    break_next: bool,
    project_root: PathBuf,
    pack: Option<Pack>,
    /// Registered by plugins at startup, folded into the context on first use.
    pending: Rc<RefCell<Vec<rune::Module>>>,
    /// Built once. Compiling needs the full context; running needs only the
    /// runtime half.
    context: Option<(Rc<rune::Context>, Arc<RuntimeContext>)>,
    scripts: HashMap<String, Script>,
    /// `script::require` results: an object of the script's public functions
    /// per key. The object's contents swap in place on hot reload, so every
    /// requirer sees the new code.
    modules: HashMap<String, rune::Value>,
    /// Insertion-ordered so `update` visits nodes the same way every run.
    instances: indexmap::IndexMap<Entity, Instance>,
    /// Suspended async methods, in suspension order — resume order on a wake.
    tasks: Vec<RuneTask>,
    events: Option<Receiver<notify::Result<notify::Event>>>,
    _watcher: Option<RecommendedWatcher>,
    breakpoints: HashMap<String, Breakpoints>,
    paused: Option<Paused>,
}

#[derive(Clone)]
pub struct RuneHost {
    engine: Engine,
    state: Rc<RefCell<State>>,
}

impl RuneHost {
    pub fn new(
        engine: Engine,
        project_root: &Path,
        pack: Option<Pack>,
        watch: bool,
    ) -> Result<Self> {
        let project_root = project_root
            .canonicalize()
            .unwrap_or_else(|_| project_root.to_path_buf());
        let (watcher, events) = if watch && pack.is_none() {
            let (tx, rx) = std::sync::mpsc::channel();
            let mut watcher = notify::recommended_watcher(move |res| {
                let _ = tx.send(res);
            })?;
            watcher
                .watch(&project_root, RecursiveMode::Recursive)
                .with_context(|| format!("watching {}", project_root.display()))?;
            (Some(watcher), Some(rx))
        } else {
            (None, None)
        };
        Ok(Self {
            engine,
            state: Rc::new(RefCell::new(State {
                break_on_error: false,
                break_next: false,
                project_root,
                pack,
                pending: Rc::new(RefCell::new(Vec::new())),
                context: None,
                scripts: HashMap::new(),
                modules: HashMap::new(),
                instances: indexmap::IndexMap::new(),
                tasks: Vec::new(),
                events,
                _watcher: watcher,
                breakpoints: HashMap::new(),
                paused: None,
            })),
        })
    }

    pub fn engine(&self) -> Engine {
        self.engine.clone()
    }

    /// Fold every registered module into a context.
    ///
    /// Deferred to first use because Rune builds a context once, and plugins
    /// are still registering bindings while the app is being assembled.
    fn context(&self) -> Result<(Rc<rune::Context>, Arc<RuntimeContext>)> {
        if let Some(built) = &self.state.borrow().context {
            return Ok(built.clone());
        }
        let mut ctx = rune::Context::with_default_modules()?;
        let mut values = rune::Module::with_crate("balaur")?;
        value::install(&mut values, &self.engine)?;
        ctx.install(values)?;
        // `task::wait(token).await` parks until the engine wakes the token.
        // `init` and handlers may be async; `update` is deliberately synchronous.
        let mut task = rune::Module::with_crate("task")?;
        task.function("wait", |token: i64| WaitFuture {
            token: u64::try_from(token).unwrap_or(u64::MAX),
        })
        .build()?;
        ctx.install(task)?;
        ctx.install(script_module(self)?)?;
        let pending = self.state.borrow().pending.clone();
        for m in pending.borrow_mut().drain(..) {
            ctx.install(m)?;
        }
        let runtime = Arc::new(ctx.runtime()?);
        let built = (Rc::new(ctx), runtime);
        self.state.borrow_mut().context = Some(built.clone());
        Ok(built)
    }

    fn normalize_key(path: &str) -> String {
        path.trim_start_matches("./").replace('\\', "/")
    }

    pub fn scene_source(&self, rel: &str) -> Option<String> {
        let state = self.state.borrow();
        let key = Self::normalize_key(rel);
        if let Some(pack) = &state.pack {
            return pack.scenes.get(&key).cloned();
        }
        std::fs::read_to_string(state.project_root.join(&key)).ok()
    }

    fn source_of(&self, key: &str) -> Result<String> {
        let state = self.state.borrow();
        if let Some(pack) = &state.pack {
            let bytes = pack
                .scripts
                .get(key)
                .ok_or_else(|| anyhow!("{key} is not in the pack"))?;
            return Ok(String::from_utf8(bytes.clone())?);
        }
        std::fs::read_to_string(state.project_root.join(key))
            .with_context(|| format!("reading {key}"))
    }

    /// The source carries its on-disk path so `mod name;` finds `name.rn`
    /// beside it.
    fn compile_unit(&self, key: &str, source: &str) -> Result<Arc<Unit>> {
        let (ctx, _) = self.context()?;
        // A packed script's `mod` resolves inside the pack, keyed the way
        // `Pack::build` keyed it; a dev run reads beside the file on disk.
        let (path, packed) = {
            let state = self.state.borrow();
            match &state.pack {
                Some(pack) => (PathBuf::from(key), Some(pack.scripts.clone())),
                None => (state.project_root.join(key), None),
            }
        };
        let mut sources = Sources::new();
        sources.insert(Source::with_path(key, source, path)?)?;
        // Warnings are the language server's business; an error report
        // should be the error.
        let mut diagnostics = Diagnostics::without_warnings();
        let mut loader = PackSourceLoader {
            scripts: packed.clone().unwrap_or_default(),
        };
        let mut prepared = rune::prepare(&mut sources)
            .with_context(&ctx)
            .with_diagnostics(&mut diagnostics);
        if packed.is_some() {
            prepared = prepared.with_source_loader(&mut loader);
        }
        let built = prepared.build();
        match built {
            Ok(unit) => Ok(Arc::new(unit)),
            Err(_) => Err(anyhow!("{}", render(&diagnostics, &sources))),
        }
    }


    fn load(&self, key: &str) -> Result<Arc<Unit>> {
        if let Some(script) = self.state.borrow().scripts.get(key) {
            return Ok(script.unit.clone());
        }
        let source = self.source_of(key)?;
        let unit = self.compile_unit(key, &source)?;
        self.state
            .borrow_mut()
            .scripts
            .insert(key.to_string(), Script::new(unit.clone(), source));
        self.apply_breakpoints(key);
        Ok(unit)
    }


    /// Look up a script function, returning `None` when it is not defined.
    fn method(&self, key: &str, name: &str) -> Option<Function> {
        if let Some(hit) = self
            .state
            .borrow()
            .scripts
            .get(key)
            .and_then(|s| s.methods.get(name))
        {
            return hit.as_ref().and_then(|f| f.try_clone().ok());
        }
        let unit = self.load(key).ok()?;
        let (_, runtime) = self.context().ok()?;
        let found = Vm::new(runtime, unit).lookup_function([name]).ok();
        let cached = found.as_ref().and_then(|f| f.try_clone().ok());
        if let Some(script) = self.state.borrow_mut().scripts.get_mut(key) {
            script.methods.insert(name.to_string(), cached);
        }
        found
    }

    pub fn attach(&self, entity: Entity, path: &str) -> Result<()> {
        self.attach_with_props(entity, path, &[])
    }

    /// Attach, writing the script's exported defaults and then the scene's
    /// `props` over them onto the instance, before `init` sees it.
    pub fn attach_with_props(
        &self,
        entity: Entity,
        path: &str,
        props: &[(String, balaur_script::Value)],
    ) -> Result<()> {
        let key = Self::normalize_key(path);
        self.load(&key)?;
        let mut obj = rune::runtime::Object::new();
        obj.insert(
            rune::alloc::String::try_from("node")?,
            rune::to_value(Node {
                id: entity.to_bits().get(),
            })?,
        )?;
        let declared = self.exports(&key)?;
        for (name, value) in &declared {
            obj.insert(
                rune::alloc::String::try_from(name.as_str())?,
                value::from_neutral(value)?,
            )?;
        }
        for (name, value) in props {
            if !declared.iter().any(|(d, _)| d == name) {
                tracing::warn!("[{key}] property '{name}' is set on a node but not exported");
            }
            obj.insert(
                rune::alloc::String::try_from(name.as_str())?,
                value::from_neutral(value)?,
            )?;
        }
        let state = rune::to_value(obj)?;
        self.state.borrow_mut().instances.insert(
            entity,
            Instance {
                key: key.clone(),
                state: state.try_clone()?,
            },
        );
        self.engine
            .world_mut()
            .insert_one(entity, ScriptAttachment { path: key.clone() })
            .map_err(|_| anyhow!("cannot attach script to a dead node"))?;
        self.invoke(entity, &key, "init", vec![state], true);
        Ok(())
    }

    /// Tasks the node's script left suspended die with it, a pause included.
    pub fn detach(&self, entity: Entity) {
        let (inst, paused) = {
            let mut state = self.state.borrow_mut();
            state.tasks.retain(|t| t.owner != entity);
            let paused = state.paused.take_if(|p| p.owner == entity);
            (state.instances.shift_remove(&entity), paused)
        };
        if let Some(paused) = paused {
            self.drop_pause(&paused);
        }
        if let Some(inst) = inst {
            if let Some(on_free) = self.method(&inst.key, "on_free") {
                if let VmResult::Err(err) = on_free.call::<()>((inst.state,)) {
                    tracing::error!("[{}] on_free: {err}", inst.key);
                }
            }
        }
    }

    pub fn update(&self, dt: f32) {
        self.tick_lifecycle("update", dt);
    }

    pub fn fixed_update(&self, dt: f32) {
        self.tick_lifecycle("fixed_update", dt);
    }

    /// Call `method(dt)` on every live instance that defines it.
    fn tick_lifecycle(&self, method: &str, dt: f32) {
        let batch = self.live_batch();
        self.run_batch(method, dt, batch);
    }

    /// Every instance's state, for a rollback snapshot: a script that
    /// defines `save_state` says what matters, one that does not gets its
    /// plain fields captured. `node` is skipped -- the host reinstates it.
    pub fn save_state(&self) -> Vec<(balaur_script::NodeId, balaur_script::Value)> {
        let batch: Vec<(Entity, String, rune::Value)> = self
            .state
            .borrow()
            .instances
            .iter()
            .filter_map(|(e, i)| Some((*e, i.key.clone(), i.state.try_clone().ok()?)))
            .collect();
        let mut out = Vec::with_capacity(batch.len());
        for (entity, key, state) in batch {
            let node = balaur_core::node_id_of(entity);
            if let Some(f) = self.method(&key, "save_state") {
                match f.call::<rune::Value>((state,)) {
                    VmResult::Ok(value) => {
                        if let Some(plain) = value::to_plain(&value) {
                            out.push((node, plain));
                        }
                    }
                    VmResult::Err(err) => tracing::error!("[{key}] save_state: {err}"),
                }
                continue;
            }
            let Ok(obj) = state.borrow_ref::<rune::runtime::Object>() else {
                continue;
            };
            let mut fields = Vec::new();
            for (k, v) in obj.iter() {
                if k.as_str() == "node" {
                    continue;
                }
                if let Some(plain) = value::to_plain(v) {
                    fields.push((k.to_string(), plain));
                }
            }
            fields.sort_by(|a, b| a.0.cmp(&b.0));
            out.push((node, balaur_script::Value::Map(fields)));
        }
        out
    }

    /// Put instances back, through `load_state` where a script defines one.
    pub fn load_state(&self, states: &[(balaur_script::NodeId, balaur_script::Value)]) {
        for (node, value) in states {
            let Ok(entity) = balaur_core::entity_of(*node) else {
                continue;
            };
            let Some((key, state)) = self
                .state
                .borrow()
                .instances
                .get(&entity)
                .and_then(|i| Some((i.key.clone(), i.state.try_clone().ok()?)))
            else {
                continue;
            };
            if let Some(f) = self.method(&key, "load_state") {
                match value::from_neutral(value) {
                    Ok(arg) => {
                        if let VmResult::Err(err) = f.call::<rune::Value>((state, arg)) {
                            tracing::error!("[{key}] load_state: {err}");
                        }
                    }
                    Err(err) => tracing::error!("[{key}] load_state: {err}"),
                }
                continue;
            }
            let balaur_script::Value::Map(fields) = value else {
                continue;
            };
            let Ok(mut obj) = state.borrow_mut::<rune::runtime::Object>() else {
                continue;
            };
            for (k, v) in fields {
                let (Ok(v), Ok(k)) = (
                    value::from_neutral(v),
                    rune::alloc::String::try_from(k.as_str()),
                ) else {
                    continue;
                };
                if let Err(err) = obj.insert(k, v) {
                    tracing::error!("[{key}] load_state: {err}");
                }
            }
        }
    }

    /// Call one node's script method — a signal, or one script calling
    /// another. Returns the method's return value; `None` when the call did
    /// not run to completion here (no instance, no such method, an async
    /// method that is now suspended, or a node the debugger holds).
    pub fn call_on(
        &self,
        entity: Entity,
        method: &str,
        args: &[balaur_script::Value],
    ) -> Option<balaur_script::Value> {
        let found = {
            let state = self.state.borrow();
            if self.is_held(entity, &state) {
                return None;
            }
            state
                .instances
                .get(&entity)
                .and_then(|i| Some((i.key.clone(), i.state.try_clone().ok()?)))
        };
        let (key, state) = found?;
        // The instance first, then the payload: `pub fn on_x(this, a, b)`,
        // the same shape `update(this, dt)` already has.
        let mut call_args = vec![state];
        for arg in args {
            match value::from_neutral(arg) {
                Ok(value) => call_args.push(value),
                Err(err) => {
                    tracing::error!("[{key}] {method}: {err}");
                    return None;
                }
            }
        }
        self.invoke(entity, &key, method, call_args, true)
    }

    pub fn call_all(&self, method: &str) {
        for (entity, key, state) in self.live_batch() {
            self.invoke(entity, &key, method, vec![state], true);
        }
    }

    /// File an async method's future as a task and run it to its first await.
    /// Anything but a future is a finished synchronous call — nothing to do.
    fn settle_call(
        &self,
        owner: Entity,
        key: &str,
        label: &str,
        value: rune::Value,
    ) -> Option<balaur_script::Value> {
        if value.type_hash() != rune::runtime::Future::HASH {
            return match value::to_neutral(&value) {
                Ok(value) => Some(value),
                Err(err) => {
                    tracing::error!("[{key}] {label}: {err}");
                    None
                }
            };
        }
        let future = match value.into_future() {
            Ok(future) => future,
            Err(err) => {
                tracing::error!("[{key}] {label}: {err}");
                return None;
            }
        };
        self.state.borrow_mut().tasks.push(RuneTask {
            owner,
            key: key.to_string(),
            label: label.to_string(),
            future: Box::pin(future),
        });
        self.poll_tasks();
        None
    }

    /// Poll every suspended task once, in suspension order. Progress only
    /// happens when a wake has put a payload where some `task::wait` looks,
    /// so a poll with nothing delivered is a cheap no-op.
    fn poll_tasks(&self) {
        // Taken out before polling: resumed code may spawn new tasks or call
        // back into the host, and the list must not be borrowed then.
        let tasks = std::mem::take(&mut self.state.borrow_mut().tasks);
        let mut context = std::task::Context::from_waker(std::task::Waker::noop());
        let mut kept = Vec::new();
        for mut task in tasks {
            match task.future.as_mut().poll(&mut context) {
                std::task::Poll::Ready(VmResult::Ok(_)) => {}
                std::task::Poll::Ready(VmResult::Err(err)) => {
                    tracing::error!("[{}] {}: {err}", task.key, task.label);
                }
                std::task::Poll::Pending => kept.push(task),
            }
        }
        let mut state = self.state.borrow_mut();
        // Tasks spawned while polling queue up behind the survivors.
        kept.append(&mut state.tasks);
        state.tasks = kept;
    }

    /// Resume every task suspended on `token` with `payload`, in suspension
    /// order. An unclaimed wake is dropped, not stored.
    pub fn wake(&self, token: u64, payload: &balaur_script::Value) {
        WAKES.with(|wakes| wakes.borrow_mut().push((token, payload.clone())));
        self.poll_tasks();
        WAKES.with(|wakes| wakes.borrow_mut().retain(|(t, _)| *t != token));
    }

    /// Drain watcher events and reload changed scripts.
    pub fn pump_reloads(&self) {
        let mut changed: Vec<String> = Vec::new();
        let mut assets: Vec<String> = Vec::new();
        let mut sources = false;
        {
            let state = self.state.borrow();
            let Some(events) = &state.events else { return };
            while let Ok(event) = events.try_recv() {
                let Ok(event) = event else { continue };
                for path in event.paths {
                    let Ok(rel) = path.strip_prefix(&state.project_root) else {
                        continue;
                    };
                    let key = rel.to_string_lossy().replace('\\', "/");
                    match path.extension().and_then(|e| e.to_str()) {
                        Some("rn") => {
                            if state.scripts.contains_key(&key) && !changed.contains(&key) {
                                changed.push(key);
                            }
                        }
                        // Assets and scenes are both TOML; `reload` drops only
                        // what was cached, so a saved scene changes nothing.
                        Some("toml") if !assets.contains(&key) => assets.push(key),
                        // A shader is source a material links, not an asset
                        // anything parsed: there is nothing cached to drop,
                        // only the counter its material watches.
                        Some("wesl") => sources = true,
                        _ => {}
                    }
                }
            }
        }
        if sources {
            balaur_core::assets::invalidate(&self.engine);
        }
        for key in assets {
            // A strings file is not an asset — nothing references it — so the
            // catalogue has its own forgetting.
            if key.starts_with("strings/") {
                let _ = balaur_core::strings::reload(&self.engine);
                continue;
            }
            if let Err(err) = balaur_core::assets::reload(&self.engine, &key) {
                tracing::warn!("could not reload asset {key}: {err}");
            }
        }
        for key in changed {
            match self.reload(&key) {
                Ok(()) => tracing::info!("hot reloaded {key}"),
                Err(err) => tracing::error!("[{key}] {err}"),
            }
        }
    }

    /// Recompile a script and rebind its live instances.
    ///
    /// Rune compiles to an immutable unit, so there is no class table to swap
    /// in place: the instances keep their state objects and the next call
    /// resolves against the new unit. A compile error leaves the previous
    /// unit running.
    pub fn reload(&self, key: &str) -> Result<()> {
        let source = self.source_of(key)?;
        if self
            .state
            .borrow()
            .scripts
            .get(key)
            .map(|s| s.source.as_str())
            == Some(source.as_str())
        {
            return Ok(());
        }
        let unit = self.compile_unit(key, &source)?;
        let paused = {
            let mut state = self.state.borrow_mut();
            // A task suspended in the old unit must not resume into it.
            state.tasks.retain(|t| t.key != key);
            state
                .scripts
                .insert(key.to_string(), Script::new(unit, source));
            state.paused.take_if(|p| p.key == key)
        };
        if let Some(paused) = paused {
            self.drop_pause(&paused);
        }
        self.apply_breakpoints(key);
        self.refresh_module(key)
    }

    /// `script::require`: an object of `key`'s public functions, cached so
    /// every requirer holds the same object.
    pub fn require_module(&self, path: &str) -> Result<rune::Value> {
        let key = Self::normalize_key(path);
        let cached = self
            .state
            .borrow()
            .modules
            .get(&key)
            .and_then(|v| v.try_clone().ok());
        if let Some(module) = cached {
            return Ok(module);
        }
        let object = self.module_object(&key)?;
        let value = rune::to_value(object)?;
        self.state
            .borrow_mut()
            .modules
            .insert(key, value.try_clone()?);
        Ok(value)
    }


    /// Build the export object for one script: its `pub fn`s by name, each
    /// wrapped in a Rust-routed trampoline (see `SHARED_FNS`).
    fn module_object(&self, key: &str) -> Result<rune::runtime::Object> {
        self.load(key)?;
        let functions = self
            .state
            .borrow()
            .scripts
            .get(key)
            .map(|s| s.functions.clone())
            .ok_or_else(|| anyhow!("{key} did not load"))?;
        let mut object = rune::runtime::Object::new();
        for declared in functions {
            let Some(function) = self.method(key, &declared.name) else {
                continue;
            };
            let Some(wrapper) = SHARED_FNS.with(|shared| {
                let mut shared = shared.borrow_mut();
                shared.push(function);
                trampoline(shared.len() - 1, declared.arity)
            }) else {
                tracing::warn!(
                    "{key}: `{}` takes too many parameters to require",
                    declared.name
                );
                continue;
            };
            object.insert(
                rune::alloc::String::try_from(declared.name.as_str())?,
                rune::to_value(wrapper)?,
            )?;
        }
        Ok(object)
    }

    /// After a reload, swap a required module's contents in place so every
    /// held copy sees the new code.
    fn refresh_module(&self, key: &str) -> Result<()> {
        let cached = self
            .state
            .borrow()
            .modules
            .get(key)
            .and_then(|v| v.try_clone().ok());
        let Some(cached) = cached else { return Ok(()) };
        let fresh = self.module_object(key)?;
        let mut object = cached.borrow_mut::<rune::runtime::Object>()?;
        object.clear();
        for (name, value) in fresh {
            object.insert(name, value)?;
        }
        Ok(())
    }

    /// Read a number out of a node's instance state, for tests and tools that
    /// need to see what a script wrote.
    pub fn number_field(&self, entity: Entity, name: &str) -> Option<f64> {
        let state = self.state.borrow();
        let inst = state.instances.get(&entity)?;
        let obj = inst.state.borrow_ref::<rune::runtime::Object>().ok()?;
        let field = obj.get(name)?;
        if let Ok(n) = rune::from_value::<f64>(field.try_clone().ok()?) {
            return Some(n);
        }
        rune::from_value::<i64>(field.try_clone().ok()?)
            .ok()
            .map(|n| n as f64)
    }

    /// Read a string out of a node's instance state.
    pub fn text_field(&self, entity: Entity, name: &str) -> Option<String> {
        let state = self.state.borrow();
        let inst = state.instances.get(&entity)?;
        let obj = inst.state.borrow_ref::<rune::runtime::Object>().ok()?;
        Some(obj.get(name)?.borrow_string_ref().ok()?.to_string())
    }

    pub fn instance_count(&self) -> usize {
        self.state.borrow().instances.len()
    }

    pub fn module(&self, name: &str) -> Result<RuneModule> {
        let state = self.state.borrow();
        if state.context.is_some() {
            return Err(anyhow!(
                "bindings must be registered before the first script runs"
            ));
        }
        RuneModule::new(name, self.engine.clone(), state.pending.clone())
    }
}


impl balaur_script::ScriptHost<Engine> for RuneHost {
    fn module(&self, name: &str) -> Result<Box<dyn balaur_script::Bindings<Engine>>> {
        Ok(Box::new(RuneHost::module(self, name)?))
    }

    fn attach_with_props(
        &self,
        node: balaur_script::NodeId,
        path: &str,
        props: &[(String, balaur_script::Value)],
    ) -> Result<()> {
        RuneHost::attach_with_props(self, balaur_core::entity_of(node)?, path, props)
    }

    fn exports(&self, path: &str) -> Result<Vec<(String, balaur_script::Value)>> {
        RuneHost::exports(self, &Self::normalize_key(path))
    }

    fn detach(&self, node: balaur_script::NodeId) {
        if let Ok(entity) = balaur_core::entity_of(node) {
            RuneHost::detach(self, entity);
        }
    }

    fn update(&self, dt: f32) {
        RuneHost::update(self, dt);
    }
    fn save_state(&self) -> Vec<(balaur_script::NodeId, balaur_script::Value)> {
        Self::save_state(self)
    }
    fn load_state(&self, states: &[(balaur_script::NodeId, balaur_script::Value)]) {
        Self::load_state(self, states);
    }

    fn fixed_update(&self, dt: f32) {
        RuneHost::fixed_update(self, dt);
    }

    fn pump_reloads(&self) {
        RuneHost::pump_reloads(self);
    }

    fn reload(&self, key: &str) -> Result<()> {
        RuneHost::reload(self, key)
    }

    fn call_on(
        &self,
        node: balaur_script::NodeId,
        method: &str,
        args: &[balaur_script::Value],
    ) -> Option<balaur_script::Value> {
        let entity = balaur_core::entity_of(node).ok()?;
        RuneHost::call_on(self, entity, method, args)
    }

    fn call_all(&self, method: &str) {
        RuneHost::call_all(self, method);
    }

    fn wake(&self, token: u64, payload: &balaur_script::Value) {
        RuneHost::wake(self, token, payload);
    }

    fn scene_source(&self, rel: &str) -> Option<String> {
        RuneHost::scene_source(self, rel)
    }

    fn instance_count(&self) -> usize {
        RuneHost::instance_count(self)
    }

    fn invoke(
        &self,
        callback: balaur_script::CallbackId,
        args: &[balaur_script::Value],
    ) -> Result<balaur_script::Value> {
        let func = bindings::lookup_callback(callback)
            .ok_or_else(|| anyhow!("callback used after its call returned"))?;
        let args: Result<Vec<rune::Value>> = args.iter().map(value::from_neutral).collect();
        let out = func.call::<rune::Value>(args?).into_result()?;
        value::to_neutral(&out)
    }

    fn call_in(
        &self,
        path: &str,
        function: &str,
        args: &[balaur_script::Value],
    ) -> Result<Option<balaur_script::Value>> {
        let key = Self::normalize_key(path);
        self.load(&key)?;
        let Some(func) = self.method(&key, function) else {
            return Ok(None);
        };
        let args: Result<Vec<rune::Value>> = args.iter().map(value::from_neutral).collect();
        let out = func
            .call::<rune::Value>(args?)
            .into_result()
            .map_err(|err| anyhow!("[{key}] {function}: {err}"))?;
        Ok(Some(value::to_neutral(&out)?))
    }

    fn set_breakpoints(&self, path: &str, lines: &[usize]) -> Result<Vec<usize>> {
        RuneHost::set_breakpoints(self, path, lines)
    }

    fn breakpoints(&self, path: &str) -> Vec<usize> {
        RuneHost::breakpoints(self, path)
    }

    fn set_break_on_error(&self, on: bool) {
        Self::set_break_on_error(self, on);
    }

    fn break_on_error(&self) -> bool {
        Self::break_on_error(self)
    }

    fn request_break(&self) {
        RuneHost::request_break(self);
    }

    fn paused(&self) -> Option<Pause> {
        RuneHost::paused(self)
    }

    fn resume(&self, mode: StepMode) {
        RuneHost::resume(self, mode);
    }

    fn as_any(&self) -> &dyn core::any::Any {
        self
    }
}
