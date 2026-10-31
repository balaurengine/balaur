//! What a soft body is made of, read the same way in both dimensions.
//!
//! The words are dimension-free and live in `crate::vocabulary`; what is
//! here is the reading, which names rapier's `SoftBodyMaterial` and so is a
//! different type per dimension.

macro_rules! material {
    (rapier = $rapier:ident, material = $material:ident, cell_model = $cell_model:ident) => {
        /// A material spring as rapier's.
        fn spring_of(
            spring: crate::soft_material::Spring,
        ) -> crate::$rapier::prelude::SpringCoefficients<crate::scalar::Real> {
            crate::$rapier::prelude::SpringCoefficients::new(
                crate::scalar::real(spring.hz),
                crate::scalar::real(spring.damping),
            )
        }

        /// The mechanical properties: stiffness per constraint family, the
        /// elastic model's own parameters, how the body yields, and when it
        /// tears; the body's own pacing of a crack beside them.
        pub(crate) fn $material(
            made: &crate::soft_material::SoftMaterial,
            max_tears_per_step: i64,
            min_piece: i64,
        ) -> crate::$rapier::prelude::SoftBodyMaterial {
            use crate::soft_material::PlasticFlow;
            use crate::$rapier::prelude::{SoftBodyMaterial, SoftEdgePlasticFlow};
            let real = crate::scalar::real;
            // A threshold a material writes as `0` and rapier reads as absent.
            let threshold = |value: f32| (value > 0.0).then(|| real(value));
            let count = |n: i64| u32::try_from(n).unwrap_or(u32::MAX);
            SoftBodyMaterial {
                edge_softness: spring_of(made.edge),
                bend_softness: spring_of(made.bend),
                volume_softness: spring_of(made.volume),
                shape_matching_softness: spring_of(made.shape_matching_spring),
                young_modulus: real(made.young_modulus),
                poisson_ratio: real(made.poisson_ratio),
                elastic_damping_ratio: real(made.elastic_damping),
                plastic_yield: real(made.plastic_yield),
                plastic_creep: real(made.plastic_creep),
                plastic_max: real(made.plastic_max),
                deformation_damping: real(made.deformation_damping),
                edge_plastic_yield: real(made.edge_plastic_yield),
                edge_plastic_creep: real(made.edge_plastic_creep),
                edge_plastic_max: real(made.edge_plastic_max),
                edge_plastic_flow: match made.edge_plastic_flow {
                    PlasticFlow::Compression => SoftEdgePlasticFlow::Compression,
                    PlasticFlow::Tension => SoftEdgePlasticFlow::Tension,
                    PlasticFlow::Both => SoftEdgePlasticFlow::Both,
                },
                tear_strain: threshold(made.tear_strain),
                tear_force: threshold(made.tear_force),
                tear_smoothing: real(made.tear_smoothing),
                interior_strength: real(made.interior_strength),
                // A schema writes "no limit" as zero; rapier writes it as the
                // largest number of tears a step could possibly ask for.
                max_tears_per_step: if max_tears_per_step >= 1 {
                    count(max_tears_per_step)
                } else {
                    u32::MAX
                },
                min_piece: (min_piece >= 1).then(|| count(min_piece)),
            }
        }

        pub(crate) fn $cell_model(
            made: &crate::soft_material::SoftMaterial,
        ) -> crate::$rapier::prelude::SoftBodyCellModel {
            use crate::soft_material::CellModel;
            use crate::$rapier::prelude::SoftBodyCellModel;
            match made.cell_model {
                CellModel::Corotational => SoftBodyCellModel::Corotational,
                CellModel::NeoHookean => SoftBodyCellModel::NeoHookean,
                CellModel::Volume => SoftBodyCellModel::Volume,
            }
        }
    };
}

pub(crate) use material;

/// How many particles a body may have. The count comes from a text field,
/// and rapier allocates every particle before a built body could be counted.
pub(crate) const MAX_PARTICLES: usize = 200_000;

/// Refuse a layout whose particle count, worked out from its parameters
/// before anything is built, is past the cap; `fewer` is what to change.
pub(crate) fn refuse_past_cap(particles: f64, fewer: Fewer) -> anyhow::Result<()> {
    if particles.is_nan() || particles > MAX_PARTICLES as f64 {
        let advice = fewer.advice();
        return Err(anyhow::anyhow!(
            "that would be {particles:.0} particles, past the {MAX_PARTICLES} a body may have: {advice}"
        ));
    }
    Ok(())
}

/// What a layout reads that sets how many particles it makes.
#[derive(Clone, Copy)]
pub(crate) enum Fewer {
    CellSize,
    ParticleCount,
    Subdivisions,
    MeshPoints,
    Points,
    Cells,
}

impl Fewer {
    /// What to change for fewer particles.
    fn advice(self) -> String {
        use crate::vocabulary::keys as k;
        match self {
            Self::CellSize => format!("raise {}", k::CELL_SIZE),
            Self::ParticleCount => format!("lower {}", k::PARTICLE_COUNT),
            Self::Subdivisions => format!("lower {}", k::SUBDIVISIONS),
            Self::MeshPoints => format!("build it from a {} with fewer points", k::MESH),
            Self::Points => format!("list fewer {}", k::POINTS),
            Self::Cells => format!("lower {}", k::CELLS),
        }
    }
}

/// How many particles a rope or a circle's rim has when `particle_count` is not set.
pub(crate) const DEFAULT_PARTICLES: f32 = 16.0;
/// The fewest particles a chain holds: its two ends.
pub(crate) const MIN_CHAIN_PARTICLES: f32 = 2.0;
/// The fewest a closed ring holds.
pub(crate) const MIN_RING_PARTICLES: f32 = 3.0;

/// The particle count `particle_count` asks for, and at least `least`.
pub(crate) fn particle_count(asked: i64, least: f32) -> f64 {
    f64::from((asked as f32).max(least)).floor()
}

/// What a soft body is made of and what its surface is: both of its
/// material rows, read once a build or a patch.
pub(crate) struct Materials {
    pub(crate) surface: std::rc::Rc<crate::material::PhysicsMaterial>,
    pub(crate) body: std::rc::Rc<crate::soft_material::SoftMaterial>,
}

impl Materials {
    /// The two a body's `soft_material` and `physics_material` rows name.
    pub(crate) fn of(
        eng: &balaur_core::Engine,
        soft_material: &str,
        physics_material: &str,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            surface: crate::material::named(eng, physics_material)?,
            body: crate::soft_material::named(eng, soft_material)?,
        })
    }
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

/// Whether `asked` differs from what the body was built from only in rows it
/// takes in place, which are what it is made of, how it is solved and what
/// it is drawn in: a stiffness tuned during play must not snap it to rest.
macro_rules! only_in_place {
    ($name:ident, $asked:ty) => {
        fn $name(built: &$asked, asked: &$asked) -> bool {
            let mut kept = built.clone();
            let (to, from) = (&mut kept.body, &asked.body);
            to.max_tears_per_step = from.max_tears_per_step;
            to.min_piece = from.min_piece;
            to.solver = from.solver;
            to.solver_iterations = from.solver_iterations;
            to.color = from.color;
            to.enabled = from.enabled;
            to.soft_material.clone_from(&from.soft_material);
            to.physics_material.clone_from(&from.physics_material);
            to.collision_layer.clone_from(&from.collision_layer);
            to.collision_mask.clone_from(&from.collision_mask);
            to.solver_layer.clone_from(&from.solver_layer);
            to.solver_mask.clone_from(&from.solver_mask);
            to.contact_pairs.clone_from(&from.contact_pairs);
            to.sensor = from.sensor;
            to.events.clone_from(&from.events);
            to.contact_force_threshold = from.contact_force_threshold;
            kept == *asked
        }
    };
}

pub(crate) use only_in_place;

/// `orientation` as rapier's `oriented`: `None` leaves it to rapier, which
/// orients a closed surface.
pub(crate) fn oriented(orientation: crate::properties::Orientation) -> Option<bool> {
    use crate::properties::Orientation;
    match orientation {
        Orientation::Solid => Some(true),
        Orientation::Shell => Some(false),
        Orientation::Auto => None,
    }
}

/// A count a solver row holds, as rapier takes it: none below zero.
pub(crate) fn iterations(asked: i64) -> usize {
    usize::try_from(asked.max(0)).unwrap_or(usize::MAX)
}

/// Patch the rows [`only_in_place`] allows onto a live body and every piece
/// torn off it, in either dimension: the material and the solver on the
/// body, the surface rows on each of its colliders.
macro_rules! patch_in_place {
    ($name:ident, $world:ty, $handle:ty, $body:ty, $material:ident, $solver:ident) => {
        fn $name(
            world: &mut $world,
            root: $handle,
            asked: &$body,
            materials: &crate::shared::softbody::Materials,
        ) {
            use crate::shared::softbody::Family;
            let made = &*materials.body;
            for piece in world.soft_bodies.family(root) {
                let Some(body) = world.soft_bodies.get_mut(piece) else {
                    continue;
                };
                body.set_material($material(made, asked.max_tears_per_step, asked.min_piece));
                body.set_solver($solver(asked.solver));
                body.set_volume_factor(crate::scalar::real(made.volume_factor));
                body.enable_volume_preservation(made.volume_preservation);
                body.set_additional_pgs_iterations(crate::shared::softbody::iterations(
                    asked.solver_iterations,
                ));
                body.set_enabled(asked.enabled);
                let colliders: Vec<_> = body.meshes().map(|mesh| mesh.collider()).collect();
                for handle in colliders {
                    if let Some(collider) = world.colliders.get_mut(handle) {
                        patch_surface(collider, asked, &materials.surface);
                    }
                }
            }
        }
    };
}

pub(crate) use patch_in_place;

/// The collider a soft body meets the world through, from its surface rows,
/// and the same rows onto a live one: rapier's collider has a setter for each.
/// `groups`, `pairs` and `events` are the dimension's collider readers.
macro_rules! surface {
    (
        rapier = $rapier:ident,
        body = $body:ty,
        groups = $groups:path,
        pairs = $pairs:path,
        events = $events:path
    ) => {
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

        /// The groups a layer row and its mask make; a soft body tests both.
        fn groups(
            layers: &[balaur_core::layers::Layer],
            mask: &[balaur_core::layers::Layer],
        ) -> crate::$rapier::prelude::InteractionGroups {
            $groups((layers, mask, crate::properties::LayerTest::Both))
        }

        /// The collider every soft body meets the world through.
        fn surface_collider(
            asked: &$body,
            material: &crate::material::PhysicsMaterial,
        ) -> crate::$rapier::prelude::ColliderBuilder {
            crate::$rapier::prelude::ColliderBuilder::ball(1.0)
                .collision_groups(groups(&asked.collision_layer, &asked.collision_mask))
                .active_events($events(&asked.events))
                .contact_force_event_threshold(crate::scalar::real(asked.contact_force_threshold))
                .friction(crate::scalar::real(material.friction))
                .restitution(crate::scalar::real(material.restitution))
                .friction_combine_rule(combine(&material.friction_combine))
                .restitution_combine_rule(combine(&material.restitution_combine))
                .solver_groups(groups(&asked.solver_layer, &asked.solver_mask))
                .active_collision_types($pairs(&asked.contact_pairs))
                .sensor(asked.sensor)
        }

        fn patch_surface(
            collider: &mut crate::$rapier::prelude::Collider,
            asked: &$body,
            material: &crate::material::PhysicsMaterial,
        ) {
            collider.set_friction(crate::scalar::real(material.friction));
            collider.set_restitution(crate::scalar::real(material.restitution));
            collider.set_friction_combine_rule(combine(&material.friction_combine));
            collider.set_restitution_combine_rule(combine(&material.restitution_combine));
            collider.set_collision_groups(groups(&asked.collision_layer, &asked.collision_mask));
            collider.set_solver_groups(groups(&asked.solver_layer, &asked.solver_mask));
            collider.set_active_collision_types($pairs(&asked.contact_pairs));
            collider.set_active_events($events(&asked.events));
            collider.set_contact_force_event_threshold(crate::scalar::real(
                asked.contact_force_threshold,
            ));
            collider.set_sensor(asked.sensor);
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

/// The rows of a body that name single particles and edges.
pub(crate) struct EdgeAsk<'a> {
    pub(crate) masses: &'a [f32],
    pub(crate) tear_resistance: &'a [crate::properties::EdgeTearResistance],
    pub(crate) edge_springs: &'a [crate::properties::EdgeSpring],
    pub(crate) tension_only: crate::properties::TensionOnly,
    pub(crate) tension_only_edges: &'a [crate::properties::ParticlePair],
}

/// Read `masses`, `tear_resistance`, `edge_springs` and `tension_only_edges`
/// against the layout a generator made: an edge is named by the two particles
/// it joins, and found among the structural edges, then the bending ones.
pub(crate) fn edge_rows(
    ask: &EdgeAsk<'_>,
    particles: usize,
    structural: &[[u32; 2]],
    bending: &[[u32; 2]],
) -> anyhow::Result<EdgeRows> {
    use crate::vocabulary::keys as k;
    anyhow::ensure!(
        ask.masses.is_empty() || ask.masses.len() == particles,
        "{} names {} masses and the body has {particles} particles",
        k::MASSES,
        ask.masses.len()
    );
    let find = |edges: &[[u32; 2]], a: u32, b: u32| {
        edges
            .iter()
            .position(|e| (e[0] == a && e[1] == b) || (e[0] == b && e[1] == a))
    };
    let edge = |first: i64, second: i64| -> anyhow::Result<u32> {
        let (Ok(a), Ok(b)) = (u32::try_from(first), u32::try_from(second)) else {
            anyhow::bail!(
                "an edge row names its two particles as {} and {}",
                k::A,
                k::B
            );
        };
        let index = find(structural, a, b)
            .or_else(|| find(bending, a, b).map(|i| structural.len() + i))
            .ok_or_else(|| anyhow::anyhow!("no edge joins particles {a} and {b}"))?;
        Ok(index as u32)
    };
    let mut tear = Vec::new();
    for row in ask.tear_resistance {
        tear.push((edge(row.first, row.second)?, row.resistance));
    }
    let mut springs = Vec::new();
    for row in ask.edge_springs {
        springs.push((edge(row.first, row.second)?, row.hz, row.damping));
    }
    let mut tension_only = Vec::new();
    if ask.tension_only == crate::properties::TensionOnly::Listed {
        for row in ask.tension_only_edges {
            tension_only.push(edge(row.first, row.second)?);
        }
    }
    Ok(EdgeRows {
        masses: ask.masses.to_vec(),
        tear,
        springs,
        tension_only,
    })
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
pub(crate) fn read_regions(
    regions: &[crate::properties::SoftRegion],
    particles: usize,
) -> anyhow::Result<Vec<Region>> {
    use crate::vocabulary::keys as k;
    regions
        .iter()
        .map(|row| {
            let indices: Option<Vec<u32>> = row
                .particles
                .iter()
                .map(|i| u32::try_from(*i).ok().filter(|i| (*i as usize) < particles))
                .collect();
            let indices = indices.filter(|found| !found.is_empty()).ok_or_else(|| {
                anyhow::anyhow!(
                    "every `{}` entry lists its `{}`, each below the {particles} the body has",
                    k::REGIONS,
                    k::PARTICLES
                )
            })?;
            Ok(Region {
                particles: indices,
                stiffness_scale: row.stiffness_scale.max(0.0),
                tear_resistance: row.tear_resistance.max(0.0),
                shape_matching: row.shape_matching,
                pinned: row.pinned,
                springs: (row.hz > 0.0).then(|| (row.hz, row.damping.max(0.0))),
            })
        })
        .collect()
}

/// A mesh's points and its triangles.
pub(crate) type Mesh<V> = (Vec<V>, Vec<[u32; 3]>);

/// How a `collision_mesh` is bound, before its particle list is known.
macro_rules! collision_mesh {
    (
        rapier = $rapier:ident,
        body = $body:ty,
        collision = $collision:ty,
        bind = $bind:ident,
        shape = $shape:expr
    ) => {
        /// Put `mesh`, in world space, on the body's whole-body cluster as a
        /// deformable collider.
        fn $bind(
            world: &mut crate::$rapier::prelude::PhysicsWorld,
            handle: crate::$rapier::prelude::SoftBodyHandle,
            asked: &$body,
            collision: &$collision,
            mesh: Option<cap::Mesh<crate::$rapier::math::Vector>>,
            materials: &cap::Materials,
        ) -> anyhow::Result<()> {
            use crate::properties::Binding;
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
            let mut collider = surface_collider(asked, &materials.surface);
            collider.shape = $shape(vertices, indices)?;
            let collider = collider.position(frame.inverse());
            let binding = match &collision.collision_binding {
                Binding::Particles => SoftMeshBinding::direct((0..count).collect()),
                Binding::Cells => SoftMeshBinding::skinned(),
                Binding::Nearest(near) => {
                    SoftMeshBinding::direct_by_position(crate::scalar::real(near.distance))
                }
            }
            .self_contacts(collision.collision_self_contacts);
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
            regions: &[crate::properties::SoftRegion],
        ) -> anyhow::Result<()> {
            let count = world
                .soft_bodies
                .get(handle)
                .map_or(0, |body| body.num_particles());
            for region in cap::read_regions(regions, count)? {
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
pub(crate) fn seams(
    pairs: &[crate::properties::ParticlePair],
    particles: usize,
) -> anyhow::Result<Vec<[u32; 2]>> {
    let rows: Vec<Vec<i64>> = pairs
        .iter()
        .map(|pair| vec![pair.first, pair.second])
        .collect();
    tuples::<2>(&rows, crate::vocabulary::keys::SEAMS, particles)
}

/// A list of index tuples, each entry `N` particle indices, every index
/// checked against `particles`.
pub(crate) fn tuples<const N: usize>(
    rows: &[Vec<i64>],
    key: &str,
    particles: usize,
) -> anyhow::Result<Vec<[u32; N]>> {
    let index = |i: i64| u32::try_from(i).ok().filter(|i| (*i as usize) < particles);
    rows.iter()
        .map(|row| {
            let read: Option<Vec<u32>> = (row.len() == N)
                .then(|| row.iter().map(|i| index(*i)).collect())
                .flatten();
            read.and_then(|found| <[u32; N]>::try_from(found).ok())
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "every `{key}` entry names {N} particles, each below the {particles} the body has"
                    )
                })
        })
        .collect()
}
