//! Which components a node carries, as bits.
//!
//! Split from the registry because it answers a different question. The
//! registry says what a component *is*; this says which nodes *have* one, and
//! it is read on every presence test and written on every attach.

use hecs::Entity;

use crate::engine::Engine;

/// The most components one build may register: a bit each in [`Attached`].
pub const MAX_COMPONENTS: usize = 128;

/// Which registered components a node carries, one bit per definition in
/// registration order.
///
/// Two masks, because the two questions have different answers. `present` is
/// what the node has, which is what a presence test and `component_names`
/// read. `hooked` is what the registry attached, which is the set of `remove`
/// hooks a free still owes. They differ for exactly one component: the node
/// bundle carries a `Transform`, so `transform` is present on almost every
/// node and hooked on none of them, and freeing fifty thousand nodes still
/// asks no plugin anything.
///
/// A component on the entity rather than a map on the engine: the node is
/// what the bits belong to, hecs drops them with it, and a read is an
/// archetype lookup instead of a resource borrow and a hash.
#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Attached {
    pub present: u128,
    pub hooked: u128,
}

/// `transform` is the first component core registers, so it owns bit 0, and
/// the node bundle can say a node has one without reaching the registry.
/// [`ComponentRegistry::insert`] asserts the index, so registering anything
/// ahead of it fails at boot rather than mislabelling every node.
pub const TRANSFORM_BIT: u128 = 1;

impl Attached {
    /// What the node bundle gives a node it spawns with a `Transform`.
    #[must_use]
    pub const fn with_transform() -> Self {
        Self {
            present: TRANSFORM_BIT,
            hooked: 0,
        }
    }

    /// Whether the definition at `index` is on the node.
    #[must_use]
    pub const fn has(&self, index: usize) -> bool {
        self.present & (1u128 << index) != 0
    }
}

/// Set or clear one node's bit for the definition at `index`.
pub(super) fn mark(eng: &Engine, entity: Entity, index: usize, on: bool) {
    let bit = 1u128 << index;
    let mut world = eng.world_mut();
    if let Ok(mut bits) = world.get::<&mut Attached>(entity) {
        if on {
            bits.present |= bit;
            bits.hooked |= bit;
        } else {
            bits.present &= !bit;
            bits.hooked &= !bit;
        }
        return;
    }
    if !on {
        return;
    }
    let _ = world.insert_one(
        entity,
        Attached {
            present: bit,
            hooked: bit,
        },
    );
}

/// Say the node has the definition at `index` without owing its `remove` hook.
///
/// For state a node acquires outside [`add`]: the bundle's `Transform`, and
/// [`crate::transform::ensure`] giving one to a node a body was put on. The
/// hook belongs to whoever attached it, which here is core.
pub(crate) fn mark_present(world: &mut hecs::World, entity: Entity, bit: u128) {
    if let Ok(mut bits) = world.get::<&mut Attached>(entity) {
        bits.present |= bit;
        return;
    }
    let _ = world.insert_one(
        entity,
        Attached {
            present: bit,
            hooked: 0,
        },
    );
}

/// The bits a node carries, or none when it carries nothing.
#[must_use]
pub fn attached_of(eng: &Engine, entity: Entity) -> Attached {
    eng.world()
        .get::<&Attached>(entity)
        .map(|bits| *bits)
        .unwrap_or_default()
}
