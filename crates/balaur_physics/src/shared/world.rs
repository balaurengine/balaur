//! The per-world housekeeping both dimensions share: restamping owners after a
//! restore, pruning what freed nodes left behind, and retrying joints that
//! pointed forwards.

macro_rules! functions {
    (state = $State:ty, component = $component:expr, prune = $prune:ident) => {
        /// Point every restored collider and soft body at the entity its node
        /// has *now*.
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
            // A body names its node the same way, for the collisions under it.
            for (entity, &handle) in &state.bodies {
                if let Some(body) = state.world.bodies.get_mut(handle) {
                    body.user_data = u128::from(entity.to_bits().get());
                }
            }
            // What was asleep in the restored world, so its first step reports
            // only what changes after it.
            let asleep: Vec<Entity> = sleeping(state).collect();
            state.asleep = asleep.into_iter().collect();
            // A soft body names its node the same way, and a tear reads it; so
            // does every piece torn off it.
            for (entity, &handle) in &state.soft_bodies {
                use crate::shared::softbody::Family;
                for piece in state.world.soft_bodies.family(handle) {
                    if let Some(body) = state.world.soft_bodies.get_mut(piece) {
                        body.user_data = u128::from(entity.to_bits().get());
                    }
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
                // Rapier drops the attached colliders with the body, and the
                // body's node is gone with it.
                if let Some(body) = state.world.bodies.get(*handle) {
                    for &collider in body.colliders() {
                        let owner = state.world.colliders.get(collider);
                        let owner = owner.and_then(|c| Entity::from_bits(c.user_data as u64));
                        if let Some(node) = owner {
                            state
                                .gone
                                .insert(collider, crate::shared::events::Owner::of(node, None));
                        }
                    }
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
                    if let Some(removed) = state.world.remove_collider(handle) {
                        let body = removed.parent().and_then(|b| state.world.bodies.get(b));
                        let owner =
                            crate::shared::events::Owner::of(entity, body.map(|b| b.user_data));
                        state.gone.insert(handle, owner);
                    }
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
            // A soft body's proxy is a rigid body of rapier's own, so it goes
            // through the set that made it rather than `remove_body`.
            state.soft_bodies.retain(|&entity, handle| {
                use crate::shared::softbody::Family;
                if world.contains(entity) {
                    return true;
                }
                let w = &mut state.world;
                for piece in w.soft_bodies.family(*handle) {
                    w.soft_bodies.remove(
                        piece,
                        &mut w.islands,
                        &mut w.bodies,
                        &mut w.colliders,
                        &mut w.impulse_joints,
                        &mut w.multibody_joints,
                    );
                }
                false
            });
            state.soft_params.retain(|e, _| world.contains(*e));
            state.collider_params.retain(|e, _| world.contains(*e));
            state.joint_params.retain(|e, _| world.contains(*e));
            state.grounded.retain(|e, _| world.contains(*e));
            state.asleep.retain(|e| world.contains(*e));
            if state.world.colliders.len() != before {
                state.queries_ready = false;
            }
        }

        /// Every body and soft body asleep now, in the order they were made.
        fn sleeping(state: &$State) -> impl Iterator<Item = Entity> + '_ {
            let bodies = state.bodies.iter().filter(|(_, handle)| {
                state
                    .world
                    .bodies
                    .get(**handle)
                    .is_some_and(|b| b.is_sleeping())
            });
            let soft = state.soft_bodies.iter().filter(|(_, handle)| {
                state
                    .world
                    .soft_bodies
                    .get(**handle)
                    .is_some_and(|b| b.is_sleeping())
            });
            bodies.map(|(e, _)| *e).chain(soft.map(|(e, _)| *e))
        }

        /// The bodies and soft bodies that fell asleep or woke this step, with
        /// whether each sleeps now, remembered for the next.
        fn sleep_changes(state: &mut $State) -> Vec<(Entity, bool)> {
            let now: balaur_core::collections::DetHashSet<Entity> = sleeping(state).collect();
            let changed: Vec<(Entity, bool)> = state
                .bodies
                .keys()
                .chain(state.soft_bodies.keys())
                .filter(|e| now.contains(*e) != state.asleep.contains(*e))
                .map(|e| (*e, now.contains(e)))
                .collect();
            state.asleep = now;
            changed
        }

        /// Announce each change [`sleep_changes`] found, from the node.
        fn announce_sleep(eng: &Engine, changed: &[(Entity, bool)]) {
            for &(entity, asleep) in changed {
                let payload = balaur_script::Value::Bool(asleep);
                balaur_core::events::announce(eng, entity, hook::SLEEPING_CHANGED, payload);
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
                joint::pending(&state, &eng.world())
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
