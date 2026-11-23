//! The order a built scene's scripts attach in.

#[allow(
    clippy::wildcard_imports,
    reason = "a part of the module above, split for length"
)]
use super::*;

/// Reorder scripts waiting to attach so every child's comes before its
/// parent's and siblings keep their order.
///
/// A child's `init` can only reach a parent that exists, which it does before
/// any script runs; a parent's `init` reading a child's script state is the
/// one that would find nothing, so children go first. Only the children
/// `base` gained from its `from`th on are walked, unless a script is on a node
/// outside them: a `repeat` builds each item beside every one before it.
pub(super) fn children_first(
    eng: &Engine,
    base: Entity,
    from: usize,
    pending: &mut [PendingScript],
) {
    fn walk(world: &hecs::World, node: Entity, order: &mut DetHashMap<Entity, usize>) {
        if let Ok(children) = world.get::<&scene::Children>(node) {
            for &child in &children.0 {
                walk(world, child, order);
            }
        }
        let next = order.len();
        order.insert(node, next);
    }
    if pending.is_empty() {
        return;
    }
    let world = eng.world();
    let added: Vec<Entity> = world
        .get::<&scene::Children>(base)
        .map(|children| children.0.iter().skip(from).copied().collect())
        .unwrap_or_default();
    let mut order = DetHashMap::default();
    for child in added {
        walk(&world, child, &mut order);
    }
    if pending
        .iter()
        .any(|(entity, _, _)| !order.contains_key(entity))
    {
        order.clear();
        walk(&world, base, &mut order);
    }
    pending.sort_by_key(|(entity, _, _)| order.get(entity).copied().unwrap_or(usize::MAX));
}
