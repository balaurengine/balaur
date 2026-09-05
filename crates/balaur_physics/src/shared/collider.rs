//! The collider functions both dimensions share: the material vocabulary, the
//! hook flag, and the lookups every collider call starts from.

macro_rules! functions {
    (state = $State:ty) => {
        fn combine_name(rule: CoefficientCombineRule) -> &'static str {
            use crate::vocabulary::words as w;
            match rule {
                CoefficientCombineRule::Min => w::MIN,
                CoefficientCombineRule::Multiply => w::MULTIPLY,
                CoefficientCombineRule::Max => w::MAX,
                CoefficientCombineRule::ClampedSum => w::CLAMPED_SUM,
                CoefficientCombineRule::GeometricMean => w::GEOMETRIC_MEAN,
                CoefficientCombineRule::Average => w::AVERAGE,
            }
        }

        fn combine_rule(name: &str) -> CoefficientCombineRule {
            use crate::vocabulary::words as w;
            match name {
                w::MIN => CoefficientCombineRule::Min,
                w::MULTIPLY => CoefficientCombineRule::Multiply,
                w::MAX => CoefficientCombineRule::Max,
                w::CLAMPED_SUM => CoefficientCombineRule::ClampedSum,
                w::GEOMETRIC_MEAN => CoefficientCombineRule::GeometricMean,
                _ => CoefficientCombineRule::Average,
            }
        }

        /// The one hook a collider can ask for: a one-way platform is contact
        /// modification with the answer written for it.
        pub(crate) fn active_hooks(params: &toml::Value) -> ActiveHooks {
            if v::boolean(params, k::ONE_WAY, false) {
                ActiveHooks::MODIFY_SOLVER_CONTACTS
            } else {
                ActiveHooks::empty()
            }
        }

        /// The nearest body at or above `entity` in the scene tree, and the node that
        /// owns it.
        ///
        /// This is what makes a compound shape authorable: a capsule and a sensor
        /// sphere as two child nodes under one body, each with its own transform and
        /// each pickable in the editor. Rapier has `compound`, but a compound shape is
        /// one collider with one material and no way to tell which part was hit.
        pub(crate) fn nearest_body(
            eng: &Engine,
            entity: Entity,
        ) -> Option<(Entity, RigidBodyHandle)> {
            let state = eng.resource::<$State>();
            let state = state.borrow();
            let world = eng.world();
            let mut current = entity;
            loop {
                if let Some(handle) = state.bodies.get(&current) {
                    return Some((current, *handle));
                }
                current = world.get::<&balaur_core::scene::Parent>(current).ok()?.0;
            }
        }

        pub(crate) fn remove_colliders(eng: &Engine, entity: Entity) {
            let state = eng.resource::<$State>();
            let mut state = state.borrow_mut();
            if let Some(handles) = state.colliders.swap_remove(&entity) {
                for handle in handles {
                    state.world.remove_collider(handle);
                }
                state.collider_params.swap_remove(&entity);
                state.queries_ready = false;
            }
        }

        /// A node's first collider handle, checked against rapier's arena.
        ///
        /// A freed node's handle survives until the next prune, and indexing the
        /// arena with one panics inside rapier rather than failing the call.
        pub(crate) fn first_collider(state: &$State, entity: Entity) -> Result<ColliderHandle> {
            let handle = state
                .colliders
                .get(&entity)
                .and_then(|handles| handles.first())
                .copied()
                .ok_or_else(|| anyhow!("node has no collider"))?;
            if !state.world.colliders.contains(handle) {
                return Err(anyhow!("this node's collider is gone: the node was freed"));
            }
            Ok(handle)
        }

        /// The non-shape half of a collider, read back off it. The inverse of
        /// `with_material`, property for property, so the inspector round-trips.
        pub(crate) fn read_material(
            collider: &Collider,
            map: &mut toml::map::Map<String, toml::Value>,
        ) {
            let f = |value: Real| toml::Value::Float(f64::from(value));
            map.insert(k::RESTITUTION.into(), f(collider.restitution()));
            map.insert(k::FRICTION.into(), f(collider.friction()));
            map.insert(k::DENSITY.into(), f(collider.density()));
            map.insert(k::MASS.into(), f(collider.mass()));
            map.insert(k::CONTACT_SKIN.into(), f(collider.contact_skin()));
            map.insert(
                k::CONTACT_FORCE_THRESHOLD.into(),
                f(collider.contact_force_event_threshold()),
            );
            map.insert(k::SENSOR.into(), collider.is_sensor().into());
            map.insert(k::ENABLED.into(), collider.is_enabled().into());
            map.insert(
                k::FRICTION_COMBINE.into(),
                combine_name(collider.friction_combine_rule()).into(),
            );
            map.insert(
                k::RESTITUTION_COMBINE.into(),
                combine_name(collider.restitution_combine_rule()).into(),
            );
            let groups = collider.collision_groups();
            map.insert(k::LAYERS.into(), v::layer_names(groups.memberships.bits()));
            map.insert(k::MASK.into(), v::layer_names(groups.filter.bits()));
            let solver = collider.solver_groups();
            map.insert(
                k::SOLVER_LAYERS.into(),
                v::layer_names(solver.memberships.bits()),
            );
            map.insert(k::SOLVER_MASK.into(), v::layer_names(solver.filter.bits()));
            map.insert(
                k::EVENTS.into(),
                v::names(collider.active_events().bits(), &v::flags::events()),
            );
            map.insert(
                k::ACTIVE_COLLISIONS.into(),
                v::names(
                    collider.active_collision_types().bits(),
                    &v::flags::collision_types(),
                ),
            );
        }
    };
}
pub(crate) use functions;
