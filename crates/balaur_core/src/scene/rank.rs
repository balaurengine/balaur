//! The tree in order, kept until its shape moves: what a per-frame pass
//! sorts the few nodes it wants by, instead of walking every node.

use std::cell::RefCell;
use std::rc::Rc;

use hecs::{Entity, World};

use super::{Children, shape_revision};

/// Every node under a root in tree order, a node before its children and a
/// child's subtree before the next child's, with each node's place in it.
///
/// What a per-frame pass that wants its nodes in tree order sorts by: it
/// queries the few nodes holding its component and sorts them by
/// [`TreeRank::of`], rather than walking every node of the tree to find them.
/// Built once and kept until [`shape_revision`] moves.
pub struct TreeRank {
    order: Vec<Entity>,
    rank: crate::collections::DetHashMap<Entity, u32>,
}

impl TreeRank {
    /// Where `entity` stands in tree order; `None` for one not under the root.
    #[must_use]
    pub fn of(&self, entity: Entity) -> Option<u32> {
        self.rank.get(&entity).copied()
    }

    /// Every node under the root, the root first, in tree order. A node freed
    /// since may still be in it, and is skipped by asking the world.
    #[must_use]
    pub fn order(&self) -> &[Entity] {
        &self.order
    }

    /// `found` in tree order, dropping any not under the root.
    #[must_use]
    pub fn sorted(&self, found: impl IntoIterator<Item = Entity>) -> Vec<Entity> {
        let mut ranked: Vec<(u32, Entity)> = found
            .into_iter()
            .filter_map(|entity| self.of(entity).map(|rank| (rank, entity)))
            .collect();
        ranked.sort_unstable_by_key(|(rank, _)| *rank);
        ranked.into_iter().map(|(_, entity)| entity).collect()
    }
}

/// Which world, root and tree shape a ranking was built for.
type RankedFor = (usize, Entity, u64);

thread_local! {
    /// The last ranking built, and what it was built for.
    static RANKED: RefCell<Option<(RankedFor, Rc<TreeRank>)>> = const { RefCell::new(None) };
}

/// The tree under `root` in order, built when [`shape_revision`] moved
/// since the last call for this world and root, kept otherwise.
#[must_use]
pub fn tree_rank(world: &World, root: Entity) -> Rc<TreeRank> {
    let key = (std::ptr::from_ref(world) as usize, root, shape_revision());
    if let Some(kept) = RANKED.with(|ranked| {
        ranked
            .borrow()
            .as_ref()
            .filter(|(at, _)| *at == key)
            .map(|(_, rank)| rank.clone())
    }) {
        return kept;
    }
    let mut order = Vec::new();
    let mut stack = vec![root];
    while let Some(entity) = stack.pop() {
        order.push(entity);
        if let Ok(children) = world.get::<&Children>(entity) {
            stack.extend(children.0.iter().rev().copied());
        }
    }
    let rank = order
        .iter()
        .enumerate()
        .map(|(at, &entity)| (entity, u32::try_from(at).unwrap_or(u32::MAX)))
        .collect();
    let built = Rc::new(TreeRank { order, rank });
    RANKED.with(|ranked| *ranked.borrow_mut() = Some((key, built.clone())));
    built
}
