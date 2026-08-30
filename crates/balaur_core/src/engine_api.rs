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

// ---- argument helpers ------------------------------------------------

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
