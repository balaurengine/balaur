//! Component handles: `node.body2d.apply_impulse(x, y)`.
//!
//! A handle pairs a node with a component name. Its methods are the functions
//! of the modules that declared they drive that component, with the node
//! argument bound, so `node.body2d.apply_impulse(x, y)` is
//! `node.body2d.apply_impulse(x, y)`. Dispatch is by component name at
//! call time: the same `apply_impulse` on a `body3d` handle reaches
//! `physics3d`. `get`, `set`, `has` and `remove` come from the node's own
//! component operations with the name filled in.
//!
//! Schema properties are fields on the same handle:
//! `node.collider3d.density = 15` patches that one property and
//! `node.collider3d.density` reads it back, so a script names a property the
//! way a scene file does instead of building a table for one number.

use std::cell::RefCell;
use std::collections::HashSet;

use rustc_hash::FxHashMap;

use balaur_core::Engine;
use balaur_core::node_api::NODE_OPS;
use balaur_script::Value as Neutral;
use rune::runtime::{InstAddress, Memory, Output, Protocol, VmError, VmResult};

use super::{Node, from_neutral, to_neutral};
use crate::bindings::{CallbackScope, bound_handle, call_bound, hold_node_fn, with_engine};
use crate::handles::{self, GENERIC, is_identifier};

/// A component on a node, as scripts see it.
#[derive(rune::Any, Clone)]
#[rune(item = ::balaur)]
pub struct Component {
    pub(crate) node: u64,
    /// Interned by [`intern`], so a handle carries a borrow rather than a
    /// string it would copy. Kept for the error messages and the method
    /// path, which still dispatch on the name.
    pub(crate) name: &'static str,
    /// Where the component sits in the registry. Resolved once, when the
    /// handle's field was installed, so reading a property costs neither a
    /// hash of the name nor a `String` to carry it.
    pub(crate) index: u32,
}

thread_local! {
    /// Rune keeps item names for the life of the context, so each name is
    /// leaked once and reused across context builds.
    static NAMES: RefCell<HashSet<&'static str>> = RefCell::new(HashSet::new());
}

pub(crate) fn intern(name: &str) -> &'static str {
    NAMES.with_borrow_mut(|names| {
        if let Some(&existing) = names.get(name) {
            return existing;
        }
        let leaked: &'static str = Box::leak(name.to_string().into_boxed_str());
        names.insert(leaked);
        leaked
    })
}

/// Give `Node` a field per registered component, and the handle its methods.
pub(crate) fn install(m: &mut rune::Module, eng: &Engine) -> Result<(), rune::ContextError> {
    m.ty::<Component>()?;

    for (component, _) in balaur_core::components::schemas(eng) {
        if !is_identifier(&component) {
            tracing::warn!("component `{component}` is not a script identifier; no handle");
            continue;
        }
        let name = intern(&component);
        let Some(index) = balaur_core::components::index_of(eng, &component) else {
            continue;
        };
        let index = index as u32;
        m.field_function(&Protocol::GET, name, move |node: &Node| Component {
            node: node.id,
            name,
            index,
        })?;
        // `node.meta = #{ ... }` describes the component whole, the scene
        // file's spelling; the handle's fields and index are the sparse one.
        let Some(set) = node_op("set_component") else {
            continue;
        };
        let set = hold_node_fn(eng.clone(), set);
        m.field_function(
            &Protocol::SET,
            name,
            move |node: &Node, value: rune::Value| set_whole(*node, name, set, &value),
        )?;
    }

    for op in GENERIC {
        let Some(declared) = NODE_OPS.iter().find(|d| d.name == op.node_op) else {
            continue;
        };
        let handle = hold_node_fn(eng.clone(), declared.call);
        m.raw_function(op.method, generic_handler(handle))
            .build_associated::<Component>()?;
    }

    // The drives table, with each module's function resolved to the bound
    // handle that calls it.
    for (method, modules) in handles::drives() {
        let targets: FxHashMap<String, usize> = modules
            .into_iter()
            .filter_map(|(component, module)| Some((component, bound_handle(&module, &method)?)))
            .collect();
        if targets.is_empty() {
            continue;
        }
        let name = intern(&method);
        m.raw_function(name, method_handler(name, targets))
            .build_associated::<Component>()?;
    }
    property_fields(m, eng)?;
    index_access(m, eng)
}

/// `node.meta["fade"]` and `node.meta["fade"] = 0.3`: a property named at run
/// time rather than by the schema, which is the only way to reach a
/// schema-less component's keys. A key the component does not hold is nil.
fn index_access(m: &mut rune::Module, eng: &Engine) -> Result<(), rune::ContextError> {
    let (Some(read), Some(write)) = (node_op("get_component"), node_op("patch_component")) else {
        return Ok(());
    };
    let read = hold_node_fn(eng.clone(), read);
    let write = hold_node_fn(eng.clone(), write);
    // The key is borrowed, not taken: a `String` parameter moves the caller's
    // local out of its slot, so `meta[key] = a` left `key` unreadable.
    m.associated_function(&Protocol::INDEX_GET, move |this: &Component, key: &str| {
        read_key(this, key, read)
    })?;
    m.associated_function(
        &Protocol::INDEX_SET,
        move |this: &Component, key: &str, value: rune::Value| write_key(this, key, write, &value),
    )?;
    Ok(())
}

fn read_key(this: &Component, key: &str, handle: usize) -> VmResult<rune::Value> {
    let _scope = CallbackScope::enter();
    let got = match call_bound(handle, &receiver(this)) {
        Some(Ok(v)) => v,
        Some(Err(err)) => return fail(err),
        None => return fail("component index was registered on another thread"),
    };
    let Neutral::Map(props) = got else {
        return match rune::to_value(()) {
            Ok(nil) => VmResult::Ok(nil),
            Err(err) => fail(err),
        };
    };
    let found = props
        .into_iter()
        .find(|(name, _)| name == key)
        .map_or(Neutral::Nil, |(_, value)| value);
    match from_neutral(&found) {
        Ok(v) => VmResult::Ok(v),
        Err(err) => fail(err),
    }
}

fn write_key(this: &Component, key: &str, handle: usize, value: &rune::Value) -> VmResult<()> {
    let _scope = CallbackScope::enter();
    let value = match to_neutral(value) {
        Ok(v) => v,
        Err(err) => return fail(err),
    };
    let [node, name] = receiver(this);
    let args = [node, name, Neutral::Map(vec![(key.to_string(), value)])];
    match call_bound(handle, &args) {
        Some(Ok(_)) => VmResult::Ok(()),
        Some(Err(err)) => fail(err),
        None => fail("component index was registered on another thread"),
    }
}

/// `node.<component> = table`: the whole component, as a scene key writes it.
fn set_whole(node: Node, name: &'static str, handle: usize, value: &rune::Value) -> VmResult<()> {
    let _scope = CallbackScope::enter();
    let value = match to_neutral(value) {
        Ok(v) => v,
        Err(err) => return fail(err),
    };
    let args = [
        Neutral::Node(node.id),
        Neutral::Str(name.to_string()),
        value,
    ];
    match call_bound(handle, &args) {
        Some(Ok(_)) => VmResult::Ok(()),
        Some(Err(err)) => fail(err),
        None => fail("component assignment was registered on another thread"),
    }
}

/// Give the handle a field per schema property, over every component that
/// declares one of that name.
///
/// Reading goes through `get_component`, so a property backed by live state
/// answers what the simulation holds rather than what the scene wrote;
/// assigning goes through `patch_component`, so one property moves and the
/// rest of the component stays where it was.
fn property_fields(m: &mut rune::Module, eng: &Engine) -> Result<(), rune::ContextError> {
    let Some(read) = node_op("get_component") else {
        return Ok(());
    };
    // Held for its engine alone: a property read calls the registry straight,
    // so nothing about it crosses the seam as a value.
    let held = hold_node_fn(eng.clone(), read);
    let handles::Properties {
        owners,
        mut vectors,
        defaults,
    } = handles::properties(eng);
    let index_of = |component: &str| balaur_core::components::index_of(eng, component);
    // A property belongs to a set of components, and a handle knows which one
    // it is by number. So the set is a bitmask and the answer for a node that
    // carries none of them is a table indexed the same way: dispatch is a
    // shift and a test, not a hash of the component's name.
    let mask = |names: &HashSet<String>| {
        names
            .iter()
            .filter_map(|c| index_of(c))
            .fold(0u128, |bits, i| bits | (1u128 << i))
    };
    let slots = balaur_core::components::names(eng).len();
    for (prop, components) in owners {
        let name = intern(&prop);
        let owned = mask(&components);
        let as_vector = mask(&vectors.remove(&prop).unwrap_or_default());
        let mut fallback: Vec<Option<Neutral>> = vec![None; slots];
        for component in &components {
            if let Some(i) = index_of(component)
                && let Some(value) = defaults.get(&(component.clone(), prop.clone()))
            {
                fallback[i] = Some(value.clone());
            }
        }
        m.field_function(&Protocol::GET, name, move |this: &Component| {
            read_property(this, name, owned, as_vector, &fallback, held)
        })?;
        m.field_function(
            &Protocol::SET,
            name,
            move |this: &Component, value: rune::Value| {
                write_property(this, name, owned, held, &value)
            },
        )?;
    }
    Ok(())
}

/// One of the node's own component operations, as `NODE_OPS` stores it.
type NodeOp = fn(&Engine, &[Neutral]) -> anyhow::Result<Neutral>;

fn node_op(name: &str) -> Option<NodeOp> {
    NODE_OPS.iter().find(|d| d.name == name).map(|d| d.call)
}

/// The node and the component name every property call opens with.
fn receiver(this: &Component) -> [Neutral; 2] {
    [
        Neutral::Node(this.node),
        Neutral::Str(this.name.to_string()),
    ]
}

fn read_property(
    this: &Component,
    prop: &'static str,
    owners: u128,
    vectors: u128,
    fallback: &[Option<Neutral>],
    held: usize,
) -> VmResult<rune::Value> {
    let bit = 1u128 << this.index;
    if owners & bit == 0 {
        return fail(format!("`{}` has no property `{prop}`", this.name));
    }
    let _scope = CallbackScope::enter();
    let entity = match balaur_core::entity_of(balaur_script::NodeId(this.node)) {
        Ok(entity) => entity,
        Err(err) => return fail(err.to_string()),
    };
    let index = this.index as usize;
    // Straight to the registry with the number the handle already holds. The
    // seam is what a script declares against; this is the backend's own sugar
    // over operations core declared, as `NODE_OPS` above it is.
    let Some(found) = with_engine(held, |eng| {
        balaur_core::components::property_at(eng, entity, index, prop)
            .as_ref()
            .and_then(|value| balaur_core::node_api::from_toml(value).ok())
    }) else {
        return fail("component property was registered on another thread");
    };
    let value = match found {
        Some(value) => value,
        // No such property on this node: the component is absent, and a scene
        // leaving one out means its declared defaults.
        None => match fallback.get(index).and_then(Option::as_ref) {
            Some(value) => value.clone(),
            None => return fail(format!("the node has no `{}`", this.name)),
        },
    };
    let value = if vectors & bit != 0 {
        as_vec3(value)
    } else {
        value
    };
    match from_neutral(&value) {
        Ok(v) => VmResult::Ok(v),
        Err(err) => fail(err),
    }
}

/// Three numbers as the vector a `vec3` property is, so `node.transform.position`
/// answers what `node.position()` does. Anything else passes through.
fn as_vec3(value: Neutral) -> Neutral {
    let Neutral::List(items) = &value else {
        return value;
    };
    let mut out = [0.0f32; 3];
    if items.len() != out.len() {
        return value;
    }
    for (slot, item) in out.iter_mut().zip(items) {
        match item {
            Neutral::Num(n) => *slot = *n as f32,
            Neutral::Int(i) => *slot = *i as f32,
            _ => return value,
        }
    }
    Neutral::Vec3(out)
}

fn write_property(
    this: &Component,
    prop: &'static str,
    owners: u128,
    held: usize,
    value: &rune::Value,
) -> VmResult<()> {
    if owners & (1u128 << this.index) == 0 {
        return fail(format!("`{}` has no property `{prop}`", this.name));
    }
    let _scope = CallbackScope::enter();
    let value = match to_neutral(value) {
        Ok(v) => v,
        Err(err) => return fail(err),
    };
    let entity = match balaur_core::entity_of(balaur_script::NodeId(this.node)) {
        Ok(entity) => entity,
        Err(err) => return fail(err.to_string()),
    };
    let index = this.index as usize;
    match with_engine(held, |eng| {
        balaur_core::node_api::set_property_at(eng, entity, index, prop, &value)
    }) {
        Some(Ok(())) => VmResult::Ok(()),
        Some(Err(err)) => fail(format!("{err:#}")),
        None => fail("component property was registered on another thread"),
    }
}

fn fail<T>(message: impl std::fmt::Display) -> VmResult<T> {
    VmResult::Err(VmError::panic(message.to_string()))
}

/// The receiver and the converted arguments of a handle method call, the
/// node first and, when `with_name`, the component name second.
fn receive(
    values: &[rune::Value],
    with_name: bool,
) -> Result<(&'static str, Vec<Neutral>), String> {
    let Some(this) = values.first() else {
        return Err("component method called without a receiver".into());
    };
    let (node, name) = match this.borrow_ref::<Component>() {
        Ok(c) => (c.node, c.name),
        Err(_) => return Err("component method called on something else".into()),
    };
    let mut neutral = Vec::with_capacity(values.len() + 1);
    neutral.push(Neutral::Node(node));
    if with_name {
        neutral.push(Neutral::Str(name.to_string()));
    }
    for v in &values[1..] {
        neutral.push(to_neutral(v).map_err(|e| e.to_string())?);
    }
    Ok((name, neutral))
}

fn finish(
    called: Option<anyhow::Result<Neutral>>,
    stack: &mut dyn Memory,
    out: Output,
) -> VmResult<()> {
    let result = match called {
        Some(Ok(v)) => v,
        Some(Err(err)) => return fail(err),
        None => return fail("component method was registered on another thread"),
    };
    match from_neutral(&result) {
        Ok(v) => rune::vm_try!(out.store(stack, v)),
        Err(err) => return fail(err),
    }
    VmResult::Ok(())
}

fn generic_handler(
    handle: usize,
) -> impl 'static + Fn(&mut dyn Memory, InstAddress, usize, Output) -> VmResult<()> + Send + Sync {
    move |stack: &mut dyn Memory, addr: InstAddress, args: usize, out: Output| {
        let values = rune::vm_try!(stack.slice_at(addr, args)).to_vec();
        let _scope = CallbackScope::enter();
        let (_, neutral) = match receive(&values, true) {
            Ok(r) => r,
            Err(err) => return fail(err),
        };
        finish(call_bound(handle, &neutral), stack, out)
    }
}

fn method_handler(
    method: &'static str,
    targets: FxHashMap<String, usize>,
) -> impl 'static + Fn(&mut dyn Memory, InstAddress, usize, Output) -> VmResult<()> + Send + Sync {
    move |stack: &mut dyn Memory, addr: InstAddress, args: usize, out: Output| {
        let values = rune::vm_try!(stack.slice_at(addr, args)).to_vec();
        let _scope = CallbackScope::enter();
        let (name, neutral) = match receive(&values, false) {
            Ok(r) => r,
            Err(err) => return fail(err),
        };
        let Some(&handle) = targets.get(name) else {
            return fail(format!(
                "`{name}` has no `{method}`; no module driving it declares one"
            ));
        };
        finish(call_bound(handle, &neutral), stack, out)
    }
}
