//! The body functions both dimensions share: the kind vocabulary, the checked
//! handle lookups, and creating and applying a body.

macro_rules! functions {
    (state = $State:ty, handle = $Handle:ty, builder = $Builder:ident, node_pose = $node_pose:ident, missing = $missing:literal) => {
        /// The body types both dimensions accept, in Balaur's vocabulary (N14).
        pub(crate) fn body_type(kind: &str) -> Result<RigidBodyType> {
            use crate::vocabulary::words as w;
            Ok(match kind {
                w::DYNAMIC => RigidBodyType::Dynamic,
                w::STATIC => RigidBodyType::Fixed,
                w::KINEMATIC => RigidBodyType::KinematicPositionBased,
                w::KINEMATIC_VELOCITY => RigidBodyType::KinematicVelocityBased,
                other => return Err(anyhow!("unknown body kind '{other}'")),
            })
        }

        pub(crate) fn kind_name(body: &RigidBody) -> &'static str {
            use crate::vocabulary::words as w;
            match body.body_type() {
                RigidBodyType::Dynamic => w::DYNAMIC,
                RigidBodyType::Fixed => w::STATIC,
                RigidBodyType::KinematicPositionBased => w::KINEMATIC,
                RigidBodyType::KinematicVelocityBased => w::KINEMATIC_VELOCITY,
            }
        }

        pub(crate) fn add_body(eng: &Engine, entity: Entity, kind: &str) -> Result<()> {
            let pose = $node_pose(eng, entity)?;
            let builder = $Builder::new(body_type(kind)?).pose(pose);
            let state = eng.resource::<$State>();
            let mut state = state.borrow_mut();
            let builder = if state.sleeping_allowed {
                builder
            } else {
                builder.can_sleep(false)
            };
            let handle = state.world.insert_body(builder);
            state.bodies.insert(entity, handle);
            Ok(())
        }

        /// Create the body if the node has none, then write every property onto it.
        pub(crate) fn apply_body(eng: &Engine, entity: Entity, params: &toml::Value) -> Result<()> {
            let exists = {
                let state = eng.resource::<$State>();
                let exists = state.borrow().bodies.contains_key(&entity);
                exists
            };
            if !exists {
                add_body(eng, entity, v::text(params, "kind", w::DYNAMIC))?;
                // A collider inserted while the node had no body is standalone
                // geometry in rapier; re-inserting it attaches it to the new body.
                if let Some(collider) = get_collider_params(eng, entity) {
                    apply_collider(eng, entity, &collider)?;
                }
            }
            with_body(eng, entity, |state, handle| {
                let may_sleep = state.sleeping_allowed;
                write_body(&mut state.world.bodies[handle], params, may_sleep);
                // Rapier folds additional mass in at the next step; a scene that sets
                // `mass = 5` and a script that reads it back in the same tick would
                // otherwise disagree.
                let colliders = &state.world.colliders;
                state.world.bodies[handle].recompute_mass_properties_from_colliders(colliders);
            })
        }

        /// A node's body handle, checked against rapier's arena.
        ///
        /// A freed node's handle outlives it until the next prune, and indexing the
        /// arena with one panics inside rapier rather than failing the call.
        pub(crate) fn body_handle(state: &$State, entity: Entity) -> Result<$Handle> {
            let handle = state
                .bodies
                .get(&entity)
                .copied()
                .ok_or_else(|| anyhow!($missing))?;
            if !state.world.bodies.contains(handle) {
                return Err(anyhow!("this node's body is gone: the node was freed"));
            }
            Ok(handle)
        }

        pub(crate) fn with_body<R>(
            eng: &Engine,
            entity: Entity,
            f: impl FnOnce(&mut $State, $Handle) -> R,
        ) -> Result<R> {
            let state = eng.resource::<$State>();
            let mut state = state.borrow_mut();
            let handle = body_handle(&state, entity)?;
            Ok(f(&mut state, handle))
        }

        /// The same, for a caller that only reads.
        pub(crate) fn read_body<R>(
            eng: &Engine,
            entity: Entity,
            f: impl FnOnce(&RigidBody) -> R,
        ) -> Result<R> {
            let state = eng.resource::<$State>();
            let state = state.borrow();
            let handle = body_handle(&state, entity)?;
            Ok(f(&state.world.bodies[handle]))
        }

        pub(crate) fn remove_body_and_colliders(eng: &Engine, entity: Entity) {
            let state = eng.resource::<$State>();
            let mut state = state.borrow_mut();
            // Attached colliders die with the body inside rapier.
            state.colliders.swap_remove(&entity);
            if let Some(handle) = state.bodies.swap_remove(&entity) {
                state.world.remove_body(handle);
            }
        }
    };
}
pub(crate) use functions;
