//! The node a free is taking every component off, which its components'
//! `remove` hooks ask about.

use hecs::Entity;

thread_local! {
    /// The node [`super::remove_present`] is taking every component off.
    static FREEING: std::cell::Cell<Option<Entity>> = const { std::cell::Cell::new(None) };
}

/// Whether `entity` is being freed: the despawn right after its `remove` hooks
/// drops whatever they leave, so a hook does what its removal owes everything
/// else and leaves the component on. Each one taken off would move the node to
/// another archetype, a copy of everything it holds.
#[must_use]
pub fn freeing(entity: Entity) -> bool {
    FREEING.with(|freeing| freeing.get() == Some(entity))
}

/// Run `hooks` with `entity` the node being freed.
pub(super) fn during<R>(entity: Entity, hooks: impl FnOnce() -> R) -> R {
    let outer = FREEING.with(|freeing| freeing.replace(Some(entity)));
    let out = hooks();
    FREEING.with(|freeing| freeing.set(outer));
    out
}
