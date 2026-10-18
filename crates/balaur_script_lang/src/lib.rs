//! `balaur_lang` behind the scripting seam.
//!
//! [`LangHost`] runs `.bl` scripts; [`Mixed`] puts it beside another backend
//! and hands each script to the one its extension names, so a project's Rune
//! scripts keep running while some move over. `docs/PLAN-balaur-lang.md`.

mod api;
mod convert;
mod export;
mod host;
mod mixed;
mod node_ops;
mod waits;

pub use export::Exporter;
pub use host::{EXTENSION, LangHost, POLL_FRAMES};
pub use mixed::Mixed;

use anyhow::anyhow;
use balaur_core::Engine;
use balaur_lang::Ty;
use balaur_lang::runtime::Value as L;
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

/// One engine function a script called: `module::name` through the bindings,
/// `component.property.get` / `.set` through the node's component.
pub(crate) fn engine_call(
    eng: &Engine,
    registry: &api::Registry,
    key: &str,
    args: &[L],
    ret: &Ty,
) -> Result<L, String> {
    if let Some(token) = waits::token(eng, key, args) {
        return Ok(L::Int(token.cast_signed()));
    }
    let neutral: Vec<Value> = args.iter().map(convert::to_neutral).collect();
    let property = key
        .rsplit_once('.')
        .filter(|(_, dir)| *dir == GET || *dir == SET)
        .and_then(|(path, dir)| Some((path.split_once('.')?, dir)));
    let out = if let Some(((comp, prop), dir)) = property {
        let node = neutral.first().cloned().unwrap_or(Value::Nil);
        if dir == GET {
            node_op(GET_COMPONENT)
                .and_then(|op| (op.call)(eng, &[node, Value::text(comp), Value::text(prop)]))
        } else {
            let value = neutral.get(1).cloned().unwrap_or(Value::Nil);
            node_op(PATCH_COMPONENT).and_then(|op| {
                (op.call)(
                    eng,
                    &[
                        node,
                        Value::text(comp),
                        Value::Map(vec![(prop.to_string(), value)]),
                    ],
                )
            })
        }
    } else if let Some(name) = node_ops::op_name(key) {
        node_op(name).and_then(|op| (op.call)(eng, &neutral))
    } else if let Some((module, name)) = key.split_once("::") {
        registry.call(eng, module, name, &neutral)
    } else {
        Err(anyhow!("unknown engine function `{key}`"))
    };
    out.map(|v| convert::from_neutral(&v, ret))
        .map_err(|e| format!("{e:#}"))
}

/// A factory running `inner`'s backend with `.bl` scripts beside it.
pub fn beside(inner: balaur_core::ScriptHostFactory) -> balaur_core::ScriptHostFactory {
    Box::new(move |setup| {
        let lang = LangHost::new(
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
