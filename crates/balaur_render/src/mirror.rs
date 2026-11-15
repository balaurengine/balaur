//! What changed since the backend last mirrored the scene: the nodes the
//! frame's sync placed, the nodes a component was written on, put on or
//! taken off, and the nodes freed, or everything when that is not known.
//!
//! What a backend redraws from. A renderable none of these name is drawn as
//! it was, which is why a write to one past the registry says so with
//! [`look_again`].

#![cfg_attr(
    not(feature = "window"),
    allow(dead_code, reason = "the mirror is the windowed backend's")
)]

use std::collections::HashSet;

use balaur_core::Engine;
use balaur_core::hecs::Entity;

/// The nodes no change tick names that the mirror has to look at again: one
/// freed, and one whose renderable a render system rewrote in place. Kept
/// only while a backend mirrors, which is what puts it in.
#[derive(Default)]
pub(crate) struct Unseen(Vec<Entity>);

/// `node`'s renderable was rewritten past the registry, by a system rather
/// than a component's write: the mirror draws it again.
pub(crate) fn look_again(eng: &Engine, node: Entity) {
    if let Some(unseen) = eng.try_resource::<Unseen>() {
        unseen.borrow_mut().0.push(node);
    }
}

fn freed(eng: &Engine, nodes: &[Entity]) {
    if let Some(unseen) = eng.try_resource::<Unseen>() {
        unseen.borrow_mut().0.extend_from_slice(nodes);
    }
}

/// What changed since the last read: everything, or the nodes named.
pub(crate) struct Dirty {
    pub(crate) all: bool,
    pub(crate) nodes: HashSet<Entity>,
}

/// Where the mirror read up to: the scene sync's pass and the change tick.
#[derive(Default)]
pub(crate) struct Since {
    passes: u64,
    tick: u64,
    started: bool,
}

impl Since {
    /// What changed since the last read, which moves on to now.
    pub(crate) fn read(&mut self, eng: &Engine) -> Dirty {
        if !self.started {
            eng.insert_resource(Unseen::default());
            balaur_core::scene::on_freed(eng, freed);
        }
        let (passes, placed) = {
            let world = eng.world();
            balaur_core::scene::placed_since(&world, eng.root(), self.passes)
        };
        self.passes = passes;
        let written = balaur_core::components::ticks::written_since(eng, self.tick);
        self.tick = eng.change_tick();
        // Past the read, so a write after it is newer than what it covered.
        eng.advance_change_tick();
        let unseen = eng
            .try_resource::<Unseen>()
            .map(|unseen| std::mem::take(&mut unseen.borrow_mut().0))
            .unwrap_or_default();
        let started = std::mem::replace(&mut self.started, true);
        match (started, placed, written) {
            (true, Some(placed), Some(written)) => Dirty {
                all: false,
                nodes: placed.into_iter().chain(written).chain(unseen).collect(),
            },
            _ => Dirty {
                all: true,
                nodes: unseen.into_iter().collect(),
            },
        }
    }
}
