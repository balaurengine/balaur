//! Component handles: `node.body2d.apply_impulse(x, y)`.
//!
//! A handle pairs a node with a component name. Its methods are the functions
//! of the modules that declared they drive that component, with the node
//! argument bound, so `node.body2d.apply_impulse(x, y)` is
//! `physics2d::apply_impulse(node, x, y)`. Dispatch is by component name at
//! call time: the same `apply_impulse` on a `body3d` handle reaches
//! `physics3d`. `get`, `set`, `has` and `remove` come from the node's own
//! component operations with the name filled in.
//!
//! Schema properties are fields on the same handle:
//! `node.collider3d.density = 15` patches that one property and
//! `node.collider3d.density` reads it back, so a script names a property the
//! way a scene file does instead of building a table for one number.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashSet};

use rustc_hash::FxHashMap;

use balaur_core::Engine;
use balaur_core::node_api::NODE_OPS;
use balaur_script::Value as Neutral;
use rune::runtime::{InstAddress, Memory, Output, Protocol, VmError, VmResult};

use super::{Node, from_neutral, to_neutral};
use crate::bindings::{CallbackScope, api_docs, bound_handle, call_bound, hold_node_fn};

/// A component on a node, as scripts see it.
#[derive(rune::Any, Clone)]
#[rune(item = ::balaur)]
pub struct Component {
    pub(crate) node: u64,
    pub(crate) name: String,
}

thread_local! {
    /// Rune keeps item names for the life of the context, so each name is
    /// leaked once and reused across context builds.
    static NAMES: RefCell<HashSet<&'static str>> = RefCell::new(HashSet::new());
}

fn intern(name: &str) -> &'static str {
    NAMES.with_borrow_mut(|names| {
        if let Some(&existing) = names.get(name) {
            return existing;
        }
        let leaked: &'static str = Box::leak(name.to_string().into_boxed_str());
        names.insert(leaked);
        leaked
    })
}

fn is_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Component operations the handle offers for every component.
const GENERIC: &[(&str, &str)] = &[
    ("get", "get_component"),
    ("set", "set_component"),
    ("patch", "patch_component"),
    ("has", "has_component"),
    ("remove", "remove_component"),
];

/// Give `Node` a field per registered component, and the handle its methods.
pub(crate) fn install(m: &mut rune::Module, eng: &Engine) -> Result<(), rune::ContextError> {
    m.ty::<Component>()?;

    for (component, _) in balaur_core::components::schemas(eng) {
        if !is_identifier(&component) {
            tracing::warn!("component `{component}` is not a script identifier; no handle");
            continue;
        }
        let name = intern(&component);
        m.field_function(&Protocol::GET, name, move |node: &Node| Component {
            node: node.id,
            name: name.to_string(),
        })?;
    }

    for (method, op) in GENERIC {
        let Some(declared) = NODE_OPS.iter().find(|d| d.name == *op) else {
            continue;
        };
        let handle = hold_node_fn(eng.clone(), declared.call);
        m.raw_function(*method, generic_handler(handle))
            .build_associated::<Component>()?;
    }

    // Method name -> component -> bound function, from what each function
    // declared it acts on.
    let mut methods: BTreeMap<String, FxHashMap<String, usize>> = BTreeMap::new();
    for entry in api_docs() {
        if entry.acts_on.is_empty() {
            continue;
        }
        let Some(handle) = bound_handle(&entry.module, &entry.name) else {
            continue;
        };
        if GENERIC.iter().any(|(g, _)| *g == entry.name) {
            tracing::warn!(
                "{}::{} hides the handle's own `{}`",
                entry.module,
                entry.name,
                entry.name
            );
            continue;
        }
        let targets = methods.entry(entry.name.clone()).or_default();
        for component in entry.acts_on {
            if targets.insert(component.clone(), handle).is_some() {
                tracing::warn!("two modules act on `{component}` with a `{}`", entry.name);
            }
        }
    }
    for (method, targets) in methods {
        let name = intern(&method);
        m.raw_function(name, method_handler(name, targets))
            .build_associated::<Component>()?;
    }
    property_fields(m, eng)
}

/// Give the handle a field per schema property, over every component that
/// declares one of that name.
///
/// Reading goes through `get_component`, so a property backed by live state
/// answers what the simulation holds rather than what the scene wrote;
/// assigning goes through `patch_component`, so one property moves and the
/// rest of the component stays where it was.
fn property_fields(m: &mut rune::Module, eng: &Engine) -> Result<(), rune::ContextError> {
    let (Some(read), Some(write)) = (node_op("get_component"), node_op("patch_component")) else {
        return Ok(());
    };
    let read = hold_node_fn(eng.clone(), read);
    let write = hold_node_fn(eng.clone(), write);
    // Property name -> the components declaring it, so dispatch is by
    // component name at call time, as the methods are.
    let mut owners: BTreeMap<String, HashSet<String>> = BTreeMap::new();
    // The same, narrowed to where the schema says `vec3`, so a position reads
    // back as the `Vec3` the node's own accessors answer with rather than as
    // three numbers in a list.
    let mut vectors: BTreeMap<String, HashSet<String>> = BTreeMap::new();
    for (component, schema) in balaur_core::components::schemas(eng) {
        let Some(table) = schema.as_table() else {
            continue;
        };
        for (prop, spec) in table {
            if !is_identifier(prop) {
                tracing::warn!("`{component}.{prop}` is not a script identifier; no field");
                continue;
            }
            owners
                .entry(prop.clone())
                .or_default()
                .insert(component.clone());
            if spec.get("type").and_then(|v| v.as_str()) == Some("vec3") {
                vectors
                    .entry(prop.clone())
                    .or_default()
                    .insert(component.clone());
            }
        }
    }
    for (prop, components) in owners {
        let name = intern(&prop);
        let readers = components.clone();
        let as_vector = vectors.remove(&prop).unwrap_or_default();
        m.field_function(&Protocol::GET, name, move |this: &Component| {
            read_property(this, name, &readers, &as_vector, read)
        })?;
        m.field_function(
            &Protocol::SET,
            name,
            move |this: &Component, value: rune::Value| {
                write_property(this, name, &components, write, &value)
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
    [Neutral::Node(this.node), Neutral::Str(this.name.clone())]
}

fn read_property(
    this: &Component,
    prop: &'static str,
    owners: &HashSet<String>,
    vectors: &HashSet<String>,
    handle: usize,
) -> VmResult<rune::Value> {
    if !owners.contains(&this.name) {
        return fail(format!("`{}` has no property `{prop}`", this.name));
    }
    let _scope = CallbackScope::enter();
    let got = match call_bound(handle, &receiver(this)) {
        Some(Ok(v)) => v,
        Some(Err(err)) => return fail(err),
        None => return fail("component property was registered on another thread"),
    };
    let Neutral::Map(props) = got else {
        return fail(format!("the node has no `{}`", this.name));
    };
    let Some((_, value)) = props.into_iter().find(|(key, _)| key == prop) else {
        return fail(format!("`{}` does not report `{prop}`", this.name));
    };
    let value = if vectors.contains(&this.name) {
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
    owners: &HashSet<String>,
    handle: usize,
    value: &rune::Value,
) -> VmResult<()> {
    if !owners.contains(&this.name) {
        return fail(format!("`{}` has no property `{prop}`", this.name));
    }
    let _scope = CallbackScope::enter();
    let value = match to_neutral(value) {
        Ok(v) => v,
        Err(err) => return fail(err),
    };
    let [node, name] = receiver(this);
    let args = [node, name, Neutral::Map(vec![(prop.to_string(), value)])];
    match call_bound(handle, &args) {
        Some(Ok(_)) => VmResult::Ok(()),
        Some(Err(err)) => fail(err),
        None => fail("component property was registered on another thread"),
    }
}

fn fail<T>(message: impl std::fmt::Display) -> VmResult<T> {
    VmResult::Err(VmError::panic(message.to_string()))
}

/// The receiver and the converted arguments of a handle method call, the
/// node first and, when `with_name`, the component name second.
fn receive(values: &[rune::Value], with_name: bool) -> Result<(String, Vec<Neutral>), String> {
    let Some(this) = values.first() else {
        return Err("component method called without a receiver".into());
    };
    let (node, name) = match this.borrow_ref::<Component>() {
        Ok(c) => (c.node, c.name.clone()),
        Err(_) => return Err("component method called on something else".into()),
    };
    let mut neutral = Vec::with_capacity(values.len() + 1);
    neutral.push(Neutral::Node(node));
    if with_name {
        neutral.push(Neutral::Str(name.clone()));
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
        let Some(&handle) = targets.get(&name) else {
            return fail(format!(
                "`{name}` has no `{method}`; no module driving it declares one"
            ));
        };
        finish(call_bound(handle, &neutral), stack, out)
    }
}
