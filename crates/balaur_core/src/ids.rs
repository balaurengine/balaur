//! Identity for every node.
//!
//! A scene file's node carries its own `id`. Every other node is given one when
//! it is spawned, because an entity index is not an identity two peers can
//! negotiate: it is reproducible within one binary and meaningless across a
//! wire. The allocator mints `<authority>:<counter>` in simulation order, so
//! two peers stepping the same simulation mint the same sequence, and two
//! authorities never mint the same id.
//!
//! The counter is a component on the root, not an engine resource, so the
//! spawn functions that only hold the `World` can mint without reaching back
//! for the engine a caller is usually already borrowing it from.
//!
//! Ids minted here are part of the snapshot: a rollback that re-simulates a
//! spawn has to mint the same id the first run did, so the counter is
//! restored with everything else.

use anyhow::{Result, anyhow};
use hecs::{Entity, World};

use crate::components::StableId;
use crate::engine::Engine;
use crate::scene::collect_subtree;

/// Who is minting, and how many have been minted.
pub struct IdAllocator {
    /// One per peer, so `p2:7` is never `p1:7`. A session sets this when it
    /// learns which peer it is; a single-machine run keeps the default.
    pub authority: String,
    pub next: u64,
}

impl Default for IdAllocator {
    fn default() -> Self {
        Self {
            authority: String::from("local"),
            next: 0,
        }
    }
}

/// The root a node hangs from, which carries the counter.
fn root_of(world: &World, mut node: Entity) -> Entity {
    while let Ok(parent) = world.get::<&crate::scene::Parent>(node) {
        node = parent.0;
    }
    node
}

/// The counter the next id takes.
#[must_use]
pub fn next(eng: &Engine) -> u64 {
    counter(&eng.world(), eng.root()).next
}

/// Put the counter back, as a replay does to where its recording started.
pub fn set_next(eng: &Engine, next: u64) {
    counter(&eng.world(), eng.root()).next = next;
}

/// The next id for a node spawned under `parent`, consumed.
pub(crate) fn mint_under(world: &World, parent: Entity) -> String {
    let mut allocator = counter(world, root_of(world, parent));
    let n = allocator.next;
    allocator.next += 1;
    format!("{}:{n}", allocator.authority)
}

/// The next id, consumed.
pub fn mint(eng: &Engine) -> String {
    mint_under(&eng.world(), eng.root())
}

/// A node's tree is always an engine's, whose root carries the counter from
/// the moment it is spawned. A tree without one is a world nothing made nodes
/// in the way nodes are made.
fn counter(world: &World, root: Entity) -> hecs::RefMut<'_, IdAllocator> {
    match world.get::<&mut IdAllocator>(root) {
        Ok(allocator) => allocator,
        Err(why) => panic!("the root of this tree carries no id allocator: {why}"),
    }
}

/// An order for nodes that two runs agree on however many nodes each freed
/// before: the stable id every node is given when it is made. Entity bits are
/// reproducible only in a fresh process, which the editor is not.
///
/// # Errors
/// When the entity is not a live node: every node is given an id when it is
/// spawned, so one without is dead or was never a node.
pub fn order_key(world: &World, entity: Entity) -> Result<String> {
    world
        .get::<&StableId>(entity)
        .map(|id| id.0.clone())
        .map_err(|_| {
            let name = world.get::<&crate::scene::Name>(entity).map_or_else(
                |_| String::from("an unnamed node"),
                |n| format!("'{}'", n.0),
            );
            anyhow!("{name} is not a live node with a stable id, so nothing can order it")
        })
}

/// `nodes` in [`order_key`] order.
///
/// # Errors
/// When any of them has no id.
pub fn sort_by_id(world: &World, nodes: &mut [Entity]) -> Result<()> {
    let keys = nodes
        .iter()
        .map(|&e| order_key(world, e))
        .collect::<Result<Vec<_>>>()?;
    let mut order: Vec<usize> = (0..nodes.len()).collect();
    order.sort_by(|&a, &b| keys[a].cmp(&keys[b]));
    let sorted: Vec<Entity> = order.iter().map(|&i| nodes[i]).collect();
    nodes.copy_from_slice(&sorted);
    Ok(())
}

/// A node's stable id, if it has one.
#[must_use]
pub fn of(world: &World, entity: Entity) -> Option<String> {
    world.get::<&StableId>(entity).ok().map(|id| id.0.clone())
}

/// The node carrying `id` somewhere under `root`.
///
/// A scan rather than an index: the tree is walked in the same order the
/// digest walks it, so a duplicate id resolves the same way every run.
#[must_use]
pub fn find(world: &World, root: Entity, id: &str) -> Option<Entity> {
    collect_subtree(world, root)
        .into_iter()
        .find(|&e| world.get::<&StableId>(e).is_ok_and(|found| found.0 == id))
}
