//! Writes asked for now and made later (PLAN-ecs §2): what a system reading
//! the world, or a lifecycle call, asks for instead of changing the world
//! under itself.
//!
//! Two queues. A command is made at the end of the stage it was asked in, or
//! of the fixed step, in the order asked; one a command asks for runs in the
//! same pass, after the rest. A free is the frame's: every node queued for
//! freeing goes in one batch in `Last`, as a script's `queue_free` says.

use hecs::{DynamicBundle, Entity};

use crate::engine::Engine;

/// One queued write.
pub(crate) type Deferred = Box<dyn FnOnce(&Engine)>;

impl Engine {
    /// Run `write` at the end of this stage, after what was asked before it.
    pub fn defer(&self, write: impl FnOnce(&Engine) + 'static) {
        self.inner.deferred.borrow_mut().push(Box::new(write));
    }

    /// A node named `name` under `parent`, made at the end of this stage: the
    /// entity is reserved now, so a later command can name it, and holds
    /// nothing until then.
    pub fn defer_spawn_node(&self, name: &str, parent: Entity) -> Entity {
        let entity = self.inner.world.borrow().reserve_entity();
        let name = name.to_owned();
        self.defer(move |eng| {
            crate::scene::spawn_node_into(&mut eng.world_mut(), entity, &name, parent);
        });
        entity
    }

    /// An entity holding `components`, made at the end of this stage; reserved
    /// now, as [`Self::defer_spawn_node`] reserves one.
    pub fn defer_spawn(&self, components: impl DynamicBundle + 'static) -> Entity {
        let entity = self.inner.world.borrow().reserve_entity();
        self.defer(move |eng| {
            if eng.insert(entity, components).is_err() {
                gone("spawn", entity);
            }
        });
        entity
    }

    /// Put `components` on `entity` at the end of this stage.
    pub fn defer_insert(&self, entity: Entity, components: impl DynamicBundle + 'static) {
        self.defer(move |eng| {
            if eng.insert(entity, components).is_err() {
                gone("insert", entity);
            }
        });
    }

    /// Put one component on `entity` at the end of this stage.
    pub fn defer_insert_one<T: hecs::Component>(&self, entity: Entity, component: T) {
        self.defer(move |eng| {
            if eng.insert_one(entity, component).is_err() {
                gone("insert_one", entity);
            }
        });
    }

    /// Take a `T` off `entity` at the end of this stage; one it does not hold
    /// is nothing to take.
    pub fn defer_remove_one<T: hecs::Component>(&self, entity: Entity) {
        self.defer(move |eng| {
            let _ = eng.remove_one::<T>(entity);
        });
    }

    /// Despawn `entity` alone at the end of this stage: an entity that is not
    /// a node. A node is freed with [`Self::defer_free`], which takes its
    /// subtree and runs what its components owe.
    pub fn defer_despawn(&self, entity: Entity) {
        self.defer(move |eng| {
            if eng.despawn(entity).is_err() {
                gone("despawn", entity);
            }
        });
    }

    /// Free `node` and its subtree with the frame's other frees, in `Last`.
    pub fn defer_free(&self, node: Entity) {
        self.inner.frees.borrow_mut().push(node);
    }

    /// The frees queued so far, taken.
    pub fn take_frees(&self) -> Vec<Entity> {
        std::mem::take(&mut self.inner.frees.borrow_mut())
    }

    /// Make every queued command, in the order asked, those the commands ask
    /// for included; answers how many ran.
    pub fn apply_deferred(&self) -> usize {
        let mut ran = 0;
        loop {
            let queued = std::mem::take(&mut *self.inner.deferred.borrow_mut());
            if queued.is_empty() {
                return ran;
            }
            ran += queued.len();
            for write in queued {
                write(self);
            }
        }
    }
}

/// A command that found its entity gone: said, since the write it asked for
/// did not happen.
fn gone(command: &str, entity: Entity) {
    tracing::warn!(command, ?entity, "a deferred command's entity is gone");
}
