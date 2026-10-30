//! One property, without the component's whole table.
//!
//! A derived component reads and writes one property by its index
//! ([`super::ComponentDef::read`] and [`super::ComponentDef::write`]). A
//! hand-written one cannot, and for those this falls back to building the
//! table and reading one key out of it, or to a whole [`super::patch`], which
//! is what a caller would have done anyway.

use anyhow::{Result, anyhow};
use hecs::Entity;

use super::{get_at, index_of, patch_at, record_one};
use crate::engine::Engine;

/// Whether the component at `index` answered `key` on its own.
///
/// The read by index and the fallback return the same value, so nothing else
/// can tell them apart: a key the component cannot read alone is answered
/// correctly and costs a whole table. A test guarding the keys a frame reads
/// needs to know which happened.
#[must_use]
pub fn answers_alone(eng: &Engine, entity: Entity, index: usize, key: &str) -> bool {
    derived_read(eng, entity, index, key).is_some()
}

/// Write one property of a component.
///
/// The component's own write by index where it has one, and a whole-table
/// [`super::patch`] where it does not. One entry point rather than two, so no
/// caller has to know which components can take a property on its own:
/// animation drives one property per track per tick and a script writing
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
    // A text may name a file, which a node of another project's content names
    // from that project's root.
    let content = value
        .is_str()
        .then(|| crate::document_paths::written_on(eng, entity, value))
        .flatten();
    let value = content.as_ref().unwrap_or(value);
    if !derived_write(eng, entity, index, key, value) {
        let params = toml::Value::Table(toml::map::Map::from_iter([(
            key.to_string(),
            value.clone(),
        )]));
        return patch_at(eng, entity, index, &params);
    }
    // The write by index changed the component but not the record a save
    // reads, which the change is logged against first: it carries what it
    // replaced.
    crate::changes::property(eng, entity, index, key);
    record_one(eng, entity, index, key, value);
    Ok(())
}

/// Write one property through the component's own write by index, where it
/// has one. `false` sends the caller to a whole patch: a node without the
/// component, which the patch adds, or a value the field refuses, which the
/// patch reports, or one it has to resolve first (an asset's definition).
fn derived_write(
    eng: &Engine,
    entity: Entity,
    index: usize,
    key: &str,
    value: &toml::Value,
) -> bool {
    let Some(registry) = eng.try_resource::<super::ComponentRegistry>() else {
        return false;
    };
    let registry = registry.borrow();
    let Some((_, def)) = registry.at(index) else {
        return false;
    };
    let (Some(write), Some(at)) = (&def.write, def.schema.position(key)) else {
        return false;
    };
    matches!(write(eng, entity, at, value), Ok(true))
}

/// One property by the component's own read by index, where it has one.
fn derived_read(eng: &Engine, entity: Entity, index: usize, key: &str) -> Option<toml::Value> {
    let registry = eng.try_resource::<super::ComponentRegistry>()?;
    let registry = registry.borrow();
    let (_, def) = registry.at(index)?;
    def.read.as_ref()?(eng, entity, def.schema.position(key)?)
}

/// One property of a component, without building the rest where the component
/// can read it alone: `get` and index is what happens otherwise.
pub fn property(eng: &Engine, entity: Entity, name: &str, key: &str) -> Option<toml::Value> {
    property_at(eng, entity, index_of(eng, name)?, key)
}

/// Whether the component's own read reports every value in `asked` already.
///
/// `false` for a component with no read by index, or a key it cannot read:
/// the caller then reads the whole table to find out.
pub(crate) fn holds_already(
    eng: &Engine,
    entity: Entity,
    index: usize,
    asked: &toml::map::Map<String, toml::Value>,
) -> bool {
    asked
        .iter()
        .all(|(key, value)| derived_read(eng, entity, index, key).as_ref() == Some(value))
}

/// [`property`] with the definition already resolved.
#[must_use]
pub fn property_at(eng: &Engine, entity: Entity, index: usize, key: &str) -> Option<toml::Value> {
    if let Some(found) = derived_read(eng, entity, index, key) {
        return Some(found);
    }
    match get_at(eng, entity, index)? {
        toml::Value::Table(mut table) => table.remove(key),
        other => other.get(key).cloned(),
    }
}
