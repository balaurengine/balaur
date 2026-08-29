//! Tooling-facing Lua modules: `fs` and `toml`.
//!
//! These exist for editors and build tools written in Luau (the balaur
//! editor is one). They are not simulation APIs: file contents and
//! directory listings are host state, so gameplay code must not let them
//! influence a deterministic simulation.

use std::path::{Path, PathBuf};

use mlua::{Lua, Value};

use crate::engine::Engine;
use crate::project::ProjectRoot;

fn resolve(eng: &Engine, path: &str) -> PathBuf {
    let path = Path::new(path);
    if path.is_absolute() {
        return path.to_path_buf();
    }
    match eng.try_resource::<ProjectRoot>() {
        Some(root) => root.borrow().0.join(path),
        None => path.to_path_buf(),
    }
}

pub(crate) fn install(lua: &Lua, engine: &Engine) -> anyhow::Result<()> {
    let m = super::env::module(lua, engine, "fs")?;
    m.function("read", |eng, path: String| {
        Ok(std::fs::read_to_string(resolve(eng, &path)).ok())
    })?;
    m.function("write", |eng, (path, contents): (String, String)| {
        std::fs::write(resolve(eng, &path), contents).map_err(mlua::Error::external)
    })?;
    m.function("exists", |eng, path: String| {
        Ok(resolve(eng, &path).exists())
    })?;
    // Sorted for stable UI and reproducible tooling runs.
    {
        let eng = engine.clone();
        m.table().set(
            "list",
            lua.create_function(move |lua, path: String| {
                let mut names: Vec<(String, bool)> = Vec::new();
                if let Ok(entries) = std::fs::read_dir(resolve(&eng, &path)) {
                    for entry in entries.flatten() {
                        let name = entry.file_name().to_string_lossy().into_owned();
                        if name.starts_with('.') {
                            continue;
                        }
                        names.push((name, entry.path().is_dir()));
                    }
                }
                names.sort();
                let out = lua.create_table()?;
                for (i, (name, is_dir)) in names.into_iter().enumerate() {
                    let t = lua.create_table()?;
                    t.set("name", name)?;
                    t.set("is_dir", is_dir)?;
                    out.set(i + 1, t)?;
                }
                Ok(out)
            })?,
        )?;
    }

    let m = super::env::module(lua, engine, "toml")?;
    m.function("parse", |_, source: String| {
        let value: toml::Value = toml::from_str(&source).map_err(mlua::Error::external)?;
        Ok(TomlToLua(value))
    })?;
    m.function("encode", |_, value: Value| {
        let toml_value = lua_to_toml(&value)?;
        toml::to_string(&toml_value).map_err(mlua::Error::external)
    })?;
    Ok(())
}

/// Newtype so `toml::Value → Lua` conversion can go through `IntoLua`.
pub(crate) struct TomlToLua(pub(crate) toml::Value);

impl mlua::IntoLua for TomlToLua {
    fn into_lua(self, lua: &Lua) -> mlua::Result<Value> {
        Ok(match self.0 {
            toml::Value::String(s) => Value::String(lua.create_string(&s)?),
            toml::Value::Integer(i) => Value::Integer(i),
            toml::Value::Float(f) => Value::Number(f),
            toml::Value::Boolean(b) => Value::Boolean(b),
            toml::Value::Datetime(d) => Value::String(lua.create_string(d.to_string())?),
            toml::Value::Array(items) => {
                let t = lua.create_table()?;
                for (i, item) in items.into_iter().enumerate() {
                    t.set(i + 1, Self(item))?;
                }
                Value::Table(t)
            }
            toml::Value::Table(map) => {
                let t = lua.create_table()?;
                for (k, v) in map {
                    t.set(k, Self(v))?;
                }
                Value::Table(t)
            }
        })
    }
}

pub(crate) fn lua_to_toml(value: &Value) -> mlua::Result<toml::Value> {
    Ok(match value {
        Value::Boolean(b) => toml::Value::Boolean(*b),
        Value::Integer(i) => toml::Value::Integer(*i),
        Value::Number(n) => {
            // Encode whole floats as integers so scene files stay tidy.
            if n.fract() == 0.0 && n.abs() < i64::MAX as f64 {
                toml::Value::Integer(*n as i64)
            } else {
                toml::Value::Float(*n)
            }
        }
        Value::String(s) => toml::Value::String(s.to_str()?.to_string()),
        Value::Table(t) => {
            // A table with only consecutive integer keys is an array.
            let len = t.raw_len();
            let mut is_array = len > 0;
            if len == 0 {
                // Distinguish empty array from empty table: treat as table.
                is_array = false;
            }
            if is_array {
                let mut items = Vec::with_capacity(len);
                for i in 1..=len {
                    items.push(lua_to_toml(&t.raw_get::<Value>(i)?)?);
                }
                toml::Value::Array(items)
            } else {
                let mut map = toml::map::Map::new();
                let mut pairs: Vec<(String, Value)> = Vec::new();
                t.for_each(|k: Value, v: Value| {
                    if let Value::String(key) = k {
                        pairs.push((key.to_str()?.to_string(), v));
                    }
                    Ok(())
                })?;
                pairs.sort_by(|a, b| a.0.cmp(&b.0));
                for (k, v) in pairs {
                    map.insert(k, lua_to_toml(&v)?);
                }
                toml::Value::Table(map)
            }
        }
        Value::Nil => toml::Value::Boolean(false),
        other => {
            return Err(mlua::Error::runtime(format!(
                "cannot encode {} to TOML",
                other.type_name()
            )))
        }
    })
}
