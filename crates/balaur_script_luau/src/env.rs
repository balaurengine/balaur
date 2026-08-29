//! Global Lua environment and the plugin binding builder.

use std::cell::{Cell, RefCell};

use mlua::{FromLuaMulti, IntoLuaMulti, Lua, Table, UserDataRef};

use crate::NodeRef;
use balaur_core::app::ScriptArgs;
use balaur_core::engine::Engine;
use balaur_core::scene;

/// Builder for a named global module exposed to scripts. This is the whole
/// plugin binding API: wrapping a Rust crate for scripting is one
/// `module.function(...)` call per entry point.
pub struct LuaModule {
    lua: Lua,
    engine: Engine,
    table: Table,
}

impl LuaModule {
    /// Register `name` as a function taking the engine plus Lua arguments.
    /// Argument and return conversions are inferred from the closure type.
    pub fn function<A, R, F>(&self, name: &str, f: F) -> anyhow::Result<()>
    where
        A: FromLuaMulti + 'static,
        R: IntoLuaMulti + 'static,
        F: Fn(&Engine, A) -> mlua::Result<R> + 'static,
    {
        let engine = self.engine.clone();
        let func = self
            .lua
            .create_function(move |_, args: A| f(&engine, args))?;
        self.table.set(name, func)?;
        Ok(())
    }

    pub const fn table(&self) -> &Table {
        &self.table
    }
}

/// Bridge: the neutral seam, implemented over Lua.
///
/// Subsystems register through `&mut dyn Bindings<Engine>` and never name a
/// language; this converts at the boundary.
impl balaur_script::Bindings<Engine> for LuaModule {
    fn function_raw(&mut self, name: &str, f: balaur_script::BoundFn<Engine>) {
        let engine = self.engine.clone();
        let Ok(func) = self
            .lua
            .create_function(move |lua, args: mlua::MultiValue| {
                // Callbacks live only for this call; the guard drops them after.
                let _scope = CallbackScope::open();
                let neutral: Vec<balaur_script::Value> = args
                    .into_iter()
                    .map(|v| to_neutral(&v))
                    .collect::<mlua::Result<_>>()?;
                let out = f(&engine, &neutral).map_err(mlua::Error::external)?;
                from_neutral(lua, &engine, &out)
            })
        else {
            tracing::error!(binding = name, "could not create the Lua function");
            return;
        };
        if self.table.set(name, func).is_err() {
            tracing::error!(binding = name, "could not register the binding");
        }
    }

    fn constant(&mut self, name: &str, value: balaur_script::Value) {
        let Ok(v) = from_neutral(&self.lua, &self.engine, &value) else {
            tracing::error!(constant = name, "could not convert the constant");
            return;
        };
        if self.table.set(name, v).is_err() {
            tracing::error!(constant = name, "could not register the constant");
        }
    }
}

pub(crate) fn to_neutral(v: &mlua::Value) -> mlua::Result<balaur_script::Value> {
    use balaur_script::Value as N;
    Ok(match v {
        mlua::Value::Nil => N::Nil,
        mlua::Value::Boolean(b) => N::Bool(*b),
        mlua::Value::Integer(i) => N::Int(*i),
        mlua::Value::Number(n) => N::Num(*n),
        mlua::Value::String(s) => N::Str(s.to_str()?.to_string()),
        mlua::Value::UserData(ud) => {
            let node = ud.borrow::<NodeRef>()?;
            N::Node(node.entity.to_bits().get())
        }
        mlua::Value::Function(f) => N::Callback(CallbackScope::register(f.clone())),
        mlua::Value::Table(t) => {
            let mut map = Vec::new();
            t.for_each(|k: mlua::Value, v: mlua::Value| {
                if let mlua::Value::String(k) = k {
                    map.push((k.to_str()?.to_string(), to_neutral(&v)?));
                }
                Ok(())
            })?;
            N::Map(map)
        }
        other => {
            return Err(mlua::Error::runtime(format!(
                "cannot pass a {} to a binding",
                other.type_name()
            )))
        }
    })
}

pub(crate) fn from_neutral(
    lua: &Lua,
    engine: &Engine,
    v: &balaur_script::Value,
) -> mlua::Result<mlua::Value> {
    use balaur_script::Value as N;
    Ok(match v {
        // A callback never travels back out to script; it is call-scoped.
        N::Nil | N::Callback(_) => mlua::Value::Nil,
        N::Bool(b) => mlua::Value::Boolean(*b),
        N::Int(i) => mlua::Value::Integer(*i),
        N::Num(n) => mlua::Value::Number(*n),
        N::Str(s) => mlua::Value::String(lua.create_string(s)?),
        N::Vec2(a) => list(lua, a)?,
        N::Vec3(a) => list(lua, a)?,
        N::Color(a) => list(lua, a)?,
        N::Node(bits) => {
            let entity = hecs::Entity::from_bits(*bits)
                .ok_or_else(|| mlua::Error::runtime("stale node handle"))?;
            mlua::Value::UserData(lua.create_userdata(NodeRef {
                entity,
                engine: engine.clone(),
            })?)
        }
        N::List(items) => {
            let t = lua.create_table()?;
            for (i, item) in items.iter().enumerate() {
                t.set(i + 1, from_neutral(lua, engine, item)?)?;
            }
            mlua::Value::Table(t)
        }
        N::Map(entries) => {
            let t = lua.create_table()?;
            for (k, item) in entries {
                t.set(k.as_str(), from_neutral(lua, engine, item)?)?;
            }
            mlua::Value::Table(t)
        }
    })
}

fn list(lua: &Lua, xs: &[f32]) -> mlua::Result<mlua::Value> {
    let t = lua.create_table()?;
    for (i, x) in xs.iter().enumerate() {
        t.set(i + 1, f64::from(*x))?;
    }
    Ok(mlua::Value::Table(t))
}

/// Fetch or create the global module table `name`.
pub(super) fn module(lua: &Lua, engine: &Engine, name: &str) -> anyhow::Result<LuaModule> {
    let globals = lua.globals();
    let table: Table = if let Some(t) = globals.get::<Option<Table>>(name)? {
        t
    } else {
        let t = lua.create_table()?;
        globals.set(name, t.clone())?;
        t
    };
    Ok(LuaModule {
        lua: lua.clone(),
        engine: engine.clone(),
        table,
    })
}

/// Install the built-in `engine`, `scene`, and `log` modules.
pub(super) fn install_globals(
    lua: &Lua,
    engine: &Engine,
    host: &super::ScriptHost,
) -> anyhow::Result<()> {
    install_engine_module(lua, engine)?;
    install_scene_module(lua, engine)?;
    install_scene_assets(lua, engine)?;
    install_log_module(lua, engine, host)?;
    install_prelude(lua, engine)?;

    Ok(())
}

/// `engine`: clock, argv, quit, and script reload.
fn install_engine_module(lua: &Lua, engine: &Engine) -> anyhow::Result<()> {
    let m = module(lua, engine, "engine")?;
    m.function("time", |eng, ()| Ok(eng.time()))?;
    m.function("delta", |eng, ()| Ok(eng.delta()))?;
    m.function("quit", |eng, ()| {
        eng.request_quit();
        Ok(())
    })?;
    m.function("args", |eng, ()| {
        Ok(eng
            .try_resource::<ScriptArgs>()
            .map(|args| args.borrow().0.clone())
            .unwrap_or_default())
    })?;
    // Force a hot reload of a loaded script/module (tools use this for files
    // outside the watched project root, e.g. the editor saving game code).
    m.function("reload_script", |eng, path: String| {
        let host = eng
            .scripts()
            .ok_or_else(|| mlua::Error::runtime("script host not running"))?;
        host.reload(&path).map_err(mlua::Error::external)
    })?;
    Ok(())
}

/// `scene`: the node tree, spawning and instancing.
fn install_scene_module(lua: &Lua, engine: &Engine) -> anyhow::Result<()> {
    let m = module(lua, engine, "scene")?;
    m.function("root", |eng, ()| {
        Ok(NodeRef {
            entity: eng.root(),
            engine: eng.clone(),
        })
    })?;
    m.function("get_node", |eng, path: String| {
        let world = eng.world();
        Ok(
            scene::find_node(&world, eng.root(), &path).map(|entity| NodeRef {
                entity,
                engine: eng.clone(),
            }),
        )
    })?;
    m.function(
        "spawn",
        |eng, (name, parent): (String, Option<UserDataRef<NodeRef>>)| {
            let parent = parent.map_or_else(|| eng.root(), |p| p.entity);
            let mut world = eng.world_mut();
            let entity = scene::spawn_node(&mut world, &name, parent);
            Ok(NodeRef {
                entity,
                engine: eng.clone(),
            })
        },
    )?;
    // Instantiate a scene document (TOML source) at runtime, under `parent`.
    // opts: { scripts = false } skips script attachment (editor mirroring).
    m.function(
        "instantiate",
        |eng, (source, parent, opts): (String, Option<UserDataRef<NodeRef>>, Option<Table>)| {
            let base = parent.map_or_else(|| eng.root(), |p| p.entity);
            let attach = opts
                .and_then(|o| o.get::<Option<bool>>("scripts").ok().flatten())
                .unwrap_or(true);
            balaur_core::project::instantiate_scene(eng, &source, base, attach)
                .map_err(mlua::Error::external)
        },
    )?;
    Ok(())
}

/// `scene` continued: project loading and component schemas.
fn install_scene_assets(lua: &Lua, engine: &Engine) -> anyhow::Result<()> {
    let m = module(lua, engine, "scene")?;
    // A scene document's source by project-relative path (works in packed
    // runs too, unlike `fs.read`). Returns nil when the scene is unknown.
    m.function("load", |eng, rel: String| {
        Ok(eng.scripts().and_then(|host| host.scene_source(&rel)))
    })?;
    m.function("components", |eng, ()| {
        Ok(balaur_core::components::names(eng))
    })?;
    m.function("component_schema", |eng, name: String| {
        let registry = eng.resource::<balaur_core::components::ComponentRegistry>();
        let registry = registry.borrow();
        Ok(registry
            .def(&name)
            .map(|def| crate::tooling::TomlToLua(def.schema.clone())))
    })?;
    Ok(())
}

/// `log`: the capture buffer and the level helpers.
fn install_log_module(lua: &Lua, engine: &Engine, host: &super::ScriptHost) -> anyhow::Result<()> {
    let m = module(lua, engine, "log")?;
    m.table().set(
        "recent",
        lua.create_function(|lua, n: Option<usize>| {
            let out = lua.create_table()?;
            for (i, e) in balaur_core::logbuf::recent(n.unwrap_or(100))
                .into_iter()
                .enumerate()
            {
                let t = lua.create_table()?;
                t.set("time", e.time)?;
                t.set("level", e.level)?;
                t.set("tag", e.tag)?;
                t.set("message", e.message)?;
                out.set(i + 1, t)?;
            }
            Ok(out)
        })?,
    )?;
    m.function("clear", |_, ()| {
        balaur_core::logbuf::clear();
        Ok(())
    })?;
    m.function("info", |_, msg: String| {
        tracing::info!("[script] {msg}");
        Ok(())
    })?;
    m.function("warn", |_, msg: String| {
        tracing::warn!("[script] {msg}");
        Ok(())
    })?;
    m.function("error", |_, msg: String| {
        tracing::error!("[script] {msg}");
        Ok(())
    })?;

    // Shared Luau modules: `require("scripts/util")` evaluates once and
    // caches; module tables hot reload in place like classes.
    {
        // `require` is Lua's own module system, so it uses the backend
        // directly rather than the neutral seam: it returns a module table,
        // which no neutral value can carry.
        let host = host.clone();
        lua.globals().set(
            "require",
            lua.create_function(move |_, path: String| {
                host.require(&path).map_err(mlua::Error::external)
            })?,
        )?;
    }

    // `print` goes through the logger too, so it shows up in editor consoles.
    Ok(())
}

/// Globals every script gets: `require` and `print`.
fn install_prelude(lua: &Lua, _engine: &Engine) -> anyhow::Result<()> {
    let globals = lua.globals();
    globals.set(
        "print",
        lua.create_function(|_, args: mlua::Variadic<mlua::Value>| {
            let parts: Vec<String> = args
                .iter()
                .map(|v| v.to_string().unwrap_or_else(|_| "?".into()))
                .collect();
            tracing::info!("[script] {}", parts.join("\t"));
            Ok(())
        })?,
    )?;
    Ok(())
}

thread_local! {
    /// Callbacks registered for the binding call currently on the stack.
    ///
    /// Thread-local because the VM is single-threaded by design, and a plain
    /// counter because ids only need to be unique while the call is running.
    static CALLBACKS: RefCell<Vec<(u64, mlua::Function)>> = const { RefCell::new(Vec::new()) };
    static NEXT_CALLBACK: Cell<u64> = const { Cell::new(1) };
}

/// Registers callbacks for one binding call and drops them on the way out.
struct CallbackScope {
    base: usize,
}

impl CallbackScope {
    /// Open a scope for one binding call. Callbacks registered while it is
    /// alive are dropped when it ends.
    fn open() -> Self {
        Self {
            base: CALLBACKS.with(|c| c.borrow().len()),
        }
    }

    fn register(f: mlua::Function) -> balaur_script::CallbackId {
        let id = NEXT_CALLBACK.with(|n| {
            let id = n.get();
            n.set(id + 1);
            id
        });
        CALLBACKS.with(|c| c.borrow_mut().push((id, f)));
        balaur_script::CallbackId(id)
    }
}

impl Drop for CallbackScope {
    fn drop(&mut self) {
        CALLBACKS.with(|c| c.borrow_mut().truncate(self.base));
    }
}

/// Look up a live callback. `None` once its scope has ended, which is what a
/// binding that stashed one would hit -- deliberately, since call-scoped means
/// call-scoped.
pub(crate) fn lookup_callback(id: balaur_script::CallbackId) -> Option<mlua::Function> {
    CALLBACKS.with(|c| {
        c.borrow()
            .iter()
            .find(|(cid, _)| *cid == id.0)
            .map(|(_, f)| f.clone())
    })
}
