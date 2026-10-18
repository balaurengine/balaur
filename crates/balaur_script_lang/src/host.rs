// `.bl` scripts behind the seam: one compiled module per file, one host-held
// object per node, its lifecycle methods called by name when the script has them.

use crate::api::{self, Module, Registry};
use crate::convert::{from_neutral, to_neutral};
use anyhow::{Context, Result, anyhow};
use balaur_core::{Engine, Pack};
use balaur_lang::runtime::{HostFns, Limits, Object, Runtime, Value as L};
use balaur_lang::{Compiled, HostApi, Literal, Options, Ty};
use balaur_script::{
    Bindings, CallbackId, NodeId, Pause, ScriptCompiler, ScriptHost, StepMode, Value,
};
use debug::Held;
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;
use std::rc::{Rc, Weak};
use tasks::Pending;

mod debug;
mod tasks;

pub const EXTENSION: &str = "bl";
const BUILT_IN: [&str; 3] = ["Vec2", "Vec3", "Color"];
const NODE_FIELD: &str = "node";
// Rune's watcher hears only its own files, so a loaded `.bl` is looked at
// every this many frames.
pub const POLL_FRAMES: u32 = 15;

struct File {
    rt: RefCell<Runtime>,
    compiled: Compiled,
    ty: String,
}

impl File {
    fn method(&self, name: &str) -> Option<&balaur_lang::FnInfo> {
        self.compiled.meta.function(&format!("{}.{name}", self.ty))
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
}

/// Where [`LangHost::of`] finds the host; weak, since the host holds the engine.
struct Registered(Weak<Inner>);

struct Inner {
    engine: Engine,
    root: PathBuf,
    pack: Option<Pack>,
    registry: Rc<RefCell<Registry>>,
    api: RefCell<Option<HostApi>>,
    files: RefCell<BTreeMap<String, Rc<File>>>,
    instances: RefCell<Vec<Rc<Instance>>>,
    held: RefCell<Vec<u64>>,
    holds: Cell<u32>,
    watch: bool,
    stamps: RefCell<BTreeMap<String, Option<u64>>>,
    frames: Cell<u32>,
    tasks: RefCell<Vec<Pending>>,
    /// Files built with probes, on an engine that can hold a call at a line.
    debugging: RefCell<HashSet<String>>,
    /// The lines a debugger asked to stop at, by file, as it asked for them.
    breaks: RefCell<BTreeMap<String, Vec<usize>>>,
    stopped: RefCell<Option<Held>>,
}

/// The `.bl` backend. Cheap to clone; clones share the one host.
#[derive(Clone)]
pub struct LangHost {
    inner: Rc<Inner>,
}

fn key_of(path: &str) -> String {
    path.trim_start_matches("./").replace('\\', "/")
}

impl LangHost {
    pub fn new(engine: Engine, root: &std::path::Path, pack: Option<Pack>, watch: bool) -> Self {
        LangHost {
            inner: Rc::new(Inner {
                engine,
                root: root.to_path_buf(),
                pack,
                registry: Rc::default(),
                api: RefCell::new(None),
                files: RefCell::default(),
                instances: RefCell::default(),
                held: RefCell::default(),
                holds: Cell::new(0),
                watch,
                stamps: RefCell::default(),
                frames: Cell::new(0),
                tasks: RefCell::default(),
                debugging: RefCell::default(),
                breaks: RefCell::default(),
                stopped: RefCell::new(None),
            }),
        }
    }

    /// Whether `path` is a script this backend runs.
    pub fn claims(path: &str) -> bool {
        std::path::Path::new(path)
            .extension()
            .is_some_and(|e| e == EXTENSION)
    }

    /// Make this host the one [`LangHost::of`] finds on `eng`.
    pub fn register(&self, eng: &Engine) {
        eng.insert_resource(Registered(Rc::downgrade(&self.inner)));
    }

    /// The host [`LangHost::register`] put on `eng`, while it lives.
    pub fn of(eng: &Engine) -> Option<LangHost> {
        let registered = eng.try_resource::<Registered>()?;
        let inner = registered.borrow().0.upgrade()?;
        Some(LangHost { inner })
    }

    // A pack holds each script's source: `Compiled` carries a `Meta` with no
    // encoding yet, and compiling at load is a millisecond a thousand lines.
    fn source(&self, key: &str) -> Result<String> {
        let bytes = match &self.inner.pack {
            Some(pack) => pack
                .scripts
                .get(key)
                .cloned()
                .ok_or_else(|| anyhow!("{key} is not in the pack"))?,
            None => balaur_core::files::backend(&self.inner.engine)
                .read(&self.inner.root.join(key))
                .with_context(|| format!("reading {key}"))?,
        };
        String::from_utf8(bytes).with_context(|| format!("{key} is not UTF-8"))
    }

    fn host_fns(&self) -> HostFns {
        let (engine, registry) = (self.inner.engine.clone(), self.inner.registry.clone());
        HostFns::default().engine(move |key, args, ret| {
            crate::engine_call(&engine, &registry.borrow(), key, args, ret)
        })
    }

    fn compile_module(&self, key: &str) -> Result<(Compiled, String)> {
        self.compile_source(key, &self.source(key)?)
    }

    fn api(&self) -> HostApi {
        self.inner
            .api
            .borrow_mut()
            .get_or_insert_with(|| api::build(&self.inner.engine, &self.inner.registry.borrow()))
            .clone()
    }

    /// What is wrong with `source` as the script at `rel`, against the
    /// engine's API: the first error, where the compiler stopped.
    pub fn check(&self, rel: &str, source: &str) -> Option<balaur_lang::Error> {
        let options = Options {
            debug: false,
            host: self.api(),
        };
        balaur_lang::compile(source, &options).err().or_else(|| {
            self.compile_source(&key_of(rel), source)
                .err()
                .map(|e| balaur_lang::Error {
                    pos: balaur_lang::Pos { line: 1, col: 1 },
                    msg: format!("{e:#}"),
                })
        })
    }

    fn compile_source(&self, key: &str, source: &str) -> Result<(Compiled, String)> {
        let compiled = balaur_lang::compile(
            source,
            &Options {
                debug: self.inner.debugging.borrow().contains(key),
                host: self.api(),
            },
        )
        .map_err(|e| anyhow!("{key}:{e}"))?;
        let meta = &compiled.meta;
        let owners: Vec<&str> = meta.fns.iter().filter_map(|f| f.owner.as_deref()).collect();
        let user: Vec<&str> = meta
            .structs
            .iter()
            .map(|(n, _)| n.as_str())
            .filter(|n| !BUILT_IN.contains(n))
            .collect();
        let ty = user
            .iter()
            .find(|n| owners.contains(n))
            .or(user.first())
            .ok_or_else(|| anyhow!("{key}: a script declares a `pub struct`"))?;
        let ty = (*ty).to_string();
        Ok((compiled, ty))
    }

    fn load(&self, key: &str) -> Result<Rc<File>> {
        let (compiled, ty) = self.compile_module(key)?;
        let limits = Limits {
            engine: self.engine_for(key),
            ..Limits::default()
        };
        let mut rt =
            Runtime::new(&compiled, self.host_fns(), limits).map_err(|e| anyhow!("{key}: {e}"))?;
        self.arm(key, &mut rt);
        Ok(Rc::new(File {
            rt: RefCell::new(rt),
            compiled,
            ty,
        }))
    }

    fn file(&self, path: &str) -> Result<Rc<File>> {
        let key = key_of(path);
        if let Some(f) = self.inner.files.borrow().get(&key) {
            return Ok(f.clone());
        }
        let f = self.load(&key)?;
        if self.inner.watch {
            let stamp = self.stamp(&key);
            self.inner.stamps.borrow_mut().insert(key.clone(), stamp);
        }
        self.inner.files.borrow_mut().insert(key, f.clone());
        Ok(f)
    }

    fn stamp(&self, key: &str) -> Option<u64> {
        balaur_core::files::backend(&self.inner.engine)
            .mtime(&self.inner.root.join(key))
            .map(f64::to_bits)
    }

    /// The loaded files whose stamp moved since they were last read.
    fn edited(&self) -> Vec<String> {
        let frame = self.inner.frames.get().wrapping_add(1);
        self.inner.frames.set(frame);
        if !self.inner.watch || !frame.is_multiple_of(POLL_FRAMES) {
            return vec![];
        }
        let keys: Vec<String> = self.inner.stamps.borrow().keys().cloned().collect();
        keys.into_iter()
            .filter(|key| {
                let stamp = self.stamp(key);
                let mut stamps = self.inner.stamps.borrow_mut();
                stamps.insert(key.clone(), stamp) != Some(stamp)
            })
            .collect()
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
        for inst in all {
            // A stop freezes the game, the rest of this pass with it.
            if self.is_held() {
                break;
            }
            let Some(file) = self.inner.files.borrow().get(&inst.path).cloned() else {
                continue;
            };
            if file
                .method(method)
                .is_some_and(|f| !(sync_only && f.is_async))
            {
                let r = self.run(&file, &inst, method, args, None);
                report(r);
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
        let Some(info) = file.method(method).cloned() else {
            return Ok(None);
        };
        let args: Vec<L> = info
            .params
            .iter()
            .zip(args.iter().chain(std::iter::repeat(&Value::Nil)))
            .map(|(t, v)| from_neutral(v, t))
            .collect();
        if info.is_async {
            return self.start(file, inst, method, &args, done);
        }
        if file.compiled.meta.debug {
            return self.run_steppable(file, inst, method, &args);
        }
        let mut rt = file.rt.try_borrow_mut().map_err(|_| {
            anyhow!(
                "[{}] {method}: a script cannot call back into its own file yet",
                inst.path
            )
        })?;
        let mut obj = inst
            .obj
            .borrow_mut()
            .take()
            .ok_or_else(|| anyhow!("[{}] {method}: the instance is busy", inst.path))?;
        let out = rt.call_method(&mut obj, method, &args);
        *inst.obj.borrow_mut() = Some(obj);
        let v = out.map_err(|e| anyhow!("[{}] {method}: {e}", inst.path))?;
        Ok(Some(to_neutral(&v)))
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

// An export compiles every `.bl` as a check and packs the source it checked.
impl ScriptCompiler for LangHost {
    fn extensions(&self) -> &[&str] {
        &[EXTENSION]
    }

    fn compile(&self, rel: &str, source: &str) -> Result<Vec<u8>> {
        self.compile_source(&key_of(rel), source)?;
        Ok(source.as_bytes().to_vec())
    }
}

impl ScriptHost<Engine> for LangHost {
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
            .rt
            .borrow_mut()
            .object(&file.ty, &init_ref)
            .map_err(|e| anyhow!("[{path}] {e}"))?;
        self.detach(node);
        self.inner.instances.borrow_mut().push(Rc::new(Instance {
            node: node.0,
            path: key_of(path),
            obj: RefCell::new(Some(obj)),
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
        if let Some(obj) = inst.obj.borrow_mut().take() {
            file.rt.borrow_mut().drop_object(obj);
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
                .map(|o| file.rt.borrow_mut().fields(o))
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
                    && let Err(e) = file.rt.borrow_mut().set(obj, name, &from_neutral(v, &t))
                {
                    tracing::error!("[{}] load_state: {e}", inst.path);
                }
            }
        }
    }

    fn pump_reloads(&self) {
        for key in self.edited() {
            match self.reload(&key) {
                Ok(()) => tracing::info!("reloaded {key}"),
                Err(e) => tracing::error!("{e:#}"),
            }
        }
    }

    // A new module, with every instance of the file carried across by field name.
    fn reload(&self, key: &str) -> Result<()> {
        let key = key_of(key);
        if self.holds_file(&key) {
            return Err(anyhow!(
                "{key} is stopped at a breakpoint; it reloads once it goes on"
            ));
        }
        let (compiled, ty) = self.compile_module(&key)?;
        self.forget(&key);
        let old = self
            .inner
            .files
            .borrow_mut()
            .remove(&key)
            .ok_or_else(|| anyhow!("{key} is not loaded"))?;
        let old = Rc::try_unwrap(old)
            .map_err(|_| anyhow!("{key} is running; reload it between frames"))?;
        let mut rt = old.rt.into_inner();
        let insts: Vec<Rc<Instance>> = self
            .inner
            .instances
            .borrow()
            .iter()
            .filter(|i| i.path == key)
            .cloned()
            .collect();
        // An instance whose object is out (busy) keeps the one it has.
        let insts: Vec<Rc<Instance>> = insts
            .into_iter()
            .filter(|i| i.obj.borrow().is_some())
            .collect();
        let mut objs: Vec<Object> = insts
            .iter()
            .filter_map(|i| i.obj.borrow_mut().take())
            .collect();
        let mut refs: Vec<&mut Object> = objs.iter_mut().collect();
        rt.set_engine(self.engine_for(&key));
        let swapped = rt.reload(&compiled, self.host_fns(), &mut refs);
        if swapped.is_ok() {
            self.arm(&key, &mut rt);
        }
        for (inst, obj) in insts.iter().zip(objs) {
            *inst.obj.borrow_mut() = Some(obj);
        }
        let (compiled, ty) = if swapped.is_ok() {
            (compiled, ty)
        } else {
            (old.compiled, old.ty)
        };
        self.inner.files.borrow_mut().insert(
            key.clone(),
            Rc::new(File {
                rt: RefCell::new(rt),
                compiled,
                ty,
            }),
        );
        swapped.map(|_| ()).map_err(|e| anyhow!("{key}: {e}"))
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
        file.rt
            .borrow_mut()
            .get(obj.as_ref()?, name)
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

    fn as_any(&self) -> &dyn core::any::Any {
        self
    }
}
