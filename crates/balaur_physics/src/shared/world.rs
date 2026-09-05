//! The per-world housekeeping both dimensions share: restamping owners after a
//! restore, pruning what freed nodes left behind, and retrying joints that
//! pointed forwards.

macro_rules! functions {
    (state = $State:ty, component = $component:literal, prune = $prune:ident) => {
        /// Point every restored collider at the entity its node has *now*.
        ///
        /// The id rides in a collider's `user_data`, so a world deserialised from
        /// before a respawn names an entity that no longer exists — and every event
        /// and query reads that field.
        fn restamp_collider_owners(state: &mut $State) {
            for (entity, handles) in &state.colliders {
                for &handle in handles {
                    let Some(collider) = state.world.colliders.get_mut(handle) else {
                        continue;
                    };
                    // The one-way platform's axis rides above the entity bits.
                    let flags = collider.user_data & !u128::from(u64::MAX);
                    collider.user_data = flags | u128::from(entity.to_bits().get());
                }
            }
        }

        /// Drop everything a freed node left behind, in every map the plugin owns.
        ///
        /// Before the pause check on purpose: an editor sits paused, and a handle
        /// left behind by a free is one script call away from indexing rapier's arena.
        pub(crate) fn $prune(eng: &Engine, state: &mut $State) {
            let world = eng.world();
            let before = state.world.colliders.len();
            state.bodies.retain(|&entity, handle| {
                if world.contains(entity) {
                    return true;
                }
                state.world.remove_body(*handle);
                false
            });
            // Rapier drops a body's colliders with the body, so a handle here can be
            // stale even when its node is alive.
            state.colliders.retain(|&entity, handles| {
                let alive = world.contains(entity);
                handles.retain(|&handle| {
                    if alive && state.world.colliders.contains(handle) {
                        return true;
                    }
                    state.world.remove_collider(handle);
                    false
                });
                !handles.is_empty()
            });
            state.joints.retain(|&entity, reference| {
                if !world.contains(entity) {
                    joint::drop_joint(&mut state.world, reference);
                    return false;
                }
                joint::is_live(&state.world, reference)
            });
            state.collider_params.retain(|e, _| world.contains(*e));
            state.joint_params.retain(|e, _| world.contains(*e));
            state.grounded.retain(|e, _| world.contains(*e));
            if state.world.colliders.len() != before {
                state.queries_ready = false;
            }
        }

        /// Make the joints whose other end had not been spawned when they were
        /// applied.
        ///
        /// A scene file names nodes in whatever order it likes, so a joint that points
        /// forwards is normal rather than an error; it is made on the first step after
        /// its partner exists.
        fn resolve_pending_joints(eng: &Engine) {
            let pending = {
                let state = eng.resource::<$State>();
                let state = state.borrow();
                joint::pending(&state)
            };
            for entity in pending {
                let params = {
                    let state = eng.resource::<$State>();
                    let state = state.borrow();
                    state.joint_params.get(&entity).cloned()
                };
                let Some(params) = params else { continue };
                if let Err(why) = joint::apply_joint(eng, entity, &params) {
                    tracing::debug!("{} is still waiting: {why:#}", $component);
                }
            }
        }
    };
}
pub(crate) use functions;
