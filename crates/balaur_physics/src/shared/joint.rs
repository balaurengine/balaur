//! The joint functions both dimensions share: dropping and checking a joint in
//! either of rapier's two sets, the retry list, the break check, and the reader
//! every joint call goes through.

macro_rules! functions {
    (state = $State:ty, world = $World:ty, reference = $Ref:ty, handle = $Handle:ident, component = $component:literal, body = $body:literal, impulse_magnitude = $impulse:ident) => {
        pub(crate) fn remove_joint(eng: &Engine, entity: Entity) {
            let state = eng.resource::<$State>();
            let mut state = state.borrow_mut();
            state.joint_params.swap_remove(&entity);
            if let Some(reference) = state.joints.swap_remove(&entity) {
                drop_joint(&mut state.world, &reference);
            }
        }

        /// Whether rapier still holds this joint.
        ///
        /// It drops one when either end's body goes, and a map that kept the handle
        /// would report a joint that is not there and never retry it.
        pub(crate) fn is_live(world: &$World, reference: &$Ref) -> bool {
            match reference.handle {
                $Handle::Impulse(handle) => world.impulse_joints.get(handle).is_some(),
                $Handle::Multibody(handle) => world.multibody_joints.get(handle).is_some(),
            }
        }

        pub(crate) fn drop_joint(world: &mut $World, reference: &$Ref) {
            match reference.handle {
                $Handle::Impulse(handle) => {
                    world.remove_impulse_joint(handle);
                }
                $Handle::Multibody(handle) => world.remove_multibody_joint(handle),
            }
        }

        fn handles(
            state: &$State,
            a: Entity,
            b: Entity,
        ) -> Result<(RigidBodyHandle, RigidBodyHandle)> {
            let first = *state
                .bodies
                .get(&a)
                .ok_or_else(|| anyhow!("the node holding the joint has no {}", $body))?;
            let second = *state
                .bodies
                .get(&b)
                .ok_or_else(|| anyhow!("the node at the joint's other end has no {}", $body))?;
            Ok((first, second))
        }

        /// Joints authored but not yet made, because the node at the other end had
        /// not been spawned when this one was.
        ///
        /// A scene file names nodes in whatever order it likes, and a joint that
        /// pointed forwards used to be silently inert. Retried once per step, over
        /// the few that are unresolved rather than over every joint.
        pub fn pending(state: &$State) -> Vec<Entity> {
            let mut out: Vec<Entity> = state
                .joint_params
                .iter()
                // A joint switched off has params and no handle for ever; retrying it
                // every step would re-apply the whole component sixty times a second.
                .filter(|(entity, params)| {
                    !state.joints.contains_key(*entity) && v::boolean(params, "enabled", true)
                })
                .map(|(entity, _)| *entity)
                .collect();
            out.sort_unstable_by_key(|e| e.to_bits());
            out
        }

        /// Joints whose reaction force passed their `break_force` this step.
        ///
        /// Checked after the step with the world still borrowed, and acted on after it
        /// is released — the same shape as an event, because a break *is* one.
        pub(crate) fn broken(state: &$State) -> Vec<Entity> {
            let mut out = Vec::new();
            for (entity, reference) in &state.joints {
                if reference.break_force <= 0.0 {
                    continue;
                }
                let $Handle::Impulse(handle) = reference.handle else {
                    // A reduced-coordinates joint has no reaction impulse to read:
                    // its constraint is built into the coordinates.
                    continue;
                };
                let Some(joint) = state.world.impulse_joints.get(handle) else {
                    continue;
                };
                if $impulse(&joint.impulses) > reference.break_force {
                    out.push(*entity);
                }
            }
            out.sort_unstable_by_key(|e| e.to_bits());
            out
        }

        /// A joint's own data, whichever set it lives in.
        fn with_joint(
            eng: &Engine,
            node: NodeId,
            f: impl FnOnce(&mut GenericJoint, &str),
        ) -> Result<()> {
            let entity = entity_of(node)?;
            let kind = balaur_core::components::get(eng, entity, $component)
                .and_then(|params| {
                    params
                        .get("kind")
                        .and_then(toml::Value::as_str)
                        .map(str::to_string)
                })
                .unwrap_or_else(|| "revolute".to_string());
            let state = eng.resource::<$State>();
            let mut state = state.borrow_mut();
            match state.joints.get(&entity).map(|j| j.handle) {
                Some($Handle::Impulse(handle)) => {
                    let joint = state
                        .world
                        .impulse_joints
                        .get_mut(handle, true)
                        .ok_or_else(|| anyhow!("this node's joint is gone"))?;
                    f(&mut joint.data, &kind);
                    Ok(())
                }
                Some($Handle::Multibody(handle)) => {
                    let (multibody, link) = state
                        .world
                        .multibody_joints
                        .get_mut(handle)
                        .ok_or_else(|| anyhow!("this node's joint is gone"))?;
                    let link = multibody
                        .link_mut(link)
                        .ok_or_else(|| anyhow!("this node's joint is gone"))?;
                    f(&mut link.joint.data, &kind);
                    Ok(())
                }
                None => Err(anyhow!("node has no {}", $component)),
            }
        }
    };
}
pub(crate) use functions;

/// The character helpers: rapier's controller from the component, and the
