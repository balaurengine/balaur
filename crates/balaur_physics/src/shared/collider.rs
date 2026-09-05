//! The collider functions both dimensions share: the material vocabulary, the
//! hook flag, and the lookups every collider call starts from.

macro_rules! functions {
    (state = $State:ty) => {
        fn combine_name(rule: CoefficientCombineRule) -> &'static str {
            match rule {
                CoefficientCombineRule::Min => "min",
                CoefficientCombineRule::Multiply => "multiply",
                CoefficientCombineRule::Max => "max",
                CoefficientCombineRule::ClampedSum => "clamped_sum",
                CoefficientCombineRule::GeometricMean => "geometric_mean",
                CoefficientCombineRule::Average => "average",
            }
        }

        fn combine_rule(name: &str) -> CoefficientCombineRule {
            match name {
                "min" => CoefficientCombineRule::Min,
                "multiply" => CoefficientCombineRule::Multiply,
                "max" => CoefficientCombineRule::Max,
                "clamped_sum" => CoefficientCombineRule::ClampedSum,
                "geometric_mean" => CoefficientCombineRule::GeometricMean,
                _ => CoefficientCombineRule::Average,
            }
        }

        /// The one hook a collider can ask for: a one-way platform is contact
        /// modification with the answer written for it.
        pub(crate) fn active_hooks(params: &toml::Value) -> ActiveHooks {
            if v::boolean(params, "one_way", false) {
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
        fn nearest_body(eng: &Engine, entity: Entity) -> Option<(Entity, RigidBodyHandle)> {
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
            map.insert("restitution".into(), f(collider.restitution()));
            map.insert("friction".into(), f(collider.friction()));
            map.insert("density".into(), f(collider.density()));
            map.insert("mass".into(), f(collider.mass()));
            map.insert("contact_skin".into(), f(collider.contact_skin()));
            map.insert(
                "contact_force_threshold".into(),
                f(collider.contact_force_event_threshold()),
            );
            map.insert("sensor".into(), collider.is_sensor().into());
            map.insert("enabled".into(), collider.is_enabled().into());
            map.insert(
                "friction_combine".into(),
                combine_name(collider.friction_combine_rule()).into(),
            );
            map.insert(
                "restitution_combine".into(),
                combine_name(collider.restitution_combine_rule()).into(),
            );
            let groups = collider.collision_groups();
            map.insert("layers".into(), v::layer_names(groups.memberships.bits()));
            map.insert("mask".into(), v::layer_names(groups.filter.bits()));
            let solver = collider.solver_groups();
            map.insert(
                "solver_layers".into(),
                v::layer_names(solver.memberships.bits()),
            );
            map.insert("solver_mask".into(), v::layer_names(solver.filter.bits()));
            map.insert(
                "events".into(),
                v::names(collider.active_events().bits(), &v::flags::events()),
            );
            map.insert(
                "active_collisions".into(),
                v::names(
                    collider.active_collision_types().bits(),
                    &v::flags::collision_types(),
                ),
            );
        }
    };
}
pub(crate) use functions;

/// The body helpers: the kind vocabulary, the checked handle lookups, and
