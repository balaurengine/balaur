//! The collider functions both dimensions share: the material vocabulary, the
//! hook flag, and the lookups every collider call starts from.

/// The rows a mesh-backed collider tidies its triangles by, from the
/// component beside it.
#[allow(
    clippy::struct_excessive_bools,
    reason = "each is one of rapier's flags, by name"
)]
pub(crate) struct Tidy {
    pub(crate) merge_vertices: bool,
    pub(crate) drop_degenerate_triangles: bool,
    pub(crate) drop_duplicate_triangles: bool,
    pub(crate) drop_bad_topology: bool,
    pub(crate) topology: bool,
    pub(crate) connected_components: bool,
    pub(crate) oriented: bool,
}

/// The `TriMeshFlags` bits a `triangle_mesh` asks for. rapier2d's flags take
/// the same bits as rapier3d's (see `crate::vocabulary::flags`).
pub(crate) fn trimesh_bits(fix_internal_edges: bool, two_sided_edges: bool, tidy: &Tidy) -> u16 {
    use crate::rapier3d::prelude::TriMeshFlags as F;
    let mut flags = F::empty();
    for (on, flag) in [
        (fix_internal_edges, F::FIX_INTERNAL_EDGES),
        (two_sided_edges, F::FIX_INTERNAL_EDGES_TWO_SIDED),
        (tidy.merge_vertices, F::MERGE_DUPLICATE_VERTICES),
        (
            tidy.drop_degenerate_triangles,
            F::DELETE_DEGENERATE_TRIANGLES,
        ),
        (tidy.drop_duplicate_triangles, F::DELETE_DUPLICATE_TRIANGLES),
        (tidy.drop_bad_topology, F::DELETE_BAD_TOPOLOGY_TRIANGLES),
        (tidy.topology, F::HALF_EDGE_TOPOLOGY),
        (tidy.connected_components, F::CONNECTED_COMPONENTS),
        (tidy.oriented, F::ORIENTED),
    ] {
        if on {
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

macro_rules! functions {
    (state = $State:ty, refit = $refit:path) => {
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

        /// The groups a collider's layers, mask and test make. An empty mask
        /// is every layer, and an empty set of layers is layer 1.
        pub(crate) fn groups_of(
            (layers, mask, test): (
                &[balaur_core::layers::Layer],
                &[balaur_core::layers::Layer],
                crate::properties::LayerTest,
            ),
        ) -> InteractionGroups {
            InteractionGroups::new(
                Group::from_bits_truncate(balaur_core::layers::Layer::bits(layers, false)),
                Group::from_bits_truncate(balaur_core::layers::Layer::bits(mask, true)),
                match test {
                    crate::properties::LayerTest::Either => InteractionTestMode::Or,
                    crate::properties::LayerTest::Both => InteractionTestMode::And,
                },
            )
        }

        /// The body pairs a collider is tested against, as rapier counts them.
        pub(crate) fn collision_types_of(
            pairs: &[crate::properties::ContactPair],
        ) -> ActiveCollisionTypes {
            use crate::properties::ContactPair as P;
            pairs
                .iter()
                .fold(ActiveCollisionTypes::empty(), |types, pair| {
                    types
                        | match pair {
                            P::DynamicDynamic => ActiveCollisionTypes::DYNAMIC_DYNAMIC,
                            P::DynamicKinematic => ActiveCollisionTypes::DYNAMIC_KINEMATIC,
                            P::DynamicStatic => ActiveCollisionTypes::DYNAMIC_FIXED,
                            P::KinematicKinematic => ActiveCollisionTypes::KINEMATIC_KINEMATIC,
                            P::KinematicStatic => ActiveCollisionTypes::KINEMATIC_FIXED,
                            P::StaticStatic => ActiveCollisionTypes::FIXED_FIXED,
                        }
                })
        }

        /// What a collider reports, as rapier counts it.
        pub(crate) fn events_of(events: &[crate::properties::ColliderEvent]) -> ActiveEvents {
            use crate::properties::ColliderEvent as E;
            events.iter().fold(ActiveEvents::empty(), |active, event| {
                active
                    | match event {
                        E::Collision => ActiveEvents::COLLISION_EVENTS,
                        E::ContactForce => ActiveEvents::CONTACT_FORCE_EVENTS,
                    }
            })
        }

        /// The hook a collider's rows ask for, if any.
        pub(crate) fn hooks_of(rows: &impl crate::properties::ColliderRows) -> ActiveHooks {
            if rows.hooked() {
                ActiveHooks::MODIFY_SOLVER_CONTACTS
            } else {
                ActiveHooks::empty()
            }
        }

        /// Every row a collider carries that is not its shape or its mass's
        /// frame: what it is made of, what it meets and what it reports.
        pub(crate) fn with_rows(
            builder: ColliderBuilder,
            rows: &impl crate::properties::ColliderRows,
            material: &crate::material::PhysicsMaterial,
        ) -> ColliderBuilder {
            let mut builder = builder
                .restitution(crate::scalar::real(material.restitution))
                .friction(crate::scalar::real(material.friction))
                .density(crate::scalar::real(rows.density().max(0.0)))
                .friction_combine_rule(combine_rule(&material.friction_combine))
                .restitution_combine_rule(combine_rule(&material.restitution_combine))
                .contact_skin(crate::scalar::real(rows.collision_margin().max(0.0)))
                .contact_force_event_threshold(crate::scalar::real(rows.contact_force_threshold()))
                .collision_groups(groups_of(rows.collision()))
                .solver_groups(groups_of(rows.solver()))
                .active_collision_types(collision_types_of(rows.contact_pairs()))
                .active_events(events_of(rows.events()))
                .active_hooks(hooks_of(rows))
                .enabled(rows.enabled())
                .sensor(rows.sensor());
            // An explicit mass overrides what the density works out to, which
            // is what an author who typed a number in kilograms means.
            let mass = crate::scalar::real(rows.mass());
            if mass > 0.0 {
                builder = builder.mass(mass);
            }
            builder
        }

        fn test_mode(name: &str) -> InteractionTestMode {
            if name == crate::vocabulary::words::EITHER {
                InteractionTestMode::Or
            } else {
                InteractionTestMode::And
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
                Group::from_bits_truncate(balaur_core::layers::bits(
                    params.get(memberships_key),
                    false,
                )),
                Group::from_bits_truncate(balaur_core::layers::bits(params.get(filter_key), true)),
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
    };
}
pub(crate) use functions;
