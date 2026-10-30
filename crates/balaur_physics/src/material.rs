//! The `physics_material` asset: what a collider's surface is made of, how
//! it slides and bounces, and how each combines with what it touches. One
//! file says ice or rubber for every collider, tile map and soft body naming
//! it, as a Godot `PhysicsMaterial` does.

use std::rc::Rc;

use anyhow::Result;
use balaur_core::Engine;
use balaur_core::components::{ComponentDef, Schema};
use balaur_plugin::Registry;

use crate::vocabulary::{self as v, keys as k, words as w};

const DOC: &str = "What a collider's surface is made of: `friction` and `restitution`, and how each combines with the surface it touches. A collider, a `tile_collision` or a soft body names one in its `physics_material` row, or writes one inline:\n\n```toml\ntype = \"physics_material\"\nfriction = 0.05\nfriction_combine = \"min\"\n```";

/// A surface as the solver reads it. The combine rules stay words: each
/// dimension's rapier names its own rule type.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PhysicsMaterial {
    pub(crate) friction: f32,
    pub(crate) friction_combine: String,
    pub(crate) restitution: f32,
    pub(crate) restitution_combine: String,
}

/// The asset's rows: the four a collider used to carry itself.
fn schema() -> Rc<Schema> {
    let average = w::AVERAGE;
    let combine = v::options(w::COMBINE_RULES);
    ComponentDef::parse_schema(
        k::PHYSICS_MATERIAL,
        &v::schema(&[
            (
                k::FRICTION,
                r#"{ type = "float", default = 0.5, min = 0.0, description = "Surface friction; 0 is ice" }"#,
            ),
            (
                k::FRICTION_COMBINE,
                &format!(
                    r#"{{ type = "enum", default = "{average}", options = [{combine}], description = "How this surface's friction combines with the other one's" }}"#
                ),
            ),
            (
                k::RESTITUTION,
                r#"{ type = "float", default = 0.0, min = 0.0, description = "Bounciness: 0 is a dead stop, 1 a full rebound, and above 1 each bounce gains energy" }"#,
            ),
            (
                k::RESTITUTION_COMBINE,
                &format!(
                    r#"{{ type = "enum", default = "{average}", options = [{combine}], description = "How this surface's bounciness combines with the other one's" }}"#
                ),
            ),
        ]),
    )
}

/// A definition with every row filled in, as the asset layer hands it over.
#[allow(
    clippy::unnecessary_wraps,
    reason = "the asset layer's parser type, which other assets fail through"
)]
fn parse(table: &toml::Value) -> Result<Rc<dyn std::any::Any>> {
    Ok(Rc::new(read(table)))
}

fn read(table: &toml::Value) -> PhysicsMaterial {
    PhysicsMaterial {
        friction: v::f(table, k::FRICTION, 0.5).max(0.0),
        friction_combine: v::text(table, k::FRICTION_COMBINE, w::AVERAGE).to_owned(),
        restitution: v::f(table, k::RESTITUTION, 0.0).max(0.0),
        restitution_combine: v::text(table, k::RESTITUTION_COMBINE, w::AVERAGE).to_owned(),
    }
}

pub(crate) fn register_physics_material_asset(reg: &mut Registry<'_>) {
    reg.register_record_asset(
        k::PHYSICS_MATERIAL,
        "physics_materials",
        DOC,
        schema(),
        parse,
    );
}

/// The surface a component's `physics_material` row names: the asset, or
/// the asset at its defaults when the row is empty.
pub(crate) fn of(eng: &Engine, params: &toml::Value) -> Result<Rc<PhysicsMaterial>> {
    balaur_core::assets::record(eng, params.get(k::PHYSICS_MATERIAL), k::PHYSICS_MATERIAL)
}

/// Whether `params` names a material kept in a file in any of `rows`, which
/// can change under the component holding it; an inline one is keyed by its
/// own content.
pub(crate) fn names_a_file(params: &toml::Value, rows: &[&str]) -> bool {
    rows.iter().any(|row| {
        let reference = v::text(params, row, "");
        !reference.is_empty() && !reference.starts_with('#')
    })
}

/// The row a component names its material in, at the end of its ungrouped
/// rows.
pub(crate) fn row_spec() -> String {
    format!(
        r#"{{ type = "asset", asset = "{}", default = "", description = "What the surface is made of: friction, bounce and how each combines; empty takes a physics_material at its defaults" }}"#,
        k::PHYSICS_MATERIAL
    )
}
