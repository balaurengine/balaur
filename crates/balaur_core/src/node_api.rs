//! The node API, declared once for every language.
//!
//! Each operation takes the node as its first argument, so a backend can
//! register these as free functions or bind them as methods on its own node
//! handle: see `NODE_OPS`. Adding a language costs the sugar, not the
//! twenty-odd operations.

// Every declaration shares one signature so they can sit in a table of
// function pointers; several of them have nothing to fail at.
#![allow(clippy::unnecessary_wraps)]

use anyhow::{Result, anyhow, bail};
use balaur_script::{Bindings, Value};
use glamx::{EulerRot, Quat, Vec3};
use hecs::Entity;

use crate::engine::{Command, Engine};
use crate::scene::{
    self, Appearance, Children, GlobalTransform, Name, Parent, ScriptAttachment, Tags, Transform,
};

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
        name: "set_parent",
        call: set_parent,
    },
    NodeOp {
        name: "sibling_index",
        call: sibling_index,
    },
    NodeOp {
        name: "set_sibling_index",
        call: set_sibling_index,
    },
    NodeOp {
        name: "set_component",
        call: set_component,
    },
    NodeOp {
        name: "patch_component",
        call: patch_component,
    },
    NodeOp {
        name: "go",
        call: go_to_state,
    },
    NodeOp {
        name: "state",
        call: current_state,
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
        name: "stable_id",
        call: stable_id,
    },
    NodeOp {
        name: "descendants",
        call: descendants,
    },
    NodeOp {
        name: "script_path",
        call: script_path,
    },
    NodeOp {
        name: "has_method",
        call: has_method,
    },
    NodeOp { name: "call", call },
    NodeOp {
        name: "call_async",
        call: call_async,
    },
    NodeOp {
        name: "emit",
        call: emit,
    },
    NodeOp {
        name: "attach_script",
        call: attach_script,
    },
    NodeOp {
        name: "detach_script",
        call: detach_script,
    },
    NodeOp {
        name: "queue_free",
        call: queue_free,
    },
    NodeOp {
        name: "visible",
        call: visible,
    },
    NodeOp {
        name: "set_visible",
        call: set_visible,
    },
    NodeOp {
        name: "global_visible",
        call: global_visible,
    },
    NodeOp {
        name: "tint",
        call: tint,
    },
    NodeOp {
        name: "set_tint",
        call: set_tint,
    },
    NodeOp {
        name: "global_tint",
        call: global_tint,
    },
    NodeOp {
        name: "material",
        call: material,
    },
    NodeOp {
        name: "set_material",
        call: set_material,
    },
    NodeOp {
        name: "global_material",
        call: global_material,
    },
    NodeOp {
        name: "z_index",
        call: z_index,
    },
    NodeOp {
        name: "set_z_index",
        call: set_z_index,
    },
    NodeOp {
        name: "global_z_index",
        call: global_z_index,
    },
    NodeOp {
        name: "tags",
        call: tags,
    },
    NodeOp {
        name: "has_tag",
        call: has_tag,
    },
    NodeOp {
        name: "add_tag",
        call: add_tag,
    },
    NodeOp {
        name: "remove_tag",
        call: remove_tag,
    },
];

/// Register every node operation into a binding group as a free function.
///
/// A backend that gives its node handle method syntax walks `NODE_OPS`
/// itself instead; this is the plain path.
pub fn install_node_api(m: &mut dyn Bindings<Engine>) {
    m.module_doc(
        "What every node has: its name and path, its place in the world, its \
         children, its components and its script. Each operation takes the \
         node as its first argument, so scripts normally call them as methods \
         on a node value (`this.node.get_node(\"Arm\")`). `position`, \
         `rotation_euler` and `scale` read the `transform` component, which \
         `this.node.transform.position` reads and writes directly.",
    );
    m.describe(&[
        ("is_valid", &[], "()", "Whether the node is still in the world; false rather than an error when the value is not a node."),
        ("name", &[], "()", "The node's own name, empty when it carries none."),
        ("set_name", &[], "(name: string)", "Rename the node, doing nothing when it carries no name."),
        ("path", &[], "()", "The node's slash-separated path, built by climbing parents from the node up to the root."),
        ("position", &[], "()", "The node's position in its parent's space."),
        ("set_position", &[], "(x: float, y: float, z: float)", "Move the node to a local position, given as three numbers or one vector."),
        ("translate", &[], "(x: float, y: float, z: float)", "Move the node by an offset in its parent's space, given as three numbers or one vector."),
        ("rotation_euler", &[], "()", "The node's local rotation as euler angles in radians, x then y then z."),
        ("set_rotation_euler", &[], "(x: float, y: float, z: float)", "Set the node's local rotation from euler angles in radians."),
        ("rotation_degrees", &[], "()", "The same local rotation as `rotation_euler`, in degrees."),
        ("set_rotation_degrees", &[], "(x: float, y: float, z: float)", "Set the node's local rotation from euler angles in degrees."),
        ("scale", &[], "()", "The node's scale relative to its parent."),
        ("set_scale", &[], "(x: float, y: float, z: float)", "Set the node's scale relative to its parent, as three numbers or one vector."),
        ("global_position", &[], "()", "The node's position in world space, as of the last transform sync."),
        ("global_rotation_euler", &[], "()", "The node's world rotation as euler angles in radians, as of the last transform sync."),
        ("global_scale", &[], "()", "The node's scale in world space, as of the last transform sync."),
        ("get_node", &[], "(path: string)", "The node at an `A/B/C` path relative to this one, `..` climbing to the parent; nil when nothing matches."),
        ("add_child", &[], "(name: string)", "Create a named child node under this one and return it."),
        ("parent", &[], "()", "The node's parent, nil at the root."),
        ("children", &[], "()", "The node's direct children, an empty list when it has none."),
        ("set_parent", &[], "(parent: node)", "Move the node under another, keeping where it is in the world; an error for a cycle or a dead parent."),
        ("sibling_index", &[], "()", "Where the node sits among its parent's children, counting from zero; 0 at the root."),
        ("set_sibling_index", &[], "(index: int)", "Move the node to that place among its siblings, clamped to the end. Order is draw order in a `row` or a `column`, and tree order in the digest."),
        ("set_component", &[], "(component: string, params: any?)", "Give the node the named component, built from the given table over the component's schema defaults. Every property the table leaves out goes back to its default; `patch_component` is the one that changes a property and leaves the rest."),
        ("go", &["states"], "(state: string)", "Put the node in one of its `states`: the state's table is patched over the components it names, and `on_state_changed(from, to)` follows. A node already in that state is left alone."),
        ("state", &["states"], "()", "The state the node is in, or \"\" for the pose the scene gave it."),
        ("patch_component", &[], "(component: string, params: table)", "Change the properties the table names and leave the rest of the component where they were. On a node without the component this adds it, the schema defaults being what it currently holds."),
        ("remove_component", &[], "(component: string)", "Take the named component off the node."),
        ("get_component", &[], "(component: string)", "The named component's properties as a table, nil when the node does not carry it."),
        ("has_component", &[], "(component: string)", "Whether the node carries the named component."),
        ("component_names", &[], "()", "The names of every component on the node."),
        ("stable_id", &[], "()", "The node's stable id, what a scene file declared or what `ids::mint` gave a spawned node, empty when it carries none. Survives rename and reparent, which a path does not."),
        ("descendants", &[], "()", "Every node under this one, in tree order, the node itself excluded."),
        ("script_path", &[], "()", "The path of the script attached to the node, nil when it has none."),
        ("has_method", &[], "(method: string)", "Whether the node's script declares this method, so a caller can tell \"no handler\" from \"a handler that answered nothing\"."),
        ("call", &[], "(method: string, args: any?)", "Call a method on the node's script and return what it gives back; nil when there is no such script or method."),
        ("call_async", &[], "(method: string, args: any?)", "Call a method that may suspend, and get a token `task.wait` resumes with its result once it returns: `task::wait(door.call_async(\"open\")).await`, a GDScript `await door.open()`."),
        ("emit", &[], "(name: string, payload: any?)", "Emit an event from this node, delivered at the top of the next frame to whoever subscribed to `name` on this node, and to whoever subscribed to `name` from anyone. `call` is the twin that reaches one known script, now."),
        ("attach_script", &[], "(path: string, props: any?)", "Attach the script at a path, with an optional table overriding what the script exports."),
        ("detach_script", &[], "()", "Drop the script instance on this node, so no further lifecycle call reaches it; the node and its components stay."),
        ("queue_free", &[], "()", "Destroy the node and its subtree at the end of the frame."),
        ("visible", &[], "(node)", "Whether the node itself is set to draw; an ancestor may still hide it."),
        ("set_visible", &[], "(node, on: bool)", "Show or hide the node and everything under it. Physics is untouched: a hidden collider still collides."),
        ("global_visible", &[], "(node)", "What the renderer sees: false when the node or any ancestor is hidden."),
        ("tint", &[], "(node)", "The node's own tint as r, g, b, a channel floats; an ancestor's multiplies into it on the way to the screen."),
        ("set_tint", &[], "(node, r: float, g: float, b: float, a: float?)", "Multiply a colour into everything the node and its subtree draw, alpha included, one meaning untinted. A renderable's own `color` is the node's alone; this is the one that inherits."),
        ("global_tint", &[], "(node)", "What the renderer multiplies by: this node's tint with every ancestor's folded in."),
        ("material", &[], "(node)", "The `material` asset the node names itself, empty when it takes its parent's."),
        ("set_material", &[], "(node, material: string)", "Draw the node and every descendant naming none with a `material` asset; empty goes back to the parent's."),
        ("global_material", &[], "(node)", "The material the node draws with: its own, or the nearest ancestor's. Empty is the built-in one."),
        ("z_index", &[], "(node)", "The node's own draw layer, added to its parent's unless set absolute."),
        ("set_z_index", &[], "(node, z: int, relative: bool)", "Put the node and its subtree on a draw layer: higher draws later. Relative by default, adding to the parent's layer; false makes it absolute."),
        ("global_z_index", &[], "(node)", "The layer the node actually draws on, with every ancestor's added in."),
        ("tags", &[], "(node)", "The names the node is filed under, sorted."),
        ("has_tag", &[], "(node, tag: string)", "Whether the node is filed under a name."),
        ("add_tag", &[], "(node, tag: string)", "File the node under a name; `scene.tagged` finds it from then on."),
        ("remove_tag", &[], "(node, tag: string)", "Take a name off the node; a name it never had is left alone."),
    ]);
    for d in NODE_OPS {
        m.function_raw(d.name, Box::new(d.call));
    }
}

pub(crate) fn node(args: &[Value]) -> Result<Entity> {
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

fn flag(args: &[Value], i: usize) -> Result<bool> {
    match args.get(i) {
        Some(Value::Bool(b)) => Ok(*b),
        other => Err(anyhow!(
            "argument {i} should be true or false, got {other:?}"
        )),
    }
}

fn integer(args: &[Value], i: usize) -> Result<i32> {
    match args.get(i) {
        Some(Value::Int(n)) => Ok(*n as i32),
        Some(Value::Num(n)) => Ok(*n as i32),
        other => Err(anyhow!(
            "argument {i} should be a whole number, got {other:?}"
        )),
    }
}

fn with_appearance<R>(eng: &Engine, e: Entity, f: impl FnOnce(&mut Appearance) -> R) -> Result<R> {
    let world = eng.world();
    let mut appearance = world
        .get::<&mut Appearance>(e)
        .map_err(|_| anyhow!("node is dead"))?;
    Ok(f(&mut appearance))
}

fn visible(eng: &Engine, args: &[Value]) -> Result<Value> {
    with_appearance(eng, node(args)?, |a| Value::Bool(a.visible))
}

fn set_visible(eng: &Engine, args: &[Value]) -> Result<Value> {
    let on = flag(args, 1)?;
    with_appearance(eng, node(args)?, |a| a.visible = on)?;
    Ok(Value::Nil)
}

/// What the renderer sees: false when any ancestor is hidden.
fn global_visible(eng: &Engine, args: &[Value]) -> Result<Value> {
    let e = node(args)?;
    let world = eng.world();
    Ok(Value::Bool(scene::composed_appearance(&world, e).visible))
}

fn tint(eng: &Engine, args: &[Value]) -> Result<Value> {
    with_appearance(eng, node(args)?, |a| Value::Color(a.tint.into()))
}

/// `set_tint(node, r, g, b, a)`, the alpha optional and one when left out.
/// It multiplies into every descendant's, which is what a renderable's own
/// `color` does not do.
fn set_tint(eng: &Engine, args: &[Value]) -> Result<Value> {
    let alpha = if args.len() > 4 {
        number(args, 4)?
    } else {
        1.0
    };
    let colour = glamx::Vec4::new(number(args, 1)?, number(args, 2)?, number(args, 3)?, alpha);
    with_appearance(eng, node(args)?, |a| a.tint = colour)?;
    Ok(Value::Nil)
}

/// What the renderer sees: every ancestor's tint multiplied into this one's.
fn global_tint(eng: &Engine, args: &[Value]) -> Result<Value> {
    let e = node(args)?;
    let world = eng.world();
    Ok(Value::Color(
        scene::composed_appearance(&world, e).tint.into(),
    ))
}

fn material(eng: &Engine, args: &[Value]) -> Result<Value> {
    with_appearance(eng, node(args)?, |a| {
        Value::Str(a.material.reference().to_string())
    })
}

fn set_material(eng: &Engine, args: &[Value]) -> Result<Value> {
    let id = scene::MaterialId::intern(text(args, 1)?);
    with_appearance(eng, node(args)?, |a| a.material = id)?;
    Ok(Value::Nil)
}

/// What the renderer draws with: the nearest material from the node up.
fn global_material(eng: &Engine, args: &[Value]) -> Result<Value> {
    let e = node(args)?;
    let world = eng.world();
    Ok(Value::Str(
        scene::composed_appearance(&world, e)
            .material
            .reference()
            .to_string(),
    ))
}

fn z_index(eng: &Engine, args: &[Value]) -> Result<Value> {
    with_appearance(eng, node(args)?, |a| Value::Int(i64::from(a.z_index)))
}

/// `set_z_index(node, z)` adds to the parent's; a third argument of false
/// makes it absolute.
fn set_z_index(eng: &Engine, args: &[Value]) -> Result<Value> {
    let z = integer(args, 1)?;
    let relative = match args.get(2) {
        Some(Value::Bool(b)) => *b,
        _ => true,
    };
    with_appearance(eng, node(args)?, |a| {
        a.z_index = z;
        a.z_relative = relative;
    })?;
    Ok(Value::Nil)
}

fn global_z_index(eng: &Engine, args: &[Value]) -> Result<Value> {
    let e = node(args)?;
    let world = eng.world();
    Ok(Value::Int(i64::from(
        scene::composed_appearance(&world, e).z_index,
    )))
}

fn tags(eng: &Engine, args: &[Value]) -> Result<Value> {
    let e = node(args)?;
    let world = eng.world();
    let list = world
        .get::<&Tags>(e)
        .map(|t| t.0.iter().cloned().map(Value::Str).collect())
        .unwrap_or_default();
    Ok(Value::List(list))
}

fn has_tag(eng: &Engine, args: &[Value]) -> Result<Value> {
    let e = node(args)?;
    let tag = text(args, 1)?;
    let world = eng.world();
    Ok(Value::Bool(world.get::<&Tags>(e).is_ok_and(|t| t.has(tag))))
}

fn add_tag(eng: &Engine, args: &[Value]) -> Result<Value> {
    let e = node(args)?;
    let tag = text(args, 1)?.to_string();
    if tag.is_empty() {
        bail!("a tag needs a name");
    }
    let mut world = eng.world_mut();
    if let Ok(mut tags) = world.get::<&mut Tags>(e) {
        tags.add(&tag);
        return Ok(Value::Nil);
    }
    let mut tags = Tags::default();
    tags.add(&tag);
    world
        .insert_one(e, tags)
        .map_err(|_| anyhow!("node is dead"))?;
    Ok(Value::Nil)
}

fn remove_tag(eng: &Engine, args: &[Value]) -> Result<Value> {
    let e = node(args)?;
    let tag = text(args, 1)?;
    let world = eng.world();
    if let Ok(mut tags) = world.get::<&mut Tags>(e) {
        tags.remove(tag);
    }
    Ok(Value::Nil)
}

/// Read the node's local transform, answering identity when it has none.
///
/// A node without the `transform` component sits where its parent does, which
/// is what `propagate_transforms` already does with one, so a reader gets that
/// answer rather than an error about a component nothing said it needed.
fn read_transform<R>(eng: &Engine, e: Entity, f: impl FnOnce(&Transform) -> R) -> Result<R> {
    let world = eng.world();
    if !world.contains(e) {
        return Err(anyhow!("node is dead"));
    }
    match world.get::<&Transform>(e) {
        Ok(transform) => Ok(f(&transform)),
        Err(_) => Ok(f(&Transform::identity())),
    }
}

/// Write the node's local transform, giving it one when it has none.
///
/// Moving a node is what says it has a transform, so a script never has to add
/// the component before setting a position. The node changes archetype the once
/// -- which is why a scene file naming a transform is spawned with one.
fn with_transform<R>(eng: &Engine, e: Entity, f: impl FnOnce(&mut Transform) -> R) -> Result<R> {
    {
        let world = eng.world();
        if let Ok(mut transform) = world.get::<&mut Transform>(e) {
            return Ok(f(&mut transform));
        }
        if !world.contains(e) {
            return Err(anyhow!("node is dead"));
        }
    }
    let mut transform = Transform::identity();
    let out = f(&mut transform);
    eng.world_mut()
        .insert_one(e, transform)
        .map_err(|_| anyhow!("node is dead"))?;
    Ok(out)
}

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
    scene::rename(&eng.world(), e, text(args, 1)?);
    Ok(Value::Nil)
}

fn path(eng: &Engine, args: &[Value]) -> Result<Value> {
    let e = node(args)?;
    Ok(Value::Str(scene::node_path(&eng.world(), e)))
}

fn position(eng: &Engine, args: &[Value]) -> Result<Value> {
    read_transform(eng, node(args)?, |t| vec3(t.position))
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
    read_transform(eng, node(args)?, |t| {
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
    read_transform(eng, node(args)?, |t| {
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
    read_transform(eng, node(args)?, |t| vec3(t.scale))
}

fn set_scale(eng: &Engine, args: &[Value]) -> Result<Value> {
    let v = xyz(args, 1)?;
    with_transform(eng, node(args)?, |t| t.scale = v)?;
    Ok(Value::Nil)
}

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

fn get_node(eng: &Engine, args: &[Value]) -> Result<Value> {
    let e = node(args)?;
    let world = eng.world();
    Ok(scene::find_node(&world, e, text(args, 1)?)
        .map_or(Value::Nil, |found| Value::Node(crate::node_id_of(found).0)))
}

fn add_child(eng: &Engine, args: &[Value]) -> Result<Value> {
    let e = node(args)?;
    let id = crate::ids::mint(eng);
    let mut world = eng.world_mut();
    let child = if id.is_empty() {
        scene::spawn_node(&mut world, text(args, 1)?, e)
    } else {
        scene::spawn_node_with_id(&mut world, text(args, 1)?, e, id)
    };
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

/// `node:set_parent(other)`: move the node under another, keeping where it
/// is in the world.
fn set_parent(eng: &Engine, args: &[Value]) -> Result<Value> {
    let e = node(args)?;
    let parent = match args.get(1) {
        Some(Value::Node(id)) => crate::entity_of(balaur_script::NodeId(*id))?,
        other => return Err(anyhow!("argument 1 should be a node, got {other:?}")),
    };
    scene::reparent(&mut eng.world_mut(), e, parent)?;
    Ok(Value::Nil)
}

/// `node:sibling_index()`: where it sits among its parent's children.
fn sibling_index(eng: &Engine, args: &[Value]) -> Result<Value> {
    let e = node(args)?;
    let world = eng.world();
    let Ok(parent) = world.get::<&Parent>(e) else {
        return Ok(Value::Int(0));
    };
    let at = world
        .get::<&Children>(parent.0)
        .ok()
        .and_then(|kids| kids.0.iter().position(|&c| c == e));
    Ok(Value::Int(
        at.and_then(|at| i64::try_from(at).ok()).unwrap_or(0),
    ))
}

/// `node:set_sibling_index(i)`: move it among its siblings. Order is what a
/// container lays out in and what the digest walks, so this is a scene edit
/// rather than a view setting.
fn set_sibling_index(eng: &Engine, args: &[Value]) -> Result<Value> {
    let e = node(args)?;
    let index = match args.get(1) {
        Some(Value::Int(i)) => usize::try_from(*i).unwrap_or(0),
        Some(Value::Num(n)) => *n as usize,
        other => return Err(anyhow!("argument 1 should be an index, got {other:?}")),
    };
    let world = eng.world_mut();
    let Ok(parent) = world.get::<&Parent>(e).map(|p| p.0) else {
        return Ok(Value::Nil);
    };
    scene::move_child_to(&world, parent, e, index);
    Ok(Value::Nil)
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

/// Writes over what the component currently holds, so the properties the
/// table does not name survive.
///
/// The difference from `set_component` is the whole reason both exist:
/// describing a component whole is what a scene file means, and changing one
/// property is what a script driving it over time means.
fn patch_component(eng: &Engine, args: &[Value]) -> Result<Value> {
    let e = node(args)?;
    let params = to_toml(
        args.get(2)
            .ok_or_else(|| anyhow!("patch_component needs the properties to change"))?,
    )?;
    crate::components::patch(eng, e, text(args, 1)?, &params)?;
    Ok(Value::Nil)
}

/// `node.go(state)` — put the node in one of its `states`.
fn go_to_state(eng: &Engine, args: &[Value]) -> Result<Value> {
    crate::states::go(eng, node(args)?, text(args, 1)?)?;
    Ok(Value::Nil)
}

/// `node.state()` — the state it is in, `""` for the pose the scene gave it.
fn current_state(eng: &Engine, args: &[Value]) -> Result<Value> {
    let entity = node(args)?;
    let world = eng.world();
    Ok(Value::Str(
        world
            .get::<&crate::states::States>(entity)
            .map(|states| states.current.clone())
            .unwrap_or_default(),
    ))
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

/// The identity that survives a rename and a reparent, where a path does not.
///
/// Empty rather than nil for a node carrying none: a caller comparing ids
/// should not have to tell two kinds of absence apart.
fn stable_id(eng: &Engine, args: &[Value]) -> Result<Value> {
    let e = node(args)?;
    Ok(Value::Str(
        crate::ids::of(&eng.world(), e).unwrap_or_default(),
    ))
}

/// Every node below this one, in tree order.
///
/// `collect_subtree` is the other walk in the tree and pops its stack, so it
/// visits siblings last-first; a script reading a subtree wants the order the
/// scene declares.
fn descendants(eng: &Engine, args: &[Value]) -> Result<Value> {
    let e = node(args)?;
    let world = eng.world();
    let mut out = Vec::new();
    let mut stack = vec![e];
    while let Some(at) = stack.pop() {
        if at != e {
            out.push(Value::Node(crate::node_id_of(at).0));
        }
        if let Ok(children) = world.get::<&Children>(at) {
            // Pushed in reverse so the stack pops them in declaration order.
            for child in children.0.iter().rev() {
                stack.push(*child);
            }
        }
    }
    Ok(Value::List(out))
}

fn script_path(eng: &Engine, args: &[Value]) -> Result<Value> {
    let e = node(args)?;
    let world = eng.world();
    Ok(world
        .get::<&ScriptAttachment>(e)
        .ok()
        .map_or(Value::Nil, |a| Value::Str(a.path.clone())))
}

/// Whether the node's script declares a method, which `call` cannot say: it
/// answers nil both for a method that is not there and for one that returned
/// nothing.
fn has_method(eng: &Engine, args: &[Value]) -> Result<Value> {
    let e = node(args)?;
    let method = text(args, 1)?;
    Ok(Value::Bool(eng.script_host().is_some_and(|host| {
        host.has_method(crate::node_id_of(e), method)
    })))
}

/// `node:call("method", ...)`: one script calling another's method, with
/// the target's return value coming back. Nil when the node has no script,
/// no such method (handlers are opt-in), or the method suspended on an
/// await; the call itself runs to completion before this returns, so a
/// handler may spawn, free or call further nodes.
fn call(eng: &Engine, args: &[Value]) -> Result<Value> {
    let e = node(args)?;
    let method = text(args, 1)?;
    let host = eng
        .script_host()
        .ok_or_else(|| anyhow!("no script backend is running"))?;
    Ok(host
        .call_on(crate::node_id_of(e), method, args.get(2..).unwrap_or(&[]))
        .unwrap_or(Value::Nil))
}

/// `node.call_async`: a token woken with the method's result once it has
/// returned, on the next step at the earliest so the caller has parked.
fn call_async(eng: &Engine, args: &[Value]) -> Result<Value> {
    let e = node(args)?;
    let method = text(args, 1)?;
    let host = eng
        .script_host()
        .ok_or_else(|| anyhow!("no script backend is running"))?;
    let token = eng.next_token();
    let rest = args.get(2..).unwrap_or(&[]);
    if let Some(result) = host.call_on_async(crate::node_id_of(e), method, rest, token) {
        crate::timers::wake_next_step(eng, token, result);
    }
    Ok(Value::Int(i64::try_from(token).unwrap_or(i64::MAX)))
}

fn emit(eng: &Engine, args: &[Value]) -> Result<Value> {
    let e = node(args)?;
    let name = text(args, 1)?.to_string();
    let payload = args.get(2).cloned().unwrap_or(Value::Nil);
    crate::events::emit_from(eng, e, &name, payload);
    Ok(Value::Nil)
}

/// `node:attach_script(path, props)`: the scene's `script` key, at run time.
/// `props` is optional and holds what this node overrides of the script's
/// exported defaults, so a spawned node is tuned the way an authored one is.
fn attach_script(eng: &Engine, args: &[Value]) -> Result<Value> {
    let e = node(args)?;
    let props = match args.get(2) {
        None | Some(Value::Nil) => Vec::new(),
        Some(Value::Map(fields)) => fields.clone(),
        Some(other) => bail!(
            "attach_script props must be a table, got {}",
            other.type_name()
        ),
    };
    let host = eng
        .script_host()
        .ok_or_else(|| anyhow!("no script backend is running"))?;
    scene::remember_script_props(eng, e, &props);
    host.attach_with_props(crate::node_id_of(e), text(args, 1)?, &props)?;
    Ok(Value::Nil)
}

/// `node:detach_script()`: drop the instance, keeping the node.
///
/// A tool that tears a running scene down needs the scripts to stop before
/// the world does: an instance whose next `update` runs against a world its
/// own components have left throws where nothing is wrong.
fn detach_script(eng: &Engine, args: &[Value]) -> Result<Value> {
    let e = node(args)?;
    let host = eng
        .script_host()
        .ok_or_else(|| anyhow!("no script backend is running"))?;
    host.detach(crate::node_id_of(e));
    Ok(Value::Nil)
}

fn queue_free(eng: &Engine, args: &[Value]) -> Result<Value> {
    eng.push_command(Command::Free(node(args)?));
    Ok(Value::Nil)
}

/// One property a script declared, against the component schema vocabulary.
///
/// Here rather than in `components` because the spec arrives as a script
/// value, and this is the module that converts one: a backend asking whether
/// an `exports()` entry is well formed needs no TOML of its own.
///
/// # Errors
/// The reason, for a caller that prefixes the script and the property.
pub fn validate_property_spec(spec: &Value) -> std::result::Result<(), String> {
    let table = to_toml(spec).map_err(|e| e.to_string())?;
    crate::components::validate_property(&table)
}

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
            return Err(anyhow!("a node or callback is not component data"));
        }
        Value::Many(_) => return Err(anyhow!("several values are not component data")),
        // TOML has no byte string, and a component that wanted one would be
        // asking for an asset reference instead.
        Value::Bytes(_) => return Err(anyhow!("bytes are not component data")),
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
