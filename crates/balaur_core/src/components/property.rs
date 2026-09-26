//! One property, without the component's whole table.
//!
//! A component may say it can answer or take a single property on its own.
//! Most cannot, and for those this falls back to building the table and
//! reading one key out of it, which is what a caller would have done anyway.
//!
//! A hook is filed by the component's registration index, so reaching one
//! costs neither a hash of its name nor a string to carry it. A component
//! that declares a hook before it registers, which is the order the built-ins
//! use, has it held by name until `register_component` comes back for it.

use anyhow::{Result, anyhow};
use hecs::Entity;

use super::{PropertyFn, get_at, index_of, patch_at, record_one};
use crate::engine::Engine;

/// The components that can answer one property on their own, by name.
///
/// A resource rather than a field on [`super::ComponentDef`]: every component builds
/// its whole table today, and this is the fast path for the one or two that a
/// UI pass reads a single property of, hundreds of times a frame.
#[derive(Default)]
pub struct PropertyReaders {
    by_index: Vec<Option<PropertyFn>>,
    /// Declared before the component registered, waiting for its number.
    pending: Vec<(String, PropertyFn)>,
}

/// Say that `name` can answer a single property, and how.
///
/// The name is resolved to its registration index here, once, so a read costs
/// an index rather than a hash. Called after the component registers.
pub fn answers_property(eng: &Engine, name: &str, read: PropertyFn) {
    if eng.try_resource::<PropertyReaders>().is_none() {
        eng.insert_resource(PropertyReaders::default());
    }
    let Some(readers) = eng.try_resource::<PropertyReaders>() else {
        return;
    };
    let mut readers = readers.borrow_mut();
    match index_of(eng, name) {
        Some(index) => slot(&mut readers.by_index, index, read),
        // Declared before the component itself, which is how the built-ins
        // read: `register_component` comes back for these.
        None => readers.pending.push((name.to_string(), read)),
    }
}

impl PropertyReaders {
    /// Whether the component at `index` can answer one property on its own.
    #[must_use]
    pub fn reads(&self, index: usize) -> bool {
        matches!(self.by_index.get(index), Some(Some(_)))
    }
}

/// Whether the component at `index` answered `key` on its own.
///
/// The fast path and the fallback return the same value, so nothing else can
/// tell them apart: a key the reader forgets is answered correctly and costs
/// a whole table. A test guarding the keys a frame reads needs to know which
/// happened.
#[must_use]
pub fn answers_alone(eng: &Engine, entity: Entity, index: usize, key: &str) -> bool {
    let Some(readers) = eng.try_resource::<PropertyReaders>() else {
        return false;
    };
    let readers = readers.borrow();
    match readers.by_index.get(index) {
        Some(Some(read)) => read(eng, entity, key).is_some(),
        _ => false,
    }
}

/// Put `hook` at `index`, growing the table to reach it.
fn slot<T>(table: &mut Vec<Option<T>>, index: usize, hook: T) {
    if table.len() <= index {
        table.resize_with(index + 1, || None);
    }
    table[index] = Some(hook);
}

/// Give the component that just registered any hook that named it first.
///
/// Declaring a fast path before the component is the order every built-in
/// writes, and an index cannot be handed out before there is one. So the
/// hooks wait here rather than the caller having to know.
pub(crate) fn resolve_property_hooks(eng: &Engine, name: &str, index: usize) {
    if let Some(readers) = eng.try_resource::<PropertyReaders>() {
        let mut readers = readers.borrow_mut();
        while let Some(at) = readers.pending.iter().position(|(n, _)| n == name) {
            let (_, read) = readers.pending.swap_remove(at);
            slot(&mut readers.by_index, index, read);
        }
    }
    if let Some(writers) = eng.try_resource::<PropertyWriters>() {
        let mut writers = writers.borrow_mut();
        while let Some(at) = writers.pending.iter().position(|(n, _)| n == name) {
            let (_, write) = writers.pending.swap_remove(at);
            slot(&mut writers.by_index, index, write);
        }
    }
}

/// A component writing one property into its own live state. `false` is a
/// property, or a node, it cannot answer for, which [`super::patch`] then does.
pub type PropertyWriteFn = Box<dyn Fn(&Engine, Entity, &str, &toml::Value) -> bool>;

/// The components that can write one property on their own, by name. The
/// twin of [`PropertyReaders`], for a script driving one value over time.
#[derive(Default)]
pub struct PropertyWriters {
    by_index: Vec<Option<PropertyWriteFn>>,
    /// Declared before the component registered, waiting for its number.
    pending: Vec<(String, PropertyWriteFn)>,
}

/// Say that `name` can write a single property, and how.
pub fn writes_property(eng: &Engine, name: &str, write: PropertyWriteFn) {
    if eng.try_resource::<PropertyWriters>().is_none() {
        eng.insert_resource(PropertyWriters::default());
    }
    let Some(writers) = eng.try_resource::<PropertyWriters>() else {
        return;
    };
    let mut writers = writers.borrow_mut();
    match index_of(eng, name) {
        Some(index) => slot(&mut writers.by_index, index, write),
        None => writers.pending.push((name.to_string(), write)),
    }
}

/// Write one property of a component.
///
/// The component's own fast path where it registered one, and a whole-table
/// [`super::patch`] where it did not. One entry point rather than two, so no caller
/// has to know which components can take a property on its own: animation
/// drives one property per track per tick and a script writing
/// `node.transform.position` does the same thing once.
///
/// # Errors
/// What [`super::patch`] errors on.
pub fn set_property(
    eng: &Engine,
    entity: Entity,
    name: &str,
    key: &str,
    value: &toml::Value,
) -> Result<()> {
    let index = index_of(eng, name).ok_or_else(|| anyhow!("unknown component '{name}'"))?;
    set_property_at(eng, entity, index, key, value)
}

/// [`set_property`] with the definition already resolved.
///
/// # Errors
/// What [`super::patch`] errors on.
pub fn set_property_at(
    eng: &Engine,
    entity: Entity,
    index: usize,
    key: &str,
    value: &toml::Value,
) -> Result<()> {
    let took = {
        match eng.try_resource::<PropertyWriters>() {
            Some(writers) => {
                let writers = writers.borrow();
                match writers.by_index.get(index) {
                    Some(Some(write)) => write(eng, entity, key, value),
                    _ => false,
                }
            }
            None => false,
        }
    };
    if !took {
        let params = toml::Value::Table(toml::map::Map::from_iter([(
            key.to_string(),
            value.clone(),
        )]));
        return patch_at(eng, entity, index, &params);
    }
    // The fast path wrote the component but not the record a save reads.
    record_one(eng, entity, index, key, value);
    Ok(())
}

/// One property of a component, without building the rest where the component
/// knows how to answer: `get` and index is what happens otherwise.
pub fn property(eng: &Engine, entity: Entity, name: &str, key: &str) -> Option<toml::Value> {
    property_at(eng, entity, index_of(eng, name)?, key)
}

/// Whether the component's own reader reports every value in `asked` already.
///
/// `false` for a component with no reader, or a key its reader cannot answer:
/// the caller then reads the whole table to find out.
pub(crate) fn holds_already(
    eng: &Engine,
    entity: Entity,
    index: usize,
    asked: &toml::map::Map<String, toml::Value>,
) -> bool {
    let Some(readers) = eng.try_resource::<PropertyReaders>() else {
        return false;
    };
    let readers = readers.borrow();
    let Some(Some(read)) = readers.by_index.get(index) else {
        return false;
    };
    asked
        .iter()
        .all(|(key, value)| read(eng, entity, key).as_ref() == Some(value))
}

/// [`property`] with the definition already resolved.
#[must_use]
pub fn property_at(eng: &Engine, entity: Entity, index: usize, key: &str) -> Option<toml::Value> {
    if let Some(readers) = eng.try_resource::<PropertyReaders>() {
        let readers = readers.borrow();
        if let Some(Some(read)) = readers.by_index.get(index)
            && let Some(found) = read(eng, entity, key)
        {
            return Some(found);
        }
    }
    match get_at(eng, entity, index)? {
        toml::Value::Table(mut table) => table.remove(key),
        other => other.get(key).cloned(),
    }
}
