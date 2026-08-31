//! The Luau script host: loading, instancing, hot reload, precompiled packs.
//!
//! Scripting model (Godot-like): a `.luau` file returns a class table with
//! optional lifecycle methods. One Lua instance table is created per node the
//! script is attached to; the instance's metatable `__index`es the class.
//!
//! ```luau
//! local Spinner = {}
//! function Spinner:init() self.angle = 0 end
//! function Spinner:update(dt) self.angle += dt end
//! return Spinner
//! ```
//!
//! Hot reload is a core service and needs zero setup from the game: the host
//! watches the project directory, recompiles a changed file, then swaps the
//! contents of the existing class table in place. Instance identity, instance
//! state, and every reference to the class survive; only the code changes.
//! A compile error keeps the previous version running and reports the error.

pub mod det;
pub mod env;
mod node_api;

pub use env::LuaModule;
pub use mlua;
pub use node_api::NodeRef;

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::mpsc::Receiver;

use anyhow::{anyhow, Context, Result};
use hecs::Entity;
use mlua::chunk::ChunkMode;
use mlua::{Function, Lua, Table, Value};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};

use balaur_core::engine::Engine;
use balaur_core::pack::Pack;
use balaur_core::scene::ScriptAttachment;

/// Luau compiler settings shared by dev mode and pack export, so shipped
/// bytecode behaves exactly like what was tested during development.
pub fn compiler() -> mlua::chunk::Compiler {
    mlua::chunk::Compiler::new()
        .set_optimization_level(2)
        .set_debug_level(1)
        // Determinism: keep the replaced math functions out of fastcalls so
        // calls route through the (rebound) global table. See `script::det`.
        .set_disabled_builtins(det::DISABLED_BUILTINS.iter().copied())
}

/// Compiles `.luau` sources for an export pack, with the same settings the
/// dev-mode host uses, so shipped bytecode behaves like what was tested.
pub struct Compiler;

impl balaur_script::ScriptCompiler for Compiler {
    fn extensions(&self) -> &[&str] {
        &["luau"]
    }

    fn compile(&self, rel: &str, source: &str) -> anyhow::Result<Vec<u8>> {
        compiler()
            .compile(source)
            .with_context(|| format!("compiling {rel}"))
    }
}

/// The Luau backend, as an `AppConfig::script_backend` factory.
pub fn factory() -> balaur_core::ScriptHostFactory {
    Box::new(|setup| {
        Ok(Rc::new(ScriptHost::new(
            setup.engine.clone(),
            setup.project_root,
            setup.pack,
            setup.watch,
        )?))
    })
}

#[derive(Clone)]
pub struct ScriptHost {
    lua: Lua,
    engine: Engine,
    state: Rc<RefCell<HostState>>,
}

struct HostState {
    project_root: PathBuf,
    /// Class tables keyed by project-relative path with forward slashes.
    classes: HashMap<String, Table>,
    /// `require` results keyed the same way; tables hot-swap like classes.
    modules: HashMap<String, Value>,
    instances: HashMap<Entity, Table>,
    pack: Option<Pack>,
    _watcher: Option<RecommendedWatcher>,
    events: Option<Receiver<notify::Result<notify::Event>>>,
    /// Last reported error per script, to avoid re-logging the same failure
    /// every frame.
    last_errors: HashMap<String, String>,
    /// Script tasks suspended through `await(token)`, in suspension order —
    /// which is resume order when one wake serves several waiters.
    waiting: Vec<WaitingTask>,
}

/// One suspended script task: a coroutine parked until its token wakes.
struct WaitingTask {
    token: u64,
    /// The node whose script suspended; freeing it cancels the task.
    owner: Entity,
    /// The script key, so reloading the script cancels its tasks rather than
    /// resuming code that no longer exists.
    key: String,
    /// What to blame in an error report when the resumed code fails.
    label: String,
    thread: mlua::Thread,
}

impl ScriptHost {
    /// `pack`: run from precompiled bytecode instead of source files (used by
    /// exported games). `watch`: enable automatic hot reload (dev mode).
    // `engine` is stored in the returned ScriptHost, so it is consumed.
    #[allow(clippy::needless_pass_by_value)]
    pub fn new(
        engine: Engine,
        project_root: &Path,
        pack: Option<Pack>,
        watch: bool,
    ) -> Result<Self> {
        let lua = Lua::new();
        // Canonicalize so watcher events (which report canonical paths, e.g.
        // /private/var on macOS) strip back to project-relative keys.
        let project_root = &project_root
            .canonicalize()
            .unwrap_or_else(|_| project_root.to_path_buf());
        let (watcher, events) = if watch && pack.is_none() {
            let (tx, rx) = std::sync::mpsc::channel();
            let mut watcher = notify::recommended_watcher(move |res| {
                let _ = tx.send(res);
            })?;
            watcher
                .watch(project_root, RecursiveMode::Recursive)
                .with_context(|| format!("watching {}", project_root.display()))?;
            (Some(watcher), Some(rx))
        } else {
            (None, None)
        };
        let host = Self {
            lua,
            engine: engine.clone(),
            state: Rc::new(RefCell::new(HostState {
                project_root: project_root.clone(),
                classes: HashMap::new(),
                modules: HashMap::new(),
                instances: HashMap::new(),
                pack,
                _watcher: watcher,
                events,
                last_errors: HashMap::new(),
                waiting: Vec::new(),
            })),
        };
        env::install_globals(&host.lua, &engine, &host)?;
        det::install(&host.lua, &engine)?;
        Ok(host)
    }

    pub fn lua(&self) -> Lua {
        self.lua.clone()
    }

    pub fn engine(&self) -> Engine {
        self.engine.clone()
    }

    /// Register (or fetch) a global module table for plugin bindings.
    pub fn module(&self, name: &str) -> Result<LuaModule> {
        env::module(&self.lua, &self.engine, name)
    }

    /// A scene document's source by project-relative path, from the pack in
    /// packed runs, from disk otherwise (backs `scene.source`).
    pub fn scene_source(&self, rel: &str) -> Option<String> {
        let state = self.state.borrow();
        match &state.pack {
            Some(pack) => pack.scenes.get(rel).cloned(),
            None => std::fs::read_to_string(state.project_root.join(rel)).ok(),
        }
    }

    fn normalize_key(path: &str) -> String {
        let mut key = path.replace('\\', "/");
        if !std::path::Path::new(&key)
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("luau"))
        {
            key.push_str(".luau");
        }
        key
    }

    /// `require("scripts/foo")`: evaluate a Luau module once and cache its
    /// value. Modules returning a table hot reload in place, like classes.
    pub fn require(&self, path: &str) -> Result<Value> {
        let key = Self::normalize_key(path);
        if let Some(value) = self.state.borrow().modules.get(&key) {
            return Ok(value.clone());
        }
        let value = self.eval_chunk(&key)?;
        self.state.borrow_mut().modules.insert(key, value.clone());
        Ok(value)
    }

    /// Attach a script to a node. Creates the Lua instance and calls `init`.
    pub fn attach(&self, entity: Entity, path: &str) -> Result<()> {
        let key = Self::normalize_key(path);
        let class = self.load_class(&key)?;
        let inst = self.lua.create_table()?;
        let meta = self.lua.create_table()?;
        meta.set("__index", class.clone())?;
        inst.set_metatable(Some(meta))?;
        inst.set(
            "node",
            NodeRef {
                entity,
                engine: self.engine.clone(),
            },
        )?;
        self.state
            .borrow_mut()
            .instances
            .insert(entity, inst.clone());
        self.engine
            .world_mut()
            .insert_one(entity, ScriptAttachment { path: key.clone() })
            .map_err(|_| anyhow!("cannot attach script to a dead node"))?;
        if let Some(init) = class.get::<Option<Function>>("init")? {
            let mut args = mlua::MultiValue::new();
            args.push_back(Value::Table(inst));
            self.run_task(entity, &format!("{key} init"), init, args);
        }
        Ok(())
    }

    /// Remove the instance for a despawned node, calling `on_free` first.
    /// Tasks the node's script left suspended die with it.
    pub fn detach(&self, entity: Entity) {
        let inst = {
            let mut state = self.state.borrow_mut();
            state.waiting.retain(|t| t.owner != entity);
            state.instances.remove(&entity)
        };
        if let Some(inst) = inst {
            if let Ok(Some(on_free)) = inst.get::<Option<Function>>("on_free") {
                if let Err(err) = on_free.call::<()>(inst) {
                    tracing::error!("on_free: {err}");
                }
            }
        }
    }

    /// Call `update(dt)` on every live instance.
    pub fn update(&self, dt: f32) {
        // Collect first so scripts can attach/detach/spawn during their own
        // update without the host state being borrowed.
        let batch: Vec<(Entity, Table)> = self
            .state
            .borrow()
            .instances
            .iter()
            .map(|(e, t)| (*e, t.clone()))
            .collect();
        for (entity, inst) in batch {
            let Ok(Some(update)) = inst.get::<Option<Function>>("update") else {
                continue;
            };
            if let Err(err) = update.call::<()>((inst, dt)) {
                self.report_error(&format!("update({entity:?})"), &err.to_string());
            }
        }
    }

    /// Drain file watcher events and hot reload any changed script. Runs
    /// automatically every frame in dev mode; costs one `try_recv` when idle.
    pub fn pump_reloads(&self) {
        let mut changed: Vec<String> = Vec::new();
        let mut assets: Vec<String> = Vec::new();
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
                        Some("luau") => {
                            if (state.classes.contains_key(&key)
                                || state.modules.contains_key(&key))
                                && !changed.contains(&key)
                            {
                                changed.push(key);
                            }
                        }
                        // Every asset document is TOML, and so is every scene:
                        // `reload` drops only what was actually cached, so a
                        // saved scene costs a lookup and changes nothing.
                        Some("toml") if !assets.contains(&key) => assets.push(key),
                        _ => {}
                    }
                }
            }
        }
        for key in assets {
            if let Err(err) = balaur_core::assets::reload(&self.engine, &key) {
                tracing::warn!("could not reload asset {key}: {err}");
            }
        }
        for key in changed {
            match self.reload(&key) {
                Ok(()) => tracing::info!("hot reloaded {key}"),
                Err(err) => self.report_error(&key, &err.to_string()),
            }
        }
    }

    /// Recompile `key` and swap the new class into the existing class table.
    /// Public so editors and tests can force a reload deterministically.
    pub fn reload(&self, key: &str) -> Result<()> {
        let key = Self::normalize_key(key);
        let maybe_class = {
            let state = self.state.borrow();
            state.classes.get(&key).cloned()
        };
        let Some(old) = maybe_class else {
            return self.reload_module(&key);
        };
        let fresh = self.eval_class(&key)?;
        // In-place swap: every live instance and cross-script reference keeps
        // pointing at `old`, which now holds the new code.
        old.clear()?;
        fresh.for_each(|k: Value, v: Value| old.set(k, v))?;
        old.set_metatable(fresh.metatable())?;
        {
            let mut state = self.state.borrow_mut();
            state.last_errors.remove(&key);
            // A task suspended in the old code must not resume into it.
            state.waiting.retain(|t| t.key != key);
        }
        // Give scripts a chance to migrate state.
        let instances: Vec<(Entity, Table)> = {
            let state = self.state.borrow();
            state
                .instances
                .iter()
                .filter(|(e, _)| self.attachment_path(**e).as_deref() == Some(&key))
                .map(|(e, t)| (*e, t.clone()))
                .collect()
        };
        if let Ok(Some(hook)) = old.get::<Option<Function>>("hot_reload") {
            for (_, inst) in instances {
                if let Err(err) = hook.call::<()>(inst) {
                    tracing::error!("[{key}] hot_reload: {err}");
                }
            }
        }
        Ok(())
    }

    /// Hot reload a `require`d module: re-evaluate and swap table contents
    /// in place so every requirer sees the new code.
    fn reload_module(&self, key: &str) -> Result<()> {
        let old = self
            .state
            .borrow()
            .modules
            .get(key)
            .cloned()
            .ok_or_else(|| anyhow!("script {key} was never loaded"))?;
        let fresh = self.eval_chunk(key)?;
        if let (Value::Table(old_t), Value::Table(new_t)) = (&old, &fresh) {
            old_t.clear()?;
            new_t.for_each(|k: Value, v: Value| old_t.set(k, v))?;
            old_t.set_metatable(new_t.metatable())?;
        } else {
            // Non-table module: requirers keep their old copy until they
            // are themselves reloaded.
            self.state
                .borrow_mut()
                .modules
                .insert(key.to_string(), fresh);
            tracing::warn!("{key}: non-table module; existing references keep the old value");
        }
        self.state.borrow_mut().last_errors.remove(key);
        Ok(())
    }

    /// Call one node's script method — how a signal reaches its handler.
    ///
    /// Missing method is not an error: a widget may name a handler the script
    /// does not implement yet, and that should not stop the frame.
    pub fn call_on(&self, entity: Entity, method: &str, args: &[balaur_script::Value]) {
        let inst = self.state.borrow().instances.get(&entity).cloned();
        let Some(inst) = inst else { return };
        let Ok(Some(func)) = inst.get::<Option<Function>>(method) else {
            return;
        };
        // The instance first, so the method reads as `function C:on_x(a, b)`,
        // the same shape `update(dt)` already has.
        let mut call_args = mlua::MultiValue::new();
        call_args.push_back(mlua::Value::Table(inst));
        for arg in args {
            match env::from_neutral(&self.lua, &self.engine, arg) {
                Ok(value) => call_args.push_back(value),
                Err(err) => {
                    self.report_error(method, &err.to_string());
                    return;
                }
            }
        }
        self.run_task(entity, method, func, call_args);
    }

    /// Call `method` on every live instance that defines it, in entity order
    /// (deterministic; UI code depends on stable call order).
    // `args` is cloned once per instance inside the loop, so it is consumed.
    #[allow(clippy::needless_pass_by_value)]
    pub fn call_all(&self, method: &str, args: impl mlua::IntoLuaMulti + Clone) {
        let mut batch: Vec<(Entity, Table)> = self
            .state
            .borrow()
            .instances
            .iter()
            .map(|(e, t)| (*e, t.clone()))
            .collect();
        batch.sort_by_key(|(e, _)| *e);
        for (entity, inst) in batch {
            let Ok(Some(func)) = inst.get::<Option<Function>>(method) else {
                continue;
            };
            let mut call_args = mlua::MultiValue::new();
            call_args.push_back(Value::Table(inst.clone()));
            if let Ok(extra) = args.clone().into_lua_multi(&self.lua) {
                call_args.extend(extra);
            }
            self.run_task(entity, method, func, call_args);
        }
    }

    /// Run a script function as a task: a coroutine that may suspend through
    /// `await(token)` and resume when [`ScriptHost::wake`] delivers that
    /// token. A function that never awaits runs to completion right here, so
    /// the plain path costs one coroutine and nothing else.
    fn run_task(&self, owner: Entity, label: &str, func: Function, args: mlua::MultiValue) {
        let thread = match self.lua.create_thread(func) {
            Ok(thread) => thread,
            Err(err) => return self.report_error(label, &err.to_string()),
        };
        let outcome = thread.resume::<mlua::MultiValue>(args);
        self.settle_task(owner, label, thread, outcome);
    }

    /// File a task that suspended, finish one that returned, report one that
    /// failed.
    fn settle_task(
        &self,
        owner: Entity,
        label: &str,
        thread: mlua::Thread,
        outcome: mlua::Result<mlua::MultiValue>,
    ) {
        let values = match outcome {
            Ok(values) => values,
            Err(err) => return self.report_error(label, &err.to_string()),
        };
        if thread.status() != mlua::prelude::LuaThreadStatus::Resumable {
            return;
        }
        let Some(token) = wait_token(&values) else {
            return self.report_error(label, "scripts suspend only through await(token)");
        };
        let key = self.attachment_path(owner).unwrap_or_default();
        self.state.borrow_mut().waiting.push(WaitingTask {
            token,
            owner,
            key,
            label: label.to_string(),
            thread,
        });
    }

    /// Resume every task suspended on `token` with `payload`, in suspension
    /// order. No waiter, no effect.
    pub fn wake(&self, token: u64, payload: &balaur_script::Value) {
        // Take the ready tasks out before resuming: resumed code may await
        // again or spawn new tasks, and the list must not be borrowed then.
        let ready: Vec<WaitingTask> = {
            let mut state = self.state.borrow_mut();
            let (ready, kept): (Vec<WaitingTask>, Vec<WaitingTask>) =
                state.waiting.drain(..).partition(|t| t.token == token);
            state.waiting = kept;
            ready
        };
        for task in ready {
            let value = match env::from_neutral(&self.lua, &self.engine, payload) {
                Ok(value) => value,
                Err(err) => {
                    self.report_error(&task.label, &err.to_string());
                    continue;
                }
            };
            let outcome = task.thread.resume::<mlua::MultiValue>(value);
            self.settle_task(task.owner, &task.label, task.thread, outcome);
        }
    }

    fn attachment_path(&self, entity: Entity) -> Option<String> {
        self.engine
            .world()
            .get::<&ScriptAttachment>(entity)
            .ok()
            .map(|a| a.path.clone())
    }

    fn load_class(&self, key: &str) -> Result<Table> {
        if let Some(class) = self.state.borrow().classes.get(key) {
            return Ok(class.clone());
        }
        let class = self.eval_class(key)?;
        self.state
            .borrow_mut()
            .classes
            .insert(key.to_string(), class.clone());
        Ok(class)
    }

    /// Compile (or fetch precompiled) bytecode for `key` and evaluate it into
    /// a fresh class table.
    fn eval_class(&self, key: &str) -> Result<Table> {
        match self.eval_chunk(key)? {
            Value::Table(t) => Ok(t),
            other => Err(anyhow!(
                "script {key} must return a class table, got {}",
                other.type_name()
            )),
        }
    }

    fn eval_chunk(&self, key: &str) -> Result<Value> {
        let bytecode = {
            let state = self.state.borrow();
            if let Some(pack) = &state.pack {
                pack.scripts
                    .get(key)
                    .cloned()
                    .ok_or_else(|| anyhow!("script {key} missing from pack"))?
            } else {
                let path = state.project_root.join(key);
                let source = std::fs::read_to_string(&path)
                    .with_context(|| format!("reading {}", path.display()))?;
                compiler()
                    .compile(&source)
                    .with_context(|| format!("compiling {key}"))?
            }
        };
        self.lua
            .load(bytecode.as_slice())
            .set_name(format!("@{key}"))
            .set_mode(ChunkMode::Binary)
            .eval()
            .map_err(|err| anyhow!("evaluating {key}: {err}"))
    }

    fn report_error(&self, key: &str, err: &str) {
        let mut state = self.state.borrow_mut();
        if state.last_errors.get(key).map(std::string::String::as_str) != Some(err) {
            tracing::error!("[{key}] {err}");
            state.last_errors.insert(key.to_string(), err.to_string());
        }
    }

    /// Number of live script instances (editor/debug info).
    pub fn instance_count(&self) -> usize {
        self.state.borrow().instances.len()
    }
}

/// The Luau host, seen through the neutral seam.
///
/// Implemented on the concrete host rather than replacing it: plugins can move
/// to the trait one at a time while the engine still holds the struct.
/// The Luau interpreter behind an engine's script host.
///
/// For code written against the Luau backend on purpose — a tool wanting the
/// raw interpreter, or a test of this backend. Code that should work with any
/// language goes through `balaur_script::ScriptHost` instead. Panics if the
/// engine is running a different backend.
///
/// # Panics
/// If the engine's script host is not the Luau one.
pub fn lua_of(eng: &Engine) -> Lua {
    eng.script_host()
        .expect("the engine always has a script host")
        .as_any()
        .downcast_ref::<ScriptHost>()
        .expect("the engine is not running the Luau backend")
        .lua()
}

impl balaur_script::ScriptHost<Engine> for ScriptHost {
    fn module(&self, name: &str) -> anyhow::Result<Box<dyn balaur_script::Bindings<Engine>>> {
        Ok(Box::new(ScriptHost::module(self, name)?))
    }

    fn attach(&self, node: balaur_script::NodeId, path: &str) -> anyhow::Result<()> {
        ScriptHost::attach(self, balaur_core::entity_of(node)?, path)
    }

    fn detach(&self, node: balaur_script::NodeId) {
        if let Ok(entity) = balaur_core::entity_of(node) {
            ScriptHost::detach(self, entity);
        }
    }

    fn update(&self, dt: f32) {
        ScriptHost::update(self, dt);
    }

    fn pump_reloads(&self) {
        ScriptHost::pump_reloads(self);
    }

    fn reload(&self, key: &str) -> anyhow::Result<()> {
        ScriptHost::reload(self, key)
    }

    fn call_on(&self, node: balaur_script::NodeId, method: &str, args: &[balaur_script::Value]) {
        if let Ok(entity) = balaur_core::entity_of(node) {
            ScriptHost::call_on(self, entity, method, args);
        }
    }

    fn call_all(&self, method: &str) {
        ScriptHost::call_all(self, method, ());
    }

    fn wake(&self, token: u64, payload: &balaur_script::Value) {
        ScriptHost::wake(self, token, payload);
    }

    fn scene_source(&self, rel: &str) -> Option<String> {
        ScriptHost::scene_source(self, rel)
    }

    fn instance_count(&self) -> usize {
        ScriptHost::instance_count(self)
    }
    fn invoke(
        &self,
        callback: balaur_script::CallbackId,
        args: &[balaur_script::Value],
    ) -> anyhow::Result<balaur_script::Value> {
        let func = env::lookup_callback(callback)
            .ok_or_else(|| anyhow!("callback used after its call returned"))?;
        let args: mlua::Result<Vec<_>> = args
            .iter()
            .map(|a| env::from_neutral(&self.lua, &self.engine, a))
            .collect();
        let ret: mlua::Value = func.call(mlua::MultiValue::from_iter(args?))?;
        Ok(env::to_neutral(&ret)?)
    }

    fn as_any(&self) -> &dyn core::any::Any {
        self
    }
}

/// The token `await` yielded, if these are its sentinel values.
///
/// Any other yield reaching the host is a script driving coroutines by hand
/// on the host's thread, which the caller reports as an error.
fn wait_token(values: &mlua::MultiValue) -> Option<u64> {
    let Some(Value::Table(table)) = values.iter().next() else {
        return None;
    };
    let token: i64 = table.get("__balaur_wait").ok()?;
    u64::try_from(token).ok()
}

/// The script API as JSON: every module table in the globals, the functions it
/// holds, and the constants with their values.
///
/// Read from a live interpreter so derived constants are included and the
/// answer cannot drift from what scripts actually see.
///
/// # Errors
/// If the Lua state cannot be walked.
pub fn api_json(lua: &Lua) -> Result<String> {
    /// One module: its name, its function names, and its constants as
    /// name/value pairs.
    type Module = (String, Vec<String>, Vec<(String, String)>);

    // Anything a bare interpreter already has is Luau's standard library, not
    // the engine's API. Diffing against one beats a hand-kept denylist, which
    // would silently rot as Luau grows.
    let baseline = Lua::new();
    let mut stdlib: Vec<String> = Vec::new();
    for pair in baseline.globals().pairs::<String, Value>() {
        stdlib.push(pair?.0);
    }

    let mut modules: Vec<Module> = Vec::new();
    for pair in lua.globals().pairs::<String, Value>() {
        let (name, value) = pair?;
        let Value::Table(table) = value else { continue };
        if stdlib.contains(&name) {
            continue;
        }
        let mut functions = Vec::new();
        let mut constants = Vec::new();
        for entry in table.pairs::<String, Value>() {
            let (key, v) = entry?;
            match v {
                Value::Function(_) => functions.push(key),
                Value::String(s) => constants.push((key, s.to_string_lossy())),
                Value::Integer(i) => constants.push((key, i.to_string())),
                Value::Number(n) => constants.push((key, n.to_string())),
                Value::Boolean(b) => constants.push((key, b.to_string())),
                _ => {}
            }
        }
        if functions.is_empty() && constants.is_empty() {
            continue;
        }
        functions.sort();
        constants.sort();
        modules.push((name, functions, constants));
    }
    modules.sort_by(|a, b| a.0.cmp(&b.0));

    let mut out = String::from("{\n  \"modules\": [\n");
    for (i, (name, functions, constants)) in modules.iter().enumerate() {
        use std::fmt::Write as _;
        let _ = write!(out, "    {{\n      \"name\": {},\n", quote(name));
        out.push_str("      \"functions\": [");
        out.push_str(
            &functions
                .iter()
                .map(|f| quote(f))
                .collect::<Vec<_>>()
                .join(", "),
        );
        out.push_str("],\n      \"constants\": [");
        out.push_str(
            &constants
                .iter()
                .map(|(k, v)| format!("{{\"name\": {}, \"value\": {}}}", quote(k), quote(v)))
                .collect::<Vec<_>>()
                .join(", "),
        );
        out.push_str("]\n    }");
        if i + 1 < modules.len() {
            out.push(',');
        }
        out.push('\n');
    }
    out.push_str("  ]\n}");
    Ok(out)
}

fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c if (c as u32) < 0x20 => {
                use std::fmt::Write as _;
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}
