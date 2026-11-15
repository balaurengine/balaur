//! The scene sync: every world pose and appearance brought up to date from
//! the root down, the whole tree or only what changed since the last pass.

use std::cell::RefCell;

use hecs::{Entity, World};

use super::{
    APPEARANCE, Appearance, Children, GlobalAppearance, GlobalTransform, Parent, Transform,
};
use crate::engine::Engine;

thread_local! {
    /// The frontier a placing walk takes, kept between frames: a fresh stack
    /// per node was an allocation per node.
    static PROPAGATE_STACK: RefCell<Vec<(Entity, GlobalTransform, GlobalAppearance)>> =
        const { RefCell::new(Vec::new()) };
}

/// Recompute every `GlobalTransform` and `GlobalAppearance` from the root down:
/// what a caller with no change ticks to read does, and a restore.
pub fn propagate_transforms(world: &mut World, root: Entity) {
    propagate_transforms_at(world, root, 1.0);
}

/// [`propagate_transforms`], placing every node that draws between fixed
/// steps `alpha` of the way along the pair of poses it kept.
///
/// `alpha` of 1.0 is the tick's own pose for every node, which is what a
/// caller with no frame to place wants.
pub fn propagate_transforms_at(world: &mut World, root: Entity, alpha: f32) {
    if let Ok(mut placement) = world.get::<&mut Placement>(root) {
        placement.pending.clear();
        placement.blended = alpha < 1.0;
        placement.passes += 1;
        placement.everything = true;
        placement.placed.clear();
    }
    let start = (
        root,
        GlobalTransform::identity(),
        GlobalAppearance::identity(),
    );
    walk(world, vec![start], alpha, None);
}

/// What the scene sync keeps on the root between its passes.
///
/// On the root rather than in a resource: a node is attached by a call that
/// holds only the world, and the tree it joins is where that is said.
#[derive(Default)]
pub struct Placement {
    /// Nodes attached, moved under another parent or restyled since the last
    /// pass: what no change tick names.
    pending: Vec<Entity>,
    cursor: crate::components::ticks::ChangeCursor,
    /// Whether the frame's sync has placed the whole tree yet: its first pass
    /// does, whatever a load placed before it.
    walked: bool,
    /// Whether the last pass drew between fixed steps.
    blended: bool,
    passes: u64,
    /// What the last pass placed: every node, or `placed`.
    everything: bool,
    placed: Vec<Entity>,
}

/// Hand `node` to the next pass, through the root above `from`.
pub(super) fn unplaced(world: &World, from: Entity, node: Entity) {
    let mut top = from;
    while let Ok(parent) = world.get::<&Parent>(top) {
        top = parent.0;
    }
    if let Ok(mut placement) = world.get::<&mut Placement>(top) {
        placement.pending.push(node);
    }
}

/// The frame's scene sync: every world pose and appearance brought up to
/// date, blended `alpha` of the way between fixed steps as
/// [`propagate_transforms_at`] blends. Only the subtrees under what changed
/// are placed: a `transform` the change ticks name, a node attached or moved
/// under another parent, an appearance written, a node drawn between steps;
/// a restore places everything.
pub fn sync_globals(eng: &Engine, alpha: f32) {
    let root = eng.root();
    let blending = alpha < 1.0;
    let (since, full, blended) = {
        let world = eng.world();
        let Ok(mut placement) = world.get::<&mut Placement>(root) else {
            drop(world);
            propagate_transforms_at(&mut eng.world_mut(), root, alpha);
            return;
        };
        let since = placement.cursor.read(eng);
        let full = !placement.walked || crate::components::ticks::restored_after(eng, since);
        (
            since,
            full,
            std::mem::replace(&mut placement.blended, blending),
        )
    };
    // Past the read, so a write after this pass is newer than what it covered.
    eng.advance_change_tick();
    if full {
        let mut world = eng.world_mut();
        propagate_transforms_at(&mut world, root, alpha);
        if let Ok(mut placement) = world.get::<&mut Placement>(root) {
            placement.walked = true;
        }
        return;
    }
    let index = crate::transform::INDEX;
    let mut dirty = crate::components::ticks::changed_since(eng, index, since);
    dirty.extend(crate::components::ticks::removed_since(eng, index, since));
    let mut world = eng.world_mut();
    if let Ok(mut placement) = world.get::<&mut Placement>(root) {
        dirty.append(&mut placement.pending);
    }
    // A blend moves with `alpha` every frame, and the frame after the last
    // one takes it back.
    if blending || blended {
        dirty.extend(
            world
                .query::<(Entity, &crate::interpolate::Interpolation)>()
                .iter()
                .map(|(node, _)| node),
        );
    }
    dirty.sort_unstable_by_key(|node| node.to_bits());
    dirty.dedup();
    let tops = topmost(&world, root, &dirty);
    let mut placed = world
        .get::<&mut Placement>(root)
        .map(|mut placement| std::mem::take(&mut placement.placed))
        .unwrap_or_default();
    placed.clear();
    place_subtrees(&mut world, root, &tops, alpha, &mut placed);
    if let Ok(mut placement) = world.get::<&mut Placement>(root) {
        placement.passes += 1;
        placement.everything = false;
        placement.placed = placed;
    }
}

/// The nodes of `dirty` under `root` with no other of them above: each
/// subtree is placed once, from the top.
fn topmost(world: &World, root: Entity, dirty: &[Entity]) -> Vec<Entity> {
    let named: crate::collections::DetHashSet<Entity> = dirty.iter().copied().collect();
    let parents = world.view::<&Parent>();
    dirty
        .iter()
        .copied()
        .filter(|&node| {
            let mut at = node;
            loop {
                if at == root {
                    return world.contains(node);
                }
                match parents.get(at) {
                    Some(parent) if named.contains(&parent.0) => return false,
                    Some(parent) => at = parent.0,
                    None => return false,
                }
            }
        })
        .collect()
}

/// [`propagate_transforms_at`] from each of `tops`, under its parent as the
/// last pass placed it; every node placed is added to `placed`.
fn place_subtrees(
    world: &mut World,
    root: Entity,
    tops: &[Entity],
    alpha: f32,
    placed: &mut Vec<Entity>,
) {
    let starts: Vec<(Entity, GlobalTransform, GlobalAppearance)> = tops
        .iter()
        .rev()
        .map(|&top| {
            let parent = (top != root)
                .then(|| world.get::<&Parent>(top).ok().map(|parent| parent.0))
                .flatten();
            let global = parent
                .and_then(|p| world.get::<&GlobalTransform>(p).ok().map(|g| *g))
                .unwrap_or_else(GlobalTransform::identity);
            let look = parent
                .and_then(|p| world.get::<&GlobalAppearance>(p).ok().map(|a| *a))
                .unwrap_or_else(GlobalAppearance::identity);
            (top, global, look)
        })
        .collect();
    walk(world, starts, alpha, Some(placed));
}

/// Place each start and everything under it, the last start first; every
/// node placed goes on `placed` when there is one.
fn walk(
    world: &mut World,
    starts: Vec<(Entity, GlobalTransform, GlobalAppearance)>,
    alpha: f32,
    mut placed: Option<&mut Vec<Entity>>,
) {
    // One lookup per node for all six, with no borrow flag to take: six
    // `world.get` calls were most of the pass.
    let mut view = world.view_mut::<PlacedView<'_>>();
    PROPAGATE_STACK.with_borrow_mut(|stack| {
        stack.clear();
        stack.extend(starts);
        while let Some((entity, parent_global, parent_appearance)) = stack.pop() {
            let at = (parent_global, parent_appearance);
            if place_one(&mut view, entity, at, alpha, stack)
                && let Some(placed) = placed.as_deref_mut()
            {
                placed.push(entity);
            }
        }
    });
}

/// What placing a node reads and writes, in one lookup.
type PlacedView<'a> = (
    Option<&'a crate::interpolate::Interpolation>,
    Option<&'a Transform>,
    Option<&'a mut GlobalTransform>,
    Option<&'a Appearance>,
    Option<&'a mut GlobalAppearance>,
    Option<&'a Children>,
);

/// Place `entity` under a parent placed at `parent`, and stack its children
/// to be placed under it; false for an entity that is gone.
fn place_one(
    view: &mut hecs::View<'_, PlacedView<'_>>,
    entity: Entity,
    (parent_global, parent_appearance): (GlobalTransform, GlobalAppearance),
    alpha: f32,
    stack: &mut Vec<(Entity, GlobalTransform, GlobalAppearance)>,
) -> bool {
    let Some((kept, local, global_slot, look, look_slot, children)) = view.get_mut(entity) else {
        return false;
    };
    let drawn = kept.filter(|_| alpha < 1.0).map(|kept| kept.pose(alpha));
    let global = match (drawn, local) {
        (Some(local), _) => parent_global.mul(&local),
        (None, Some(local)) => parent_global.mul(local),
        (None, None) => parent_global,
    };
    if let Some(slot) = global_slot {
        *slot = global;
    }
    let appearance = look.map_or(parent_appearance, |local| parent_appearance.mul(*local));
    if let Some(slot) = look_slot {
        if slot.visible != appearance.visible || slot.tint != appearance.tint {
            APPEARANCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        *slot = appearance;
    }
    if let Some(children) = children {
        // Reversed, so popping visits siblings in the order they are listed.
        stack.extend(children.0.iter().rev().map(|&c| (c, global, appearance)));
    }
    true
}

/// What the scene syncs placed after pass `seen`, with the passes so far:
/// `None` for every node, which a pass in between or a whole walk means.
#[must_use]
pub fn placed_since(world: &World, root: Entity, seen: u64) -> (u64, Option<Vec<Entity>>) {
    let Ok(placement) = world.get::<&Placement>(root) else {
        return (seen.wrapping_add(1), None);
    };
    let passes = placement.passes;
    let placed = match passes.wrapping_sub(seen) {
        0 => Some(Vec::new()),
        1 if !placement.everything => Some(placement.placed.clone()),
        _ => None,
    };
    (passes, placed)
}

/// `node`'s own [`Appearance`] was written: its subtree's world appearance
/// is composed again at the next sync.
pub fn appearance_written(eng: &Engine, node: Entity) {
    let world = eng.world();
    if let Ok(mut placement) = world.get::<&mut Placement>(eng.root()) {
        placement.pending.push(node);
    }
}
