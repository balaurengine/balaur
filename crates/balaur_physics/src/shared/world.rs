//! The per-world housekeeping both dimensions share: restamping owners after a
//! restore, pruning what freed nodes left behind, and retrying joints that
//! pointed forwards.

macro_rules! functions {
    (state = $State:ty, component = $component:expr, forget = $forget:ident, collider_handle = $ColliderHandle:ty, body = $Body:ty, collider = $Collider:ty, soft = $Soft:ty, events = $Events:ty) => {
        /// Bring these nodes' colliders, every one where `everyone`, to report
        /// what is heard from them and from the body they hang on.
        pub(crate) fn follow_hearers(eng: &Engine, everyone: bool, nodes: &[Entity]) {
            use crate::shared::softbody::Family;
            use crate::vocabulary::hook;
            use balaur_core::events::is_heard;
            let state = eng.resource::<$State>();
            let mut state = state.borrow_mut();
            let state = &mut *state;
            let mut handles = Vec::new();
            if everyone {
                handles.extend(state.world.colliders.iter().map(|(handle, _)| handle));
            }
            for node in nodes.iter().filter(|_| !everyone) {
                handles.extend(state.colliders.get(node).into_iter().flatten().copied());
                if let Some(body) = state
                    .bodies
                    .get(node)
                    .and_then(|&handle| state.world.bodies.get(handle))
                {
                    handles.extend_from_slice(body.colliders());
                }
                if let Some(&soft) = state.soft_bodies.get(node) {
                    for piece in state.world.soft_bodies.family(soft) {
                        if let Some(body) = state.world.soft_bodies.get(piece) {
                            handles.extend(body.meshes().map(|mesh| mesh.collider()));
                        }
                    }
                }
            }
            for handle in handles {
                let Some(collider) = state.world.colliders.get(handle) else {
                    continue;
                };
                let Some(owner) = Entity::from_bits(collider.user_data as u64) else {
                    continue;
                };
                let body = collider
                    .parent()
                    .and_then(|body| state.world.bodies.get(body))
                    .and_then(|body| Entity::from_bits(body.user_data as u64));
                let heard = |name: &str| {
                    is_heard(eng, owner, name) || body.is_some_and(|body| is_heard(eng, body, name))
                };
                let mut wanted = <$Events>::empty();
                wanted.set(
                    <$Events>::COLLISION_EVENTS,
                    heard(hook::COLLISION_ENTER) || heard(hook::COLLISION_EXIT),
                );
                wanted.set(<$Events>::CONTACT_FORCE_EVENTS, heard(hook::CONTACT_FORCE));
                if collider.active_events() != wanted
                    && let Some(collider) = state.world.colliders.get_mut(handle)
                {
                    collider.set_active_events(wanted);
                }
            }
        }

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
                    collider.user_data = u128::from(entity.to_bits().get());
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

        /// Drop what freed nodes left in the maps the plugin owns, in the order
        /// the free visits them. Each component's way off took the rest.
        pub(crate) fn $forget(state: &mut $State, nodes: &[Entity]) {
            let before = state.world.colliders.len();
            let mut lightened = Vec::new();
            let mut jointed = false;
            for &entity in nodes {
                if let Some(handle) = state.bodies.shift_remove(&entity) {
                    // Rapier drops the attached colliders with the body, and the
                    // body's node is gone with it.
                    let mut dropped = Vec::new();
                    if let Some(body) = state.world.bodies.get(handle) {
                        for &collider in body.colliders() {
                            let owner = state.world.colliders.get(collider);
                            let owner = owner.and_then(|c| Entity::from_bits(c.user_data as u64));
                            if let Some(node) = owner {
                                state
                                    .gone
                                    .insert(collider, crate::shared::events::Owner::of(node, None));
                                dropped.push((node, collider));
                            }
                        }
                    }
                    jointed |= state
                        .world
                        .impulse_joints
                        .attached_joints(handle)
                        .next()
                        .is_some()
                        || state
                            .world
                            .multibody_joints
                            .rigid_body_link(handle)
                            .is_some();
                    state.world.remove_body(handle);
                    for (owner, collider) in dropped {
                        forget_collider(state, owner, collider);
                    }
                }
                for handle in state.colliders.shift_remove(&entity).unwrap_or_default() {
                    state.surfaces.swap_remove(&handle);
                    if let Some(removed) = state.world.remove_collider(handle) {
                        let body = removed.parent().and_then(|b| state.world.bodies.get(b));
                        let owner =
                            crate::shared::events::Owner::of(entity, body.map(|b| b.user_data));
                        state.gone.insert(handle, owner);
                        lightened.extend(removed.parent());
                    }
                }
                if let Some(reference) = state.joints.shift_remove(&entity) {
                    joint::drop_joint(&mut state.world, &reference);
                }
                // A soft body's proxy is a rigid body of rapier's own, so it goes
                // through the set that made it rather than `remove_body`.
                if let Some(handle) = state.soft_bodies.shift_remove(&entity) {
                    use crate::shared::softbody::Family;
                    let w = &mut state.world;
                    for piece in w.soft_bodies.family(handle) {
                        w.soft_bodies.remove(
                            piece,
                            &mut w.islands,
                            &mut w.bodies,
                            &mut w.colliders,
                            &mut w.impulse_joints,
                            &mut w.multibody_joints,
                        );
                    }
                    jointed = true;
                }
                state.soft_params.shift_remove(&entity);
                state.body_authored.shift_remove(&entity);
                state.collider_params.shift_remove(&entity);
                state.joint_params.shift_remove(&entity);
                state.asleep.shift_remove(&entity);
            }
            for handle in lightened {
                if state.world.bodies.contains(handle) {
                    body::refit_mass(state, handle);
                }
            }
            if jointed {
                forget_dead_joints(state);
            }
            if state.world.colliders.len() != before {
                state.queries_ready = false;
            }
        }

        /// Forget `owner`'s handle to `collider`, which rapier dropped with the
        /// body it hung on, and the surface it had.
        pub(crate) fn forget_collider(
            state: &mut $State,
            owner: Entity,
            collider: $ColliderHandle,
        ) {
            state.surfaces.swap_remove(&collider);
            if let Some(handles) = state.colliders.get_mut(&owner) {
                handles.retain(|held| *held != collider);
                if handles.is_empty() {
                    state.colliders.shift_remove(&owner);
                }
            }
            state.queries_ready = false;
        }

        /// Forget the joints rapier dropped with a body or soft body it took out.
        pub(crate) fn forget_dead_joints(state: &mut $State) {
            let world = &state.world;
            state
                .joints
                .retain(|_, reference| joint::is_live(world, reference));
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
            state.awake = moving(state);
            changed
        }

        /// Whether the step left anything moving: a dynamic body awake, a
        /// kinematic one in motion, or a soft body or a torn piece not asleep.
        fn moving(state: &$State) -> bool {
            use crate::shared::softbody::Family;
            let bodies = state.bodies.values();
            let mut bodies = bodies.filter_map(|handle| state.world.bodies.get(*handle));
            let rigid = bodies.any(|body| {
                let moves = if body.is_kinematic() {
                    body.is_moving()
                } else {
                    body.is_dynamic() && !body.is_sleeping()
                };
                moves && body.is_enabled()
            });
            let soft_bodies = &state.world.soft_bodies;
            rigid
                || state.soft_bodies.values().any(|&root| {
                    let pieces = soft_bodies.family(root);
                    let mut pieces = pieces.iter().filter_map(|piece| soft_bodies.get(*piece));
                    pieces.any(|soft| soft.is_enabled() && !soft.is_sleeping())
                })
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
