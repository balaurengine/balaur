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
