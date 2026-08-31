//! The engine-level script modules, declared once for every language.
//!
//! `engine` is the clock, argv and quit; `scene` is the tree, spawning and
//! instancing. Same shape as `node_api`: a list of function pointers a backend
//! registers, so a new language inherits them.

// Every declaration shares one signature so they can sit in a table of
// function pointers; several of them have nothing to fail at.
#![allow(clippy::unnecessary_wraps)]

use anyhow::{anyhow, Result};
use balaur_script::{Bindings as _, Value};

use crate::engine::Engine;
use crate::rng::Pcg32;
use crate::scene;

/// One engine operation, tagged with the module it belongs to.
pub struct EngineOp {
    pub module: &'static str,
    pub name: &'static str,
    pub call: fn(&Engine, &[Value]) -> Result<Value>,
}

/// Everything the engine itself exposes to scripts.
pub const ENGINE_OPS: &[EngineOp] = &[
    EngineOp {
        module: "engine",
        name: "time",
        call: time,
    },
    EngineOp {
        module: "engine",
        name: "delta",
        call: delta,
    },
    EngineOp {
        module: "engine",
        name: "quit",
        call: quit,
    },
    EngineOp {
        module: "engine",
        name: "args",
        call: args,
    },
    EngineOp {
        module: "engine",
        name: "reload_script",
        call: reload_script,
    },
    EngineOp {
        module: "scene",
        name: "root",
        call: root,
    },
    EngineOp {
        module: "scene",
        name: "get_node",
        call: get_node,
    },
    EngineOp {
        module: "scene",
        name: "spawn",
        call: spawn,
    },
    EngineOp {
        module: "scene",
        name: "instantiate",
        call: instantiate,
    },
    EngineOp {
        module: "scene",
        name: "source",
        call: source,
    },
    EngineOp {
        module: "scene",
        name: "component_types",
        call: component_types,
    },
    EngineOp {
        module: "scene",
        name: "component_schema",
        call: component_schema,
    },
    EngineOp {
        module: "assets",
        name: "load",
        call: assets_load,
    },
    EngineOp {
        module: "assets",
        name: "duplicate",
        call: assets_duplicate,
    },
    EngineOp {
        module: "assets",
        name: "exists",
        call: assets_exists,
    },
    EngineOp {
        module: "assets",
        name: "reload",
        call: assets_reload,
    },
    EngineOp {
        module: "assets",
        name: "save",
        call: assets_save,
    },
    EngineOp {
        module: "assets",
        name: "directory",
        call: assets_directory,
    },
    EngineOp {
        module: "log",
        name: "info",
        call: log_info,
    },
    EngineOp {
        module: "log",
        name: "warn",
        call: log_warn,
    },
    EngineOp {
        module: "log",
        name: "error",
        call: log_error,
    },
    EngineOp {
        module: "log",
        name: "recent",
        call: log_recent,
    },
    EngineOp {
        module: "log",
        name: "clear",
        call: log_clear,
    },
    EngineOp {
        module: "rng",
        name: "seed",
        call: rng_seed,
    },
    EngineOp {
        module: "rng",
        name: "random",
        call: rng_random,
    },
    EngineOp {
        module: "rng",
        name: "range",
        call: rng_range,
    },
    EngineOp {
        module: "rng",
        name: "int",
        call: rng_int,
    },
    EngineOp {
        module: "fs",
        name: "read",
        call: fs_read,
    },
    EngineOp {
        module: "fs",
        name: "write",
        call: fs_write,
    },
    EngineOp {
        module: "fs",
        name: "exists",
        call: fs_exists,
    },
    EngineOp {
        module: "fs",
        name: "list",
        call: fs_list,
    },
    EngineOp {
        module: "toml",
        name: "parse",
        call: toml_parse,
    },
    EngineOp {
        module: "toml",
        name: "encode",
        call: toml_encode,
    },
    EngineOp {
        module: "json",
        name: "parse",
        call: json_parse,
    },
    EngineOp {
        module: "json",
        name: "encode",
        call: json_encode,
    },
];

/// Register every engine module into the host, plus the node API under `node`.
///
/// Called once when an app gains a script backend. A backend that gives its
/// node handle method syntax still walks `node_api::NODE_OPS` for the
/// sugar; this is what makes the operations reachable at all.
///
/// Takes `&Engine` rather than a `Bindings` — unlike every other
/// `install_*`, it creates the modules on the host itself instead of filling
/// one it was handed, because the operations it registers span several
/// modules.
pub fn install_engine_api(eng: &Engine) -> Result<()> {
    let host = eng
        .script_host()
        .ok_or_else(|| anyhow!("no script backend is running"))?;
    let mut current: Option<(&str, Box<dyn balaur_script::Bindings<Engine>>)> = None;
    for d in ENGINE_OPS {
        let m = match &mut current {
            Some((name, m)) if *name == d.module => m,
            _ => {
                current = Some((d.module, host.module(d.module)?));
                &mut current.as_mut().expect("just assigned").1
            }
        };
        m.function_raw(d.name, Box::new(d.call));
    }
    let mut node = host.module("node")?;
    crate::node_api::install_node_api(&mut *node);
    Ok(())
}

fn time(eng: &Engine, _: &[Value]) -> Result<Value> {
    Ok(Value::Num(eng.time()))
}

fn delta(eng: &Engine, _: &[Value]) -> Result<Value> {
    Ok(Value::Num(f64::from(eng.delta())))
}

fn quit(eng: &Engine, _: &[Value]) -> Result<Value> {
    eng.request_quit();
    Ok(Value::Nil)
}

fn args(eng: &Engine, _: &[Value]) -> Result<Value> {
    let list = eng
        .try_resource::<crate::app::ScriptArgs>()
        .map(|a| a.borrow().0.clone())
        .unwrap_or_default();
    Ok(Value::List(list.into_iter().map(Value::Str).collect()))
}

fn reload_script(eng: &Engine, args: &[Value]) -> Result<Value> {
    let host = eng
        .script_host()
        .ok_or_else(|| anyhow!("no script backend is running"))?;
    host.reload(text(args, 0)?)?;
    Ok(Value::Nil)
}

fn root(eng: &Engine, _: &[Value]) -> Result<Value> {
    Ok(Value::Node(crate::node_id_of(eng.root()).0))
}

fn get_node(eng: &Engine, args: &[Value]) -> Result<Value> {
    let world = eng.world();
    Ok(scene::find_node(&world, eng.root(), text(args, 0)?)
        .map_or(Value::Nil, |e| Value::Node(crate::node_id_of(e).0)))
}

fn spawn(eng: &Engine, args: &[Value]) -> Result<Value> {
    let parent = optional_node(args, 1)?.unwrap_or_else(|| eng.root());
    let mut world = eng.world_mut();
    let entity = scene::spawn_node(&mut world, text(args, 0)?, parent);
    Ok(Value::Node(crate::node_id_of(entity).0))
}

fn instantiate(eng: &Engine, args: &[Value]) -> Result<Value> {
    let base = optional_node(args, 1)?.unwrap_or_else(|| eng.root());
    let attach = match args.get(2) {
        Some(Value::Map(pairs)) => !pairs
            .iter()
            .any(|(k, v)| k == "scripts" && matches!(v, Value::Bool(false))),
        _ => true,
    };
    crate::project::instantiate_scene(eng, text(args, 0)?, base, attach)?;
    Ok(Value::Nil)
}

/// The scene file's raw TOML text, or nil. Not a load: nothing is parsed or
/// spawned. Unlike `fs.read` it goes through the script host, so it finds the
/// file inside the pack in a packed run.
fn source(eng: &Engine, args: &[Value]) -> Result<Value> {
    let rel = text(args, 0)?;
    Ok(eng
        .script_host()
        .and_then(|host| host.scene_source(rel))
        .map_or(Value::Nil, Value::Str))
}

/// The names of every registered component TYPE, not the components on any
/// node. Pairs with `scene.component_schema(name)`.
fn component_types(eng: &Engine, _: &[Value]) -> Result<Value> {
    Ok(Value::List(
        crate::components::names(eng)
            .into_iter()
            .map(Value::Str)
            .collect(),
    ))
}

fn component_schema(eng: &Engine, args: &[Value]) -> Result<Value> {
    let registry = eng.resource::<crate::components::ComponentRegistry>();
    let registry = registry.borrow();
    registry.def(text(args, 0)?).map_or(Ok(Value::Nil), |def| {
        crate::node_api::from_toml(&def.schema)
    })
}

/// An asset's definition table, from any of the three reference forms.
///
/// A script gets the data, not the engine's parsed object: a table is what a
/// script can read, edit and hand to `toml.encode`. The parsed side belongs to
/// the plugin that registered the type.
fn assets_load(eng: &Engine, args: &[Value]) -> Result<Value> {
    let definition = crate::assets::definition(eng, text(args, 0)?)?;
    crate::node_api::from_toml(&definition)
}

/// A private copy: read past the cache, so editing it cannot disturb what
/// every other holder of that reference sees.
fn assets_duplicate(eng: &Engine, args: &[Value]) -> Result<Value> {
    let definition = crate::assets::duplicate_definition(eng, text(args, 0)?)?;
    crate::node_api::from_toml(&definition)
}

fn assets_exists(eng: &Engine, args: &[Value]) -> Result<Value> {
    Ok(Value::Bool(crate::assets::exists(eng, text(args, 0)?)))
}

/// Forget a reference, so the next load re-reads its source. What the editor
/// calls after writing an asset file.
fn assets_reload(eng: &Engine, args: &[Value]) -> Result<Value> {
    crate::assets::reload(eng, text(args, 0)?)?;
    Ok(Value::Nil)
}

/// Write a definition table back to the file a reference names, and forget the
/// cached copy so the next load reads what was written.
fn assets_save(eng: &Engine, args: &[Value]) -> Result<Value> {
    let definition = crate::node_api::to_toml(
        args.get(1)
            .ok_or_else(|| anyhow!("assets.save needs the table to write"))?,
    )?;
    crate::assets::save(eng, text(args, 0)?, &definition)?;
    Ok(Value::Nil)
}

/// Where files of an asset type belong, as its plugin declared it.
///
/// The editor promotes an inline definition to a file and has to put it
/// somewhere; only the type knows where. Empty when the type is unknown or
/// declared no directory, which a caller reads as "cannot promote".
fn assets_directory(eng: &Engine, args: &[Value]) -> Result<Value> {
    Ok(Value::Str(crate::assets::directory(eng, text(args, 0)?)))
}

/// The three writers a script has. They emit through `tracing`, so a scripted
/// line lands in the same stream, and the same `logbuf`, as an engine one --
/// which is what makes `log.recent` able to show both.
fn log_info(_: &Engine, args: &[Value]) -> Result<Value> {
    tracing::info!("[script] {}", text(args, 0)?);
    Ok(Value::Nil)
}

fn log_warn(_: &Engine, args: &[Value]) -> Result<Value> {
    tracing::warn!("[script] {}", text(args, 0)?);
    Ok(Value::Nil)
}

fn log_error(_: &Engine, args: &[Value]) -> Result<Value> {
    tracing::error!("[script] {}", text(args, 0)?);
    Ok(Value::Nil)
}

fn log_recent(_: &Engine, args: &[Value]) -> Result<Value> {
    let n = match args.first() {
        Some(Value::Int(n)) => usize::try_from(*n).unwrap_or(100),
        Some(Value::Num(n)) => *n as usize,
        _ => 100,
    };
    Ok(Value::List(
        crate::logbuf::recent(n)
            .into_iter()
            .map(|e| {
                Value::Map(vec![
                    ("time".into(), Value::Num(e.time)),
                    ("level".into(), Value::Str(e.level.clone())),
                    ("tag".into(), Value::Str(e.tag.clone())),
                    ("message".into(), Value::Str(e.message.clone())),
                ])
            })
            .collect(),
    ))
}

fn log_clear(_: &Engine, _: &[Value]) -> Result<Value> {
    crate::logbuf::clear();
    Ok(Value::Nil)
}

fn rng_seed(eng: &Engine, args: &[Value]) -> Result<Value> {
    let seed = match args.first() {
        Some(Value::Int(n)) => *n,
        Some(Value::Num(n)) => *n as i64,
        other => return Err(anyhow!("seed should be a number, got {other:?}")),
    };
    crate::rng::with_rng(eng, |rng| *rng = Pcg32::new(seed as u64));
    Ok(Value::Nil)
}

fn rng_random(eng: &Engine, _: &[Value]) -> Result<Value> {
    let v = crate::rng::with_rng(eng, Pcg32::next_f64);
    Ok(Value::Num(v))
}

fn rng_range(eng: &Engine, args: &[Value]) -> Result<Value> {
    let (lo, hi) = (number(args, 0)?, number(args, 1)?);
    let v = crate::rng::with_rng(eng, Pcg32::next_f64);
    Ok(Value::Num(v.mul_add(hi - lo, lo)))
}

fn rng_int(eng: &Engine, args: &[Value]) -> Result<Value> {
    let (lo, hi) = (integer(args, 0)?, integer(args, 1)?);
    let v = crate::rng::with_rng(eng, |rng| rng.next_range_i64(lo, hi));
    Ok(Value::Int(v))
}

/// Project-relative unless absolute, so a script cannot wander the disk by
/// accident.
fn resolve(eng: &Engine, path: &str) -> std::path::PathBuf {
    let p = std::path::Path::new(path);
    if p.is_absolute() {
        return p.to_path_buf();
    }
    eng.try_resource::<crate::project::ProjectRoot>()
        .map_or_else(|| p.to_path_buf(), |r| r.borrow().0.join(p))
}

fn fs_read(eng: &Engine, args: &[Value]) -> Result<Value> {
    Ok(std::fs::read_to_string(resolve(eng, text(args, 0)?)).map_or(Value::Nil, Value::Str))
}

/// Write a file, making the directory it goes in.
///
/// Creating the parent is part of writing: a tool that saves an asset into
/// `animations/` should not fail because the project has never had one.
fn fs_write(eng: &Engine, args: &[Value]) -> Result<Value> {
    let path = resolve(eng, text(args, 0)?);
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, text(args, 1)?)?;
    Ok(Value::Nil)
}

fn fs_exists(eng: &Engine, args: &[Value]) -> Result<Value> {
    Ok(Value::Bool(resolve(eng, text(args, 0)?).exists()))
}

fn fs_list(eng: &Engine, args: &[Value]) -> Result<Value> {
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

fn toml_parse(_: &Engine, args: &[Value]) -> Result<Value> {
    let parsed: toml::Value = toml::from_str(text(args, 0)?)?;
    crate::node_api::from_toml(&parsed)
}

fn toml_encode(_: &Engine, args: &[Value]) -> Result<Value> {
    let value = args.first().ok_or_else(|| anyhow!("nothing to encode"))?;
    Ok(Value::Str(toml::to_string(&crate::node_api::to_toml(
        value,
    )?)?))
}

fn json_parse(_: &Engine, args: &[Value]) -> Result<Value> {
    let parsed: serde_json::Value = serde_json::from_str(text(args, 0)?)?;
    from_json(&parsed)
}

fn json_encode(_: &Engine, args: &[Value]) -> Result<Value> {
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

fn number(args: &[Value], i: usize) -> Result<f64> {
    match args.get(i) {
        Some(Value::Num(n)) => Ok(*n),
        Some(Value::Int(n)) => Ok(*n as f64),
        other => Err(anyhow!("argument {i} should be a number, got {other:?}")),
    }
}

fn integer(args: &[Value], i: usize) -> Result<i64> {
    match args.get(i) {
        Some(Value::Int(n)) => Ok(*n),
        Some(Value::Num(n)) => Ok(*n as i64),
        other => Err(anyhow!("argument {i} should be a number, got {other:?}")),
    }
}

fn text(args: &[Value], i: usize) -> Result<&str> {
    match args.get(i) {
        Some(Value::Str(s)) => Ok(s),
        other => Err(anyhow!("argument {i} should be a string, got {other:?}")),
    }
}

fn optional_node(args: &[Value], i: usize) -> Result<Option<hecs::Entity>> {
    match args.get(i) {
        None | Some(Value::Nil) => Ok(None),
        Some(Value::Node(id)) => Ok(Some(crate::entity_of(balaur_script::NodeId(*id))?)),
        other => Err(anyhow!("argument {i} should be a node, got {other:?}")),
    }
}
