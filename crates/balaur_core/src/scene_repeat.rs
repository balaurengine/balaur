//! `scene.repeat`: one instance of a scene file per item under a host, kept
//! from call to call, so a list asked for every pass builds a node only when
//! it grows and frees none when it shrinks. Each instance is named for its
//! item's key: input finds a node by its path, so siblings need their own names.

use anyhow::{Result, anyhow};
use balaur_script::Value;
use hecs::Entity;

use crate::collections::{DetHashMap, DetHashSet};
use crate::engine::Engine;
use crate::scene::{Children, Parent};
use crate::vocabulary::keys;

/// The instances each host holds: the last call's items in order, then its
/// spares; and how many each host has built, which numbers the next.
#[derive(Default)]
struct Repeats {
    slots: DetHashMap<Entity, Vec<Slot>>,
    built: DetHashMap<Entity, usize>,
}

/// What a hidden instance is named, numbered among the host's spares.
const SPARE: &str = "spare";

struct Slot {
    scene: String,
    /// The node's name: its item's key, or its spare's.
    key: String,
    node: Entity,
}

struct Item<'a> {
    key: &'a str,
    scene: &'a str,
}

/// `scene.repeat(host, items)`: the node standing for each item, in order.
pub(crate) fn scene_repeat(eng: &Engine, args: &[Value]) -> Result<Value> {
    let host = crate::engine_api::optional_node(args, 0)?
        .ok_or_else(|| anyhow!("repeat needs the node the items go under"))?;
    let items = items(args.get(1))?;
    let mut slots = take(eng, host);
    let mut used = vec![false; slots.len()];
    let mut chosen: Vec<Option<usize>> = vec![None; items.len()];
    let held: DetHashMap<(&str, &str), usize> = slots
        .iter()
        .enumerate()
        .map(|(at, slot)| ((slot.scene.as_str(), slot.key.as_str()), at))
        .collect();
    for (index, item) in items.iter().enumerate() {
        if let Some(&at) = held.get(&(item.scene, item.key)) {
            used[at] = true;
            chosen[index] = Some(at);
        }
    }
    for (index, item) in items.iter().enumerate() {
        if chosen[index].is_some() {
            continue;
        }
        let spare = (0..slots.len()).find(|&at| !used[at] && slots[at].scene == item.scene);
        let at = if let Some(at) = spare {
            at
        } else {
            let node = match build(eng, host, item.scene) {
                Ok(node) => node,
                Err(why) => {
                    // What was built stays this host's, so a later call finds
                    // it, and out of sight until one names it.
                    for slot in slots.iter().filter(|slot| slot.key.is_empty()) {
                        crate::scene::set_visible(eng, slot.node, false);
                    }
                    eng.resource::<Repeats>()
                        .borrow_mut()
                        .slots
                        .insert(host, slots);
                    return Err(why);
                }
            };
            slots.push(Slot {
                scene: item.scene.to_owned(),
                key: String::new(),
                node,
            });
            used.push(true);
            slots.len() - 1
        };
        used[at] = true;
        chosen[index] = Some(at);
    }
    let order: Vec<usize> = chosen.into_iter().flatten().collect();
    for (&at, item) in order.iter().zip(&items) {
        let slot = &mut slots[at];
        if slot.key != item.key {
            crate::scene::rename(&eng.world(), slot.node, item.key);
            item.key.clone_into(&mut slot.key);
        }
        crate::scene::set_visible(eng, slot.node, true);
    }
    // A spare gives up its key's name, which a node of another scene may now hold.
    let mut spares = 0;
    for (at, slot) in slots.iter_mut().enumerate() {
        if !used[at] {
            let name = format!("{SPARE}{spares}");
            spares += 1;
            if slot.key != name {
                crate::scene::rename(&eng.world(), slot.node, &name);
                slot.key = name;
            }
            crate::scene::set_visible(eng, slot.node, false);
        }
    }
    let nodes: Vec<Entity> = order.iter().map(|&at| slots[at].node).collect();
    place(eng, host, &nodes, &slots);
    let mut kept: Vec<Slot> = Vec::with_capacity(slots.len());
    let mut rest: Vec<Option<Slot>> = slots.into_iter().map(Some).collect();
    for &at in &order {
        kept.extend(rest[at].take());
    }
    kept.extend(rest.into_iter().flatten());
    eng.resource::<Repeats>()
        .borrow_mut()
        .slots
        .insert(host, kept);
    Ok(Value::List(
        nodes
            .into_iter()
            .map(|node| Value::Node(crate::node_id_of(node).0))
            .collect(),
    ))
}

fn items(value: Option<&Value>) -> Result<Vec<Item<'_>>> {
    let Some(Value::List(list)) = value else {
        return Err(anyhow!(
            "repeat takes a list of {{ key, scene }}, got {value:?}"
        ));
    };
    let mut seen = DetHashSet::default();
    list.iter()
        .map(|entry| {
            let Value::Map(pairs) = entry else {
                return Err(anyhow!("a repeat item is {{ key, scene }}, got {entry:?}"));
            };
            let field = |name: &str| pairs.iter().find(|(k, _)| k == name).map(|(_, v)| v);
            let (Some(Value::Str(key)), Some(Value::Str(scene))) =
                (field(keys::KEY), field(keys::SCENE))
            else {
                return Err(anyhow!(
                    "a repeat item is {{ key: string, scene: string }}, got {entry:?}"
                ));
            };
            if key.is_empty() || key.contains('/') {
                return Err(anyhow!(
                    "a repeat key names its node, so `{key}` cannot be one"
                ));
            }
            if !seen.insert(key.as_str()) {
                return Err(anyhow!("two repeat items share the key `{key}`"));
            }
            Ok(Item { key, scene })
        })
        .collect()
}

/// The host's slots whose node is still its child: one freed or moved away
/// by a script is no longer the repeat's.
fn take(eng: &Engine, host: Entity) -> Vec<Slot> {
    if eng.try_resource::<Repeats>().is_none() {
        eng.insert_resource(Repeats::default());
        crate::scene::on_freed(eng, forget);
    }
    let slots = eng
        .resource::<Repeats>()
        .borrow_mut()
        .slots
        .swap_remove(&host);
    let world = eng.world();
    slots
        .unwrap_or_default()
        .into_iter()
        .filter(|slot| world.get::<&Parent>(slot.node).is_ok_and(|p| p.0 == host))
        .collect()
}

fn forget(eng: &Engine, freed: &[Entity]) {
    if let Some(repeats) = eng.try_resource::<Repeats>() {
        let mut repeats = repeats.borrow_mut();
        for node in freed {
            repeats.slots.swap_remove(node);
            repeats.built.swap_remove(node);
        }
    }
}

/// One instance of `scene` under `host`, its scripts running: the one root
/// the file holds. Its stable ids are the file's under `<host id>/<n>/`, so a
/// binding row finding its node by id finds this instance's.
fn build(eng: &Engine, host: Entity, scene: &str) -> Result<Entity> {
    let number = {
        let repeats = eng.resource::<Repeats>();
        let mut repeats = repeats.borrow_mut();
        let count = repeats.built.entry(host).or_insert(0);
        *count += 1;
        *count
    };
    let owner = eng
        .world()
        .get::<&crate::components::StableId>(host)
        .map_or_else(|_| host.to_bits().to_string(), |id| id.0.clone());
    let before = crate::scene::child_count(&eng.world(), host);
    let built =
        crate::project::instantiate_scene_file_as(eng, scene, host, format!("{owner}/{number}/"));
    let added: Vec<Entity> = eng
        .world()
        .get::<&Children>(host)
        .map(|c| c.0.iter().skip(before).copied().collect())
        .unwrap_or_default();
    if built.is_ok()
        && let [root] = added.as_slice()
    {
        crate::scene::announce_added_since(eng, host, before);
        return Ok(*root);
    }
    // Built in part, or not one item: none of it is left under the host.
    crate::scene::free_nodes(eng, &added);
    built?;
    Err(anyhow!(
        "a repeated scene has one root node; `{scene}` has {}",
        added.len()
    ))
}

/// The items' nodes after the host's other children, in order, moving only
/// a node that is out of place.
fn place(eng: &Engine, host: Entity, nodes: &[Entity], slots: &[Slot]) {
    let world = eng.world();
    let ours: DetHashSet<Entity> = slots.iter().map(|slot| slot.node).collect();
    let children = |world: &hecs::World| -> Vec<Entity> {
        world
            .get::<&Children>(host)
            // copy: ends the `Children` borrow, which a move writes
            .map(|c| c.0.clone())
            .unwrap_or_default()
    };
    let base = children(&world)
        .iter()
        .filter(|child| !ours.contains(*child))
        .count();
    let mut now = children(&world);
    for (offset, &node) in nodes.iter().enumerate() {
        if now.get(base + offset) != Some(&node) {
            crate::scene::move_child_to(&world, host, node, base + offset);
            now = children(&world);
        }
    }
}
