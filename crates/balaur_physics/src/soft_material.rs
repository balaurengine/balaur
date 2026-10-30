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

/// A soft body's material, as its definition holds it with every row filled
/// in: the readers that build rapier's material read it by row.
#[derive(Debug)]
pub(crate) struct SoftMaterial(pub(crate) toml::Value);

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
    Ok(Rc::new(SoftMaterial(table.clone())))
}

pub(crate) fn register_soft_material_asset(reg: &mut Registry<'_>) {
    reg.register_record_asset(k::SOFT_MATERIAL, "soft_materials", DOC, schema(), parse);
}

/// The material a soft body's `soft_material` row names: the asset, or the
/// asset at its defaults when the row is empty.
pub(crate) fn of(eng: &Engine, params: &toml::Value) -> Result<Rc<SoftMaterial>> {
    balaur_core::assets::record(eng, params.get(k::SOFT_MATERIAL), k::SOFT_MATERIAL)
}

/// The row a soft body names its material in, among its ungrouped rows.
pub(crate) fn row_spec() -> String {
    format!(
        r#"{{ type = "asset", asset = "{}", default = "", description = "What the body is made of: its cells, springs, volume, yield and tearing; empty takes a soft_material at its defaults" }}"#,
        k::SOFT_MATERIAL
    )
}
