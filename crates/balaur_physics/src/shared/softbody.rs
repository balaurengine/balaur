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
        /// The spring a `<name>_hz`/`<name>_damping` pair spells.
        fn $springs(
            params: &toml::Value,
            hz: &str,
            damping: &str,
            defaults: (f32, f32),
        ) -> crate::$rapier::prelude::SpringCoefficients<crate::scalar::Real> {
            crate::$rapier::prelude::SpringCoefficients::new(
                crate::scalar::real(v::f(params, hz, defaults.0)),
                crate::scalar::real(v::f(params, damping, defaults.1)),
            )
        }

        /// The mechanical properties: stiffness per constraint family, the
        /// elastic model's own parameters, how the body yields, and when it
        /// tears.
        pub(crate) fn $material(
            params: &toml::Value,
            made: &toml::Value,
        ) -> crate::$rapier::prelude::SoftBodyMaterial {
            use crate::$rapier::prelude::{SoftBodyMaterial, SoftEdgePlasticFlow};
            SoftBodyMaterial {
                edge_softness: $springs(made, k::EDGE_HZ, k::EDGE_DAMPING, (30.0, 1.0)),
                bend_softness: $springs(made, k::BEND_HZ, k::BEND_DAMPING, (10.0, 1.0)),
                volume_softness: $springs(made, k::VOLUME_HZ, k::VOLUME_DAMPING, (30.0, 1.0)),
                shape_matching_softness: $springs(
                    made,
                    k::SHAPE_MATCHING_HZ,
                    k::SHAPE_MATCHING_DAMPING,
                    (10.0, 1.0),
                ),
                young_modulus: crate::scalar::real(v::f(made, k::YOUNG_MODULUS, 1.0e4)),
                poisson_ratio: crate::scalar::real(v::f(made, k::POISSON_RATIO, 0.3)),
                elastic_damping_ratio: crate::scalar::real(v::f(made, k::ELASTIC_DAMPING, 1.0)),
                plastic_yield: crate::scalar::real(v::f(made, k::PLASTIC_YIELD, 0.0)),
                plastic_creep: crate::scalar::real(v::f(made, k::PLASTIC_CREEP, 1.0)),
                plastic_max: crate::scalar::real(v::f(made, k::PLASTIC_MAX, 1.0)),
                deformation_damping: crate::scalar::real(v::f(made, k::DEFORMATION_DAMPING, 0.0)),
                edge_plastic_yield: crate::scalar::real(v::f(made, k::EDGE_PLASTIC_YIELD, 0.0)),
                edge_plastic_creep: crate::scalar::real(v::f(made, k::EDGE_PLASTIC_CREEP, 1.0)),
                edge_plastic_max: crate::scalar::real(v::f(made, k::EDGE_PLASTIC_MAX, 0.5)),
                edge_plastic_flow: match v::text(made, k::EDGE_PLASTIC_FLOW, w::BOTH) {
                    w::COMPRESSION_FLOW => SoftEdgePlasticFlow::Compression,
                    w::TENSION => SoftEdgePlasticFlow::Tension,
                    _ => SoftEdgePlasticFlow::Both,
                },
                tear_strain: $flow(made, k::TEAR_STRAIN),
                tear_force: $flow(made, k::TEAR_FORCE),
                tear_smoothing: crate::scalar::real(v::f(made, k::TEAR_SMOOTHING, 0.0)),
                interior_strength: crate::scalar::real(v::f(made, k::INTERIOR_STRENGTH, 1.0)),
                // A schema writes "no limit" as zero; rapier writes it as the
                // largest number of tears a step could possibly ask for.
                max_tears_per_step: match v::f(params, k::MAX_TEARS_PER_STEP, 0.0) {
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
            made: &toml::Value,
        ) -> crate::$rapier::prelude::SoftBodyCellModel {
            use crate::$rapier::prelude::SoftBodyCellModel;
            match v::text(made, k::CELL_MODEL, w::VOLUME) {
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
        w::ROPE_SOFT | w::CIRCLE => format!("lower {}", k::PARTICLE_COUNT),
        w::SPHERE => format!("lower {}", k::SUBDIVISIONS),
        w::TRIANGLE_MESH | w::SOFT_POLYGON | w::POLYLINE => {
            format!("build it from a {} with fewer points", k::MESH)
        }
        w::CUSTOM | w::OUTLINE => format!("list fewer {}", k::POINTS),
        _ => format!("lower {}", k::CELLS),
    }
}

/// How many particles a rope or a circle's rim has when `particle_count` is not set.
pub(crate) const DEFAULT_PARTICLES: f32 = 16.0;
/// The fewest particles a chain holds: its two ends.
pub(crate) const MIN_CHAIN_PARTICLES: f32 = 2.0;
/// The fewest a closed ring holds.
pub(crate) const MIN_RING_PARTICLES: f32 = 3.0;

/// The particle count `particle_count` asks for, and at least `least`.
pub(crate) fn particle_count(params: &toml::Value, least: f32) -> f64 {
    let asked = crate::vocabulary::f(
        params,
        crate::vocabulary::keys::PARTICLE_COUNT,
        DEFAULT_PARTICLES,
    );
    f64::from(asked.max(least)).floor()
}

/// What a body with nothing of its own to deform is drawn in.
mod schema;
pub(crate) use schema::{
    collision_schema, filtering_schema, overrides_schema, particles_schema, primary_schema,
    seams_schema, solver_schema, tearing_schema,
};

/// What a soft body is made of and what its surface is: both of its
/// material rows, read once a build or a patch.
pub(crate) struct Materials {
    pub(crate) surface: std::rc::Rc<crate::material::PhysicsMaterial>,
    pub(crate) body: std::rc::Rc<crate::soft_material::SoftMaterial>,
}

impl Materials {
    pub(crate) fn of(eng: &balaur_core::Engine, params: &toml::Value) -> anyhow::Result<Self> {
        Ok(Self {
            surface: crate::material::of(eng, params)?,
            body: crate::soft_material::of(eng, params)?,
        })
    }
}

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
        k::MAX_TEARS_PER_STEP,
        k::MIN_PIECE,
        k::SOLVER,
        k::SOLVER_ITERATIONS,
        k::COLOR,
        k::ENABLED,
        k::SOFT_MATERIAL,
        k::PHYSICS_MATERIAL,
        k::COLLISION_LAYER,
        k::COLLISION_MASK,
        k::SOLVER_LAYER,
        k::SOLVER_MASK,
        k::CONTACT_PAIRS,
        k::SENSOR,
        k::EVENTS,
        k::CONTACT_FORCE_THRESHOLD,
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

/// `orientation` as rapier's `oriented`: `None` leaves it to rapier, which
/// orients a closed surface.
pub(crate) fn oriented(params: &toml::Value) -> Option<bool> {
    use crate::vocabulary::{self as v, keys as k, words as w};
    match v::text(params, k::ORIENTATION, w::AUTO) {
        w::SOLID => Some(true),
        w::SHELL => Some(false),
        _ => None,
    }
}

/// A material's `shape_matching` as a choice over the layout's own: `None`
/// keeps what the generator chose.
pub(crate) fn shape_matching(made: &toml::Value) -> Option<bool> {
    use crate::vocabulary::{self as v, keys as k, words as w};
    match v::text(made, k::SHAPE_MATCHING, w::AUTO) {
        w::ON => Some(true),
        w::OFF => Some(false),
        _ => None,
    }
}

/// Patch the rows [`only_in_place`] allows onto a live body and every piece
/// torn off it, in either dimension: the material and the solver on the
/// body, the surface rows on each of its colliders.
macro_rules! patch_in_place {
    ($name:ident, $world:ty, $handle:ty, $material:ident, $solver:ident) => {
        fn $name(
            world: &mut $world,
            root: $handle,
            params: &toml::Value,
            materials: &crate::shared::softbody::Materials,
        ) {
            use crate::shared::softbody::Family;
            let made = &materials.body.0;
            for piece in world.soft_bodies.family(root) {
                let Some(body) = world.soft_bodies.get_mut(piece) else {
                    continue;
                };
                body.set_material($material(params, made));
                body.set_solver($solver(params));
                body.set_volume_factor(crate::scalar::real(v::f(made, k::VOLUME_FACTOR, 1.0)));
                body.enable_volume_preservation(v::boolean(made, k::VOLUME_PRESERVATION, true));
                body.set_additional_pgs_iterations(
                    v::f(params, k::SOLVER_ITERATIONS, 3.0).max(0.0) as usize,
                );
                body.set_enabled(v::boolean(params, k::ENABLED, true));
                let colliders: Vec<_> = body.meshes().map(|mesh| mesh.collider()).collect();
                for handle in colliders {
                    if let Some(collider) = world.colliders.get_mut(handle) {
                        patch_surface(collider, params, &materials.surface);
                    }
                }
            }
        }
    };
}

pub(crate) use patch_in_place;

/// The collider a soft body meets the world through, from its surface rows,
/// and the same rows onto a live one: rapier's collider has a setter for each.
macro_rules! surface {
    (rapier = $rapier:ident, layers = $layers:path, events = $events:path) => {
        fn combine(word: &str) -> crate::$rapier::prelude::CoefficientCombineRule {
            use crate::$rapier::prelude::CoefficientCombineRule as Rule;
            match word {
                w::MIN => Rule::Min,
                w::MULTIPLY => Rule::Multiply,
                w::MAX => Rule::Max,
                w::CLAMPED_SUM => Rule::ClampedSum,
                w::GEOMETRIC_MEAN => Rule::GeometricMean,
                _ => Rule::Average,
            }
        }

        fn groups(
            params: &toml::Value,
            layer: &str,
            mask: &str,
        ) -> crate::$rapier::prelude::InteractionGroups {
            use crate::$rapier::prelude::{Group, InteractionGroups, InteractionTestMode};
            InteractionGroups::new(
                Group::from_bits_truncate(balaur_core::layers::bits(params.get(layer), false)),
                Group::from_bits_truncate(balaur_core::layers::bits(params.get(mask), true)),
                InteractionTestMode::And,
            )
        }

        /// `contact_pairs`, and rapier's own pairs for a table that names
        /// none: a script's `set_softbody` gets no schema defaults.
        fn collision_types(params: &toml::Value) -> crate::$rapier::prelude::ActiveCollisionTypes {
            use crate::$rapier::prelude::ActiveCollisionTypes;
            if params.get(k::CONTACT_PAIRS).is_none() {
                return ActiveCollisionTypes::default();
            }
            let table = v::flags::collision_types();
            ActiveCollisionTypes::from_bits_truncate(v::bits(params, k::CONTACT_PAIRS, &table))
        }

        fn events(params: &toml::Value) -> crate::$rapier::prelude::ActiveEvents {
            let table = v::flags::events();
            crate::$rapier::prelude::ActiveEvents::from_bits_truncate(v::bits(
                params,
                k::EVENTS,
                &table,
            ))
        }

        /// The collider every soft body meets the world through.
        fn surface_collider(
            params: &toml::Value,
            material: &crate::material::PhysicsMaterial,
        ) -> crate::$rapier::prelude::ColliderBuilder {
            let builder = $events(
                $layers(crate::$rapier::prelude::ColliderBuilder::ball(1.0), params),
                params,
            );
            builder
                .friction(crate::scalar::real(material.friction))
                .restitution(crate::scalar::real(material.restitution))
                .friction_combine_rule(combine(&material.friction_combine))
                .restitution_combine_rule(combine(&material.restitution_combine))
                .solver_groups(groups(params, k::SOLVER_LAYER, k::SOLVER_MASK))
                .active_collision_types(collision_types(params))
                .sensor(v::boolean(params, k::SENSOR, false))
        }

        fn patch_surface(
            collider: &mut crate::$rapier::prelude::Collider,
            params: &toml::Value,
            material: &crate::material::PhysicsMaterial,
        ) {
            let real = |key: &str, default: f32| crate::scalar::real(v::f(params, key, default));
            collider.set_friction(crate::scalar::real(material.friction));
            collider.set_restitution(crate::scalar::real(material.restitution));
            collider.set_friction_combine_rule(combine(&material.friction_combine));
            collider.set_restitution_combine_rule(combine(&material.restitution_combine));
            collider.set_collision_groups(groups(params, k::COLLISION_LAYER, k::COLLISION_MASK));
            collider.set_solver_groups(groups(params, k::SOLVER_LAYER, k::SOLVER_MASK));
            collider.set_active_collision_types(collision_types(params));
            collider.set_active_events(events(params));
            collider.set_contact_force_event_threshold(real(k::CONTACT_FORCE_THRESHOLD, 0.0));
            collider.set_sensor(v::boolean(params, k::SENSOR, false));
        }
    };
}

pub(crate) use surface;

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

/// What a body's rows say of single particles and edges, read into plain
/// numbers either dimension turns into its own rapier types.
pub(crate) struct EdgeRows {
    pub(crate) masses: Vec<f32>,
    /// `(edge index, resistance)`, the index as rapier counts: the structural
    /// edges, then the bending ones.
    pub(crate) tear: Vec<(u32, f32)>,
    /// `(edge index, hertz, damping)`, indexed as `tear` is.
    pub(crate) springs: Vec<(u32, f32, f32)>,
    /// The edges that resist stretching only, indexed as `tear` is.
    pub(crate) tension_only: Vec<u32>,
}

/// Read `masses`, `tear_resistance`, `edge_springs` and `tension_only_edges`
/// against the layout a generator made: an edge is named by the two particles
/// it joins, and found among the structural edges, then the bending ones.
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
    let edge = |row: &toml::Value| -> anyhow::Result<u32> {
        let (a, b) = joined(row)?;
        let index = find(structural, a, b)
            .or_else(|| find(bending, a, b).map(|i| structural.len() + i))
            .ok_or_else(|| anyhow::anyhow!("no edge joins particles {a} and {b}"))?;
        Ok(index as u32)
    };
    let mut tear = Vec::new();
    for row in rows(k::TEAR_RESISTANCE) {
        tear.push((edge(&row)?, number(&row, k::RESISTANCE, 1.0)));
    }
    let mut springs = Vec::new();
    for row in rows(k::EDGE_SPRINGS) {
        springs.push((
            edge(&row)?,
            number(&row, k::HZ, 30.0),
            number(&row, k::DAMPING, 1.0),
        ));
    }
    let mut tension_only = Vec::new();
    if tension(params) == Tension::Listed {
        for row in rows(k::TENSION_ONLY_EDGES) {
            tension_only.push(edge(&row)?);
        }
    }
    Ok(EdgeRows {
        masses,
        tear,
        springs,
        tension_only,
    })
}

/// What `tension_only` says of the body's edges.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Tension {
    None,
    All,
    Listed,
}

pub(crate) fn tension(params: &toml::Value) -> Tension {
    use crate::vocabulary::{self as v, keys as k, words as w};
    match v::text(params, k::TENSION_ONLY, w::NONE) {
        w::ALL => Tension::All,
        w::LISTED => Tension::Listed,
        _ => Tension::None,
    }
}

/// One `regions` entry: a part of the body with a material of its own.
pub(crate) struct Region {
    pub(crate) particles: Vec<u32>,
    pub(crate) stiffness_scale: f32,
    pub(crate) tear_resistance: f32,
    pub(crate) shape_matching: bool,
    pub(crate) pinned: bool,
    /// The region's edge spring, or `None` to keep the body's.
    pub(crate) springs: Option<(f32, f32)>,
}

/// The `regions` list, every particle checked against the `particles` the
/// body was built with.
pub(crate) fn read_regions(params: &toml::Value, particles: usize) -> anyhow::Result<Vec<Region>> {
    use crate::vocabulary::keys as k;
    let Some(rows) = params.get(k::REGIONS).and_then(toml::Value::as_array) else {
        return Ok(Vec::new());
    };
    let number = |row: &toml::Value, key: &str| {
        row.get(key)
            .and_then(balaur_core::components::as_f64)
            .map_or(0.0, |n| n as f32)
    };
    let flag =
        |row: &toml::Value, key: &str| row.get(key).and_then(toml::Value::as_bool) == Some(true);
    rows.iter()
        .map(|row| {
            let listed = row.get(k::PARTICLES).and_then(toml::Value::as_array);
            let indices: Option<Vec<u32>> = listed.map_or(Some(Vec::new()), |items| {
                items
                    .iter()
                    .map(|item| {
                        item.as_integer()
                            .and_then(|i| u32::try_from(i).ok())
                            .filter(|i| (*i as usize) < particles)
                    })
                    .collect()
            });
            let indices = indices.filter(|found| !found.is_empty()).ok_or_else(|| {
                anyhow::anyhow!(
                    "every `{}` entry lists its `{}`, each below the {particles} the body has",
                    k::REGIONS,
                    k::PARTICLES
                )
            })?;
            let hz = number(row, k::HZ);
            Ok(Region {
                particles: indices,
                stiffness_scale: number(row, k::STIFFNESS_SCALE).max(0.0),
                tear_resistance: number(row, k::TEAR_RESISTANCE).max(0.0),
                shape_matching: flag(row, k::SHAPE_MATCHING),
                pinned: flag(row, k::PINNED),
                springs: (hz > 0.0).then(|| (hz, number(row, k::DAMPING).max(0.0))),
            })
        })
        .collect()
}

/// A mesh's points and its triangles.
pub(crate) type Mesh<V> = (Vec<V>, Vec<[u32; 3]>);

/// How a `collision_mesh` is bound, before its particle list is known.
macro_rules! collision_mesh {
    (rapier = $rapier:ident, bind = $bind:ident, shape = $shape:expr) => {
        /// Put `mesh`, in world space, on the body's whole-body cluster as a
        /// deformable collider.
        fn $bind(
            world: &mut crate::$rapier::prelude::PhysicsWorld,
            handle: crate::$rapier::prelude::SoftBodyHandle,
            params: &toml::Value,
            mesh: Option<cap::Mesh<crate::$rapier::math::Vector>>,
            materials: &cap::Materials,
        ) -> anyhow::Result<()> {
            use crate::$rapier::dynamics::SoftMeshBinding;
            let Some((vertices, indices)) = mesh else {
                return Ok(());
            };
            let proxy = world
                .soft_bodies
                .get(handle)
                .and_then(|body| body.cluster_proxy(0))
                .ok_or_else(|| anyhow::anyhow!("the soft body has no cluster to bind a mesh to"))?;
            let frame = *world
                .bodies
                .get(proxy)
                .ok_or_else(|| anyhow::anyhow!("the soft body's cluster has no body"))?
                .position();
            let count = vertices.len() as u32;
            let mut collider = surface_collider(params, &materials.surface);
            collider.shape = $shape(vertices, indices)?;
            let collider = collider.position(frame.inverse());
            let binding = match v::text(params, k::COLLISION_BINDING, w::NEAREST) {
                w::BIND_PARTICLES => SoftMeshBinding::direct((0..count).collect()),
                w::BIND_CELLS => SoftMeshBinding::skinned(),
                _ => SoftMeshBinding::direct_by_position(crate::scalar::real(v::f(
                    params,
                    k::COLLISION_BINDING_DISTANCE,
                    0.01,
                ))),
            }
            .self_contacts(v::boolean(params, k::COLLISION_SELF_CONTACTS, false));
            world
                .insert_deformable(collider, binding, proxy)
                .map_err(|why| anyhow::anyhow!("`{}`: {why}", k::COLLISION_MESH))?;
            Ok(())
        }
    };
}
pub(crate) use collision_mesh;

/// Add each region to the body at `handle` as a rapier cluster, which is a
/// `soft_frame` body the snapshot carries with the rest of the world.
macro_rules! regions {
    (rapier = $rapier:ident, add = $add:ident) => {
        fn $add(
            world: &mut crate::$rapier::prelude::PhysicsWorld,
            handle: crate::$rapier::prelude::SoftBodyHandle,
            params: &toml::Value,
        ) -> anyhow::Result<()> {
            let count = world
                .soft_bodies
                .get(handle)
                .map_or(0, |body| body.num_particles());
            for region in cap::read_regions(params, count)? {
                let Some(i) = world.soft_bodies.add_cluster(
                    handle,
                    &region.particles,
                    &mut world.bodies,
                    &mut world.colliders,
                ) else {
                    continue;
                };
                let Some(body) = world.soft_bodies.get_mut(handle) else {
                    break;
                };
                body.set_cluster_stiffness_scale(i, crate::scalar::real(region.stiffness_scale));
                body.set_cluster_tear_resistance(i, crate::scalar::real(region.tear_resistance));
                body.enable_cluster_shape_matching(i, region.shape_matching);
                body.set_cluster_pinned(i, region.pinned);
                if let Some((hz, damping)) = region.springs {
                    body.set_cluster_edge_softness(
                        i,
                        Some(crate::$rapier::prelude::SpringCoefficients::new(
                            crate::scalar::real(hz),
                            crate::scalar::real(damping),
                        )),
                    );
                }
            }
            Ok(())
        }
    };
}
pub(crate) use regions;

/// The particle pairs `seams` sews together with new structural edges.
pub(crate) fn seams(params: &toml::Value, particles: usize) -> anyhow::Result<Vec<[u32; 2]>> {
    tuples::<2>(params, crate::vocabulary::keys::SEAMS, particles)
}

/// The vectors a list-of-vectors property holds; an entry that is not `N`
/// numbers is skipped.
pub(crate) fn points<const N: usize>(params: &toml::Value, key: &str) -> Vec<[f32; N]> {
    let Some(rows) = params.get(key).and_then(toml::Value::as_array) else {
        return Vec::new();
    };
    rows.iter()
        .filter_map(|row| {
            let numbers: Vec<f32> = row
                .as_array()?
                .iter()
                .map(|n| balaur_core::components::as_f64(n).map(|n| n as f32))
                .collect::<Option<_>>()?;
            <[f32; N]>::try_from(numbers).ok()
        })
        .collect()
}

/// A list of index tuples, each entry a list of `N` particle indices or a
/// record `{ a, b }` when `N` is 2, every index checked against `particles`.
pub(crate) fn tuples<const N: usize>(
    params: &toml::Value,
    key: &str,
    particles: usize,
) -> anyhow::Result<Vec<[u32; N]>> {
    use crate::vocabulary::keys as k;
    let Some(rows) = params.get(key).and_then(toml::Value::as_array) else {
        return Ok(Vec::new());
    };
    let index = |value: Option<&toml::Value>| {
        value
            .and_then(toml::Value::as_integer)
            .and_then(|i| u32::try_from(i).ok())
            .filter(|i| (*i as usize) < particles)
    };
    rows.iter()
        .map(|row| {
            let read: Option<Vec<u32>> = match row {
                toml::Value::Array(items) if items.len() == N => {
                    items.iter().map(|item| index(Some(item))).collect()
                }
                toml::Value::Table(_) if N == 2 => {
                    [k::A, k::B].iter().map(|end| index(row.get(*end))).collect()
                }
                _ => None,
            };
            read.and_then(|found| <[u32; N]>::try_from(found).ok())
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "every `{key}` entry names {N} particles, each below the {particles} the body has"
                    )
                })
        })
        .collect()
}
