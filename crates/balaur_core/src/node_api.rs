//! The node API, declared once for every language.
//!
//! Each operation takes the node as its first argument, so a backend can
//! register these as free functions or bind them as methods on its own node
//! handle — see `NODE_OPS`. Adding a language costs the sugar, not the
//! twenty-odd operations.

// Every declaration shares one signature so they can sit in a table of
// function pointers; several of them have nothing to fail at.
#![allow(clippy::unnecessary_wraps)]

use anyhow::{anyhow, Result};
use balaur_script::{Bindings, Value};
use glamx::{EulerRot, Quat, Vec3};
use hecs::Entity;

use crate::engine::{Command, Engine};
use crate::scene::{self, Children, GlobalTransform, Name, Parent, ScriptAttachment, Transform};

/// One node operation, as a plain function pointer so the list stays a `const`.
pub struct NodeOp {
    pub name: &'static str,
    pub call: fn(&Engine, &[Value]) -> Result<Value>,
}

/// Every node operation, in one list.
pub const NODE_OPS: &[NodeOp] = &[
    NodeOp {
        name: "is_valid",
        call: is_valid,
    },
    NodeOp {
        name: "name",
        call: name,
    },
    NodeOp {
        name: "set_name",
        call: set_name,
    },
    NodeOp {
        name: "path",
        call: path,
    },
    NodeOp {
        name: "position",
        call: position,
    },
    NodeOp {
        name: "set_position",
        call: set_position,
    },
    NodeOp {
        name: "translate",
        call: translate,
    },
    NodeOp {
        name: "rotation_euler",
        call: rotation_euler,
    },
    NodeOp {
        name: "set_rotation_euler",
        call: set_rotation_euler,
    },
    NodeOp {
        name: "rotation_degrees",
        call: rotation_degrees,
    },
    NodeOp {
        name: "set_rotation_degrees",
        call: set_rotation_degrees,
    },
    NodeOp {
        name: "scale",
        call: scale,
    },
    NodeOp {
        name: "set_scale",
        call: set_scale,
    },
    NodeOp {
        name: "global_position",
        call: global_position,
    },
    NodeOp {
        name: "global_rotation_euler",
        call: global_rotation_euler,
    },
    NodeOp {
        name: "global_scale",
        call: global_scale,
    },
    NodeOp {
        name: "get_node",
        call: get_node,
    },
    NodeOp {
        name: "add_child",
        call: add_child,
    },
    NodeOp {
        name: "parent",
        call: parent,
    },
    NodeOp {
        name: "children",
        call: children,
    },
    NodeOp {
        name: "set_component",
        call: set_component,
    },
    NodeOp {
        name: "remove_component",
        call: remove_component,
    },
    NodeOp {
        name: "get_component",
        call: get_component,
    },
    NodeOp {
        name: "has_component",
        call: has_component,
    },
    NodeOp {
        name: "component_names",
        call: component_names,
    },
    NodeOp {
        name: "script_path",
        call: script_path,
    },
    NodeOp {
        name: "attach_script",
        call: attach_script,
    },
    NodeOp {
        name: "queue_free",
        call: queue_free,
    },
];

/// Register every node operation into a binding group as a free function.
///
/// A backend that gives its node handle method syntax walks `NODE_OPS`
/// itself instead; this is the plain path.
pub fn install_node_api(m: &mut dyn Bindings<Engine>) {
    for d in NODE_OPS {
        m.function_raw(d.name, Box::new(d.call));
    }
}

// ---- argument helpers ------------------------------------------------

fn node(args: &[Value]) -> Result<Entity> {
    match args.first() {
        Some(Value::Node(id)) => crate::entity_of(balaur_script::NodeId(*id)),
        _ => Err(anyhow!("expected a node as the first argument")),
    }
}

fn text(args: &[Value], i: usize) -> Result<&str> {
    match args.get(i) {
        Some(Value::Str(s)) => Ok(s),
        other => Err(anyhow!("argument {i} should be a string, got {other:?}")),
    }
}

fn number(args: &[Value], i: usize) -> Result<f32> {
    match args.get(i) {
        Some(Value::Num(n)) => Ok(*n as f32),
        Some(Value::Int(n)) => Ok(*n as f32),
        other => Err(anyhow!("argument {i} should be a number, got {other:?}")),
    }
}

/// Read three numbers, or one vector, so `set_position(v)` and
/// `set_position(x, y, z)` both work.
fn xyz(args: &[Value], from: usize) -> Result<Vec3> {
    if let Some(Value::Vec3([x, y, z])) = args.get(from) {
        return Ok(Vec3::new(*x, *y, *z));
    }
    Ok(Vec3::new(
        number(args, from)?,
        number(args, from + 1)?,
        number(args, from + 2)?,
    ))
}

fn vec3(v: Vec3) -> Value {
    Value::Vec3([v.x, v.y, v.z])
}

fn with_transform<R>(eng: &Engine, e: Entity, f: impl FnOnce(&mut Transform) -> R) -> Result<R> {
    let world = eng.world();
    let mut transform = world
        .get::<&mut Transform>(e)
        .map_err(|_| anyhow!("node is dead or has no transform"))?;
    Ok(f(&mut transform))
}

// ---- identity --------------------------------------------------------

fn is_valid(eng: &Engine, args: &[Value]) -> Result<Value> {
    let Ok(e) = node(args) else {
        return Ok(Value::Bool(false));
    };
    Ok(Value::Bool(eng.world().contains(e)))
}

fn name(eng: &Engine, args: &[Value]) -> Result<Value> {
    let e = node(args)?;
    let world = eng.world();
    Ok(Value::Str(
        world
            .get::<&Name>(e)
            .map(|n| n.0.clone())
            .unwrap_or_default(),
    ))
}

fn set_name(eng: &Engine, args: &[Value]) -> Result<Value> {
    let e = node(args)?;
    let world = eng.world();
    if let Ok(mut n) = world.get::<&mut Name>(e) {
        n.0 = text(args, 1)?.to_string();
    }
    Ok(Value::Nil)
}

fn path(eng: &Engine, args: &[Value]) -> Result<Value> {
    let e = node(args)?;
    Ok(Value::Str(scene::node_path(&eng.world(), e)))
}

// ---- local transform -------------------------------------------------

fn position(eng: &Engine, args: &[Value]) -> Result<Value> {
    with_transform(eng, node(args)?, |t| vec3(t.position))
}

fn set_position(eng: &Engine, args: &[Value]) -> Result<Value> {
    let v = xyz(args, 1)?;
    with_transform(eng, node(args)?, |t| t.position = v)?;
    Ok(Value::Nil)
}

fn translate(eng: &Engine, args: &[Value]) -> Result<Value> {
    let v = xyz(args, 1)?;
    with_transform(eng, node(args)?, |t| t.position += v)?;
    Ok(Value::Nil)
}

fn rotation_euler(eng: &Engine, args: &[Value]) -> Result<Value> {
    with_transform(eng, node(args)?, |t| {
        let (yaw, pitch, roll) = t.rotation.to_euler(EulerRot::ZYX);
        Value::Vec3([roll, pitch, yaw])
    })
}

fn set_rotation_euler(eng: &Engine, args: &[Value]) -> Result<Value> {
    let v = xyz(args, 1)?;
    with_transform(eng, node(args)?, |t| {
        t.rotation = Quat::from_euler(EulerRot::ZYX, v.z, v.y, v.x);
    })?;
    Ok(Value::Nil)
}

/// The same rotation as `rotation_euler`, in degrees.
///
/// Radians are the engine's unit and stay the default; degrees are what a
/// person authors, so the pair exists rather than every caller carrying its
/// own `math.deg` conversion the way the editor's inspector used to.
fn rotation_degrees(eng: &Engine, args: &[Value]) -> Result<Value> {
    with_transform(eng, node(args)?, |t| {
        let (yaw, pitch, roll) = t.rotation.to_euler(EulerRot::ZYX);
        Value::Vec3([roll.to_degrees(), pitch.to_degrees(), yaw.to_degrees()])
    })
}

fn set_rotation_degrees(eng: &Engine, args: &[Value]) -> Result<Value> {
    let v = xyz(args, 1)?;
    with_transform(eng, node(args)?, |t| {
        t.rotation = Quat::from_euler(
            EulerRot::ZYX,
            v.z.to_radians(),
            v.y.to_radians(),
            v.x.to_radians(),
        );
    })?;
    Ok(Value::Nil)
}

fn scale(eng: &Engine, args: &[Value]) -> Result<Value> {
    with_transform(eng, node(args)?, |t| vec3(t.scale))
}

fn set_scale(eng: &Engine, args: &[Value]) -> Result<Value> {
    let v = xyz(args, 1)?;
    with_transform(eng, node(args)?, |t| t.scale = v)?;
    Ok(Value::Nil)
}

// ---- world transform, read only --------------------------------------

fn global<R>(eng: &Engine, args: &[Value], f: impl FnOnce(&GlobalTransform) -> R) -> Result<R> {
    let e = node(args)?;
    let world = eng.world();
    let g = world
        .get::<&GlobalTransform>(e)
        .map_err(|_| anyhow!("node is dead"))?;
    Ok(f(&g))
}

fn global_position(eng: &Engine, args: &[Value]) -> Result<Value> {
    global(eng, args, |g| vec3(g.position))
}

fn global_scale(eng: &Engine, args: &[Value]) -> Result<Value> {
    global(eng, args, |g| vec3(g.scale))
}

fn global_rotation_euler(eng: &Engine, args: &[Value]) -> Result<Value> {
    global(eng, args, |g| {
        let (yaw, pitch, roll) = g.rotation.to_euler(EulerRot::ZYX);
        Value::Vec3([roll, pitch, yaw])
    })
}

// ---- hierarchy -------------------------------------------------------

fn get_node(eng: &Engine, args: &[Value]) -> Result<Value> {
    let e = node(args)?;
    let world = eng.world();
    Ok(scene::find_node(&world, e, text(args, 1)?)
        .map_or(Value::Nil, |found| Value::Node(crate::node_id_of(found).0)))
}

fn add_child(eng: &Engine, args: &[Value]) -> Result<Value> {
    let e = node(args)?;
    let mut world = eng.world_mut();
    let child = scene::spawn_node(&mut world, text(args, 1)?, e);
    Ok(Value::Node(crate::node_id_of(child).0))
}

fn parent(eng: &Engine, args: &[Value]) -> Result<Value> {
    let e = node(args)?;
    let world = eng.world();
    Ok(world
        .get::<&Parent>(e)
        .ok()
        .map_or(Value::Nil, |p| Value::Node(crate::node_id_of(p.0).0)))
}

fn children(eng: &Engine, args: &[Value]) -> Result<Value> {
    let e = node(args)?;
    let world = eng.world();
    let out = world.get::<&Children>(e).map_or_else(
        |_| Vec::new(),
        |c| {
            c.0.iter()
                .map(|&child| Value::Node(crate::node_id_of(child).0))
                .collect()
        },
    );
    Ok(Value::List(out))
}

// ---- components ------------------------------------------------------

/// Adds the component if the node lacks it, merges over it if it has it, so
/// one verb covers both. There is deliberately no `add_component`: the family
/// reads set_ / get_ / has_ / remove_.
fn set_component(eng: &Engine, args: &[Value]) -> Result<Value> {
    let e = node(args)?;
    let params = match args.get(2) {
        None | Some(Value::Nil) => None,
        Some(v) => Some(to_toml(v)?),
    };
    crate::components::add(eng, e, text(args, 1)?, params.as_ref())?;
    Ok(Value::Nil)
}

fn remove_component(eng: &Engine, args: &[Value]) -> Result<Value> {
    let e = node(args)?;
    crate::components::remove(eng, e, text(args, 1)?)?;
    Ok(Value::Nil)
}

fn get_component(eng: &Engine, args: &[Value]) -> Result<Value> {
    let e = node(args)?;
    crate::components::get(eng, e, text(args, 1)?)
        .as_ref()
        .map_or(Ok(Value::Nil), from_toml)
}

fn has_component(eng: &Engine, args: &[Value]) -> Result<Value> {
    let e = node(args)?;
    Ok(Value::Bool(
        crate::components::get(eng, e, text(args, 1)?).is_some(),
    ))
}

fn component_names(eng: &Engine, args: &[Value]) -> Result<Value> {
    let e = node(args)?;
    Ok(Value::List(
        crate::components::present_on(eng, e)
            .into_iter()
            .map(Value::Str)
            .collect(),
    ))
}

// ---- scripting -------------------------------------------------------

fn script_path(eng: &Engine, args: &[Value]) -> Result<Value> {
    let e = node(args)?;
    let world = eng.world();
    Ok(world
        .get::<&ScriptAttachment>(e)
        .ok()
        .map_or(Value::Nil, |a| Value::Str(a.path.clone())))
}

fn attach_script(eng: &Engine, args: &[Value]) -> Result<Value> {
    let e = node(args)?;
    let host = eng
        .script_host()
        .ok_or_else(|| anyhow!("no script backend is running"))?;
    host.attach(crate::node_id_of(e), text(args, 1)?)?;
    Ok(Value::Nil)
}

fn queue_free(eng: &Engine, args: &[Value]) -> Result<Value> {
    eng.push_command(Command::Free(node(args)?));
    Ok(Value::Nil)
}

// ---- component parameters, as TOML -----------------------------------

/// Component parameters travel as TOML, so a script table and a scene file
/// describe a component the same way.
pub fn to_toml(v: &Value) -> Result<toml::Value> {
    Ok(match v {
        Value::Nil => toml::Value::String(String::new()),
        Value::Bool(b) => toml::Value::Boolean(*b),
        Value::Int(i) => toml::Value::Integer(*i),
        Value::Num(n) => toml::Value::Float(*n),
        Value::Str(s) => toml::Value::String(s.clone()),
        Value::Node(_) | Value::Callback(_) => {
            return Err(anyhow!("a node or callback is not component data"))
        }
        Value::Many(_) => return Err(anyhow!("several values are not component data")),
        Value::Vec2(a) => number_list(a),
        Value::Vec3(a) => number_list(a),
        Value::Color(a) => number_list(a),
        Value::List(items) => toml::Value::Array(items.iter().map(to_toml).collect::<Result<_>>()?),
        Value::Map(pairs) => toml::Value::Table(
            pairs
                .iter()
                .map(|(k, val)| Ok((k.clone(), to_toml(val)?)))
                .collect::<Result<_>>()?,
        ),
    })
}

fn number_list(a: &[f32]) -> toml::Value {
    toml::Value::Array(
        a.iter()
            .map(|n| toml::Value::Float(f64::from(*n)))
            .collect(),
    )
}

pub fn from_toml(v: &toml::Value) -> Result<Value> {
    Ok(match v {
        toml::Value::String(s) => Value::Str(s.clone()),
        toml::Value::Integer(i) => Value::Int(*i),
        toml::Value::Float(f) => Value::Num(*f),
        toml::Value::Boolean(b) => Value::Bool(*b),
        toml::Value::Datetime(d) => Value::Str(d.to_string()),
        toml::Value::Array(items) => {
            Value::List(items.iter().map(from_toml).collect::<Result<_>>()?)
        }
        toml::Value::Table(table) => Value::Map(
            table
                .iter()
                .map(|(k, val)| Ok((k.clone(), from_toml(val)?)))
                .collect::<Result<_>>()?,
        ),
    })
}
