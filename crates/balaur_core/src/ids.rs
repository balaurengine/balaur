//! Identity for nodes spawned after the scene loaded.
//!
//! A scene file's node carries its own `id`. One a script spawns carries
//! none, and an entity index is not an identity two peers can negotiate: it
//! is reproducible within one binary and meaningless across a wire. The
//! allocator mints `<authority>:<counter>` in simulation order, so two peers
//! stepping the same simulation mint the same sequence, and two authorities
//! never mint the same id.
//!
//! Ids minted here are part of the snapshot: a rollback that re-simulates a
//! spawn has to mint the same id the first run did, so the counter is
//! restored with everything else.

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

/// The next id, consumed.
///
/// Empty when no allocator is installed, which is the signal to leave the
/// node without an id rather than to invent a colliding one.
pub fn mint(eng: &Engine) -> String {
    let Some(allocator) = eng.try_resource::<IdAllocator>() else {
        return String::new();
    };
    let mut allocator = allocator.borrow_mut();
    let n = allocator.next;
    allocator.next += 1;
    format!("{}:{n}", allocator.authority)
}

/// Mint an id and put it on a freshly spawned node.
///
/// Takes the world it is handed rather than borrowing it again, so a caller
/// mid-spawn does not deadlock on its own borrow.
pub fn assign(eng: &Engine, world: &mut World, entity: Entity) {
    let id = mint(eng);
    if id.is_empty() {
        return;
    }
    let _ = world.insert_one(entity, StableId(id));
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
