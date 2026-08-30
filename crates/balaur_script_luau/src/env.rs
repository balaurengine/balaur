//! Global Lua environment and the plugin binding builder.

use std::cell::{Cell, RefCell};

use mlua::{FromLuaMulti, IntoLuaMulti, Lua, Table};

use crate::NodeRef;
use balaur_core::engine::Engine;

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
                let _scope = CallbackScope::open();
                let neutral: Vec<balaur_script::Value> = args
                    .into_iter()
                    .map(|v| to_neutral(&v))
                    .collect::<mlua::Result<_>>()?;
                let out = f(&engine, &neutral).map_err(mlua::Error::external)?;
                if let balaur_script::Value::Many(items) = &out {
                    let mut multi = mlua::MultiValue::new();
                    for item in items {
                        multi.push_back(from_neutral(lua, &engine, item)?);
                    }
                    return Ok(multi);
                }
                Ok(mlua::MultiValue::from_vec(vec![from_neutral(
                    lua, &engine, &out,
                )?]))
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
            // A contiguous 1..n is a list, anything else a map.
            let len = t.raw_len();
            let mut sequence = len > 0;
            let mut entries: Vec<(mlua::Value, balaur_script::Value)> = Vec::with_capacity(len);
            t.for_each(|k: mlua::Value, v: mlua::Value| {
                if sequence
                    && !matches!(&k, mlua::Value::Integer(i) if *i >= 1 && (*i as usize) <= len)
                {
                    sequence = false;
                }
                entries.push((k, to_neutral(&v)?));
                Ok(())
            })?;
            if sequence && entries.len() == len {
                let mut items: Vec<Option<balaur_script::Value>> = (0..len).map(|_| None).collect();
                for (k, v) in entries {
                    if let mlua::Value::Integer(i) = k {
                        items[(i - 1) as usize] = Some(v);
                    }
                }
                N::List(items.into_iter().flatten().collect())
            } else {
                let mut map = Vec::with_capacity(entries.len());
                for (k, v) in entries {
                    let key = match &k {
                        mlua::Value::String(k) => k.to_str()?.to_string(),
                        mlua::Value::Integer(i) => i.to_string(),
                        mlua::Value::Number(n) => n.to_string(),
                        _ => continue,
                    };
                    map.push((key, v));
                }
                N::Map(map)
            }
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
    eng: &Engine,
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
                engine: eng.clone(),
            })?)
        }
        // Splatted at the call boundary; nested, the best that can be done is
        // a table, which is what a list already is.
        N::Many(items) | N::List(items) => {
            let t = lua.create_table()?;
            for (i, item) in items.iter().enumerate() {
                t.set(i + 1, from_neutral(lua, eng, item)?)?;
            }
            mlua::Value::Table(t)
        }
        N::Map(entries) => {
            let t = lua.create_table()?;
            for (k, item) in entries {
                t.set(k.as_str(), from_neutral(lua, eng, item)?)?;
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
pub(super) fn module(lua: &Lua, eng: &Engine, name: &str) -> anyhow::Result<LuaModule> {
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
        engine: eng.clone(),
        table,
    })
}

/// Declare the built-ins that only the Lua state can carry.
///
/// `engine`, `scene`, `log` and `node` are declared through the neutral seam
/// by `balaur_core::engine_api`; what is left here is the prelude.
///
/// Takes `&Lua` rather than a `Bindings` because the functions below are
/// globals rather than members of a module, and `require` returns a module
/// table, which no neutral value can carry.
pub(super) fn install_globals(
    lua: &Lua,
    eng: &Engine,
    host: &super::ScriptHost,
) -> anyhow::Result<()> {
    install_prelude(lua, eng, host)?;

    Ok(())
}

/// Globals every script gets: `require` and `print`.
///
/// Takes `&Lua` rather than a `Bindings` because both are globals rather than
/// members of a module, and `require` is Lua's own module system: it returns a
/// module table, which no neutral value can carry.
fn install_prelude(lua: &Lua, _eng: &Engine, host: &super::ScriptHost) -> anyhow::Result<()> {
    // Shared Luau modules: `require("scripts/util")` evaluates once and
    // caches; module tables hot reload in place like classes.
    {
        let host = host.clone();
        lua.globals().set(
            "require",
            lua.create_function(move |_, path: String| {
                host.require(&path).map_err(mlua::Error::external)
            })?,
        )?;
    }

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
