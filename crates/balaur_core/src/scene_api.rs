//! The scene's own state, as script calls: its `[variables]`, the words a
//! `[[nodes.bindings]]` row may use, and the switch to another scene.
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

/// The events a `[[nodes.bindings]]` row may answer, for the Events view.
pub(crate) fn bindable_events(_eng: &Engine, _args: &[Value]) -> Result<Value> {
    Ok(Value::List(
        crate::hooks::BINDABLE
            .iter()
            .map(|name| Value::Str((*name).to_string()))
            .collect(),
    ))
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

/// `scene.switch(path, options)` — replace the scene at the end of the tick.
///
/// Spelled `switch`, not `load`: `load` is reserved for producing a live
/// object from a path, which this is not.
pub(crate) fn scene_switch(eng: &Engine, args: &[Value]) -> Result<Value> {
    let path = text(args, 0)?;
    let fade = args
        .get(1)
        .and_then(|options| match options {
            Value::Map(pairs) => pairs
                .iter()
                .find(|(key, _)| key == "fade")
                .and_then(|(_, v)| match v {
                    Value::Num(n) => Some(*n as f32),
                    _ => None,
                }),
            _ => None,
        })
        .unwrap_or(0.0);
    crate::scene_switch::request_with_fade(eng, path, fade);
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
                    ("name".into(), Value::Str(name.clone())),
                    (
                        "type".into(),
                        Value::Str(spec.map_or("float", |s| s.kind.word()).to_string()),
                    ),
                    ("value".into(), spec.map_or(Value::Nil, |s| s.value.clone())),
                    (
                        "persist".into(),
                        Value::Bool(spec.is_some_and(|s| s.persist)),
                    ),
                ])
            })
            .collect(),
    ))
}
