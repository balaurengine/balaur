//! One property, without the component's whole table.
//!
//! A derived component reads and writes one property by its index
//! ([`super::ComponentDef::read`] and [`super::ComponentDef::write`]). A
//! hand-written one cannot, and for those this falls back to building the
//! table and reading one key out of it, or to a whole [`super::patch`], which
//! is what a caller would have done anyway.

use anyhow::{Result, anyhow};
use hecs::Entity;

use super::{get_at, index_of, patch_at, written_at};
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
    set_property_known(eng, entity, index, key, None, value)
}

/// [`set_property_at`] with the property's place in the schema when the
/// caller found it before, so the write by index looks nothing up.
fn set_property_known(
    eng: &Engine,
    entity: Entity,
    index: usize,
    key: &str,
    position: Option<usize>,
    value: &toml::Value,
) -> Result<()> {
    // A text may name a file, which a node of another project's content names
    // from that project's root.
    let content = value
        .is_str()
        .then(|| crate::document_paths::written_on(eng, entity, value))
        .flatten();
    let value = content.as_ref().unwrap_or(value);
    // What the component held before the write, for a kept change.
    let was = crate::changes::keeps(eng, entity)
        .then(|| written_at(eng, entity, index))
        .flatten();
    if !derived_write(eng, entity, index, key, position, value) {
        let params = toml::Value::Table(toml::map::Map::from_iter([(
            key.to_string(),
            value.clone(),
        )]));
        return patch_at(eng, entity, index, &params);
    }
    crate::changes::property(eng, entity, index, key, was);
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
    position: Option<usize>,
    value: &toml::Value,
) -> bool {
    let Some(registry) = eng.try_resource::<super::ComponentRegistry>() else {
        return false;
    };
    let registry = registry.borrow();
    let Some((_, def)) = registry.at(index) else {
        return false;
    };
    let at = position.or_else(|| def.schema.position(key));
    let (Some(write), Some(at)) = (&def.write, at) else {
        return false;
    };
    let lifecycle = def.lifecycle;
    let had = super::attached_of(eng, entity).has(index);
    if had {
        super::call(lifecycle.on_replace, eng, entity);
    }
    let written = matches!(write(eng, entity, at, value), Ok(true));
    // A refused value leaves what the field held, which is written as much.
    if had {
        super::call(lifecycle.on_insert, eng, entity);
    }
    written
}

/// One property by the component's own read by index, where it has one.
fn derived_read(eng: &Engine, entity: Entity, index: usize, key: &str) -> Option<toml::Value> {
    let registry = eng.try_resource::<super::ComponentRegistry>()?;
    let registry = registry.borrow();
    let (_, def) = registry.at(index)?;
    def.read.as_ref()?(eng, entity, def.schema.position(key)?)
}

/// Where a property sits: its component's place in the registry and its own
/// in the schema, found once by a caller that reads or writes it again and
/// again, as a script's `node.c.p` does (PLAN-balaur-script §9 step 6).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PropertyPlace {
    pub component: usize,
    pub position: usize,
}

/// Where `key` of component `name` sits, or `None` for either unknown.
#[must_use]
pub fn place_of(eng: &Engine, name: &str, key: &str) -> Option<PropertyPlace> {
    let registry = eng.try_resource::<super::ComponentRegistry>()?;
    let registry = registry.borrow();
    let component = registry.index_of(name)?;
    let (_, def) = registry.at(component)?;
    let position = def.schema.position(key)?;
    Some(PropertyPlace {
        component,
        position,
    })
}

/// [`property`] at a place found before: the one field of a derived
/// component, the whole table's key for any other.
#[must_use]
pub fn property_placed(eng: &Engine, entity: Entity, place: PropertyPlace) -> Option<toml::Value> {
    let key = {
        let registry = eng.try_resource::<super::ComponentRegistry>()?;
        let registry = registry.borrow();
        let (_, def) = registry.at(place.component)?;
        if let Some(read) = &def.read {
            return read(eng, entity, place.position);
        }
        def.schema.order().get(place.position)?.clone()
    };
    property_at(eng, entity, place.component, &key)
}

/// [`set_property`] at a place found before.
///
/// # Errors
/// What [`super::patch`] errors on.
pub fn set_property_placed(
    eng: &Engine,
    entity: Entity,
    place: PropertyPlace,
    value: &toml::Value,
) -> Result<()> {
    let key = {
        let registry = eng
            .try_resource::<super::ComponentRegistry>()
            .ok_or_else(|| anyhow!("component registry missing"))?;
        let registry = registry.borrow();
        let (_, def) = registry
            .at(place.component)
            .ok_or_else(|| anyhow!("no component at {}", place.component))?;
        def.schema
            .order()
            .get(place.position)
            .ok_or_else(|| anyhow!("no property at {}", place.position))?
            .clone()
    };
    set_property_known(
        eng,
        entity,
        place.component,
        &key,
        Some(place.position),
        value,
    )
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
