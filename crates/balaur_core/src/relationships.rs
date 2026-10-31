//! A pair of components kept in step (PLAN-ecs §2, Bevy's relationships): a
//! source holds the one target it points at, and the target holds every
//! source pointing at it, in order. The scene tree is the first: a node's
//! [`Parent`](crate::scene::Parent) is the relationship and its parent's
//! [`Children`](crate::scene::Children) the sources, the order the digest
//! walks.
//!
//! Every write of either side goes through the calls here, so the two cannot
//! disagree: set the one side and the other follows. Freeing a target can
//! take its sources with it ([`linked`]), which is what freeing a node does
//! to its subtree.

use hecs::{Entity, World};

/// The side a source holds: the one target it points at.
pub trait Relationship: hecs::Component + Sized {
    /// The side its target holds.
    type Sources: Sources;
    fn new(target: Entity) -> Self;
    fn target(&self) -> Entity;
}

/// The side a target holds: every source pointing at it, in order.
pub trait Sources: hecs::Component {
    fn list(&self) -> &[Entity];
    fn list_mut(&mut self) -> &mut Vec<Entity>;
}

/// The target `source` points at.
#[must_use]
pub fn target_of<R: Relationship>(world: &World, source: Entity) -> Option<Entity> {
    world.get::<&R>(source).ok().map(|held| held.target())
}

/// Every source pointing at `target`, in order.
#[must_use]
pub fn sources_of<R: Relationship>(world: &World, target: Entity) -> Vec<Entity> {
    world
        .get::<&R::Sources>(target)
        .map(|held| held.list().to_vec())
        .unwrap_or_default()
}

/// Count `source`, which already points at `target`, among the target's
/// sources: at place `at`, the end for `None` or past it. What a source
/// spawned holding its relationship needs.
pub fn link<R: Relationship>(world: &World, source: Entity, target: Entity, at: Option<usize>) {
    debug_assert_eq!(target_of::<R>(world, source), Some(target));
    if let Ok(mut sources) = world.get::<&mut R::Sources>(target) {
        let list = sources.list_mut();
        match at {
            Some(at) if at < list.len() => list.insert(at, source),
            _ => list.push(source),
        }
    }
}

/// Take `source` out of its target's sources, leaving the source's own side
/// to whoever is about to replace it or free it; answers the target.
pub fn unlink<R: Relationship>(world: &World, source: Entity) -> Option<Entity> {
    let target = target_of::<R>(world, source)?;
    if let Ok(mut sources) = world.get::<&mut R::Sources>(target) {
        sources.list_mut().retain(|&held| held != source);
    }
    Some(target)
}

/// Point `source` at `target`, at place `at` among its sources: out of the
/// sources of the target it pointed at before, into the new one's.
///
/// # Errors
/// If `source` is gone.
pub fn relate<R: Relationship>(
    world: &mut World,
    source: Entity,
    target: Entity,
    at: Option<usize>,
) -> Result<(), hecs::NoSuchEntity> {
    unlink::<R>(world, source);
    world.insert_one(source, R::new(target))?;
    link::<R>(world, source, target, at);
    Ok(())
}

/// Move `source` to place `at` among its target's sources, clamped to the
/// end; answers whether it was there to move.
pub fn place<R: Relationship>(world: &World, source: Entity, at: usize) -> bool {
    let Some(target) = target_of::<R>(world, source) else {
        return false;
    };
    let Ok(mut sources) = world.get::<&mut R::Sources>(target) else {
        return false;
    };
    let list = sources.list_mut();
    let Some(from) = list.iter().position(|&held| held == source) else {
        return false;
    };
    let moved = list.remove(from);
    list.insert(at.min(list.len()), moved);
    true
}

/// Take every source in `gone` out of `target`'s sources in one pass,
/// answering those that were there, in order: freeing many siblings scans
/// their list once.
pub fn unlink_all<R: Relationship>(
    world: &World,
    target: Entity,
    gone: &crate::collections::DetHashSet<Entity>,
) -> Vec<Entity> {
    let Ok(mut sources) = world.get::<&mut R::Sources>(target) else {
        return Vec::new();
    };
    let list = sources.list_mut();
    let taken = list
        .iter()
        .copied()
        .filter(|held| gone.contains(held))
        .collect();
    list.retain(|held| !gone.contains(held));
    taken
}

/// `target` and everything pointing at it through `R`, the sources' sources
/// included, appended to `out`: `target` first, then depth first from its
/// last source back. What freeing a target that takes its sources with it
/// frees, in the order a free has always gone; a leaf costs no allocation.
pub fn linked<R: Relationship>(world: &World, target: Entity, out: &mut Vec<Entity>) {
    out.push(target);
    let mut stack: Vec<Entity> = match world.get::<&R::Sources>(target) {
        Ok(sources) if !sources.list().is_empty() => sources.list().to_vec(),
        _ => return,
    };
    while let Some(at) = stack.pop() {
        out.push(at);
        if let Ok(sources) = world.get::<&R::Sources>(at) {
            stack.extend(sources.list().iter().copied());
        }
    }
}
