//! What holds a script instance back from a pass this frame.

use super::{BsHost, NodeId};

/// What holds an instance back from a pass: a game pause and `process` modes,
/// and the subtree a debugger stop or a paused replay holds.
#[derive(Clone, Copy)]
pub(super) struct Holds {
    pause: balaur_core::process::Pause,
    frozen: Option<balaur_core::hecs::Entity>,
}

impl BsHost {
    // What keeps an instance from a pass, read once per pass.
    pub(super) fn holds(&self) -> Holds {
        let engine = &self.inner.engine;
        Holds {
            pause: balaur_core::process::pause(engine),
            frozen: engine.frozen_root(),
        }
    }

    // Whether a debugger stop or a paused replay holds `node`.
    pub(super) fn frozen(&self, node: u64, holds: Holds) -> bool {
        holds.frozen.is_some_and(|root| {
            balaur_core::entity_of(NodeId(node)).is_ok_and(|entity| {
                balaur_core::scene::is_within(&self.inner.engine.world(), entity, root)
            })
        })
    }

    // Whether nothing holds `node` this frame.
    pub(super) fn ticks(&self, node: u64, holds: Holds) -> bool {
        !self.frozen(node, holds)
            && (!holds.pause.holds()
                || balaur_core::entity_of(NodeId(node)).is_ok_and(|entity| {
                    balaur_core::process::ticks(&self.inner.engine.world(), entity, holds.pause)
                }))
    }
}
