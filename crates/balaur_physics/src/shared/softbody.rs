//! What a soft body is made of, read the same way in both dimensions.
//!
//! The words and the schema text are dimension-free and live in
//! `crate::softbody`; what is here is the reading, which names rapier's
//! `SoftBodyMaterial` and so is a different type per dimension.

macro_rules! material {
    (
        rapier = $rapier:ident,
        material = $material:ident,
        cell_model = $cell_model:ident,
        flow = $flow:ident,
        springs = $springs:ident
    ) => {
        /// The spring a `<name>_frequency`/`<name>_damping` pair spells.
        fn $springs(
            params: &toml::Value,
            frequency: &str,
            damping: &str,
            defaults: (f32, f32),
        ) -> crate::$rapier::prelude::SpringCoefficients<crate::scalar::Real> {
            crate::$rapier::prelude::SpringCoefficients::new(
                crate::scalar::real(v::f(params, frequency, defaults.0)),
                crate::scalar::real(v::f(params, damping, defaults.1)),
            )
        }

        /// The mechanical properties: stiffness per constraint family, the
        /// elastic model's own parameters, how the body yields, and when it
        /// tears.
        pub(crate) fn $material(params: &toml::Value) -> crate::$rapier::prelude::SoftBodyMaterial {
            use crate::$rapier::prelude::{SoftBodyMaterial, SoftEdgePlasticFlow};
            SoftBodyMaterial {
                edge_softness: $springs(params, k::EDGE_FREQUENCY, k::EDGE_DAMPING, (30.0, 1.0)),
                bend_softness: $springs(params, k::BEND_FREQUENCY, k::BEND_DAMPING, (10.0, 1.0)),
                volume_softness: $springs(
                    params,
                    k::VOLUME_FREQUENCY,
                    k::VOLUME_DAMPING,
                    (30.0, 1.0),
                ),
                shape_matching_softness: $springs(
                    params,
                    k::SHAPE_MATCHING_FREQUENCY,
                    k::SHAPE_MATCHING_DAMPING,
                    (10.0, 1.0),
                ),
                young_modulus: crate::scalar::real(v::f(params, k::YOUNG_MODULUS, 1.0e4)),
                poisson_ratio: crate::scalar::real(v::f(params, k::POISSON_RATIO, 0.3)),
                elastic_damping_ratio: crate::scalar::real(v::f(params, k::ELASTIC_DAMPING, 1.0)),
                plastic_yield: crate::scalar::real(v::f(params, k::PLASTIC_YIELD, 0.0)),
                plastic_creep: crate::scalar::real(v::f(params, k::PLASTIC_CREEP, 1.0)),
                plastic_max: crate::scalar::real(v::f(params, k::PLASTIC_MAX, 1.0)),
                deformation_damping: crate::scalar::real(v::f(params, k::DEFORMATION_DAMPING, 0.0)),
                edge_plastic_yield: crate::scalar::real(v::f(params, k::EDGE_PLASTIC_YIELD, 0.0)),
                edge_plastic_creep: crate::scalar::real(v::f(params, k::EDGE_PLASTIC_CREEP, 1.0)),
                edge_plastic_max: crate::scalar::real(v::f(params, k::EDGE_PLASTIC_MAX, 0.5)),
                edge_plastic_flow: match v::text(params, k::EDGE_PLASTIC_FLOW, w::BOTH) {
                    w::COMPRESSION_FLOW => SoftEdgePlasticFlow::Compression,
                    w::TENSION => SoftEdgePlasticFlow::Tension,
                    _ => SoftEdgePlasticFlow::Both,
                },
                tear_strain: $flow(params, k::TEAR_STRAIN),
                tear_force: $flow(params, k::TEAR_FORCE),
                tear_smoothing: crate::scalar::real(v::f(params, k::TEAR_SMOOTHING, 0.0)),
                interior_strength: crate::scalar::real(v::f(params, k::INTERIOR_STRENGTH, 1.0)),
                // A schema writes "no limit" as zero; rapier writes it as the
                // largest number of tears a step could possibly ask for.
                max_tears_per_step: match v::f(params, k::MAX_TEARS, 0.0) {
                    limit if limit >= 1.0 => limit as u32,
                    _ => u32::MAX,
                },
                min_piece: match v::f(params, k::MIN_PIECE, 0.0) {
                    smallest if smallest >= 1.0 => Some(smallest as u32),
                    _ => None,
                },
            }
        }

        /// A threshold a schema writes as `0` and rapier reads as absent.
        fn $flow(params: &toml::Value, key: &str) -> Option<crate::scalar::Real> {
            let value = v::f(params, key, 0.0);
            (value > 0.0).then(|| crate::scalar::real(value))
        }

        pub(crate) fn $cell_model(
            params: &toml::Value,
        ) -> crate::$rapier::prelude::SoftBodyCellModel {
            use crate::$rapier::prelude::SoftBodyCellModel;
            match v::text(params, k::CELL_MODEL, w::VOLUME) {
                w::COROTATIONAL => SoftBodyCellModel::Corotational,
                w::NEO_HOOKEAN => SoftBodyCellModel::NeoHookean,
                _ => SoftBodyCellModel::Volume,
            }
        }
    };
}

pub(crate) use material;

/// How many particles a body may have. The count comes from a text field,
/// and rapier allocates every particle before a built body could be counted.
pub(crate) const MAX_PARTICLES: usize = 200_000;

/// Refuse a layout of `kind` whose particle count, worked out from its
/// parameters before anything is built, is past the cap.
pub(crate) fn refuse_past_cap(particles: f64, kind: &str) -> anyhow::Result<()> {
    if particles.is_nan() || particles > MAX_PARTICLES as f64 {
        let advice = cap_advice(kind);
        return Err(anyhow::anyhow!(
            "that would be {particles:.0} particles, past the {MAX_PARTICLES} a body may have: {advice}"
        ));
    }
    Ok(())
}

/// What to change for fewer particles, which depends on what the layout reads.
fn cap_advice(kind: &str) -> String {
    use crate::vocabulary::{keys as k, words as w};
    match kind {
        w::VOLUMETRIC => format!("raise {}", k::CELL_SIZE),
        w::ROPE_SOFT | w::DISK => format!("lower {}", k::PARTICLES),
        w::SPHERE => format!("lower {}", k::SUBDIVISIONS),
        w::SURFACE_MESH | w::SOFT_POLYGON | w::POLYLINE => {
            format!("build it from a {} with fewer points", k::MESH)
        }
        _ => format!("lower {}", k::CELLS),
    }
}

/// How many particles a rope or a disk's rim has when `particles` is not set.
pub(crate) const DEFAULT_PARTICLES: f32 = 16.0;
/// The fewest particles a chain holds: its two ends.
pub(crate) const MIN_CHAIN_PARTICLES: f32 = 2.0;
/// The fewest a closed ring holds.
pub(crate) const MIN_RING_PARTICLES: f32 = 3.0;

/// The particle count `particles` asks for, and at least `least`.
pub(crate) fn particle_count(params: &toml::Value, least: f32) -> f64 {
    let asked = crate::vocabulary::f(
        params,
        crate::vocabulary::keys::PARTICLES,
        DEFAULT_PARTICLES,
    );
    f64::from(asked.max(least)).floor()
}

/// What a body with nothing of its own to deform is drawn in.
pub(crate) const DEFAULT_COLOR: [f32; 4] = [0.8, 0.8, 0.8, 1.0];

/// A particle radius past this share of the body's width reads as hovering.
const HOVER_FRACTION: f32 = 0.1;

/// The particles along an axis `cells` cells long, as the generators count
/// them: one more than the cells between them.
pub(crate) fn particles_along(cells: f32) -> f64 {
    f64::from(cells.max(1.0)).floor() + 1.0
}

/// The most particles a volumetric fill of a box `extents` wide can make at a
/// cell of `size`: one at each corner of the grid covering it.
pub(crate) fn grid_particles(extents: &[f32], size: f32) -> f64 {
    extents
        .iter()
        .map(|extent| (f64::from(*extent) / f64::from(size)).ceil().max(0.0) + 1.0)
        .product()
}

/// A particle radius the layout worked out that is large against the body,
/// which leaves it resting that far above whatever it lands on.
pub(crate) fn hovering(radius: f32, widest: f32) -> Option<balaur_core::warnings::Warning> {
    let key = crate::vocabulary::keys::PARTICLE_RADIUS;
    (widest > 0.0 && radius > HOVER_FRACTION * widest).then(|| {
        balaur_core::warnings::Warning::on(
            key,
            format!(
                "the particles are {radius:.2} thick on a body {widest:.2} wide, so it rests that far above what it lands on: set a smaller {key}"
            ),
        )
    })
}

/// A body and every piece a tear or a cut split off it, transitively, in the
/// order they were made; a piece whose body is gone is left out.
///
/// Rapier makes each piece a soft body of its own, and a node keeps only the
/// handle it was built with: everything that draws, frees or hashes a node's
/// body goes through here, or a torn-off piece simulates unseen forever.
pub(crate) trait Family<H> {
    fn family(&self, root: H) -> Vec<H>;
}

macro_rules! family {
    ($set:ty, $handle:ty) => {
        impl Family<$handle> for $set {
            fn family(&self, root: $handle) -> Vec<$handle> {
                let mut out = Vec::new();
                let mut todo = vec![root];
                while let Some(handle) = todo.pop() {
                    let Some(body) = self.get(handle) else {
                        continue;
                    };
                    out.push(handle);
                    todo.extend(body.pieces().iter().rev().copied());
                }
                out
            }
        }
    };
}

family!(
    crate::rapier3d::dynamics::SoftBodySet,
    crate::rapier3d::dynamics::SoftBodyHandle
);
family!(
    crate::rapier2d::dynamics::SoftBodySet,
    crate::rapier2d::dynamics::SoftBodyHandle
);

/// Point a body's colliders, and its pieces', at the node: a contact or a ray
/// that meets a soft body reads the node out of the collider it met.
macro_rules! stamp_colliders {
    ($name:ident, $world:ty, $handle:ty) => {
        pub(crate) fn $name(world: &mut $world, root: $handle, entity: balaur_core::hecs::Entity) {
            use crate::shared::softbody::Family;
            let bits = u128::from(entity.to_bits().get());
            for handle in world.soft_bodies.family(root) {
                let Some(body) = world.soft_bodies.get_mut(handle) else {
                    continue;
                };
                body.user_data = bits;
                for mesh in body.meshes() {
                    if let Some(collider) = world.colliders.get_mut(mesh.collider()) {
                        collider.user_data = bits;
                    }
                }
            }
        }
    };
}

pub(crate) use stamp_colliders;

/// The rows a live body takes as they are, without being built again from
/// rest: what it is made of, how it is solved, and what it is drawn in.
const IN_PLACE: &[&str] = {
    use crate::vocabulary::keys as k;
    &[
        k::EDGE_FREQUENCY,
        k::EDGE_DAMPING,
        k::BEND_FREQUENCY,
        k::BEND_DAMPING,
        k::VOLUME_FREQUENCY,
        k::VOLUME_DAMPING,
        k::SHAPE_MATCHING_FREQUENCY,
        k::SHAPE_MATCHING_DAMPING,
        k::YOUNG_MODULUS,
        k::POISSON_RATIO,
        k::ELASTIC_DAMPING,
        k::PLASTIC_YIELD,
        k::PLASTIC_CREEP,
        k::PLASTIC_MAX,
        k::DEFORMATION_DAMPING,
        k::EDGE_PLASTIC_YIELD,
        k::EDGE_PLASTIC_CREEP,
        k::EDGE_PLASTIC_MAX,
        k::EDGE_PLASTIC_FLOW,
        k::TEAR_STRAIN,
        k::TEAR_FORCE,
        k::TEAR_SMOOTHING,
        k::INTERIOR_STRENGTH,
        k::MAX_TEARS,
        k::MIN_PIECE,
        k::SOLVER,
        k::VOLUME_FACTOR,
        k::VOLUME_PRESERVATION,
        k::PGS_ITERATIONS,
        k::COLOR,
    ]
};

/// Whether `asked` differs from what the body was built from only in rows it
/// takes in place: a stiffness tuned during play must not snap it to rest.
pub(crate) fn only_in_place(built: &toml::Value, asked: &toml::Value) -> bool {
    let (Some(built), Some(asked)) = (built.as_table(), asked.as_table()) else {
        return false;
    };
    built
        .keys()
        .chain(asked.keys())
        .all(|key| built.get(key) == asked.get(key) || IN_PLACE.contains(&key.as_str()))
}

/// Patch the rows [`only_in_place`] allows onto a live body and every piece
/// torn off it, in either dimension.
macro_rules! patch_in_place {
    ($name:ident, $world:ty, $handle:ty, $material:ident, $solver:ident) => {
        fn $name(world: &mut $world, root: $handle, params: &toml::Value) {
            use crate::shared::softbody::Family;
            for piece in world.soft_bodies.family(root) {
                let Some(body) = world.soft_bodies.get_mut(piece) else {
                    continue;
                };
                body.set_material($material(params));
                body.set_solver($solver(params));
                body.set_volume_factor(crate::scalar::real(v::f(params, k::VOLUME_FACTOR, 1.0)));
                body.enable_volume_preservation(v::boolean(params, k::VOLUME_PRESERVATION, true));
                body.set_additional_pgs_iterations(
                    v::f(params, k::PGS_ITERATIONS, 3.0).max(0.0) as usize
                );
            }
        }
    };
}

pub(crate) use patch_in_place;

/// A vector a script passed: a `Vec2`, a `Vec3` or a list of numbers, of
/// which the first `N` are read.
pub(crate) fn vector_arg<const N: usize>(value: &balaur_script::Value) -> anyhow::Result<[f32; N]> {
    use balaur_script::Value;
    let numbers: Vec<f32> = match value {
        Value::Vec2(v) => v.to_vec(),
        Value::Vec3(v) => v.to_vec(),
        Value::List(items) => items
            .iter()
            .map(|item| match item {
                Value::Num(n) => Ok(*n as f32),
                Value::Int(n) => Ok(*n as f32),
                _ => Err(anyhow::anyhow!("a vector holds numbers only")),
            })
            .collect::<anyhow::Result<_>>()?,
        _ => anyhow::bail!("expected a vector of {N} numbers"),
    };
    anyhow::ensure!(
        numbers.len() >= N,
        "expected a vector of {N} numbers, got {}",
        numbers.len()
    );
    Ok(std::array::from_fn(|i| numbers[i]))
}

/// Particles held, moved and pushed from a script, and what the body's edges
/// carry: the same calls on `softbody3d` and `softbody2d`.
macro_rules! runtime_api {
    (
        install = $install:ident,
        state = $State:ty,
        handle = $Handle:ty,
        component = $component:expr,
        dims = $N:literal,
        vector = $vector:path,
        value = $value:path,
        array = $array:path
    ) => {
        pub(crate) fn $install(m: &mut dyn balaur_script::Bindings<balaur_core::Engine>) {
            m.describe(&[
                ("pin_particle", &[$component], "(index: int)", "Hold one particle where it is, which is how a cloth hangs from a hook."),
                ("unpin_particle", &[$component], "(index: int)", "Let a held particle go; it keeps the velocity it had."),
                ("set_particle_target", &[$component], "(index: int, at: vec)", "Move a held particle to `at` over the next step, with the velocity that takes, which is how a cloth is dragged."),
                ("set_particle_position", &[$component], "(index: int, at: vec)", "Put one particle at `at` with no change of velocity."),
                ("set_particle_velocity", &[$component], "(index: int, velocity: vec)", "Set one particle's velocity; a held one keeps moving at it."),
                ("softbody_velocity", &[$component], "(index: int)", "How fast one particle is moving, in world space."),
                ("attach_particle", &[$component], "(index: int, body: node)", "Tie one particle to a node's rigid body where it is now: the body and the particle pull on each other."),
                ("detach_particle", &[$component], "(index: int)", "Untie one particle from every body it was attached to; answers whether it was attached."),
                ("add_softbody_force", &[$component], "(force: vec)", "Push every free particle with `force` each step until `reset_softbody_forces`."),
                ("reset_softbody_forces", &[$component], "()", "Take back every force `add_softbody_force` gave the body."),
                ("apply_softbody_impulse", &[$component], "(impulse: vec)", "Change every free particle's velocity by `impulse` at once, as a kick to the whole body."),
                ("apply_particle_impulse", &[$component], "(index: int, impulse: vec)", "Strike one particle."),
                ("apply_softbody_impulse_at", &[$component], "(impulse: vec, point: vec, radius: float)", "Strike the particles within `radius` of `point`, less the further they are; a radius of 0 strikes them all."),
                ("apply_softbody_radial_impulse", &[$component], "(center: vec, magnitude: float, radius: float)", "Push the particles within `radius` away from `center`, as a blast does."),
                ("softbody_edges", &[$component], "()", "Every edge as the two particle indices it joins, in the order `softbody_stress` reports them."),
                ("softbody_stress", &[$component], "()", "How far each edge is stretched past its rest length, as a fraction of it: what a tear is judged on."),
                ("softbody_sleeping", &[$component], "()", "Whether the body has come to rest and stopped being simulated."),
                ("wake_softbody", &[$component], "()", "Start simulating a resting body again."),
            ]);
            held(m);
            pushed(m);
            read(m);
        }


        // A node whose soft body went away between the lookup and the call.
        fn gone() -> anyhow::Error {
            anyhow::anyhow!("node has no soft body")
        }
        // Run `f` on the node's soft body with the rest of the state beside it.
        fn with<T>(
            eng: &Engine,
            node: NodeId,
            f: impl FnOnce(&mut $State, $Handle) -> anyhow::Result<T>,
        ) -> anyhow::Result<T> {
            let entity = entity_of(node)?;
            let state = eng.resource::<$State>();
            let mut state = state.borrow_mut();
            let handle = *state
                .soft_bodies
                .get(&entity)
                .ok_or_else(|| anyhow::anyhow!("node has no soft body"))?;
            anyhow::ensure!(state.world.soft_bodies.get(handle).is_some(), "node has no soft body");
            f(&mut state, handle)
        }
        // A particle index, checked against the body it indexes.
        fn particle(count: usize, index: i64) -> anyhow::Result<usize> {
            usize::try_from(index)
                .ok()
                .filter(|i| *i < count)
                .ok_or_else(|| anyhow::anyhow!("this body has no particle {index}"))
        }
        fn at(value: &balaur_script::Value) -> anyhow::Result<crate::scalar::Real> {
            match value {
                balaur_script::Value::Num(n) => Ok(*n as crate::scalar::Real),
                balaur_script::Value::Int(n) => Ok(*n as crate::scalar::Real),
                _ => anyhow::bail!("expected a number"),
            }
        }

        // Particles held, moved and read one at a time.
        fn held(m: &mut dyn balaur_script::Bindings<balaur_core::Engine>) {
            use balaur_core::Engine;
            use balaur_script::{BindingsExt, NodeId, Value};
            use crate::shared::softbody::vector_arg;
            m.function("pin_particle", |eng: &Engine, (node, index): (NodeId, i64)| {
                with(eng, node, |state, handle| {
                    let body = state.world.soft_bodies.get_mut(handle).ok_or_else(gone)?;
                    let i = particle(body.num_particles(), index)?;
                    body.set_particle_pinned(i, true);
                    Ok(())
                })
            });
            m.function("unpin_particle", |eng: &Engine, (node, index): (NodeId, i64)| {
                with(eng, node, |state, handle| {
                    let body = state.world.soft_bodies.get_mut(handle).ok_or_else(gone)?;
                    let i = particle(body.num_particles(), index)?;
                    body.set_particle_pinned(i, false);
                    Ok(())
                })
            });
            m.function(
                "set_particle_target",
                |eng: &Engine, (node, index, target): (NodeId, i64, Value)| {
                    let target = $vector(vector_arg::<$N>(&target)?);
                    with(eng, node, |state, handle| {
                        let body = state.world.soft_bodies.get_mut(handle).ok_or_else(gone)?;
                        let i = particle(body.num_particles(), index)?;
                        anyhow::ensure!(body.particles()[i].is_pinned(), "particle {index} is free: pin it first");
                        body.set_particle_kinematic_target(i, target);
                        Ok(())
                    })
                },
            );
            m.function(
                "set_particle_position",
                |eng: &Engine, (node, index, target): (NodeId, i64, Value)| {
                    let target = $vector(vector_arg::<$N>(&target)?);
                    with(eng, node, |state, handle| {
                        let body = state.world.soft_bodies.get_mut(handle).ok_or_else(gone)?;
                        let i = particle(body.num_particles(), index)?;
                        body.set_particle_position(i, target);
                        Ok(())
                    })
                },
            );
            m.function(
                "set_particle_velocity",
                |eng: &Engine, (node, index, velocity): (NodeId, i64, Value)| {
                    let velocity = $vector(vector_arg::<$N>(&velocity)?);
                    with(eng, node, |state, handle| {
                        let body = state.world.soft_bodies.get_mut(handle).ok_or_else(gone)?;
                        let i = particle(body.num_particles(), index)?;
                        body.set_particle_velocity(i, velocity);
                        Ok(())
                    })
                },
            );
            m.function("softbody_velocity", |eng: &Engine, (node, index): (NodeId, i64)| {
                with(eng, node, |state, handle| {
                    let body = state.world.soft_bodies.get(handle).ok_or_else(gone)?;
                    let i = particle(body.num_particles(), index)?;
                    Ok($value($array(body.particle_velocity(i))))
                })
            });
        }

        // A body tied to rigid bodies, and pushed by forces and impulses.
        fn pushed(m: &mut dyn balaur_script::Bindings<balaur_core::Engine>) {
            use balaur_core::{Engine, entity_of};
            use balaur_script::{BindingsExt, NodeId, Value};
            use crate::shared::softbody::vector_arg;

            m.function(
                "attach_particle",
                |eng: &Engine, (node, index, other): (NodeId, i64, NodeId)| {
                    let other = entity_of(other)?;
                    with(eng, node, |state, handle| {
                        let target = *state
                            .bodies
                            .get(&other)
                            .ok_or_else(|| anyhow::anyhow!("that node has no rigid body to attach to"))?;
                        let world = &mut state.world;
                        let body = world.soft_bodies.get_mut(handle).ok_or_else(gone)?;
                        let i = particle(body.num_particles(), index)?;
                        body.attach_particle(i, target, &world.bodies);
                        Ok(())
                    })
                },
            );
            m.function("detach_particle", |eng: &Engine, (node, index): (NodeId, i64)| {
                with(eng, node, |state, handle| {
                    let body = state.world.soft_bodies.get_mut(handle).ok_or_else(gone)?;
                    let i = particle(body.num_particles(), index)?;
                    Ok(body.detach_particle(i))
                })
            });
            m.function("add_softbody_force", |eng: &Engine, (node, force): (NodeId, Value)| {
                let force = $vector(vector_arg::<$N>(&force)?);
                with(eng, node, |state, handle| {
                    state.world.soft_bodies.get_mut(handle).ok_or_else(gone)?.add_force(force, true);
                    Ok(())
                })
            });
            m.function("reset_softbody_forces", |eng: &Engine, node: NodeId| {
                with(eng, node, |state, handle| {
                    state.world.soft_bodies.get_mut(handle).ok_or_else(gone)?.reset_forces(true);
                    Ok(())
                })
            });
            m.function(
                "apply_softbody_impulse",
                |eng: &Engine, (node, impulse): (NodeId, Value)| {
                    let impulse = $vector(vector_arg::<$N>(&impulse)?);
                    with(eng, node, |state, handle| {
                        state.world.soft_bodies.get_mut(handle).ok_or_else(gone)?.apply_impulse(impulse, true);
                        Ok(())
                    })
                },
            );
            m.function(
                "apply_particle_impulse",
                |eng: &Engine, (node, index, impulse): (NodeId, i64, Value)| {
                    let impulse = $vector(vector_arg::<$N>(&impulse)?);
                    with(eng, node, |state, handle| {
                        let body = state.world.soft_bodies.get_mut(handle).ok_or_else(gone)?;
                        let i = particle(body.num_particles(), index)?;
                        body.apply_particle_impulse(i, impulse, true);
                        Ok(())
                    })
                },
            );
            m.function(
                "apply_softbody_impulse_at",
                |eng: &Engine, (node, impulse, point, radius): (NodeId, Value, Value, Value)| {
                    let impulse = $vector(vector_arg::<$N>(&impulse)?);
                    let point = $vector(vector_arg::<$N>(&point)?);
                    let radius = at(&radius)?;
                    with(eng, node, |state, handle| {
                        state
                            .world
                            .soft_bodies
                            .get_mut(handle)
                            .ok_or_else(gone)?
                            .apply_impulse_at_point(impulse, point, radius, true);
                        Ok(())
                    })
                },
            );
            m.function(
                "apply_softbody_radial_impulse",
                |eng: &Engine, (node, center, magnitude, radius): (NodeId, Value, Value, Value)| {
                    let center = $vector(vector_arg::<$N>(&center)?);
                    let (magnitude, radius) = (at(&magnitude)?, at(&radius)?);
                    with(eng, node, |state, handle| {
                        state
                            .world
                            .soft_bodies
                            .get_mut(handle)
                            .ok_or_else(gone)?
                            .apply_radial_impulse(center, magnitude, radius, true);
                        Ok(())
                    })
                },
            );
        }

        // What the edges carry, and whether the body rests.
        fn read(m: &mut dyn balaur_script::Bindings<balaur_core::Engine>) {
            use balaur_core::Engine;
            use balaur_script::{BindingsExt, NodeId, Value};

            m.function("softbody_edges", |eng: &Engine, node: NodeId| {
                with(eng, node, |state, handle| {
                    let body = state.world.soft_bodies.get(handle).ok_or_else(gone)?;
                    Ok(Value::List(
                        body.edges()
                            .iter()
                            .map(|edge| {
                                Value::List(edge.vertices.iter().map(|&i| Value::Int(i64::from(i))).collect())
                            })
                            .collect(),
                    ))
                })
            });
            m.function("softbody_stress", |eng: &Engine, node: NodeId| {
                with(eng, node, |state, handle| {
                    let body = state.world.soft_bodies.get(handle).ok_or_else(gone)?;
                    Ok(Value::List(
                        body.edges().iter().map(|edge| Value::Num(f64::from(edge.stress()))).collect(),
                    ))
                })
            });
            m.function("softbody_sleeping", |eng: &Engine, node: NodeId| {
                with(eng, node, |state, handle| {
                    Ok(state.world.soft_bodies.get(handle).ok_or_else(gone)?.is_sleeping())
                })
            });
            m.function("wake_softbody", |eng: &Engine, node: NodeId| {
                with(eng, node, |state, handle| {
                    state.world.soft_bodies.get_mut(handle).ok_or_else(gone)?.wake_up();
                    Ok(())
                })
            });

        }
    };
}

pub(crate) use runtime_api;

/// What a body's rows say of single particles and edges, read into plain
/// numbers either dimension turns into its own rapier types.
pub(crate) struct EdgeRows {
    pub(crate) masses: Vec<f32>,
    /// `(edge index, resistance)`, the index as rapier counts: the structural
    /// edges, then the bending ones.
    pub(crate) tear: Vec<(u32, f32)>,
    /// `(structural edge index, frequency, damping)`.
    pub(crate) springs: Vec<(u32, f32, f32)>,
}

/// Read `masses`, `tear_resistance` and `edge_springs` against the layout a
/// generator made: an edge is named by the two particles it joins.
pub(crate) fn edge_rows(
    params: &toml::Value,
    particles: usize,
    structural: &[[u32; 2]],
    bending: &[[u32; 2]],
) -> anyhow::Result<EdgeRows> {
    use crate::vocabulary::keys as k;
    let masses: Vec<f32> = params
        .get(k::MASSES)
        .and_then(toml::Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(balaur_core::components::as_f64)
                .map(|m| m as f32)
                .collect()
        })
        .unwrap_or_default();
    anyhow::ensure!(
        masses.is_empty() || masses.len() == particles,
        "{} names {} masses and the body has {particles} particles",
        k::MASSES,
        masses.len()
    );
    let find = |edges: &[[u32; 2]], a: u32, b: u32| {
        edges
            .iter()
            .position(|e| (e[0] == a && e[1] == b) || (e[0] == b && e[1] == a))
    };
    let joined = |row: &toml::Value| -> anyhow::Result<(u32, u32)> {
        let end = |key: &str| {
            row.get(key)
                .and_then(toml::Value::as_integer)
                .and_then(|i| u32::try_from(i).ok())
        };
        match (end(k::A), end(k::B)) {
            (Some(a), Some(b)) => Ok((a, b)),
            _ => anyhow::bail!(
                "an edge row names its two particles as {} and {}",
                k::A,
                k::B
            ),
        }
    };
    let rows = |key: &str| {
        params
            .get(key)
            .and_then(toml::Value::as_array)
            .cloned()
            .unwrap_or_default()
    };
    let number = |row: &toml::Value, key: &str, default: f64| {
        row.get(key)
            .and_then(balaur_core::components::as_f64)
            .unwrap_or(default) as f32
    };
    let mut tear = Vec::new();
    for row in rows(k::TEAR_RESISTANCE) {
        let (a, b) = joined(&row)?;
        let index = find(structural, a, b)
            .or_else(|| find(bending, a, b).map(|i| structural.len() + i))
            .ok_or_else(|| anyhow::anyhow!("no edge joins particles {a} and {b}"))?;
        tear.push((index as u32, number(&row, k::RESISTANCE, 1.0)));
    }
    let mut springs = Vec::new();
    for row in rows(k::EDGE_SPRINGS) {
        let (a, b) = joined(&row)?;
        let index = find(structural, a, b)
            .ok_or_else(|| anyhow::anyhow!("no structural edge joins particles {a} and {b}"))?;
        springs.push((
            index as u32,
            number(&row, k::FREQUENCY, 30.0),
            number(&row, k::DAMPING, 1.0),
        ));
    }
    Ok(EdgeRows {
        masses,
        tear,
        springs,
    })
}

/// The schema rows every body shares for single particles and edges.
pub(crate) fn edge_schema() -> String {
    use crate::vocabulary::keys as k;
    let (a, b) = (k::A, k::B);
    let (resistance, frequency, damping) = (k::RESISTANCE, k::FREQUENCY, k::DAMPING);
    crate::vocabulary::schema(&[
        (
            k::MASSES,
            r#"{ type = "list", of = { type = "float" }, default = [], description = "Each particle's own mass, by index; empty spreads `mass` over them evenly", group = "particles" }"#,
        ),
        (
            k::TEAR_RESISTANCE,
            &format!(
                r#"{{ type = "list", of = {{ type = "record", fields = {{ {a} = {{ type = "int", default = 0 }}, {b} = {{ type = "int", default = 1 }}, {resistance} = {{ type = "float", default = 1.0 }} }} }}, default = [], description = "Edges that tear sooner or later than the rest, each named by the two particles it joins: below 1 is a perforation, above 1 a seam", group = "tearing" }}"#
            ),
        ),
        (
            k::EDGE_SPRINGS,
            &format!(
                r#"{{ type = "list", of = {{ type = "record", fields = {{ {a} = {{ type = "int", default = 0 }}, {b} = {{ type = "int", default = 1 }}, {frequency} = {{ type = "float", default = 30.0 }}, {damping} = {{ type = "float", default = 1.0 }} }} }}, default = [], description = "Edges with a spring of their own instead of the edge rows', each named by the two particles it joins", group = "stiffness" }}"#
            ),
        ),
        (
            k::COLLIDES,
            r#"{ type = "bool", default = true, description = "Meet the world at all; off, the body passes through everything and only its pins and ties hold it", group = "surface" }"#,
        ),
    ])
}
