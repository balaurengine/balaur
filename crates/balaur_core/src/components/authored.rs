//! What a scene or a script asked of each component, beside the live state:
//! a component's `get` is the only way back from that state to a table.

use hecs::Entity;

use super::index_of;
use crate::engine::Engine;

/// What a scene or a script asked of each component, by node and definition.
///
/// A component's live state is its own Rust struct, and `get` is the only way
/// back to a table. [`patch`](super::patch) builds on this rather than on
/// `get` alone, so a `get` that does not mention a property cannot have it
/// reset.
#[derive(Default)]
pub struct Authored(pub crate::collections::DetHashMap<(Entity, usize), toml::Value>);

/// Merge what is being asked for into what was asked before, for the
/// definition at `index`.
pub(super) fn record_at(
    eng: &Engine,
    entity: Entity,
    index: usize,
    params: Option<&toml::Value>,
    over: bool,
) {
    let Some(authored) = eng.try_resource::<Authored>() else {
        return;
    };
    let mut authored = authored.borrow_mut();
    let slot = authored
        .0
        .entry((entity, index))
        .or_insert_with(|| toml::Value::Table(toml::map::Map::new()));
    if over {
        *slot = toml::Value::Table(toml::map::Map::new());
    }
    let (Some(slot), Some(asked)) = (slot.as_table_mut(), params.and_then(toml::Value::as_table))
    else {
        return;
    };
    for (key, value) in asked {
        slot.insert(key.clone(), value.clone());
    }
}

/// [`record_at`] for one property, with no table built to carry it: a fast
/// property write pays this every time.
pub(super) fn record_one(
    eng: &Engine,
    entity: Entity,
    index: usize,
    key: &str,
    value: &toml::Value,
) {
    let Some(authored) = eng.try_resource::<Authored>() else {
        return;
    };
    let mut authored = authored.borrow_mut();
    let slot = authored
        .0
        .entry((entity, index))
        .or_insert_with(|| toml::Value::Table(toml::map::Map::new()));
    let Some(slot) = slot.as_table_mut() else {
        return;
    };
    match slot.get_mut(key) {
        Some(held) => *held = value.clone(),
        None => {
            slot.insert(key.to_string(), value.clone());
        }
    }
}

/// What was asked of this component before now, if anything.
pub(super) fn asked_for_at(eng: &Engine, entity: Entity, index: usize) -> Option<toml::Value> {
    let authored = eng.try_resource::<Authored>()?;
    let asked = authored.borrow().0.get(&(entity, index)).cloned();
    drop(authored);
    asked
}

/// Forget what was asked of one component on one node.
pub(super) fn forget(eng: &Engine, entity: Entity, name: &str) {
    let (Some(authored), Some(index)) = (eng.try_resource::<Authored>(), index_of(eng, name))
    else {
        return;
    };
    authored.borrow_mut().0.swap_remove(&(entity, index));
}
