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
use crate::scene;

/// One engine operation, tagged with the module it belongs to.
pub struct Decl {
    pub module: &'static str,
    pub name: &'static str,
    pub call: fn(&Engine, &[Value]) -> Result<Value>,
}

/// Everything the engine itself exposes to scripts.
pub const DECLARATIONS: &[Decl] = &[
    Decl {
        module: "engine",
        name: "time",
        call: time,
    },
    Decl {
        module: "engine",
        name: "delta",
        call: delta,
    },
    Decl {
        module: "engine",
        name: "quit",
        call: quit,
    },
    Decl {
        module: "engine",
        name: "args",
        call: args,
    },
    Decl {
        module: "engine",
        name: "reload_script",
        call: reload_script,
    },
    Decl {
        module: "scene",
        name: "root",
        call: root,
    },
    Decl {
        module: "scene",
        name: "get_node",
        call: get_node,
    },
    Decl {
        module: "scene",
        name: "spawn",
        call: spawn,
    },
    Decl {
        module: "scene",
        name: "instantiate",
        call: instantiate,
    },
    Decl {
        module: "scene",
        name: "load",
        call: load,
    },
    Decl {
        module: "scene",
        name: "components",
        call: components,
    },
    Decl {
        module: "scene",
        name: "component_schema",
        call: component_schema,
    },
    Decl {
        module: "log",
        name: "recent",
        call: log_recent,
    },
    Decl {
        module: "log",
        name: "clear",
        call: log_clear,
    },
    Decl {
        module: "rng",
        name: "seed",
        call: rng_seed,
    },
    Decl {
        module: "rng",
        name: "random",
        call: rng_random,
    },
    Decl {
        module: "rng",
        name: "range",
        call: rng_range,
    },
    Decl {
        module: "rng",
        name: "int",
        call: rng_int,
    },
    Decl {
        module: "fs",
        name: "read",
        call: fs_read,
    },
    Decl {
        module: "fs",
        name: "write",
        call: fs_write,
    },
    Decl {
        module: "fs",
        name: "exists",
        call: fs_exists,
    },
    Decl {
        module: "fs",
        name: "list",
        call: fs_list,
    },
    Decl {
        module: "toml",
        name: "parse",
        call: toml_parse,
    },
    Decl {
        module: "toml",
        name: "encode",
        call: toml_encode,
    },
];

/// Register every engine module into the host, plus the node API under `node`.
///
/// Called once when an app gains a script backend. A backend that gives its
/// node handle method syntax still walks `node_api::DECLARATIONS` for the
/// sugar; this is what makes the operations reachable at all.
pub fn install(engine: &Engine) -> Result<()> {
    let host = engine
        .scripts()
        .ok_or_else(|| anyhow!("no script backend is running"))?;
    let mut current: Option<(&str, Box<dyn balaur_script::Bindings<Engine>>)> = None;
    for d in DECLARATIONS {
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
    crate::node_api::install(&mut *node);
    Ok(())
}

// ---- engine ----------------------------------------------------------

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
        .scripts()
        .ok_or_else(|| anyhow!("no script backend is running"))?;
    host.reload(text(args, 0)?)?;
    Ok(Value::Nil)
}

// ---- scene -----------------------------------------------------------

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
    // opts: { scripts = false } skips script attachment (editor mirroring).
    let attach = match args.get(2) {
        Some(Value::Map(pairs)) => !pairs
            .iter()
            .any(|(k, v)| k == "scripts" && matches!(v, Value::Bool(false))),
        _ => true,
    };
    crate::project::instantiate_scene(eng, text(args, 0)?, base, attach)?;
    Ok(Value::Nil)
}

fn load(eng: &Engine, args: &[Value]) -> Result<Value> {
    let rel = text(args, 0)?;
    Ok(eng
        .scripts()
        .and_then(|host| host.scene_source(rel))
        .map_or(Value::Nil, Value::Str))
}

fn components(eng: &Engine, _: &[Value]) -> Result<Value> {
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

// ---- log -------------------------------------------------------------

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

// ---- rng -------------------------------------------------------------

fn rng_seed(eng: &Engine, args: &[Value]) -> Result<Value> {
    let seed = match args.first() {
        Some(Value::Int(n)) => *n,
        Some(Value::Num(n)) => *n as i64,
        other => return Err(anyhow!("seed should be a number, got {other:?}")),
    };
    let rng = eng.resource::<crate::rng::DetRng>();
    rng.borrow_mut().0 = crate::rng::Pcg32::new(seed as u64);
    Ok(Value::Nil)
}

fn rng_random(eng: &Engine, _: &[Value]) -> Result<Value> {
    let rng = eng.resource::<crate::rng::DetRng>();
    let v = rng.borrow_mut().0.next_f64();
    Ok(Value::Num(v))
}

fn rng_range(eng: &Engine, args: &[Value]) -> Result<Value> {
    let (lo, hi) = (number(args, 0)?, number(args, 1)?);
    let rng = eng.resource::<crate::rng::DetRng>();
    let v = rng.borrow_mut().0.next_f64();
    Ok(Value::Num(v.mul_add(hi - lo, lo)))
}

fn rng_int(eng: &Engine, args: &[Value]) -> Result<Value> {
    let (lo, hi) = (integer(args, 0)?, integer(args, 1)?);
    let rng = eng.resource::<crate::rng::DetRng>();
    let v = rng.borrow_mut().0.next_range_i64(lo, hi);
    Ok(Value::Int(v))
}

// ---- fs, rooted at the project ---------------------------------------

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

fn fs_write(eng: &Engine, args: &[Value]) -> Result<Value> {
    std::fs::write(resolve(eng, text(args, 0)?), text(args, 1)?)?;
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

// ---- toml ------------------------------------------------------------

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

// ---- argument helpers ------------------------------------------------

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
