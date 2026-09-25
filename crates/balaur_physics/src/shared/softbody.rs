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
                ..SoftBodyMaterial::default()
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
