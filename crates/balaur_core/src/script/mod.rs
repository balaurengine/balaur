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
mod env;
mod node_api;
pub(crate) mod tooling;

pub use env::LuaModule;
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

use crate::engine::Engine;
use crate::pack::Pack;
use crate::scene::ScriptAttachment;

/// CLI arguments exposed to scripts through `engine.args()`.
pub struct ScriptArgs(pub Vec<String>);

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
            })),
        };
        env::install_globals(&host.lua, &engine)?;
        det::install(&host.lua, &engine)?;
        tooling::install(&host.lua, &engine)?;
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
    /// packed runs, from disk otherwise (backs `scene.load`).
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
            if let Err(err) = init.call::<()>(inst) {
                tracing::error!("[{key}] init: {err}");
            }
        }
        Ok(())
    }

    /// Remove the instance for a despawned node, calling `on_free` first.
    pub fn detach(&self, entity: Entity) {
        let inst = self.state.borrow_mut().instances.remove(&entity);
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
        {
            let state = self.state.borrow();
            let Some(events) = &state.events else { return };
            while let Ok(event) = events.try_recv() {
                let Ok(event) = event else { continue };
                for path in event.paths {
                    if path.extension().and_then(|e| e.to_str()) != Some("luau") {
                        continue;
                    }
                    let Ok(rel) = path.strip_prefix(&state.project_root) else {
                        continue;
                    };
                    let key = rel.to_string_lossy().replace('\\', "/");
                    if (state.classes.contains_key(&key) || state.modules.contains_key(&key))
                        && !changed.contains(&key)
                    {
                        changed.push(key);
                    }
                }
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
        self.state.borrow_mut().last_errors.remove(&key);
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
        for (_, inst) in batch {
            let Ok(Some(func)) = inst.get::<Option<Function>>(method) else {
                continue;
            };
            let mut call_args = mlua::MultiValue::new();
            call_args.push_back(Value::Table(inst.clone()));
            if let Ok(extra) = args.clone().into_lua_multi(&self.lua) {
                call_args.extend(extra);
            }
            if let Err(err) = func.call::<()>(call_args) {
                self.report_error(method, &err.to_string());
            }
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
