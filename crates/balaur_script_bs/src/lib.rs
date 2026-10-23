//! Balaur Script (`balaur_bs`) behind the scripting seam.
//!
//! [`BsHost`] runs `.bs` scripts; [`factory`] makes the engine's script host
//! of it. [`Mixed`] puts it beside another backend and hands each script to
//! the one its extension names. `docs/PLAN-balaur-script.md`.

mod api;
mod convert;
mod doc_types;
mod export;
mod host;
mod mixed;
mod native;
mod node_ops;
mod store;
mod tool_verbs;
mod waits;

pub use balaur_bs::analysis::{builtins, value_types};
pub use balaur_bs::fmt;
pub use balaur_bs::{HostApi, HostSignature, Literal};
pub use export::Exporter;
pub use host::{BsHost, EXTENSION};
pub use mixed::Mixed;
pub use native::NATIVE_FRAMEWORK;
#[cfg(feature = "native")]
pub use native::load_native;
#[cfg(feature = "c")]
pub use native::native_source;

use anyhow::anyhow;
use balaur_bs::Ty;
use balaur_bs::runtime::{Bound, Value as L};
use balaur_core::Engine;
use balaur_script::Value;
use std::rc::Rc;

const GET: &str = "get";
const SET: &str = "set";
const GET_COMPONENT: &str = "get_component";
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
/// waits. A call then converts its arguments and calls, nothing else.
pub(crate) fn bind(
    eng: &Engine,
    registry: &Rc<std::cell::RefCell<api::Registry>>,
    key: &str,
    ret: &Ty,
) -> Result<Bound, String> {
    let (eng, ret) = (eng.clone(), ret.clone());
    let structs = registry.borrow().structs();
    let answer = move |v: anyhow::Result<Value>, ret: &Ty| {
        v.map(|v| convert::from_neutral_in(&v, ret, &structs))
            .map_err(|e| format!("{e:#}"))
    };
    if store::is_store(key) {
        let key = key.to_string();
        return Ok(Rc::new(move |args: &[L]| store::call(&key, args)));
    }
    if let Some(verb) = tool_verbs::verb(key) {
        let structs = registry.borrow().structs();
        return Ok(Rc::new(move |args: &[L]| {
            tool_verbs::call(&eng, verb, args, &ret, &structs)
        }));
    }
    if waits::is_wait(key) {
        let key = key.to_string();
        return Ok(Rc::new(move |args: &[L]| {
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
        let (comp, prop) = (Value::text(comp), prop.to_string());
        if dir == GET {
            let op = node_op(GET_COMPONENT).map_err(|e| format!("{e:#}"))?;
            let prop = Value::text(prop);
            return Ok(Rc::new(move |args: &[L]| {
                let node = args.first().map_or(Value::Nil, convert::to_neutral);
                answer((op.call)(&eng, &[node, comp.clone(), prop.clone()]), &ret)
            }));
        }
        let op = node_op(PATCH_COMPONENT).map_err(|e| format!("{e:#}"))?;
        return Ok(Rc::new(move |args: &[L]| {
            let node = args.first().map_or(Value::Nil, convert::to_neutral);
            let value = args.get(1).map_or(Value::Nil, convert::to_neutral);
            let patch = Value::Map(vec![(prop.clone(), value)]);
            answer((op.call)(&eng, &[node, comp.clone(), patch]), &ret)
        }));
    }
    let driven = registry.borrow().component_fn(key);
    if let Some(name) = node_ops::op_name(key).filter(|_| driven.is_none()) {
        let op = node_op(name).map_err(|e| format!("{e:#}"))?;
        return Ok(Rc::new(move |args: &[L]| {
            let mut neutral: Neutral = args.iter().map(convert::to_neutral).collect();
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
    let call = registry
        .borrow()
        .find(module, name)
        .map_err(|e| format!("{e:#}"))?;
    Ok(Rc::new(move |args: &[L]| {
        let mut neutral: Neutral = args.iter().map(convert::to_neutral).collect();
        let out = match &call {
            api::Call::Raw(f) => f(&eng, &neutral),
            api::Call::Owned(f) => f(&eng, &mut neutral),
        };
        answer(out, &ret)
    }))
}

/// A call's arguments as the engine takes them, inline for the usual counts.
type Neutral = smallvec::SmallVec<[Value; 4]>;

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

/// A factory running `inner`'s backend with `.bs` scripts beside it.
pub fn beside(inner: balaur_core::ScriptHostFactory) -> balaur_core::ScriptHostFactory {
    Box::new(move |setup| {
        let lang = BsHost::new(
            setup.engine.clone(),
            setup.project_root,
            setup.pack.clone(),
            setup.watch,
        );
        lang.register(setup.engine);
        let primary = inner(setup)?;
        Ok(Rc::new(Mixed::new(primary, lang)))
    })
}
