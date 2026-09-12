//! Values a script files on a node by name, Godot's `set_meta`: state that
//! belongs to the node rather than to the script on it, so a helper with no
//! instance of its own (a static fade, a pool) can find it again. Runtime
//! only: a snapshot and a scene file carry none of it.

use std::collections::BTreeMap;

use anyhow::{Result, anyhow};
use balaur_script::Value;

use crate::engine::Engine;
use crate::node_api::node;

/// A node's named values, added with its first `set_meta`.
#[derive(Clone, Debug, Default)]
pub struct Meta(pub BTreeMap<String, Value>);

fn key(args: &[Value]) -> Result<&str> {
    match args.get(1) {
        Some(Value::Str(s)) => Ok(s),
        _ => Err(anyhow!("expected a name as the second argument")),
    }
}

pub(crate) fn get_meta(eng: &Engine, args: &[Value]) -> Result<Value> {
    let e = node(args)?;
    let name = key(args)?;
    let world = eng.world();
    let found = world.get::<&Meta>(e).ok().and_then(|m| m.0.get(name).cloned());
    Ok(found.unwrap_or_else(|| args.get(2).cloned().unwrap_or(Value::Nil)))
}

pub(crate) fn has_meta(eng: &Engine, args: &[Value]) -> Result<Value> {
    let e = node(args)?;
    let name = key(args)?;
    let world = eng.world();
    Ok(Value::Bool(world.get::<&Meta>(e).is_ok_and(|m| m.0.contains_key(name))))
}

pub(crate) fn set_meta(eng: &Engine, args: &[Value]) -> Result<Value> {
    let e = node(args)?;
    let name = key(args)?.to_string();
    let value = args.get(2).cloned().unwrap_or(Value::Nil);
    // Setting nil is removing, as in Godot.
    if matches!(value, Value::Nil) {
        return remove_meta(eng, args);
    }
    let mut world = eng.world_mut();
    if let Ok(mut meta) = world.get::<&mut Meta>(e) {
        meta.0.insert(name, value);
        return Ok(Value::Nil);
    }
    let meta = Meta(BTreeMap::from([(name, value)]));
    world.insert_one(e, meta).map_err(|_| anyhow!("node is dead"))?;
    Ok(Value::Nil)
}

pub(crate) fn remove_meta(eng: &Engine, args: &[Value]) -> Result<Value> {
    let e = node(args)?;
    let name = key(args)?;
    let world = eng.world();
    if let Ok(mut meta) = world.get::<&mut Meta>(e) {
        meta.0.remove(name);
    }
    Ok(Value::Nil)
}

pub(crate) fn meta_names(eng: &Engine, args: &[Value]) -> Result<Value> {
    let e = node(args)?;
    let world = eng.world();
    let names = world
        .get::<&Meta>(e)
        .map(|m| m.0.keys().cloned().map(|k| Value::Str(k.into())).collect())
        .unwrap_or_default();
    Ok(Value::List(names))
}
