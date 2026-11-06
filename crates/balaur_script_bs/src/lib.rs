//! Balaur Script (`balaur_bs`) behind the scripting seam.
//!
//! [`BsHost`] runs `.bs` scripts; [`factory`] makes the engine's script host
//! of it. `docs/PLAN-balaur-script.md`.

mod api;
mod convert;
mod doc_types;
mod host;
mod native;
mod node_ops;
mod store;
mod tool_verbs;
mod waits;

pub use balaur_bs::analysis::{builtins, value_types};
pub use balaur_bs::fmt;
pub use balaur_bs::{HostApi, HostSignature, Literal};
pub use host::{BsHost, EXTENSION};
#[cfg(all(feature = "native", not(target_arch = "wasm32")))]
pub use native::load_native;
#[cfg(feature = "c")]
pub use native::native_source;

use anyhow::anyhow;
use balaur_bs::Ty;
use balaur_bs::runtime::{Bound, CallArgs, Value as L};
use balaur_core::Engine;
use balaur_core::components::derived::Given;
use balaur_script::Value;
use std::rc::Rc;

const GET: &str = "get";
const SET: &str = "set";
const COMPONENT_PROPERTY: &str = "component_property";
const PATCH_COMPONENT: &str = "patch_component";

fn node_op(name: &str) -> anyhow::Result<&'static balaur_core::node_api::NodeOp> {
    balaur_core::node_api::NODE_OPS
        .iter()
        .find(|o| o.name == name)
        .ok_or_else(|| anyhow!("the engine has no node operation `{name}`"))
}

/// One engine function a script calls, bound once by its key: `module::name`
/// through the bindings, `node.op` through the node operations,
/// `component.property.get` / `.set` through the node's component, and the
/// waits. A call reads its arguments where the script left them and calls.
pub(crate) fn bind(
    eng: &Engine,
    registry: &Rc<api::Registry>,
    key: &str,
    ret: &Ty,
) -> Result<Bound, String> {
    let (eng, ret) = (eng.clone(), ret.clone());
    let structs = registry.structs();
    let answer = move |v: anyhow::Result<Value>, ret: &Ty| match v {
        Ok(v) => Ok(scalar(&v, ret).unwrap_or_else(|| convert::from_neutral(&v, ret, &*structs))),
        Err(e) => Err(format!("{e:#}")),
    };
    if store::is_store(key) {
        let key = key.to_string();
        return Ok(Rc::new(move |args: &dyn CallArgs| store::call(&key, args)));
    }
    if let Some(verb) = tool_verbs::verb(key) {
        let structs = registry.structs();
        return Ok(Rc::new(move |args: &dyn CallArgs| {
            tool_verbs::call(&eng, verb, args, &ret, &structs)
        }));
    }
    if waits::is_wait(key) {
        let key = key.to_string();
        return Ok(Rc::new(move |args: &dyn CallArgs| {
            waits::token(&eng, &key, args)
                .map(|t| L::Int(t.cast_signed()))
                .ok_or_else(|| format!("`{key}` was not given what it waits on"))
        }));
    }
    let property = key
        .rsplit_once('.')
        .filter(|(_, dir)| *dir == GET || *dir == SET)
        .and_then(|(path, dir)| Some((path.split_once('.')?, dir)));
    if let Some(((comp, prop), dir)) = property {
        // Bound once to the property's place (PLAN-balaur-script §9 step 6):
        // no name looked up and no node operation per call.
        if let Some(place) = balaur_core::components::place_of(&eng, comp, prop) {
            if dir == GET {
                return Ok(Rc::new(move |args: &dyn CallArgs| {
                    let entity = entity_at(args).map_err(|e| format!("{e:#}"))?;
                    let mut read = None;
                    balaur_core::components::given_placed(&eng, entity, &place, &mut |given| {
                        read = read_as(given, &ret);
                    });
                    if let Some(value) = read {
                        return Ok(value);
                    }
                    let value = balaur_core::components::property_placed(&eng, entity, &place)
                        .as_ref()
                        .map_or(Ok(Value::Nil), balaur_core::node_api::from_toml);
                    answer(value, &ret)
                }));
            }
            // A property's write gives nothing back.
            return Ok(Rc::new(move |args: &dyn CallArgs| {
                let entity = entity_at(args).map_err(|e| format!("{e:#}"))?;
                write_at(&eng, entity, &place, args)
                    .map(|()| L::Void)
                    .map_err(|e| format!("{e:#}"))
            }));
        }
        let (comp, prop) = (Value::text(comp), prop.to_string());
        if dir == GET {
            let op = node_op(COMPONENT_PROPERTY).map_err(|e| format!("{e:#}"))?;
            let prop = Value::text(prop);
            return Ok(Rc::new(move |args: &dyn CallArgs| {
                let node = convert::to_neutral(&args.value(0));
                answer((op.call)(&eng, &[node, comp.clone(), prop.clone()]), &ret)
            }));
        }
        let op = node_op(PATCH_COMPONENT).map_err(|e| format!("{e:#}"))?;
        return Ok(Rc::new(move |args: &dyn CallArgs| {
            let node = convert::to_neutral(&args.value(0));
            let value = convert::to_neutral(&args.value(1));
            let patch = Value::Map(vec![(prop.clone(), value)]);
            answer((op.call)(&eng, &[node, comp.clone(), patch]), &ret)
        }));
    }
    let driven = registry.component_fn(key);
    if let Some(name) = node_ops::op_name(key).filter(|_| driven.is_none()) {
        let op = node_op(name).map_err(|e| format!("{e:#}"))?;
        return Ok(Rc::new(move |args: &dyn CallArgs| {
            let mut neutral = convert::neutral(args);
            // Arguments left out arrive as nil: a method is not handed them.
            while neutral.len() > 2 && matches!(neutral.last(), Some(Value::Nil)) {
                neutral.pop();
            }
            answer((op.call)(&eng, &neutral), &ret)
        }));
    }
    let (module, name) = match &driven {
        Some((m, n)) => (m.as_str(), n.as_str()),
        None => key
            .split_once("::")
            .ok_or_else(|| format!("unknown engine function `{key}`"))?,
    };
    let call = registry.find(module, name).map_err(|e| format!("{e:#}"))?;
    Ok(Rc::new(move |args: &dyn CallArgs| {
        let out = match &call {
            api::Call::Raw(func) if args.is_empty() => func(&eng, &[]),
            api::Call::Raw(func) => func(&eng, &convert::neutral(args)),
            api::Call::Typed(func) => func(&eng, &convert::Seam(args)),
        };
        answer(out, &ret)
    }))
}

/// What nearly every engine function answers, a number or a flag the script
/// asked for, without the neutral value's whole conversion.
fn scalar(v: &Value, ret: &Ty) -> Option<L> {
    Some(match (ret, v) {
        (Ty::Void, _) => L::Void,
        (Ty::Float, Value::Num(x)) => L::Float(*x),
        (Ty::Int, Value::Int(x)) => L::Int(*x),
        (Ty::Bool, Value::Bool(b)) => L::Bool(*b),
        _ => return None,
    })
}

/// What a property's read answers, for the type the script asked for: `None`
/// where they differ, which the neutral value converts.
fn read_as(given: Given<'_>, ret: &Ty) -> Option<L> {
    Some(match (given, ret) {
        (Given::Float(number), Ty::Float) => L::Float(number),
        (Given::Int(number), Ty::Int) => L::Int(number),
        (Given::Bool(flag), Ty::Bool) => L::Bool(flag),
        (Given::Text(text), Ty::Str) => L::Str(text.to_owned()),
        (Given::Numbers(numbers, _), Ty::Struct(_)) => L::Vector(numbers.map(f64::from)),
        _ => return None,
    })
}

/// The call's second argument, where it is, written to the property at `place`:
/// a value is built only for a kind a call's slots do not hold.
fn write_at(
    eng: &Engine,
    entity: balaur_core::hecs::Entity,
    place: &balaur_core::components::PropertyPlace,
    args: &dyn CallArgs,
) -> anyhow::Result<()> {
    let given = if let Some((numbers, count)) = args.vector(1) {
        Given::Numbers(numbers, count)
    } else if let Some(number) = args.float(1) {
        Given::Float(number)
    } else if let Some(number) = args.int(1) {
        Given::Int(number)
    } else if let Some(flag) = args.bool(1) {
        Given::Bool(flag)
    } else if let Some(text) = args.text(1) {
        Given::Text(text)
    } else {
        let value = balaur_core::node_api::to_toml(&convert::neutral_at(args, 1))?;
        return balaur_core::components::set_property_placed(
            eng,
            entity,
            place,
            Given::Value(&value),
        );
    };
    balaur_core::components::set_property_placed(eng, entity, place, given)
}

/// The node a component property is read on or written to: the call's first
/// argument.
fn entity_at(args: &dyn CallArgs) -> anyhow::Result<balaur_core::hecs::Entity> {
    let id = args
        .node(0)
        .ok_or_else(|| anyhow!("expected a node as the first argument"))?;
    balaur_core::entity_of(balaur_script::NodeId(id.cast_unsigned()))
}

/// A factory running `.bs` scripts.
pub fn factory() -> balaur_core::ScriptHostFactory {
    Box::new(|setup| {
        let lang = BsHost::new(
            setup.engine.clone(),
            setup.project_root,
            setup.pack.clone(),
            setup.watch,
        );
        lang.register(setup.engine);
        Ok(Rc::new(lang))
    })
}
