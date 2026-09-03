//! The script operations that touch files and the two text formats.
//!
//! Split out of `engine_api`, which registers them: `fs` reads and writes
//! inside the project, `toml` and `json` convert between text and script
//! values. The conversions are public because plugins speak JSON to servers.

use anyhow::{anyhow, Result};
use balaur_script::Value;

use crate::engine::Engine;
use crate::engine_api::text;

pub(crate) fn resolve(eng: &Engine, path: &str) -> std::path::PathBuf {
    let p = std::path::Path::new(path);
    if p.is_absolute() {
        return p.to_path_buf();
    }
    eng.try_resource::<crate::project::ProjectRoot>()
        .map_or_else(|| p.to_path_buf(), |r| r.borrow().0.join(p))
}

pub(crate) fn fs_read(eng: &Engine, args: &[Value]) -> Result<Value> {
    Ok(std::fs::read_to_string(resolve(eng, text(args, 0)?)).map_or(Value::Nil, Value::Str))
}

/// Write a file, making the directory it goes in.
///
/// Creating the parent is part of writing: a tool that saves an asset into
/// `animations/` should not fail because the project has never had one.
pub(crate) fn fs_write(eng: &Engine, args: &[Value]) -> Result<Value> {
    let path = resolve(eng, text(args, 0)?);
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, text(args, 1)?)?;
    Ok(Value::Nil)
}

pub(crate) fn fs_exists(eng: &Engine, args: &[Value]) -> Result<Value> {
    Ok(Value::Bool(resolve(eng, text(args, 0)?).exists()))
}

/// Delete a file, or a directory and everything under it.
///
/// Answers whether there was anything there, rather than failing: a tool
/// deleting what a previous run already deleted has nothing to report.
pub(crate) fn fs_remove(eng: &Engine, args: &[Value]) -> Result<Value> {
    let path = resolve(eng, text(args, 0)?);
    if !path.exists() {
        return Ok(Value::Bool(false));
    }
    if path.is_dir() {
        std::fs::remove_dir_all(path)?;
    } else {
        std::fs::remove_file(path)?;
    }
    Ok(Value::Bool(true))
}

pub(crate) fn fs_mkdir(eng: &Engine, args: &[Value]) -> Result<Value> {
    std::fs::create_dir_all(resolve(eng, text(args, 0)?))?;
    Ok(Value::Nil)
}

/// Move a file or directory. The destination's parent is made first, for the
/// same reason `fs::write` makes one.
pub(crate) fn fs_rename(eng: &Engine, args: &[Value]) -> Result<Value> {
    let from = resolve(eng, text(args, 0)?);
    let to = resolve(eng, text(args, 1)?);
    if let Some(parent) = to.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::rename(from, to)?;
    Ok(Value::Nil)
}

/// When a file last changed, in seconds since the epoch, or `()` for one that
/// is not there. A tool polling for edits compares this instead of re-reading.
pub(crate) fn fs_mtime(eng: &Engine, args: &[Value]) -> Result<Value> {
    let Ok(meta) = std::fs::metadata(resolve(eng, text(args, 0)?)) else {
        return Ok(Value::Nil);
    };
    let Ok(modified) = meta.modified() else {
        return Ok(Value::Nil);
    };
    Ok(modified
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(Value::Nil, |d| Value::Num(d.as_secs_f64())))
}

pub(crate) fn fs_list(eng: &Engine, args: &[Value]) -> Result<Value> {
    let mut names: Vec<(String, bool)> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(resolve(eng, text(args, 0)?)) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue;
            }
            names.push((name, entry.path().is_dir()));
        }
    }
    // Sorted for stable UI and reproducible tooling runs.
    names.sort();
    Ok(Value::List(
        names
            .into_iter()
            .map(|(name, is_dir)| {
                Value::Map(vec![
                    ("name".into(), Value::Str(name)),
                    ("is_dir".into(), Value::Bool(is_dir)),
                ])
            })
            .collect(),
    ))
}

pub(crate) fn toml_parse(_: &Engine, args: &[Value]) -> Result<Value> {
    let parsed: toml::Value = toml::from_str(text(args, 0)?)?;
    crate::node_api::from_toml(&parsed)
}

pub(crate) fn toml_encode(_: &Engine, args: &[Value]) -> Result<Value> {
    let value = args.first().ok_or_else(|| anyhow!("nothing to encode"))?;
    Ok(Value::Str(toml::to_string(&crate::node_api::to_toml(
        value,
    )?)?))
}

pub(crate) fn json_parse(_: &Engine, args: &[Value]) -> Result<Value> {
    let parsed: serde_json::Value = serde_json::from_str(text(args, 0)?)?;
    from_json(&parsed)
}

pub(crate) fn json_encode(_: &Engine, args: &[Value]) -> Result<Value> {
    let value = args.first().ok_or_else(|| anyhow!("nothing to encode"))?;
    Ok(Value::Str(serde_json::to_string(&to_json(value)?)?))
}

/// Unlike TOML, JSON has null, so nil survives a round trip.
pub fn from_json(v: &serde_json::Value) -> Result<Value> {
    Ok(match v {
        serde_json::Value::Null => Value::Nil,
        serde_json::Value::Bool(b) => Value::Bool(*b),
        serde_json::Value::Number(n) => match n.as_i64() {
            Some(i) => Value::Int(i),
            None => Value::Num(
                n.as_f64()
                    .ok_or_else(|| anyhow!("{n} does not fit a script number"))?,
            ),
        },
        serde_json::Value::String(s) => Value::Str(s.clone()),
        serde_json::Value::Array(items) => {
            Value::List(items.iter().map(from_json).collect::<Result<_>>()?)
        }
        serde_json::Value::Object(map) => Value::Map(
            map.iter()
                .map(|(k, val)| Ok((k.clone(), from_json(val)?)))
                .collect::<Result<_>>()?,
        ),
    })
}

pub fn to_json(v: &Value) -> Result<serde_json::Value> {
    Ok(match v {
        Value::Nil => serde_json::Value::Null,
        Value::Bool(b) => serde_json::Value::Bool(*b),
        Value::Int(i) => serde_json::Value::Number((*i).into()),
        // JSON has no NaN or infinity, so those error instead of encoding.
        Value::Num(n) => serde_json::Number::from_f64(*n)
            .map(serde_json::Value::Number)
            .ok_or_else(|| anyhow!("{n} has no JSON representation"))?,
        Value::Str(s) => serde_json::Value::String(s.clone()),
        Value::Node(_) | Value::Callback(_) => {
            return Err(anyhow!("a node or callback is not JSON data"))
        }
        // JSON has no byte string, and guessing an encoding here would make
        // the round trip lossy in a way the caller never asked for.
        Value::Bytes(_) => return Err(anyhow!("bytes are not JSON data; encode them first")),
        Value::Many(_) => return Err(anyhow!("several values are not one JSON document")),
        Value::Vec2(a) => json_number_list(a)?,
        Value::Vec3(a) => json_number_list(a)?,
        Value::Color(a) => json_number_list(a)?,
        Value::List(items) => {
            serde_json::Value::Array(items.iter().map(to_json).collect::<Result<_>>()?)
        }
        Value::Map(pairs) => serde_json::Value::Object(
            pairs
                .iter()
                .map(|(k, val)| Ok((k.clone(), to_json(val)?)))
                .collect::<Result<_>>()?,
        ),
    })
}

fn json_number_list(a: &[f32]) -> Result<serde_json::Value> {
    Ok(serde_json::Value::Array(
        a.iter()
            .map(|n| {
                serde_json::Number::from_f64(f64::from(*n))
                    .map(serde_json::Value::Number)
                    .ok_or_else(|| anyhow!("{n} has no JSON representation"))
            })
            .collect::<Result<_>>()?,
    ))
}
