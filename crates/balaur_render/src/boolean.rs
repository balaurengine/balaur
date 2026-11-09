//! `boolean3d` and `boolean2d`: a node whose shape is what its children add
//! up to, cut out of one another or overlap in.
//!
//! The operands stay in the tree, hidden and editable; moving one recomputes
//! the result. Nothing here knows how to combine geometry -- that is
//! `balaur_geometry::csg` in 3D and `geometry2d` in 2D -- so a script asking for
//! the same operation gets the same answer.

use anyhow::{Result, anyhow};
use balaur_core::components::derived::{Component, Enum, Written};
use balaur_core::hecs::Entity;
use balaur_core::scene::{Appearance, Children, GlobalTransform};
use balaur_core::{Engine, entity_of};
use balaur_geometry::csg::{self, Op};
use balaur_geometry::geometry2d::{self, BooleanOptions, FillRule, Op2d, Shapes2d};
use balaur_geometry::mesh::MeshData;
use balaur_plugin::Registry;
use balaur_script::{Module, NodeId};
use glamx::{Mat4, Vec2, Vec3};

use crate::{PolygonMesh, Renderable2d, Renderable3d, Shape2d, Shape3d};
use balaur_core::layers::Layer;

const BUILT_MESH: &str = "built_mesh";

/// How a `boolean3d` combines its children: `balaur_geometry::csg`'s words.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Enum)]
pub enum Operation3d {
    Union,
    Difference,
    Intersection,
}

/// How a `boolean2d` combines its children: `geometry2d`'s words.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Enum)]
pub enum Operation2d {
    Union,
    Difference,
    Intersection,
    SymmetricDifference,
    ReverseDifference,
}

/// Which parts of crossing or nested outlines a `boolean2d` counts as
/// inside: `geometry2d`'s words.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Enum)]
pub enum FillRule2d {
    EvenOdd,
    NonZero,
    Positive,
    Negative,
}

/// Draws the node as its children combined by `operation`: `union`, `difference` or `intersection`. The children stay in the tree, hidden and editable; `color`, `texture` and `material` dress the result.
#[derive(Component)]
#[component(
    name = "boolean3d",
    on_remove = boolean3d_removed,
    tags = [crate::vocabulary::words::PERSPECTIVE, "render"],
    after_write = boolean3d_written,
)]
pub struct Boolean3d {
    /// How the children are combined, in the order they are declared
    #[property(default = Operation3d::Union)]
    pub operation: Operation3d,
    /// Tint of the result, as channel floats or #rrggbb / #rrggbbaa
    #[property(color, default = [0.8, 0.8, 0.8, 1.0])]
    pub color: [f32; 4],
    /// Image file, project-relative, or a `texture` asset the result is drawn with, over the UVs the children carry through the cut; empty draws the colour alone
    #[property(asset = "texture")]
    pub texture: String,
    /// The material the result draws with; empty takes an inherited `material` component, else the built-in one
    #[property(asset = "material")]
    pub material: String,
    /// Whether the result casts a shadow from the lights that cast
    #[property(default = true, group = "shadows")]
    pub cast_shadow: bool,
    /// The layers the result is drawn on: a `camera3d` draws it when their `render_layers` share one; empty is every layer
    #[property(flags, group = "layers")]
    pub render_layers: Vec<Layer>,
    /// The light layers the result is lit on: a `light3d` lights it when their `light_layers` share one; empty is every layer
    #[property(flags, group = "layers")]
    pub light_layers: Vec<Layer>,
    /// Whether shadows land on it; off lights it as if nothing stood between it and every light
    #[property(default = true, group = "shadows")]
    pub receive_shadows: bool,
    /// The id `render.snap_aov("segmentation")` colours this node by; nodes sharing one share a colour. Zero takes one of its own, from 1 up
    #[property(min = 0, group = "layers")]
    pub segmentation_id: i64,
}

/// Draws the node as its 2D children combined by `operation`, holes and all. The children stay in the tree, hidden and editable; `fill_rule` and the cleanup keys tune the combination, and `color`, `texture` and `material` dress the result.
#[derive(Component)]
#[component(
    name = "boolean2d",
    on_remove = boolean2d_removed,
    tags = [crate::vocabulary::words::ORTHOGRAPHIC, "render"],
    after_write = boolean2d_written,
)]
pub struct Boolean2d {
    /// How the children are combined, in the order they are declared
    #[property(default = Operation2d::Union)]
    pub operation: Operation2d,
    /// Which parts of crossing or nested outlines count as inside: an odd number of them, any winding, or only counter-clockwise or clockwise winding
    #[property(default = FillRule2d::EvenOdd)]
    pub fill_rule: FillRule2d,
    /// Tint of the result, as channel floats or #rrggbb / #rrggbbaa
    #[property(color, default = [1.0, 1.0, 1.0, 1.0])]
    pub color: [f32; 4],
    /// Image file, project-relative, or a `texture` asset the result is drawn with, centred on the node at 100 texture pixels per unit, as a `polygon`'s default UVs are; empty draws the colour alone
    #[property(asset = "texture")]
    pub texture: String,
    /// The material the result draws with; empty takes an inherited `material` component, else the built-in one
    #[property(asset = "material")]
    pub material: String,
    /// Clear the result of the near-duplicate points rounding leaves
    #[property(default = true, group = "mesh cleanup")]
    pub clean_result: bool,
    /// Keep a point that sits on the straight line between its neighbours, in the children and in the result
    #[property(group = "mesh cleanup")]
    pub keep_collinear: bool,
    /// A result outline or hole enclosing less than this area, in square units of the node's own space, is dropped
    #[property(min = 0.0, group = "mesh cleanup")]
    pub min_area: f32,
}

/// What a boolean's result is drawn with, as authored.
#[derive(Clone, PartialEq)]
struct Look {
    color: [f32; 4],
    texture: String,
    material: String,
}

/// A [`Boolean3d`] as the combining reads it: which operation, how the
/// result draws, and what the operands looked like when it last ran.
pub(crate) struct Combined3d {
    pub(crate) op: Op,
    look: Look,
    lighting: crate::Lighting3d,
    /// A digest of the children's geometry and poses. Recomputing a boolean
    /// is not cheap, so it happens when this changes and not every tick.
    signature: u64,
}

/// The same for a [`Boolean2d`], with the outlines it settled on so a
/// boolean nested in it hands over its holes too.
pub(crate) struct Combined2d {
    pub(crate) op: Op2d,
    options: BooleanOptions,
    look: Look,
    shapes: Shapes2d,
    signature: u64,
}

impl Combined3d {
    /// `asked`, not yet combined.
    fn of(asked: &Boolean3d) -> Result<Self> {
        let word = asked.operation.option();
        Ok(Self {
            op: Op::from_word(word)
                .ok_or_else(|| anyhow!("unknown boolean3d operation '{word}'"))?,
            look: Look {
                color: asked.color,
                texture: asked.texture.clone(),
                material: asked.material.clone(),
            },
            lighting: crate::Lighting3d::of(
                asked.cast_shadow,
                &asked.light_layers,
                &asked.render_layers,
                asked.receive_shadows,
                asked.segmentation_id,
            ),
            signature: 0,
        })
    }
}

impl Combined2d {
    /// `asked`, not yet combined.
    fn of(asked: &Boolean2d) -> Result<Self> {
        let word = asked.operation.option();
        let rule = asked.fill_rule.option();
        Ok(Self {
            op: Op2d::from_word(word)
                .ok_or_else(|| anyhow!("unknown boolean2d operation '{word}'"))?,
            options: BooleanOptions {
                fill_rule: FillRule::from_word(rule)
                    .ok_or_else(|| anyhow!("unknown boolean2d fill_rule '{rule}'"))?,
                min_area: asked.min_area.max(0.0),
                keep_collinear: asked.keep_collinear,
                clean_result: asked.clean_result,
            },
            look: Look {
                color: asked.color,
                texture: asked.texture.clone(),
                material: asked.material.clone(),
            },
            shapes: Vec::new(),
            signature: 0,
        })
    }
}

/// What the result's renderable is drawn with, onto one that already exists.
fn dress_3d(eng: &Engine, entity: Entity, boolean: &Combined3d) -> Result<()> {
    {
        let world = eng.world();
        let Ok(mut r) = world.get::<&mut Renderable3d>(entity) else {
            return Ok(());
        };
        r.color = boolean.look.color;
        if r.texture != boolean.look.texture {
            r.texture.clone_from(&boolean.look.texture);
            r.version += 1;
        }
    }
    crate::set_lighting(eng, entity, &boolean.lighting);
    crate::material::set_material_3d(eng, entity, &boolean.look.material)
}

/// A written `boolean3d` dresses its result again; a changed operation is a
/// changed answer, whatever the operands, so it combines them again too.
fn boolean3d_written(eng: &Engine, entity: Entity, _written: Written) -> Result<()> {
    let mut next = {
        let world = eng.world();
        let Ok(asked) = world.get::<&Boolean3d>(entity) else {
            return Ok(());
        };
        Combined3d::of(&asked)?
    };
    dress_3d(eng, entity, &next)?;
    if let Ok(existing) = eng.world().get::<&Combined3d>(entity)
        && existing.op == next.op
    {
        next.signature = existing.signature;
    }
    eng.insert_one(entity, next)
        .map_err(|_| anyhow!("node is dead"))
}

/// What a removed `Boolean3d` leaves behind, taken with it.
fn boolean3d_removed(eng: &Engine, entity: Entity) {
    let _ = eng.remove_one::<Combined3d>(entity);
    let _ = eng.remove_one::<Renderable3d>(entity);
}

/// The `boolean3d` component: writes [`Combined3d`], and a `Renderable3d`
/// whose shape is built rather than authored.
pub(crate) fn register_boolean3d_component(reg: &mut Registry<'_>) {
    reg.add_component::<Boolean3d>();
}

/// A written `boolean2d` dresses its result again. The texture sizes the
/// UVs, so only a new tint or material leaves the result standing.
fn boolean2d_written(eng: &Engine, entity: Entity, _written: Written) -> Result<()> {
    let mut next = {
        let world = eng.world();
        let Ok(asked) = world.get::<&Boolean2d>(entity) else {
            return Ok(());
        };
        Combined2d::of(&asked)?
    };
    {
        if let Ok(mut existing) = eng.world().get::<&mut Combined2d>(entity)
            && existing.op == next.op
            && existing.options == next.options
            && existing.look.texture == next.look.texture
        {
            next.signature = existing.signature;
            next.shapes = std::mem::take(&mut existing.shapes);
        }
        eng.insert_one(entity, next)
            .map_err(|_| anyhow!("node is dead"))?;
    }
    dress_2d(eng, entity)
}

/// What a removed `Boolean2d` leaves behind, taken with it.
fn boolean2d_removed(eng: &Engine, entity: Entity) {
    let _ = eng.remove_one::<Combined2d>(entity);
    let _ = eng.remove_one::<Renderable2d>(entity);
}

/// The `boolean2d` component: the same over filled outlines.
pub(crate) fn register_boolean2d_component(reg: &mut Registry<'_>) {
    reg.add_component::<Boolean2d>();
}

/// The 2D result's tint and material, onto its renderable if it has one yet.
fn dress_2d(eng: &Engine, entity: Entity) -> Result<()> {
    let look = {
        let world = eng.world();
        let Ok(boolean) = world.get::<&Combined2d>(entity) else {
            return Ok(());
        };
        boolean.look.clone()
    };
    {
        let world = eng.world();
        let Ok(mut r) = world.get::<&mut Renderable2d>(entity) else {
            return Ok(());
        };
        r.color = look.color;
    }
    crate::material::set_material_2d(eng, entity, &look.material)
}

/// Fold bits into a running digest. Not a hash of anything security cares
/// about: it only has to change when an operand does.
fn fold(state: &mut u64, bits: u64) {
    *state = state.rotate_left(7) ^ bits.wrapping_mul(0x9e37_79b9_7f4a_7c15);
}

/// What the operands look like now: enough that a moved, resized or
/// replaced child changes it, and a still frame does not.
fn signature_of(eng: &Engine, operands: &[Entity], flat: bool) -> u64 {
    let world = eng.world();
    let mut state = 0xcbf2_9ce4_8422_2325;
    for entity in operands {
        fold(&mut state, entity.to_bits().get());
        if flat {
            if let Ok(r) = world.get::<&Renderable2d>(*entity) {
                fold(&mut state, r.version);
            }
        } else if let Ok(r) = world.get::<&Renderable3d>(*entity) {
            fold(&mut state, r.version);
        }
        if let Ok(at) = world.get::<&GlobalTransform>(*entity) {
            for value in at
                .position
                .to_array()
                .into_iter()
                .chain(at.scale.to_array())
                .chain(at.rotation.to_array())
            {
                fold(&mut state, u64::from(value.to_bits()));
            }
        }
    }
    state
}

/// A child's geometry in the boolean node's space.
///
/// A primitive is built from its parameters, a mesh asset is loaded, and a
/// boolean's own result is taken as it stands -- so booleans nest.
fn operand_mesh(eng: &Engine, parent: &GlobalTransform, entity: Entity) -> Option<MeshData> {
    let (shape, reference, built) = {
        let world = eng.world();
        let r = world.get::<&Renderable3d>(entity).ok()?;
        (r.shape, r.mesh.clone(), r.built.clone())
    };
    let mut mesh = match shape {
        Shape3d::Solid(solid) => solid.build(),
        Shape3d::Built => built.as_deref()?.clone(),
        Shape3d::Mesh => {
            let reference = reference?;
            let definition = balaur_core::assets::load_typed::<MeshData>(eng, &reference).ok()?;
            balaur_geometry::mesh::load_from(eng, &definition).ok()?
        }
    };
    let world = eng.world();
    let at = world.get::<&GlobalTransform>(entity).ok()?;
    // Into the parent's frame: the operands are combined where they sit
    // relative to the node that owns them, not where they sit in the world.
    let into_parent = matrix(parent).inverse() * matrix(&at);
    for position in &mut mesh.positions {
        *position = into_parent
            .transform_point3(Vec3::from_array(*position))
            .to_array();
    }
    if let Some(normals) = &mut mesh.normals {
        for normal in normals {
            *normal = into_parent
                .transform_vector3(Vec3::from_array(*normal))
                .normalize_or_zero()
                .to_array();
        }
    }
    Some(mesh)
}

fn matrix(at: &GlobalTransform) -> Mat4 {
    Mat4::from_scale_rotation_translation(at.scale, at.rotation, at.position)
}

/// A child's outlines in the boolean node's space, for the 2D case: one ring
/// for a shape, or a nested boolean's result, holes and all.
fn operand_shapes(eng: &Engine, parent: &GlobalTransform, entity: Entity) -> Option<Shapes2d> {
    let world = eng.world();
    let shapes: Shapes2d = if let Ok(nested) = world.get::<&Combined2d>(entity) {
        nested.shapes.clone()
    } else {
        let r = world.get::<&Renderable2d>(entity).ok()?;
        let ring = match r.shape {
            Shape2d::Flat(flat) => flat.outline(),
            Shape2d::Sprite { hx, hy } => balaur_geometry::primitive::Flat::rect(hx, hy).outline(),
            Shape2d::Polygon => r.polygon.as_ref()?.positions.clone(),
            Shape2d::Polyline { .. } => return None,
        };
        vec![vec![ring]]
    };
    let at = world.get::<&GlobalTransform>(entity).ok()?;
    let into_parent = matrix(parent).inverse() * matrix(&at);
    let moved = |p: &Vec2| {
        let moved = into_parent.transform_point3(Vec3::new(p.x, p.y, 0.0));
        Vec2::new(moved.x, moved.y)
    };
    Some(
        shapes
            .iter()
            .map(|shape| {
                shape
                    .iter()
                    .map(|path| path.iter().map(moved).collect())
                    .collect()
            })
            .collect(),
    )
}

/// The script surface: the triangles a boolean settled on, so an editor can
/// bake one out to a plain `mesh` asset under `models/`.
pub(crate) fn install_boolean_api(m: &mut Module<Engine>) {
    m.describe(&[(
        BUILT_MESH,
        &["boolean3d"],
        "(node: node) -> BuiltMesh?",
        "The triangles the node's boolean settled on, ready to be written out as a `mesh` asset; nil when the node draws no built geometry.",
    )]);
    m.records(&[(
        "BuiltMesh",
        "{ positions: [Vec3], indices: [[int]] }",
        "A boolean's triangles, under the keys a `mesh` asset spells: its positions, and three corners a triangle.",
    )]);
    m.function(BUILT_MESH, |eng: &Engine, node: NodeId| {
        let world = eng.world();
        let Ok(renderable) = world.get::<&Renderable3d>(entity_of(node)?) else {
            return Ok(balaur_script::Value::Nil);
        };
        let Some(mesh) = renderable.built.as_deref() else {
            return Ok(balaur_script::Value::Nil);
        };
        Ok(mesh_value(mesh))
    });
}

/// A mesh as the table a `mesh` asset is written from.
fn mesh_value(mesh: &MeshData) -> balaur_script::Value {
    use balaur_script::Value;
    let positions = Value::List(mesh.positions.iter().map(|p| Value::Vec3(*p)).collect());
    let indices = Value::List(
        mesh.indices
            .iter()
            .map(|t| Value::List(t.iter().map(|i| Value::Int(i64::from(*i))).collect()))
            .collect(),
    );
    Value::Map(vec![
        ("positions".to_string(), positions),
        ("indices".to_string(), indices),
    ])
}

/// Recompute every boolean whose operands moved or changed, and hide the
/// operands so only the result is drawn.
pub(crate) fn resolve_booleans_system(eng: &Engine, _dt: f32) {
    resolve_3d(eng);
    resolve_2d(eng);
}

/// Which booleans need recomputing, with their operands and what those look
/// like now.
fn stale(eng: &Engine, flat: bool) -> Vec<(Entity, Vec<Entity>, u64)> {
    let owners: Vec<Entity> = {
        if flat {
            eng.world()
                .query::<(Entity, &Combined2d)>()
                .iter()
                .map(|(e, _)| e)
                .collect()
        } else {
            eng.world()
                .query::<(Entity, &Combined3d)>()
                .iter()
                .map(|(e, _)| e)
                .collect()
        }
    };
    owners
        .into_iter()
        .filter_map(|entity| {
            let operands: Vec<Entity> = eng
                .world()
                .get::<&Children>(entity)
                .map(|children| children.0.clone())
                .unwrap_or_default();
            let signature = signature_of(eng, &operands, flat);
            let world = eng.world();
            let seen = if flat {
                world.get::<&Combined2d>(entity).ok().map(|b| b.signature)
            } else {
                world.get::<&Combined3d>(entity).ok().map(|b| b.signature)
            };
            (seen != Some(signature)).then_some((entity, operands, signature))
        })
        .collect()
}

/// Where a node has settled this tick, or the origin when it has no pose.
fn pose_of(eng: &Engine, entity: Entity) -> GlobalTransform {
    eng.world()
        .get::<&GlobalTransform>(entity)
        .map_or(GlobalTransform::identity(), |at| *at)
}

fn resolve_3d(eng: &Engine) {
    for (entity, operands, signature) in stale(eng, false) {
        let Some(op) = eng.world().get::<&Combined3d>(entity).ok().map(|b| b.op) else {
            continue;
        };
        let parent = pose_of(eng, entity);
        let meshes: Vec<MeshData> = operands
            .iter()
            .filter_map(|child| operand_mesh(eng, &parent, *child))
            .collect();
        let result = meshes
            .into_iter()
            .reduce(|left, right| csg::combine(&left, &right, op))
            .unwrap_or_default();
        let bounds = result.bounds().map(|(min, max)| {
            let (min, max) = (Vec3::from_array(min), Vec3::from_array(max));
            crate::Bounds3d {
                centre: (min + max) / 2.0,
                half: (max - min) / 2.0,
            }
        });
        let world = eng.world();
        let Ok(mut boolean) = world.get::<&mut Combined3d>(entity) else {
            continue;
        };
        boolean.signature = signature;
        let fresh = Renderable3d {
            shape: Shape3d::Built,
            bounds,
            color: boolean.look.color,
            mesh: None,
            built: None,
            skeleton: String::new(),
            texture: boolean.look.texture.clone(),
            material: boolean.look.material.clone(),
            shadows: boolean.lighting.shadows,
            layers: boolean.lighting.layers,
            render_layers: boolean.lighting.render_layers,
            overlay: boolean.lighting.overlay,
            version: 0,
        };
        drop(boolean);
        drop(world);
        let built = Some(std::sync::Arc::new(result));
        if let Ok(mut renderable) = eng.world().get::<&mut Renderable3d>(entity) {
            renderable.shape = Shape3d::Built;
            renderable.built = built;
            renderable.bounds = bounds;
            renderable.version += 1;
        } else {
            let _ = eng.insert_one(entity, Renderable3d { built, ..fresh });
        }
        hide(eng, &operands);
    }
}

fn resolve_2d(eng: &Engine) {
    for (entity, operands, signature) in stale(eng, true) {
        let Some((op, options, texture)) = eng
            .world()
            .get::<&Combined2d>(entity)
            .ok()
            .map(|b| (b.op, b.options, b.look.texture.clone()))
        else {
            continue;
        };
        let parent = pose_of(eng, entity);
        let operands_shapes: Vec<Shapes2d> = operands
            .iter()
            .filter_map(|child| operand_shapes(eng, &parent, *child))
            .collect();
        let shapes = combine_shapes(&operands_shapes, op, options);
        let size = if texture.is_empty() {
            (1, 1)
        } else {
            crate::texture::size_of(eng, &texture).unwrap_or((1, 1))
        };
        let polygon = std::sync::Arc::new(fill_shapes(&shapes, texture, size));
        let created = eng.world().get::<&Renderable2d>(entity).is_err();
        {
            let world = eng.world();
            if let Ok(mut boolean) = world.get::<&mut Combined2d>(entity) {
                boolean.signature = signature;
                boolean.shapes = shapes;
            }
        }
        let _ = crate::set_polygon(eng, entity, polygon);
        if created {
            let _ = dress_2d(eng, entity);
        }
        hide(eng, &operands);
    }
}

/// The operands folded together, in the order they are declared.
fn combine_shapes(operands: &[Shapes2d], op: Op2d, options: BooleanOptions) -> Shapes2d {
    let mut current = operands.first().cloned().unwrap_or_default();
    for next in operands.iter().skip(1) {
        current = geometry2d::combine(&current, next, op, options);
    }
    current
}

/// Every shape filled with its holes cut out, as one polygon. The texture sits
/// centred on the node at the default pixels per unit, as a `polygon`'s does.
fn fill_shapes(shapes: &Shapes2d, texture: String, size: (u32, u32)) -> PolygonMesh {
    let mut positions: Vec<Vec2> = Vec::new();
    let mut indices: Vec<[u32; 3]> = Vec::new();
    for shape in shapes {
        let contours: Vec<Vec<[f32; 2]>> = shape
            .iter()
            .map(|path| path.iter().map(|p| [p.x, p.y]).collect())
            .collect();
        let (points, triangles) = balaur_geometry::triangulate::triangulate_shape(&contours);
        let base = positions.len() as u32;
        positions.extend(points.iter().map(|p| Vec2::new(p[0], p[1])));
        indices.extend(triangles.iter().map(|t| t.map(|i| i + base)));
    }
    let ppu = crate::DEFAULT_PIXELS_PER_UNIT;
    PolygonMesh {
        mesh: String::new(),
        texture,
        skeleton: String::new(),
        pixels_per_unit: ppu,
        uvs: positions
            .iter()
            .map(|p| PolygonMesh::default_uv(*p, ppu, size))
            .collect(),
        positions,
        indices,
        skin: None,
    }
}

/// Hide the operands: the node draws their result, and drawing them too
/// would put the parts on top of the whole.
fn hide(eng: &Engine, operands: &[Entity]) {
    let world = eng.world();
    for entity in operands {
        if let Ok(mut appearance) = world.get::<&mut Appearance>(*entity) {
            appearance.visible = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use balaur_geometry::{csg, geometry2d};

    use super::{FillRule2d, Operation2d, Operation3d};

    /// The operations and fill rules a boolean offers are the ones core
    /// combines by.
    #[test]
    fn a_boolean_offers_the_words_core_reads() {
        assert_eq!(Operation3d::OPTIONS, csg::words::OPS);
        assert_eq!(Operation2d::OPTIONS, geometry2d::words::OPS);
        assert_eq!(FillRule2d::OPTIONS, geometry2d::words::FILL_RULES);
    }
}
