//! The Rune script host: loading, instancing, hot reload, precompiled packs.
//!
//! Scripting model, matching the Luau backend: a `.rn` file declares free
//! functions taking the instance as their first argument. One instance object
//! is created per node the script is attached to, and the host hands it back
//! on every call.
//!
//! ```rune
//! pub fn init(this) { this.angle = 0.0; }
//! pub fn update(this, dt) { this.angle += dt; }
//! ```
//!
//! A Rune object handed into a function is mutated in place, so what a script
//! writes to `this` is what the host sees on the next frame.

mod bindings;
mod value;

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::mpsc::Receiver;
use std::sync::Arc;

use anyhow::{anyhow, Context as _, Result};
use balaur_core::scene::ScriptAttachment;
use balaur_core::{Engine, Pack};
use hecs::Entity;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use rune::alloc::clone::TryClone as _;
use rune::runtime::{Function, RuntimeContext, Unit, VmResult};
use rune::{Diagnostics, Source, Sources, Vm};

pub use bindings::RuneModule;
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

fn render(diagnostics: &Diagnostics, sources: &Sources) -> String {
    let mut buf = rune::termcolor::Buffer::no_color();
    if diagnostics.emit(&mut buf, sources).is_err() {
        return "unprintable diagnostics".into();
    }
    String::from_utf8_lossy(buf.as_slice()).trim().to_string()
}

struct Script {
    unit: Arc<Unit>,
    source: String,
    /// Resolved lifecycle and signal handlers. A miss is cached too: most
    /// scripts define none of `on_free`, and asking every frame is not free.
    methods: HashMap<String, Option<Function>>,
}

struct Instance {
    key: String,
    state: rune::Value,
}

struct State {
    project_root: PathBuf,
    pack: Option<Pack>,
    /// Registered by plugins at startup, folded into the context on first use.
    pending: Rc<RefCell<Vec<rune::Module>>>,
    /// Built once. Compiling needs the full context; running needs only the
    /// runtime half.
    context: Option<(Rc<rune::Context>, Arc<RuntimeContext>)>,
    scripts: HashMap<String, Script>,
    /// Insertion-ordered so `update` visits nodes the same way every run.
    instances: indexmap::IndexMap<Entity, Instance>,
    events: Option<Receiver<notify::Result<notify::Event>>>,
    _watcher: Option<RecommendedWatcher>,
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
                project_root,
                pack,
                pending: Rc::new(RefCell::new(Vec::new())),
                context: None,
                scripts: HashMap::new(),
                instances: indexmap::IndexMap::new(),
                events,
                _watcher: watcher,
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

    fn compile_unit(&self, key: &str, source: &str) -> Result<Arc<Unit>> {
        let (ctx, _) = self.context()?;
        let mut sources = Sources::new();
        sources.insert(Source::new(key, source)?)?;
        let mut diagnostics = Diagnostics::new();
        let built = rune::prepare(&mut sources)
            .with_context(&ctx)
            .with_diagnostics(&mut diagnostics)
            .build();
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
        self.state.borrow_mut().scripts.insert(
            key.to_string(),
            Script {
                unit: unit.clone(),
                source,
                methods: HashMap::new(),
            },
        );
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
        let key = Self::normalize_key(path);
        self.load(&key)?;
        let mut obj = rune::runtime::Object::new();
        obj.insert(
            rune::alloc::String::try_from("node")?,
            rune::to_value(Node {
                id: entity.to_bits().get(),
            })?,
        )?;
        let state = rune::to_value(obj)?;
        self.state.borrow_mut().instances.insert(
            entity,
            Instance {
                key: key.clone(),
                state: state.clone(),
            },
        );
        self.engine
            .world_mut()
            .insert_one(entity, ScriptAttachment { path: key.clone() })
            .map_err(|_| anyhow!("cannot attach script to a dead node"))?;
        if let Some(init) = self.method(&key, "init") {
            if let VmResult::Err(err) = init.call::<()>((state,)) {
                tracing::error!("[{key}] init: {err}");
            }
        }
        Ok(())
    }

    pub fn detach(&self, entity: Entity) {
        let inst = self.state.borrow_mut().instances.shift_remove(&entity);
        if let Some(inst) = inst {
            if let Some(on_free) = self.method(&inst.key, "on_free") {
                if let VmResult::Err(err) = on_free.call::<()>((inst.state,)) {
                    tracing::error!("[{}] on_free: {err}", inst.key);
                }
            }
        }
    }

    pub fn update(&self, dt: f32) {
        // Collect first so a script may attach, detach or spawn during its own
        // update without the host state being borrowed.
        let batch: Vec<(String, rune::Value)> = self
            .state
            .borrow()
            .instances
            .values()
            .filter_map(|i| Some((i.key.clone(), i.state.try_clone().ok()?)))
            .collect();
        for (key, state) in batch {
            let Some(update) = self.method(&key, "update") else {
                continue;
            };
            if let VmResult::Err(err) = update.call::<()>((state, f64::from(dt))) {
                tracing::error!("[{key}] update: {err}");
            }
        }
    }

    pub fn call_on(&self, entity: Entity, method: &str) {
        let found = self
            .state
            .borrow()
            .instances
            .get(&entity)
            .and_then(|i| Some((i.key.clone(), i.state.try_clone().ok()?)));
        let Some((key, state)) = found else { return };
        let Some(f) = self.method(&key, method) else {
            return;
        };
        if let VmResult::Err(err) = f.call::<()>((state,)) {
            tracing::error!("[{key}] {method}: {err}");
        }
    }

    pub fn call_all(&self, method: &str) {
        let batch: Vec<(String, rune::Value)> = self
            .state
            .borrow()
            .instances
            .values()
            .filter_map(|i| Some((i.key.clone(), i.state.try_clone().ok()?)))
            .collect();
        for (key, state) in batch {
            let Some(f) = self.method(&key, method) else {
                continue;
            };
            if let VmResult::Err(err) = f.call::<()>((state,)) {
                tracing::error!("[{key}] {method}: {err}");
            }
        }
    }

    /// Drain watcher events and reload changed scripts.
    pub fn pump_reloads(&self) {
        let mut changed: Vec<String> = Vec::new();
        {
            let state = self.state.borrow();
            let Some(events) = &state.events else { return };
            while let Ok(event) = events.try_recv() {
                let Ok(event) = event else { continue };
                for path in event.paths {
                    if path.extension().and_then(|e| e.to_str()) != Some("rn") {
                        continue;
                    }
                    let Ok(rel) = path.strip_prefix(&state.project_root) else {
                        continue;
                    };
                    let key = rel.to_string_lossy().replace('\\', "/");
                    if state.scripts.contains_key(&key) && !changed.contains(&key) {
                        changed.push(key);
                    }
                }
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
    /// Rune compiles to an immutable unit, so unlike Luau there is no class
    /// table to swap in place: the instances keep their state objects and the
    /// next call resolves against the new unit. A compile error leaves the
    /// previous unit running.
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
        let mut state = self.state.borrow_mut();
        state.scripts.insert(
            key.to_string(),
            Script {
                unit,
                source,
                methods: HashMap::new(),
            },
        );
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

    fn attach(&self, node: balaur_script::NodeId, path: &str) -> Result<()> {
        RuneHost::attach(self, balaur_core::entity_of(node)?, path)
    }

    fn detach(&self, node: balaur_script::NodeId) {
        if let Ok(entity) = balaur_core::entity_of(node) {
            RuneHost::detach(self, entity);
        }
    }

    fn update(&self, dt: f32) {
        RuneHost::update(self, dt);
    }

    fn pump_reloads(&self) {
        RuneHost::pump_reloads(self);
    }

    fn reload(&self, key: &str) -> Result<()> {
        RuneHost::reload(self, key)
    }

    fn call_on(&self, node: balaur_script::NodeId, method: &str) {
        if let Ok(entity) = balaur_core::entity_of(node) {
            RuneHost::call_on(self, entity, method);
        }
    }

    fn call_all(&self, method: &str) {
        RuneHost::call_all(self, method);
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

    fn as_any(&self) -> &dyn core::any::Any {
        self
    }
}
