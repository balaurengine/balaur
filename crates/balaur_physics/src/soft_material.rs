//! The `soft_material` asset: what a soft body is made of. How its cells
//! resist, the springs along its edges, how it keeps its volume, how it
//! yields and when it tears; one file says jelly, cloth, rubber or clay for
//! every soft body naming it. A cloth's woven springs are read by a 3D cloth
//! alone.

use std::rc::Rc;

use anyhow::Result;
use balaur_core::Engine;
use balaur_core::components::{ComponentDef, Schema};
use balaur_plugin::Registry;

use crate::vocabulary::{self as v, keys as k, words as w};

const DOC: &str = "What a soft body is made of: how its cells resist, its springs, how it keeps its volume, how it yields and when it tears. A `softbody2d` or `softbody3d` names one in its `soft_material` row, or writes one inline:\n\n```toml\ntype = \"soft_material\"\ncell_model = \"neo_hookean\"\nyoung_modulus = 50000.0\n```";

/// A soft body's material, read out of its definition once: the builders in
/// both dimensions turn it into rapier's own types.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SoftMaterial {
    pub(crate) cell_model: CellModel,
    pub(crate) young_modulus: f32,
    pub(crate) poisson_ratio: f32,
    pub(crate) elastic_damping: f32,
    pub(crate) deformation_damping: f32,
    pub(crate) edge: Spring,
    pub(crate) bend: Spring,
    pub(crate) volume: Spring,
    pub(crate) shape_matching_spring: Spring,
    /// A 3D cloth's woven springs; a zero takes the edge spring's.
    pub(crate) shear: Spring,
    pub(crate) warp: Spring,
    pub(crate) weft: Spring,
    /// `None` keeps what the layout chose.
    pub(crate) shape_matching: Option<bool>,
    pub(crate) volume_preservation: bool,
    pub(crate) volume_factor: f32,
    pub(crate) plastic_yield: f32,
    pub(crate) plastic_creep: f32,
    pub(crate) plastic_max: f32,
    pub(crate) edge_plastic_yield: f32,
    pub(crate) edge_plastic_creep: f32,
    pub(crate) edge_plastic_max: f32,
    pub(crate) edge_plastic_flow: PlasticFlow,
    /// Zero is unbreakable.
    pub(crate) tear_force: f32,
    pub(crate) tear_strain: f32,
    pub(crate) tear_smoothing: f32,
    pub(crate) interior_strength: f32,
}

/// What a cell resists with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CellModel {
    Volume,
    Corotational,
    NeoHookean,
}

/// Whether an edge sets under a squeeze, a stretch or both.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PlasticFlow {
    Both,
    Compression,
    Tension,
}

/// A spring's frequency in hertz and its damping ratio.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Spring {
    pub(crate) hz: f32,
    pub(crate) damping: f32,
}

/// The asset's rows: the ones a soft body used to carry itself, the cells
/// and springs first, then its volume, its yield and its tearing.
fn schema() -> Rc<Schema> {
    ComponentDef::parse_schema(
        k::SOFT_MATERIAL,
        &[resisting_rows(), holding_rows()].join("\n"),
    )
}

/// What the cells and the springs resist with. Split from [`schema`] under
/// `MAX_FN_LINES`.
fn resisting_rows() -> String {
    let volume = w::VOLUME;
    let models = v::options(w::CELL_MODELS);
    v::schema(&[
        (
            k::CELL_MODEL,
            &format!(
                r#"{{ type = "enum", default = "{volume}", options = [{models}], description = "What a cell resists with: a volume constraint for a cheap jelly, or an elastic model a Young modulus parameterises", group = "elasticity" }}"#
            ),
        ),
        (
            k::YOUNG_MODULUS,
            r#"{ type = "float", default = 10000.0, min = 0.0, description = "Stiffness of the elastic cells, as force per unit area; a finer mesh does not get stiffer for it", group = "elasticity" }"#,
        ),
        (
            k::POISSON_RATIO,
            r#"{ type = "float", default = 0.3, min = 0.0, max = 0.499, description = "How much an elastic cell bulges sideways when squeezed; towards 0.5 it stops changing volume at all", group = "elasticity" }"#,
        ),
        (
            k::ELASTIC_DAMPING,
            r#"{ type = "float", default = 1.0, min = 0.0, max = 100.0, description = "Damping ratio of the elastic cells", group = "elasticity" }"#,
        ),
        (
            k::DEFORMATION_DAMPING,
            r#"{ type = "float", default = 0.0, min = 0.0, max = 1000.0, description = "How fast the particles are pulled towards the body's own rigid motion, which settles a residual sway without slowing the body down", group = "elasticity" }"#,
        ),
        (
            k::EDGE_HZ,
            r#"{ type = "float", default = 30.0, min = 0.0, max = 10000.0, description = "The frequency of the spring a structural edge is solved as, in hertz; higher is stiffer", group = "springs" }"#,
        ),
        (
            k::EDGE_DAMPING,
            r#"{ type = "float", default = 1.0, min = 0.0, max = 100.0, description = "The damping ratio of that spring; 1 settles without overshooting", group = "springs" }"#,
        ),
        (
            k::BEND_HZ,
            r#"{ type = "float", default = 10.0, min = 0.0, max = 10000.0, description = "The same for the bending edges, which are what stop a cloth folding flat", group = "springs" }"#,
        ),
        (
            k::BEND_DAMPING,
            r#"{ type = "float", default = 1.0, min = 0.0, max = 100.0, description = "The damping ratio of the bending springs", group = "springs" }"#,
        ),
        (
            k::SHEAR_HZ,
            r#"{ type = "float", default = 0.0, min = 0.0, description = "A cloth's resistance to being skewed, in hertz; 0 takes edge_hz", group = "springs" }"#,
        ),
        (
            k::SHEAR_DAMPING,
            r#"{ type = "float", default = 0.0, min = 0.0, description = "The damping ratio of the shear springs; 0 takes edge_damping", group = "springs" }"#,
        ),
        (
            k::WARP_HZ,
            r#"{ type = "float", default = 0.0, min = 0.0, description = "A cloth's stiffness along its first axis, in hertz, as woven cloth is stiffer along the warp; 0 takes edge_hz", group = "springs" }"#,
        ),
        (
            k::WARP_DAMPING,
            r#"{ type = "float", default = 0.0, min = 0.0, description = "The damping ratio of the warp springs; 0 takes edge_damping. Any of the six woven keys set gives the cloth its own springs", group = "springs" }"#,
        ),
        (
            k::WEFT_HZ,
            r#"{ type = "float", default = 0.0, min = 0.0, description = "The same across it, along the weft; 0 takes edge_hz", group = "springs" }"#,
        ),
        (
            k::WEFT_DAMPING,
            r#"{ type = "float", default = 0.0, min = 0.0, description = "The damping ratio of the weft springs; 0 takes edge_damping", group = "springs" }"#,
        ),
        (
            k::VOLUME_HZ,
            r#"{ type = "float", default = 30.0, min = 0.0, max = 10000.0, description = "The same for the constraints holding a cell's volume, and for the whole-body one", group = "springs" }"#,
        ),
        (
            k::VOLUME_DAMPING,
            r#"{ type = "float", default = 1.0, min = 0.0, max = 100.0, description = "The damping ratio of the volume constraints", group = "springs" }"#,
        ),
        (
            k::SHAPE_MATCHING_HZ,
            r#"{ type = "float", default = 10.0, min = 0.0, max = 10000.0, description = "The same for shape matching, which pulls the body back towards the shape it was built in", group = "springs" }"#,
        ),
        (
            k::SHAPE_MATCHING_DAMPING,
            r#"{ type = "float", default = 1.0, min = 0.0, max = 100.0, description = "The damping ratio of the shape-matching constraints", group = "springs" }"#,
        ),
    ])
}

/// How the body keeps its volume, yields and tears.
fn holding_rows() -> String {
    let auto = w::AUTO;
    let matching = v::options(w::SHAPE_MATCHING_MODES);
    let both = w::BOTH;
    let flows = v::options(w::PLASTIC_FLOWS);
    v::schema(&[
        (
            k::SHAPE_MATCHING,
            &format!(
                r#"{{ type = "enum", default = "{auto}", options = [{matching}], description = "Pull the body back towards the shape it was built in, which is what keeps a jelly a jelly; auto keeps the layout's own choice, on for a triangle_mesh or a polyline", group = "volume" }}"#
            ),
        ),
        (
            k::VOLUME_PRESERVATION,
            r#"{ type = "bool", default = true, description = "Hold the volume each closed piece of the body encloses; an open sheet or a rope encloses none, and a hoop without it caves in", group = "volume" }"#,
        ),
        (
            k::VOLUME_FACTOR,
            r#"{ type = "float", default = 1.0, min = 0.0, description = "What that volume is held at, as a multiple of the rest volume; above 1 inflates the body", group = "volume" }"#,
        ),
        (
            k::PLASTIC_YIELD,
            r#"{ type = "float", default = 0.0, min = 0.0, description = "The cell strain past which the rest shape flows towards the current one; 0 is perfectly elastic", group = "plasticity" }"#,
        ),
        (
            k::PLASTIC_CREEP,
            r#"{ type = "float", default = 1.0, min = 0.0, description = "How fast, per second, the strain past the yield is absorbed into the rest shape", group = "plasticity" }"#,
        ),
        (
            k::PLASTIC_MAX,
            r#"{ type = "float", default = 1.0, min = 0.0, description = "The most permanent deformation a cell may take, so a crushed cell cannot flow to a sliver", group = "plasticity" }"#,
        ),
        (
            k::EDGE_PLASTIC_YIELD,
            r#"{ type = "float", default = 0.0, min = 0.0, description = "The edge strain past which its rest length flows towards its current length", group = "plasticity" }"#,
        ),
        (
            k::EDGE_PLASTIC_CREEP,
            r#"{ type = "float", default = 1.0, min = 0.0, description = "How fast, per second, an edge's excess strain is absorbed into its rest length", group = "plasticity" }"#,
        ),
        (
            k::EDGE_PLASTIC_MAX,
            r#"{ type = "float", default = 0.5, min = 0.0, description = "The largest permanent set an edge may take, as a fraction of its first length", group = "plasticity" }"#,
        ),
        (
            k::EDGE_PLASTIC_FLOW,
            &format!(
                r#"{{ type = "enum", default = "{both}", options = [{flows}], description = "Whether an edge sets under a squeeze, a stretch, or both: clay dents but does not stay stretched", group = "plasticity" }}"#
            ),
        ),
        (
            k::TEAR_FORCE,
            r#"{ type = "float", default = 0.0, min = 0.0, description = "The pull past which an edge breaks; 0 is unbreakable. Either criterion tears an edge", group = "tearing" }"#,
        ),
        (
            k::TEAR_STRAIN,
            r#"{ type = "float", default = 0.0, min = 0.0, description = "The stretch past which an element breaks, as a fraction of its rest length; 0 is unbreakable", group = "tearing" }"#,
        ),
        (
            k::TEAR_SMOOTHING,
            r#"{ type = "float", default = 0.0, min = 0.0, description = "Over how many seconds a load is averaged before it is tested, so one hard frame does not tear a body", group = "tearing" }"#,
        ),
        (
            k::INTERIOR_STRENGTH,
            r#"{ type = "float", default = 1.0, min = 1.0, description = "How many times tougher an undamaged inside element is than a surface one, so cracks start at the surface and run inward", group = "tearing" }"#,
        ),
    ])
}

/// A definition with every row filled in, as the asset layer hands it over.
#[allow(
    clippy::unnecessary_wraps,
    reason = "the asset layer's parser type, which other assets fail through"
)]
fn parse(table: &toml::Value) -> Result<Rc<dyn std::any::Any>> {
    Ok(Rc::new(read(table)))
}

fn read(made: &toml::Value) -> SoftMaterial {
    let spring = |hz: &str, damping: &str, defaults: (f32, f32)| Spring {
        hz: v::f(made, hz, defaults.0),
        damping: v::f(made, damping, defaults.1),
    };
    SoftMaterial {
        cell_model: match v::text(made, k::CELL_MODEL, w::VOLUME) {
            w::COROTATIONAL => CellModel::Corotational,
            w::NEO_HOOKEAN => CellModel::NeoHookean,
            _ => CellModel::Volume,
        },
        young_modulus: v::f(made, k::YOUNG_MODULUS, 1.0e4),
        poisson_ratio: v::f(made, k::POISSON_RATIO, 0.3),
        elastic_damping: v::f(made, k::ELASTIC_DAMPING, 1.0),
        deformation_damping: v::f(made, k::DEFORMATION_DAMPING, 0.0),
        edge: spring(k::EDGE_HZ, k::EDGE_DAMPING, (30.0, 1.0)),
        bend: spring(k::BEND_HZ, k::BEND_DAMPING, (10.0, 1.0)),
        volume: spring(k::VOLUME_HZ, k::VOLUME_DAMPING, (30.0, 1.0)),
        shape_matching_spring: spring(k::SHAPE_MATCHING_HZ, k::SHAPE_MATCHING_DAMPING, (10.0, 1.0)),
        shear: spring(k::SHEAR_HZ, k::SHEAR_DAMPING, (0.0, 0.0)),
        warp: spring(k::WARP_HZ, k::WARP_DAMPING, (0.0, 0.0)),
        weft: spring(k::WEFT_HZ, k::WEFT_DAMPING, (0.0, 0.0)),
        shape_matching: match v::text(made, k::SHAPE_MATCHING, w::AUTO) {
            w::ON => Some(true),
            w::OFF => Some(false),
            _ => None,
        },
        volume_preservation: v::boolean(made, k::VOLUME_PRESERVATION, true),
        volume_factor: v::f(made, k::VOLUME_FACTOR, 1.0),
        plastic_yield: v::f(made, k::PLASTIC_YIELD, 0.0),
        plastic_creep: v::f(made, k::PLASTIC_CREEP, 1.0),
        plastic_max: v::f(made, k::PLASTIC_MAX, 1.0),
        edge_plastic_yield: v::f(made, k::EDGE_PLASTIC_YIELD, 0.0),
        edge_plastic_creep: v::f(made, k::EDGE_PLASTIC_CREEP, 1.0),
        edge_plastic_max: v::f(made, k::EDGE_PLASTIC_MAX, 0.5),
        edge_plastic_flow: match v::text(made, k::EDGE_PLASTIC_FLOW, w::BOTH) {
            w::COMPRESSION_FLOW => PlasticFlow::Compression,
            w::TENSION => PlasticFlow::Tension,
            _ => PlasticFlow::Both,
        },
        tear_force: v::f(made, k::TEAR_FORCE, 0.0),
        tear_strain: v::f(made, k::TEAR_STRAIN, 0.0),
        tear_smoothing: v::f(made, k::TEAR_SMOOTHING, 0.0),
        interior_strength: v::f(made, k::INTERIOR_STRENGTH, 1.0),
    }
}

pub(crate) fn register_soft_material_asset(reg: &mut Registry<'_>) {
    reg.register_record_asset(k::SOFT_MATERIAL, "soft_materials", DOC, schema(), parse);
}

/// The material a soft body's `soft_material` row names: a file, an inline
/// one's key, or the asset at its defaults for none.
pub(crate) fn named(eng: &Engine, reference: &str) -> Result<Rc<SoftMaterial>> {
    let value = (!reference.is_empty()).then(|| toml::Value::String(reference.to_owned()));
    balaur_core::assets::record(eng, value.as_ref(), k::SOFT_MATERIAL)
}
