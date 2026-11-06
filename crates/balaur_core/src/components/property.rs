//! One property, without the component's whole table.
//!
//! A derived component reads and writes one property by its index
//! ([`super::ComponentDef::read`] and [`super::ComponentDef::write`]). A
//! hand-written one cannot, and for those this falls back to building the
//! table and reading one key out of it, or to a whole [`super::patch`], which
//! is what a caller would have done anyway.

use std::cell::RefCell;
use std::rc::Rc;

use anyhow::{Result, anyhow};
use hecs::Entity;
use smol_str::SmolStr;

use super::derived::Given;
use super::{ComponentRegistry, get_at, index_of, patch_at, written_at};
use crate::changes::Feeds;
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
    let registry = eng
        .try_resource::<ComponentRegistry>()
        .ok_or_else(|| anyhow!("component registry missing"))?;
    let write = PropertyWrite {
        registry: &registry,
        feeds: &Feeds::of(eng),
        component: index,
        position: None,
        key: &SmolStr::new(key),
    };
    write.set(eng, entity, value.into())
}

/// Where one write goes, each part found before it. What a write owes, it owes
/// here: the change log, a kept change's old value, the lifecycle calls and the
/// component's hook.
struct PropertyWrite<'a> {
    registry: &'a RefCell<ComponentRegistry>,
    feeds: &'a Feeds,
    component: usize,
    position: Option<usize>,
    key: &'a SmolStr,
}

impl PropertyWrite<'_> {
    fn set(&self, eng: &Engine, entity: Entity, value: Given<'_>) -> Result<()> {
        // A text may name a file, which a node of another project's content
        // names from that project's root.
        let content = match value {
            Given::Text(_) if crate::project::in_content(eng, entity) => {
                crate::document_paths::written_on(eng, entity, &value.to_value())
            }
            Given::Value(held) if held.is_str() => {
                crate::document_paths::written_on(eng, entity, held)
            }
            _ => None,
        };
        let value = content.as_ref().map_or(value, Given::Value);
        // What the component held before the write, for a kept change.
        let was = self
            .feeds
            .keeps(eng, entity)
            .then(|| written_at(eng, entity, self.component))
            .flatten();
        if !self.derived(eng, entity, value) {
            let params = toml::Value::Table(toml::map::Map::from_iter([(
                self.key.to_string(),
                value.to_value(),
            )]));
            return patch_at(eng, entity, self.component, &params);
        }
        self.feeds
            .property(eng, entity, self.component, self.key, was);
        Ok(())
    }

    /// Write one property through the component's own write by index, where it
    /// has one. `false` sends the caller to a whole patch: a node without the
    /// component, which the patch adds, or a value the field refuses, which the
    /// patch reports, or one it has to resolve first (an asset's definition).
    fn derived(&self, eng: &Engine, entity: Entity, value: Given<'_>) -> bool {
        let registry = self.registry.borrow();
        let Some((_, def)) = registry.at(self.component) else {
            return false;
        };
        let at = self
            .position
            .or_else(|| def.schema.position(self.key.as_str()));
        let (Some(write), Some(at)) = (&def.write, at) else {
            return false;
        };
        let (on_replace, on_insert) = (def.lifecycle.on_replace, def.lifecycle.on_insert);
        // Asking what the node carries is a lookup of its own, for the
        // components with a call to make.
        let had = (on_replace.is_some() || on_insert.is_some())
            && super::attached_of(eng, entity).has(self.component);
        if had {
            super::call(on_replace, eng, entity);
        }
        let written = matches!(write(eng, entity, at, value), Ok(true));
        // A refused value leaves what the field held, which is written as much.
        if had {
            super::call(on_insert, eng, entity);
        }
        written
    }
}

/// One property by the component's own read by index, where it has one.
fn derived_read(eng: &Engine, entity: Entity, index: usize, key: &str) -> Option<toml::Value> {
    let registry = eng.try_resource::<super::ComponentRegistry>()?;
    let registry = registry.borrow();
    let (_, def) = registry.at(index)?;
    def.read.as_ref()?(eng, entity, def.schema.position(key)?)
}

/// Where a property sits and what a write of it feeds: its component's place
/// in the registry, its own in the schema, the registry and the change log,
/// found once by a caller that reads or writes it again and again, as a
/// script's `node.c.p` does (PLAN-balaur-script §9 step 6).
pub struct PropertyPlace {
    component: usize,
    position: usize,
    key: SmolStr,
    registry: Rc<RefCell<ComponentRegistry>>,
    feeds: Feeds,
}

/// Where `key` of component `name` sits, or `None` for either unknown.
#[must_use]
pub fn place_of(eng: &Engine, name: &str, key: &str) -> Option<PropertyPlace> {
    let registry = eng.try_resource::<ComponentRegistry>()?;
    let (component, position) = {
        let held = registry.borrow();
        let component = held.index_of(name)?;
        let (_, def) = held.at(component)?;
        (component, def.schema.position(key)?)
    };
    Some(PropertyPlace {
        component,
        position,
        key: SmolStr::new(key),
        registry,
        feeds: Feeds::of(eng),
    })
}

/// What the property at a place found before holds, handed to `out` with
/// nothing built. `false` for a node without it or a type with no such form,
/// which [`property_placed`] reads.
pub fn given_placed(
    eng: &Engine,
    entity: Entity,
    place: &PropertyPlace,
    out: &mut dyn FnMut(Given<'_>),
) -> bool {
    let registry = place.registry.borrow();
    let Some((_, def)) = registry.at(place.component) else {
        return false;
    };
    def.read_given
        .as_ref()
        .is_some_and(|read| read(eng, entity, place.position, out))
}

/// [`property`] at a place found before: the one field of a derived
/// component, the whole table's key for any other.
#[must_use]
pub fn property_placed(eng: &Engine, entity: Entity, place: &PropertyPlace) -> Option<toml::Value> {
    {
        let registry = place.registry.borrow();
        let (_, def) = registry.at(place.component)?;
        if let Some(read) = &def.read {
            return read(eng, entity, place.position);
        }
    }
    property_at(eng, entity, place.component, &place.key)
}

/// [`set_property`] at a place found before.
///
/// # Errors
/// What [`super::patch`] errors on.
pub fn set_property_placed(
    eng: &Engine,
    entity: Entity,
    place: &PropertyPlace,
    value: Given<'_>,
) -> Result<()> {
    let write = PropertyWrite {
        registry: &place.registry,
        feeds: &place.feeds,
        component: place.component,
        position: Some(place.position),
        key: &place.key,
    };
    write.set(eng, entity, value)
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
