//! The collider functions both dimensions share: the material vocabulary, the
//! hook flag, and the lookups every collider call starts from.

use crate::vocabulary::{self as v, keys as k};

/// The `TriMeshFlags` bits a `triangle_mesh`'s keys ask for. rapier2d's flags
/// take the same bits as rapier3d's (see `crate::vocabulary::flags`).
pub(crate) fn trimesh_bits(params: &toml::Value) -> u16 {
    use crate::rapier3d::prelude::TriMeshFlags as F;
    let mut flags = F::empty();
    for (key, default, flag) in [
        (k::FIX_INTERNAL_EDGES, true, F::FIX_INTERNAL_EDGES),
        (k::TWO_SIDED_EDGES, false, F::FIX_INTERNAL_EDGES_TWO_SIDED),
        (k::MERGE_VERTICES, false, F::MERGE_DUPLICATE_VERTICES),
        (
            k::DROP_DEGENERATE_TRIANGLES,
            false,
            F::DELETE_DEGENERATE_TRIANGLES,
        ),
        (
            k::DROP_DUPLICATE_TRIANGLES,
            false,
            F::DELETE_DUPLICATE_TRIANGLES,
        ),
        (
            k::DROP_BAD_TOPOLOGY,
            false,
            F::DELETE_BAD_TOPOLOGY_TRIANGLES,
        ),
        (k::TOPOLOGY, false, F::HALF_EDGE_TOPOLOGY),
        (k::CONNECTED_COMPONENTS, false, F::CONNECTED_COMPONENTS),
        (k::ORIENTED, false, F::ORIENTED),
    ] {
        if v::boolean(params, key, default) {
            flags |= flag;
        }
    }
    flags.bits()
}

/// Every edge of a mesh's triangles, once each, in the order first met.
pub(crate) fn mesh_edges(indices: &[[u32; 3]]) -> Vec<[u32; 2]> {
    let mut seen = std::collections::BTreeSet::new();
    let mut edges = Vec::new();
    for &[a, b, c] in indices {
        for edge in [[a, b], [b, c], [c, a]] {
            if seen.insert((edge[0].min(edge[1]), edge[0].max(edge[1]))) {
                edges.push(edge);
            }
        }
    }
    edges
}

/// Whether `params` asks for any surface velocity, in either dimension.
pub(crate) fn slides(params: &toml::Value) -> bool {
    v::vec3(params, k::SURFACE_VELOCITY, [0.0; 3])
        .iter()
        .any(|n| *n != 0.0)
}

macro_rules! functions {
    (state = $State:ty, refit = $refit:path) => {
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

        /// The one hook a collider can ask for: a one-way platform and a
        /// conveyor belt are both contact modification with the answer
        /// written for them.
        pub(crate) fn active_hooks(params: &toml::Value) -> ActiveHooks {
            if v::boolean(params, k::ONE_WAY, false) || crate::shared::collider::slides(params) {
                ActiveHooks::MODIFY_SOLVER_CONTACTS
            } else {
                ActiveHooks::empty()
            }
        }

        fn test_mode(name: &str) -> InteractionTestMode {
            if name == crate::vocabulary::words::EITHER {
                InteractionTestMode::Or
            } else {
                InteractionTestMode::And
            }
        }

        fn test_mode_name(mode: InteractionTestMode) -> &'static str {
            match mode {
                InteractionTestMode::Or => crate::vocabulary::words::EITHER,
                InteractionTestMode::And => crate::vocabulary::words::BOTH,
            }
        }

        /// The 32 layers a `flags` pair names, tested as `test_key` says. An
        /// empty mask means every layer: the alternative is 32 strings in every
        /// scene file that wants the default.
        pub(crate) fn interaction_groups(
            params: &toml::Value,
            memberships_key: &str,
            filter_key: &str,
            test_key: &str,
        ) -> InteractionGroups {
            InteractionGroups::new(
                Group::from_bits_truncate(v::layer_bits(params, memberships_key, false)),
                Group::from_bits_truncate(v::layer_bits(params, filter_key, true)),
                test_mode(v::text(params, test_key, crate::vocabulary::words::BOTH)),
            )
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
                    state.surfaces.swap_remove(&handle);
                    if let Some(removed) = state.world.remove_collider(handle) {
                        let body = removed.parent().and_then(|b| state.world.bodies.get(b));
                        let owner =
                            crate::shared::events::Owner::of(entity, body.map(|b| b.user_data));
                        state.gone.insert(handle, owner);
                        if let Some(body) = removed.parent() {
                            $refit(&mut state, body);
                        }
                    }
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
        /// `body_states_mass` says the body zeroed this collider's weight.
        pub(crate) fn read_material(
            collider: &Collider,
            body_states_mass: bool,
            map: &mut toml::map::Map<String, toml::Value>,
        ) {
            let f = |value: Real| toml::Value::Float(f64::from(value));
            map.insert(k::RESTITUTION.into(), f(collider.restitution()));
            map.insert(k::FRICTION.into(), f(collider.friction()));
            // Each of `mass` and `density` is derived from the other, so
            // reporting both would pin one on the next patch or re-save.
            let authored = |key: &str| map.get(key).and_then(balaur_core::components::as_f64);
            let own_mass = authored(k::MASS).unwrap_or(0.0) > 0.0;
            let zeroed = body_states_mass;
            if own_mass {
                let mass = if zeroed {
                    authored(k::MASS).unwrap_or(0.0) as Real
                } else {
                    collider.mass()
                };
                map.insert(k::MASS.into(), f(mass));
            } else {
                let density = if zeroed {
                    authored(k::DENSITY).unwrap_or(1.0) as Real
                } else {
                    collider.density()
                };
                map.insert(k::MASS.into(), f(0.0));
                map.insert(k::DENSITY.into(), f(density));
            }
            map.insert(k::COLLISION_MARGIN.into(), f(collider.contact_skin()));
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
            map.insert(
                k::COLLISION_LAYER.into(),
                v::layer_names(groups.memberships.bits()),
            );
            map.insert(
                k::COLLISION_MASK.into(),
                v::layer_names(groups.filter.bits()),
            );
            let solver = collider.solver_groups();
            map.insert(
                k::SOLVER_LAYER.into(),
                v::layer_names(solver.memberships.bits()),
            );
            map.insert(k::SOLVER_MASK.into(), v::layer_names(solver.filter.bits()));
            map.insert(
                k::COLLISION_TEST.into(),
                test_mode_name(groups.test_mode).into(),
            );
            map.insert(
                k::SOLVER_TEST.into(),
                test_mode_name(solver.test_mode).into(),
            );
            map.insert(
                k::EVENTS.into(),
                v::names(collider.active_events().bits(), &v::flags::events()),
            );
            map.insert(
                k::CONTACT_PAIRS.into(),
                v::names(
                    collider.active_collision_types().bits(),
                    &v::flags::collision_types(),
                ),
            );
        }
    };
}
pub(crate) use functions;
