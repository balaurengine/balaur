//! The scene's own state, as script calls: its `[variables]`, the words a
//! `[[nodes.bindings.rows]]` row may use, and the switch to another scene.
//!
//! Here rather than in `engine_api`, which is the table that names them: this
//! is what those rows call, and it is one subject.

// Every declaration shares one signature so they can sit in the same table of
// function pointers; several of them have nothing to fail at.
#![allow(clippy::unnecessary_wraps)]

use anyhow::Result;
use balaur_script::Value;

use crate::engine::Engine;
use crate::engine_api::text;
use crate::vocabulary::keys;
use crate::vocabulary::words;

/// The events the node's script declares with `event`, read from its file.
fn script_events(eng: &Engine, entity: crate::hecs::Entity) -> Vec<String> {
    let path = eng
        .world()
        .get::<&crate::scene::NodeScript>(entity)
        .ok()
        .map(|script| script.path.clone());
    match (path, eng.script_host()) {
        (Some(path), Some(host)) => host
            .events(&path)
            .into_iter()
            .map(|(name, _)| name)
            .collect(),
        _ => Vec::new(),
    }
}

/// The events a `[[nodes.bindings.rows]]` row may answer, for the Events view:
/// the core hooks, then on a node what it, its components and its script
/// announce.
pub(crate) fn bindable_events(eng: &Engine, args: &[Value]) -> Result<Value> {
    let mut events: Vec<String> = crate::hooks::BINDABLE
        .iter()
        .map(|name| (*name).to_string())
        .collect();
    let node = match args.first() {
        Some(Value::Node(id)) => Some(crate::entity_of(balaur_script::NodeId(*id))?),
        _ => None,
    };
    if let Some(entity) = node {
        let mut announced: Vec<String> = crate::events::on_nodes(eng)
            .iter()
            .map(|(name, _)| (*name).to_string())
            .collect();
        announced.extend(
            crate::components::events_on(eng, entity)
                .into_iter()
                .map(str::to_string),
        );
        announced.extend(script_events(eng, entity));
        // A core hook's name is already listed, spelled without the prefix.
        for name in announced {
            if crate::hooks::BINDABLE.contains(&name.as_str()) {
                continue;
            }
            let row = format!("{}{name}", crate::hooks::EMITTED);
            if !events.contains(&row) {
                events.push(row);
            }
        }
    }
    Ok(Value::List(events.into_iter().map(Value::Str).collect()))
}

/// The actions one may do, in the order the Events view offers them.
pub(crate) fn binding_actions(_eng: &Engine, _args: &[Value]) -> Result<Value> {
    Ok(Value::List(
        crate::bindings::ACTIONS
            .iter()
            .map(|(word, _)| Value::Str((*word).to_string()))
            .collect(),
    ))
}

/// `scene.switch(path, options)` — at the end of the tick, free the scene the
/// last switch built under `under`, the tree's root when nil, and build
/// `path` there.
///
/// Spelled `switch`, not `load`: `load` is reserved for producing a live
/// object from a path, which this is not.
pub(crate) fn scene_switch(eng: &Engine, args: &[Value]) -> Result<Value> {
    let path = text(args, 0)?;
    let pairs: &[(String, Value)] = match args.get(1) {
        Some(Value::Map(pairs)) => pairs,
        _ => &[],
    };
    let field = |name: &str| pairs.iter().find(|(key, _)| key == name).map(|(_, v)| v);
    let fade = match field(words::FADE) {
        Some(Value::Num(n)) => *n as f32,
        _ => 0.0,
    };
    let under = match field(words::UNDER) {
        Some(Value::Node(id)) => Some(crate::entity_of(balaur_script::NodeId(*id))?),
        _ => None,
    };
    crate::scene_switch::ask(
        eng,
        crate::scene_switch::Request {
            scene: path.to_string(),
            under,
            fade,
        },
    );
    Ok(Value::Nil)
}

/// `scene.current(under)` — the root of the scene the last switch built under
/// a node, the tree's root when nil; no node while none stands there.
pub(crate) fn scene_current(eng: &Engine, args: &[Value]) -> Result<Value> {
    let under = crate::engine_api::optional_node(args, 0)?;
    Ok(
        crate::scene_switch::current(eng, under).map_or(Value::Nil, |built| {
            Value::Node(crate::node_id_of(built.root).0)
        }),
    )
}

/// `scene.preload(path)` — the scene and its prefabs read and parsed now, and
/// what they name decoding off the frame, so a switch to it builds from warm
/// caches. No node is built.
pub(crate) fn scene_preload(eng: &Engine, args: &[Value]) -> Result<Value> {
    crate::preload::scene(eng, text(args, 0)?)?;
    Ok(Value::Nil)
}

/// `scene.unload(path)` — what `preload` or a pack warmed, let go.
pub(crate) fn scene_unload(eng: &Engine, args: &[Value]) -> Result<Value> {
    crate::preload::unload(eng, text(args, 0)?);
    Ok(Value::Nil)
}

/// `scene.variable(name)` — a declared value, or `()` for a name nothing
/// declared. Reading a typo answers nothing rather than failing: a condition
/// asking about a variable a level does not have is false, not an error.
pub(crate) fn scene_variable(eng: &Engine, args: &[Value]) -> Result<Value> {
    let name = text(args, 0)?;
    let variables = eng.resource::<crate::variables::Variables>();
    let variables = variables.borrow();
    Ok(variables.get(name).cloned().unwrap_or(Value::Nil))
}

/// `scene.set_variable(name, value)`. Writing an undeclared one fails: a
/// value nobody declared has no type and reaches no `on_variable_changed`.
pub(crate) fn scene_set_variable(eng: &Engine, args: &[Value]) -> Result<Value> {
    let name = text(args, 0)?;
    let value = args.get(1).cloned().unwrap_or(Value::Nil);
    let variables = eng.resource::<crate::variables::Variables>();
    let mut variables = variables.borrow_mut();
    variables.set(name, &value)?;
    Ok(Value::Nil)
}

/// `scene.variables()` — every declared name with its type, for a picker.
pub(crate) fn scene_variables(eng: &Engine, _args: &[Value]) -> Result<Value> {
    let variables = eng.resource::<crate::variables::Variables>();
    let variables = variables.borrow();
    Ok(Value::List(
        variables
            .names()
            .into_iter()
            .map(|name| {
                let spec = variables.spec(&name);
                Value::Map(vec![
                    (words::scene_toml::NAME.into(), Value::Str(name.clone())),
                    (
                        keys::TYPE.into(),
                        Value::Str(
                            spec.map_or(words::datatypes::FLOAT, |s| s.kind.word())
                                .to_string(),
                        ),
                    ),
                    (
                        keys::VALUE.into(),
                        spec.map_or(Value::Nil, |s| s.value.clone()),
                    ),
                ])
            })
            .collect(),
    ))
}
