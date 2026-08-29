//! Global Lua environment and the plugin binding builder.

use mlua::{FromLuaMulti, IntoLuaMulti, Lua, Table, UserDataRef};

use crate::engine::Engine;
use crate::scene;
use crate::script::{NodeRef, ScriptArgs};

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
pub(super) fn install_globals(lua: &Lua, engine: &Engine) -> anyhow::Result<()> {
    install_engine_module(lua, engine)?;
    install_scene_module(lua, engine)?;
    install_scene_assets(lua, engine)?;
    install_log_module(lua, engine)?;
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
            crate::project::instantiate_scene(eng, &source, base, attach)
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
    m.function("components", |eng, ()| Ok(crate::components::names(eng)))?;
    m.function("component_schema", |eng, name: String| {
        let registry = eng.resource::<crate::components::ComponentRegistry>();
        let registry = registry.borrow();
        Ok(registry
            .def(&name)
            .map(|def| crate::script::tooling::TomlToLua(def.schema.clone())))
    })?;
    Ok(())
}

/// `log`: the capture buffer and the level helpers.
fn install_log_module(lua: &Lua, engine: &Engine) -> anyhow::Result<()> {
    let m = module(lua, engine, "log")?;
    m.table().set(
        "recent",
        lua.create_function(|lua, n: Option<usize>| {
            let out = lua.create_table()?;
            for (i, e) in crate::logbuf::recent(n.unwrap_or(100))
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
        crate::logbuf::clear();
        Ok(())
    })?;
    m.function("info", |_, msg: String| {
        log::info!("[script] {msg}");
        Ok(())
    })?;
    m.function("warn", |_, msg: String| {
        log::warn!("[script] {msg}");
        Ok(())
    })?;
    m.function("error", |_, msg: String| {
        log::error!("[script] {msg}");
        Ok(())
    })?;

    // Shared Luau modules: `require("scripts/util")` evaluates once and
    // caches; module tables hot reload in place like classes.
    {
        let eng = engine.clone();
        lua.globals().set(
            "require",
            lua.create_function(move |_, path: String| {
                let host = eng
                    .scripts()
                    .ok_or_else(|| mlua::Error::runtime("script host not running"))?;
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
            log::info!("[script] {}", parts.join("\t"));
            Ok(())
        })?,
    )?;
    Ok(())
}
