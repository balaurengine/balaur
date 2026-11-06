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
use balaur_script::{Module, Value};
use glamx::{EulerRot, Vec3};
use hecs::Entity;

use crate::engine::Engine;
use crate::scene::{self, Appearance, Children, GlobalTransform, Name, NodeScript, Parent, Tags};

mod scripts;
use scripts::{
    attach_script, authored, call, call_async, detach_script, emit, free, has_method, instance,
    overrides, queue_free, script_field, script_path, script_props, set_script,
};

/// One node operation: what a script calls, what the reference says of it, and
/// what runs, as a plain function pointer so the list stays a `const`.
pub struct NodeOp {
    pub name: &'static str,
    /// The components it drives: a script calls it on the first,
    /// `node.transform.translate(..)`.
    pub acts_on: &'static [&'static str],
    /// What a script passes after the node, and what it gets back:
    /// `"(name: string) -> node"`.
    pub signature: &'static str,
    pub doc: &'static str,
    pub call: fn(&Engine, &[Value]) -> Result<Value>,
}

const fn op(
    name: &'static str,
    acts_on: &'static [&'static str],
    signature: &'static str,
    doc: &'static str,
    call: fn(&Engine, &[Value]) -> Result<Value>,
) -> NodeOp {
    NodeOp {
        name,
        acts_on,
        signature,
        doc,
        call,
    }
}

/// Every node operation, in one list.
#[rustfmt::skip]
pub const NODE_OPS: &[NodeOp] = &[
    op("is_valid", &[], "() -> bool", "Whether the node is still in the world; false rather than an error when the value is not a node.", is_valid),
    op("name", &[], "() -> string", "The node's own name, empty when it carries none.", name),
    op("set_name", &[], "(name: string)", "Rename the node, doing nothing when it carries no name.", set_name),
    op("path", &[], "() -> string", "The node's slash-separated path, built by climbing parents from the node up to the root.", path),
    op("global_position", &["transform"], "() -> vec3", "The node's position in world space, as of the last transform sync.", global_position),
    op("global_rotation_euler", &["transform"], "() -> vec3", "The node's world rotation as euler angles in radians, as of the last transform sync.", global_rotation_euler),
    op("global_scale", &["transform"], "() -> vec3", "The node's scale in world space, as of the last transform sync.", global_scale),
    op("get_node", &[], "(path: string) -> node", "The node at an `A/B/C` path relative to this one, `..` climbing to the parent and a leading `/` starting from the root; nil when nothing matches.", get_node),
    op("add_child", &[], "(name: string) -> node", "Create a named child node under this one and return it.", add_child),
    op("parent", &[], "() -> node", "The node's parent, nil at the root.", parent),
    op("children", &[], "() -> [node]", "The node's direct children, an empty list when it has none.", children),
    op("set_parent", &[], "(parent: node)", "Move the node under another, keeping where it is in the world; an error for a cycle or a dead parent.", set_parent),
    op("sibling_index", &[], "() -> int", "Where the node sits among its parent's children, counting from zero; 0 at the root.", sibling_index),
    op("set_sibling_index", &[], "(index: int)", "Move the node to that place among its siblings, clamped to the end. Order is draw order in a `row` or a `column`, and tree order in the digest.", set_sibling_index),
    op("set_component", &[], "(component: string, params: any)", "Give the node the named component, built from the given table over the component's schema defaults. Every property the table leaves out goes back to its default; `patch_component` is the one that changes a property and leaves the rest. A script hears every write to the scene, this one and a node added, freed, renamed or moved, with `events::listen(this.node, \"changed\")`, as `on_changed(entries)` at the top of the next frame.", set_component),
    op("set_property", &[], "(component: string, property: string, value: any) -> bool", "Write one property, by its place in the component: what `node.component.property` writes, for a name known only at run time. Answers whether the component took the value, as `patch_component` does.", set_property),
    op("patch_component", &[], "(component: string, params: any) -> bool", "Change the properties the table names and leave the rest of the component where they were. On a node without the component this adds it, the schema defaults being what it currently holds. Answers whether the component took the values: one it refuses goes to the log with its reason, and the script goes on. A component or property nothing has, or a node that is gone, is an error.", patch_component),
    op("set_state", &["states"], "(state: string)", "Put the node in one of its `states`: the state's table is patched over the components it names, and `on_state_changed(from, to)` follows. A node already in that state is left alone.", go_to_state),
    op("state", &["states"], "() -> string", "The state the node is in, or \"\" for the pose the scene gave it.", current_state),
    op("remove_component", &[], "(component: string)", "Take the named component off the node.", remove_component),
    op("get_component", &[], "(component: string) -> any", "The named component's properties as a table, nil when the node does not carry it.", get_component),
    op("component_property", &[], "(component: string, property: string) -> any", "One property of the named component, nil when the node does not carry it: what a caller reading a single value should ask for.", component_property),
    op("has_component", &[], "(component: string) -> bool", "Whether the node carries the named component.", has_component),
    op("component_names", &[], "() -> [string]", "The names of every component on the node.", component_names),
    op("stable_id", &[], "() -> string", "The node's stable id: what a scene file declared, or what it was given when it was spawned. Survives rename and reparent, which a path does not.", stable_id),
    op("descendants", &[], "() -> [node]", "Every node under this one, in tree order, the node itself excluded.", descendants),
    op("script_path", &[], "() -> string", "The path of the script the node names, nil when it names none; named whether or not it runs.", script_path),
    op("overrides", &[], "() -> any", "What the node changes of what the prefab that built it asks: a table by component of the properties it sets differently, and its script's under `script`. Empty for a node no prefab built.", overrides),
    op("script_props", &[], "() -> any", "What the node sets over its script's exported defaults, as a table; empty when it sets nothing.", script_props),
    op("set_script", &[], "(path: string?, props: any)", "Name the node's script and what it sets, without running it: what a scene's `script` key says and `attach_script` also starts. Nil takes the script off.", set_script),
    op("instance", &[], "() -> string?", "The prefab file the node instances, nil when it instances none.", instance),
    op("has_method", &[], "(method: string) -> bool", "Whether the node's script declares this method, so a caller can tell \"no handler\" from \"a handler that answered nothing\".", has_method),
    op("script_field", &[], "(name: string) -> any", "A member the node's script instance holds, read by another script: GDScript's `node.speed`. Nil for no script or no such member.", script_field),
    op("call", &[], "(method: string, arg1: any, arg2: any, arg3: any, arg4: any) -> any", "Call a method on the node's script and return what it gives back; nil when there is no such script or method.", call),
    op("call_async", &[], "(method: string, arg1: any, arg2: any, arg3: any, arg4: any) -> int", "Call a method that may suspend, and get a token `task.wait` resumes with its result once it returns: `task::wait(door.call_async(\"open\")).await`, a GDScript `await door.open()`.", call_async),
    op("emit", &[], "(name: string, payload: any)", "Emit an event from this node, delivered at the top of the next frame to whoever subscribed to `name` on this node, and to whoever subscribed to `name` from anyone. `call` is the twin that reaches one known script, now.", emit),
    op("attach_script", &[], "(path: string, props: any)", "Attach the script at a path, with an optional table overriding what the script exports.", attach_script),
    op("detach_script", &[], "()", "Drop the script instance on this node, so no further lifecycle call reaches it; the node and its components stay.", detach_script),
    op("queue_free", &[], "()", "Destroy the node and its subtree at the end of the frame.", queue_free),
    op("free", &[], "()", "Destroy the node and its subtree now, scripts detached first: what a tool editing a scene does, where no script of the subtree is running. A game frees with `queue_free`.", free),
    op("authored", &[], "(component: string) -> any", "The component as a save writes it: every property it holds, an inline asset as its definition, a colour as `#rrggbb` where it is one, and no read-only property. Nil when the node does not carry it.", authored),
    op("visible", &[], "() -> bool", "Whether the node itself is set to draw; an ancestor may still hide it.", visible),
    op("set_visible", &[], "(on: bool)", "Show or hide the node and everything under it, emitting `visibility_changed` with the new value when the flag moves. Physics is untouched: a hidden collider still collides.", set_visible),
    op("visible_in_tree", &[], "() -> bool", "What the renderer sees: false when the node or any ancestor is hidden.", visible_in_tree),
    op("tint", &[], "() -> color", "The node's own tint as r, g, b, a channel floats; an ancestor's multiplies into it on the way to the screen.", tint),
    op("set_tint", &[], "(r: float, g: float, b: float, a: float)", "Multiply a colour into everything the node and its subtree draw, alpha included, one meaning untinted. A renderable's own `color` is the node's alone; this is the one that inherits.", set_tint),
    op("effective_tint", &[], "() -> color", "What the renderer multiplies by: this node's tint with every ancestor's folded in.", effective_tint),
    op("material", &[], "() -> string", "The `material` asset the node names itself, empty when it takes its parent's.", material),
    op("effective_material", &[], "() -> string", "The material the node draws with: its own, or the nearest ancestor's. Empty is the built-in one.", effective_material),
    op("z_index", &[], "() -> int", "The node's own draw layer, added to its parent's unless set absolute.", z_index),
    op("set_z_index", &[], "(z: int, relative: bool?)", "Put the node and its subtree on a draw layer: higher draws later. It adds to the parent's layer unless `relative` is false, which makes it absolute.", set_z_index),
    op("effective_z_index", &[], "() -> int", "The layer the node actually draws on, with every ancestor's added in.", effective_z_index),
    op(crate::process::KEY, &[], "() -> string", "When this node ticks: \"inherit\", \"pausable\", \"always\" or \"disabled\". \"inherit\" is the default and takes the nearest ancestor's answer.", process),
    op("set_process", &[], "(mode: string)", "Set when the node and its subtree tick. \"always\" runs through a pause, which is what a pause menu is; \"disabled\" never runs; \"inherit\" goes back to the parent's. Physics is one world and is held whole by a pause whatever this says.", set_process),
    op("ticking", &[], "() -> bool", "Whether the node ticks this frame, its process mode and the game's pause together.", ticking),
    op("reset_interpolation", &[], "()", "Throw away the poses the node was blending and start again from where it is, so a teleport does not streak across the level. `physics3d.teleport` and `physics2d.teleport` call it for you.", reset_interpolation),
    op("tags", &[], "() -> [string]", "The names the node is filed under, sorted.", tags),
    op("has_tag", &[], "(tag: string) -> bool", "Whether the node is filed under a name.", has_tag),
    op("add_tag", &[], "(tag: string)", "File the node under a name; `scene.tagged` finds it from then on.", add_tag),
    op("remove_tag", &[], "(tag: string)", "Take a name off the node; a name it never had is left alone.", remove_tag),
];

/// Register every node operation into a binding group as a free function.
///
/// A backend that gives its node handle method syntax walks `NODE_OPS`
/// itself instead; this is the plain path.
pub fn install_node_api(m: &mut Module<Engine>) {
    m.module_doc(
        "What every node has: name, path, transform, children, components and script. Each function takes the node first; scripts call it as a method, `self.node.get_node(\"Arm\")`.",
    );
    for op in NODE_OPS {
        m.describe(&[(op.name, op.acts_on, op.signature, op.doc)]);
        m.function_raw(op.name, Box::new(op.call));
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
    let e = node(args)?;
    crate::changes::node_keys(eng, e);
    if !crate::scene::set_visible(eng, e, on) {
        bail!("node is dead");
    }
    Ok(Value::Nil)
}

/// What a node emits when its own `visible` flips, carrying the new value.
/// An ancestor hiding it does not: the flag this reports is the node's own.
pub const VISIBILITY_EVENT: &str = "visibility_changed";
/// What a parent announces when a node is added under it, carrying the child.
pub const CHILD_ADDED_EVENT: &str = "child_added";
/// What a parent announces as a child leaves it, freed or moved: a freed
/// child is still readable in the parent's own hook, gone by the next pump.
pub const CHILD_REMOVED_EVENT: &str = "child_removed";
/// What a node announces when its name changes, carrying the name it had.
pub const RENAMED_EVENT: &str = "renamed";
/// What a node announces when it moves under another parent, carrying the
/// parent it left.
pub const REPARENTED_EVENT: &str = "reparented";

/// What every node announces, whatever its components, with its payload.
pub const NODE_EVENTS: &[(&str, &str)] = &[
    (VISIBILITY_EVENT, "its own `visible`, now"),
    (CHILD_ADDED_EVENT, "the child"),
    (
        CHILD_REMOVED_EVENT,
        "the child; freed, it is still readable in this node's own hook",
    ),
    (RENAMED_EVENT, "the name it had; `name()` says the new one"),
    (
        REPARENTED_EVENT,
        "the parent it left; `parent()` says the new one",
    ),
];

/// What the renderer sees: false when any ancestor is hidden.
fn visible_in_tree(eng: &Engine, args: &[Value]) -> Result<Value> {
    let e = node(args)?;
    let world = eng.world();
    Ok(Value::Bool(scene::composed_appearance(&world, e).visible))
}

fn process(eng: &Engine, args: &[Value]) -> Result<Value> {
    let e = node(args)?;
    let mode = crate::process::own(&eng.world(), e);
    Ok(Value::Str(mode.name().to_string()))
}

fn set_process(eng: &Engine, args: &[Value]) -> Result<Value> {
    let named = text(args, 1)?;
    let mode = crate::process::ProcessMode::parse(named)
        .ok_or_else(|| anyhow!("'{named}' is not a process mode"))?;
    let e = node(args)?;
    crate::changes::node_keys(eng, e);
    crate::process::set(&mut eng.world_mut(), e, mode);
    Ok(Value::Nil)
}

fn ticking(eng: &Engine, args: &[Value]) -> Result<Value> {
    let e = node(args)?;
    Ok(Value::Bool(crate::process::ticking(eng, e)))
}

fn reset_interpolation(eng: &Engine, args: &[Value]) -> Result<Value> {
    let e = node(args)?;
    crate::interpolate::reset(eng, e);
    Ok(Value::Nil)
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
    let e = node(args)?;
    crate::changes::node_keys(eng, e);
    with_appearance(eng, e, |a| a.tint = colour)?;
    Ok(Value::Nil)
}

/// What the renderer sees: every ancestor's tint multiplied into this one's.
fn effective_tint(eng: &Engine, args: &[Value]) -> Result<Value> {
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

/// What the renderer draws with: the nearest material from the node up.
fn effective_material(eng: &Engine, args: &[Value]) -> Result<Value> {
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

/// `set_z_index(node, z)` adds to the parent's; `relative` false makes it
/// absolute.
fn set_z_index(eng: &Engine, args: &[Value]) -> Result<Value> {
    let z = integer(args, 1)?;
    let relative = !matches!(args.get(2), Some(Value::Bool(false)));
    let e = node(args)?;
    crate::changes::node_keys(eng, e);
    with_appearance(eng, e, |a| {
        a.z_index = z;
        a.z_as_relative = relative;
    })?;
    Ok(Value::Nil)
}

fn effective_z_index(eng: &Engine, args: &[Value]) -> Result<Value> {
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
        .map(|t| t.0.iter().cloned().map(Value::text).collect())
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
    crate::changes::node_keys(eng, e);
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
    crate::changes::node_keys(eng, e);
    let world = eng.world();
    if let Ok(mut tags) = world.get::<&mut Tags>(e) {
        tags.remove(tag);
    }
    Ok(Value::Nil)
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
    let name = text(args, 1)?;
    let Ok(was) = eng.world().get::<&Name>(e).map(|n| n.0.clone()) else {
        bail!("node is dead");
    };
    scene::rename(&eng.world(), e, name);
    if was != name {
        crate::changes::renamed(eng, e, was);
    }
    Ok(Value::Nil)
}

fn path(eng: &Engine, args: &[Value]) -> Result<Value> {
    let e = node(args)?;
    Ok(Value::Str(scene::node_path(&eng.world(), e)))
}

/// The same rotation as `rotation_euler`, in degrees.
///
/// Radians are the engine's unit and stay the default; degrees are what a
/// person authors, so the pair exists rather than every caller carrying its
/// own `math.to_degrees` conversion the way the editor's inspector used to.
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
    crate::parked::refuse_if_parked(eng, e, "node.add_child")?;
    let mut world = eng.world_mut();
    let name = text(args, 1)?.to_string();
    let child = scene::spawn_node(&mut world, &name, e);
    drop(world);
    // A game that adds a node is a game whose world changed, which is what a
    // session timeline is for. Scene loading does not come through here.
    crate::replay::event(
        eng,
        "node.add_child",
        format!("added {name}"),
        Some(serde_json::json!({ "name": name })),
    );
    crate::changes::added(eng, e, child);
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
    crate::parked::refuse_if_parked(eng, e, "node.set_parent")?;
    crate::parked::refuse_if_parked(eng, parent, "node.set_parent")?;
    let stood = crate::changes::Stood::of(&eng.world(), e);
    let placed = crate::components::has(eng, e, crate::transform::COMPONENT);
    scene::reparent(&mut eng.world_mut(), e, parent)?;
    // Keeping the world pose gave a node that had none a transform to hold
    // it in, which the registry has to hear of.
    if !placed && let Some(held) = crate::components::get(eng, e, crate::transform::COMPONENT) {
        crate::components::add(eng, e, crate::transform::COMPONENT, Some(&held))?;
    }
    if stood.as_ref().map(|stood| stood.parent) != Some(parent) {
        crate::changes::moved(eng, e, stood.as_ref(), parent);
    }
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
    let Some(stood) = crate::changes::Stood::of(&eng.world(), e) else {
        return Ok(Value::Nil);
    };
    scene::move_child_to(&eng.world_mut(), stood.parent, e, index);
    if crate::changes::Stood::of(&eng.world(), e).is_some_and(|now| now.place != stood.place) {
        crate::changes::reordered(eng, e, &stood);
    }
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

/// Write one property of the component at `index`, for a backend that has
/// already resolved both the component and the key.
///
/// The conversion to TOML is core's, so a backend needs no opinion about the
/// scene format to drive one property over time.
///
/// # Errors
/// When the node is gone, the value is not one a component can hold, or the
/// component's `apply` refuses it.
pub fn set_property_at(
    eng: &Engine,
    entity: Entity,
    index: usize,
    key: &str,
    value: &Value,
) -> Result<()> {
    crate::components::set_property_at(eng, entity, index, key, &to_toml(value)?)
}

/// Writes over what the component currently holds, so the properties the
/// table does not name survive.
///
/// The difference from `set_component` is the whole reason both exist:
/// describing a component whole is what a scene file means, and changing one
/// property is what a script driving it over time means.
///
/// A value the component refuses answers `false` rather than ending the
/// script's call, which nothing could catch: an inspector writing what a reader
/// typed expects them. A name nothing has is a typo, and ends the call.
fn patch_component(eng: &Engine, args: &[Value]) -> Result<Value> {
    let e = node(args)?;
    let params = to_toml(
        args.get(2)
            .ok_or_else(|| anyhow!("patch_component needs the properties to change"))?,
    )?;
    let name = text(args, 1)?;
    let Err(why) = patched(eng, e, name, &params) else {
        return Ok(Value::Bool(true));
    };
    if names_nothing(eng, e, name, &params) {
        return Err(why);
    }
    let node = scene::node_path(&eng.world(), e);
    tracing::warn!(node, "write refused: {why:#}");
    Ok(Value::Bool(false))
}

/// One property by name, through its place: `patch_component` for one key,
/// with no table made for it.
fn set_property(eng: &Engine, args: &[Value]) -> Result<Value> {
    let e = node(args)?;
    let name = text(args, 1)?;
    let key = text(args, 2)?;
    let value = args
        .get(3)
        .ok_or_else(|| anyhow!("set_property needs the value"))?;
    let written = match crate::components::place_of(eng, name, key) {
        Some(place) => write_placed(eng, e, &place, value),
        None => crate::components::set_property(eng, e, name, key, &to_toml(value)?),
    };
    let Err(why) = written else {
        return Ok(Value::Bool(true));
    };
    let params = toml::Value::Table(toml::map::Map::from_iter([(
        key.to_string(),
        to_toml(value)?,
    )]));
    if names_nothing(eng, e, name, &params) {
        return Err(why);
    }
    let node = scene::node_path(&eng.world(), e);
    tracing::warn!(node, "write refused: {why:#}");
    Ok(Value::Bool(false))
}

/// A script's value written at a place: a value is built only for a kind
/// [`Given`] does not hold.
fn write_placed(
    eng: &Engine,
    e: Entity,
    place: &crate::components::PropertyPlace,
    value: &Value,
) -> Result<()> {
    use crate::components::derived::Given;
    let numbers = |held: &[f32]| {
        let mut out = [0.0; 4];
        let count = held.len().min(4);
        out[..count].copy_from_slice(&held[..count]);
        Given::Numbers(out, count)
    };
    let given = match value {
        Value::Num(number) => Given::Float(*number),
        Value::Int(number) => Given::Int(*number),
        Value::Bool(flag) => Given::Bool(*flag),
        Value::Str(text) => Given::Text(text),
        Value::Vec2(held) => numbers(held),
        Value::Vec3(held) => numbers(held),
        Value::Color(held) => numbers(held),
        other => {
            let built = to_toml(other)?;
            return crate::components::set_property_placed(eng, e, place, Given::Value(&built));
        }
    };
    crate::components::set_property_placed(eng, e, place, given)
}

/// Whether a write that failed named a node that is gone, or a component or
/// property nothing has, rather than a value the component would not take.
/// Asked only of a failure, so a write that lands checks its names once.
fn names_nothing(eng: &Engine, e: Entity, name: &str, params: &toml::Value) -> bool {
    !eng.world().contains(e)
        || !crate::components::is_registered(eng, name)
        || crate::components::refuse_unknown_keys(eng, name, Some(params)).is_err()
}

fn patched(eng: &Engine, e: Entity, name: &str, params: &toml::Value) -> Result<()> {
    // One property is what a script driving a value over time writes, and
    // `set_property` is the path that does not read the table back for it.
    if let Some(table) = params.as_table()
        && table.len() == 1
        && let Some((key, value)) = table.iter().next()
    {
        return crate::components::set_property(eng, e, name, key, value);
    }
    crate::components::patch(eng, e, name, params)
}

/// `node.states.set_state(state)` — put the node in one of its `states`.
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

// One property: a control reading what it now holds asked for the whole
// table, and building that was a twelfth of the editor's frame.
fn component_property(eng: &Engine, args: &[Value]) -> Result<Value> {
    let e = node(args)?;
    crate::components::property(eng, e, text(args, 1)?, text(args, 2)?)
        .as_ref()
        .map_or(Ok(Value::Nil), from_toml)
}

fn has_component(eng: &Engine, args: &[Value]) -> Result<Value> {
    let e = node(args)?;
    Ok(Value::Bool(crate::components::has(eng, e, text(args, 1)?)))
}

fn component_names(eng: &Engine, args: &[Value]) -> Result<Value> {
    let e = node(args)?;
    Ok(Value::List(
        crate::components::present_on(eng, e)
            .into_iter()
            .map(Value::text)
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

/// One property a script declared, against the component schema vocabulary,
/// and back with every nested `default` filled in.
///
/// Here rather than in `components` because the spec arrives as a script
/// value, and this is the module that converts one: a backend asking whether
/// an `exports()` entry is well formed needs no TOML of its own.
///
/// # Errors
/// The reason, for a caller that prefixes the script and the property.
pub fn checked_property_spec(spec: &Value) -> std::result::Result<Value, String> {
    let mut table = to_toml(spec).map_err(|e| e.to_string())?;
    crate::components::validate_property(&table)?;
    crate::components::complete_property(&mut table);
    from_toml(&table).map_err(|e| e.to_string())
}

/// One value against the spec that governs it, for a caller holding a list's
/// entries to what its first one said.
///
/// # Errors
/// The reason the value is not what the spec declares.
pub fn check_property_value(spec: &Value, value: &Value) -> std::result::Result<(), String> {
    let spec = to_toml(spec).map_err(|e| e.to_string())?;
    let value = to_toml(value).map_err(|e| e.to_string())?;
    crate::components::validate_value(&spec, &value)
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
        Value::Node(_) => {
            return Err(anyhow!("a node is not component data"));
        }
        Value::Many(_) => return Err(anyhow!("several values are not component data")),
        // TOML has no byte string, and a component that wanted one would be
        // asking for an asset reference instead.
        Value::Bytes(_) => return Err(anyhow!("bytes are not component data")),
        Value::Vec2(a) => number_list(a),
        Value::Vec3(a) => number_list(a),
        Value::Color(a) => number_list(a),
        Value::Transform2d(a) => number_list(a),
        Value::Transform3d(a) => number_list(a),
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
                .map(|(k, val)| Ok((k.as_str().into(), from_toml(val)?)))
                .collect::<Result<_>>()?,
        ),
    })
}
