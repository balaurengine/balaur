//! `boolean3d` and `boolean2d`: a node whose shape is what its children add
//! up to, cut out of one another or overlap in.
//!
//! The operands stay in the tree, hidden and editable; moving one recomputes
//! the result. Nothing here knows how to combine geometry -- that is
//! `balaur_core::csg` in 3D and `geometry2d` in 2D -- so a script asking for
//! the same operation gets the same answer.

use anyhow::{Result, anyhow};
use balaur_core::components::ComponentDef;
use balaur_core::csg::{self, Op};
use balaur_core::hecs::Entity;
use balaur_core::mesh::MeshData;
use balaur_core::scene::{Appearance, Children, GlobalTransform};
use balaur_core::{Engine, entity_of};
use balaur_plugin::Registry;
use balaur_script::{Bindings, BindingsExt, NodeId};
use glamx::{Mat4, Vec2, Vec3};

use crate::shape::keys as k;
use crate::{PolygonMesh, Renderable, Renderable2d, Shape, Shape2d};

/// The `boolean3d` component: which operation, and what the operands looked
/// like when it last ran.
pub(crate) struct Boolean3d {
    pub(crate) op: Op,
    /// A digest of the children's geometry and poses. Recomputing a boolean
    /// is not cheap, so it happens when this changes and not every tick.
    signature: u64,
}

/// The `boolean2d` component, the same in the plane.
pub(crate) struct Boolean2d {
    pub(crate) op: Op,
    signature: u64,
}

/// What a boolean's schema offers.
fn schema(name: &str) -> ComponentDef {
    ComponentDef {
        doc: "",
        schema: ComponentDef::parse_schema(
            name,
            &ComponentDef::schema(&[(
                k::OP,
                &format!(
                    r#"{{ type = "enum", default = "{}", options = [{}], description = "How the children are combined, in the order they are declared" }}"#,
                    csg::words::UNION,
                    crate::shape::options(csg::words::OPS)
                ),
            )]),
        ),
        tags: &[],
        expects: &[],
        apply: Box::new(|_, _, _| Ok(())),
        remove: Box::new(|_, _| Ok(())),
        get: Box::new(|_, _| None),
    }
}

/// The operation a params table names.
fn op_from_params(params: &toml::Value) -> Result<Op> {
    let word = params
        .get(k::OP)
        .and_then(toml::Value::as_str)
        .unwrap_or(csg::words::UNION);
    Op::from_word(word).ok_or_else(|| anyhow!("unknown boolean op '{word}'"))
}

/// The `boolean3d` component: writes [`Boolean3d`], and a `Renderable` whose
/// shape is built rather than authored.
pub(crate) fn register_boolean3d_component(reg: &mut Registry<'_>) {
    let mut def = schema("boolean3d");
    def.doc = "Draws the node as its children combined by `op`: `union`, `difference` or `intersection`. The children stay in the tree, hidden and editable.";
    def.tags = &[crate::shape::words::PERSPECTIVE, "render"];
    def.apply = Box::new(|eng, entity, params| {
        let op = op_from_params(params)?;
        let mut world = eng.world_mut();
        if let Ok(mut existing) = world.get::<&mut Boolean3d>(entity) {
            existing.op = op;
            // A changed operation is a changed answer, whatever the operands.
            existing.signature = 0;
            return Ok(());
        }
        world
            .insert_one(entity, Boolean3d { op, signature: 0 })
            .map_err(|_| anyhow!("node is dead"))
    });
    def.remove = Box::new(|eng, entity| {
        let mut world = eng.world_mut();
        let _ = world.remove_one::<Boolean3d>(entity);
        let _ = world.remove_one::<Renderable>(entity);
        Ok(())
    });
    def.get = Box::new(|eng, entity| {
        let world = eng.world();
        let boolean = world.get::<&Boolean3d>(entity).ok()?;
        let mut map = toml::map::Map::new();
        map.insert(k::OP.into(), toml::Value::String(boolean.op.word().into()));
        Some(toml::Value::Table(map))
    });
    reg.register_component("boolean3d", def);
}

/// The `boolean2d` component: the same over filled outlines.
pub(crate) fn register_boolean2d_component(reg: &mut Registry<'_>) {
    let mut def = schema("boolean2d");
    def.doc = "Draws the node as its 2D children combined by `op`: `union`, `difference` or `intersection`. The children stay in the tree, hidden and editable.";
    def.tags = &[crate::shape::words::ORTHOGRAPHIC, "render"];
    def.apply = Box::new(|eng, entity, params| {
        let op = op_from_params(params)?;
        let mut world = eng.world_mut();
        if let Ok(mut existing) = world.get::<&mut Boolean2d>(entity) {
            existing.op = op;
            existing.signature = 0;
            return Ok(());
        }
        world
            .insert_one(entity, Boolean2d { op, signature: 0 })
            .map_err(|_| anyhow!("node is dead"))
    });
    def.remove = Box::new(|eng, entity| {
        let mut world = eng.world_mut();
        let _ = world.remove_one::<Boolean2d>(entity);
        let _ = world.remove_one::<Renderable2d>(entity);
        Ok(())
    });
    def.get = Box::new(|eng, entity| {
        let world = eng.world();
        let boolean = world.get::<&Boolean2d>(entity).ok()?;
        let mut map = toml::map::Map::new();
        map.insert(k::OP.into(), toml::Value::String(boolean.op.word().into()));
        Some(toml::Value::Table(map))
    });
    reg.register_component("boolean2d", def);
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
        } else if let Ok(r) = world.get::<&Renderable>(*entity) {
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
        let r = world.get::<&Renderable>(entity).ok()?;
        (r.shape, r.mesh.clone(), r.built.clone())
    };
    let mut mesh = match shape {
        Shape::Solid(solid) => solid.build(),
        Shape::Built => built.as_deref()?.clone(),
        Shape::Mesh => {
            let reference = reference?;
            let definition = balaur_core::assets::load_typed::<MeshData>(eng, &reference).ok()?;
            balaur_core::mesh::load_from(eng, &definition).ok()?
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

/// A child's outline in the boolean node's space, for the 2D case.
fn operand_outline(eng: &Engine, parent: &GlobalTransform, entity: Entity) -> Option<Vec<Vec2>> {
    let world = eng.world();
    let r = world.get::<&Renderable2d>(entity).ok()?;
    let points: Vec<Vec2> = match r.shape {
        Shape2d::Flat(flat) => flat.outline(),
        Shape2d::Sprite { hx, hy } => balaur_core::primitive::Flat::rect(hx, hy).outline(),
        Shape2d::Polygon => r.polygon.as_ref()?.positions.clone(),
        Shape2d::Polyline { .. } => return None,
    };
    let at = world.get::<&GlobalTransform>(entity).ok()?;
    let into_parent = matrix(parent).inverse() * matrix(&at);
    Some(
        points
            .into_iter()
            .map(|p| {
                let moved = into_parent.transform_point3(Vec3::new(p.x, p.y, 0.0));
                Vec2::new(moved.x, moved.y)
            })
            .collect(),
    )
}

/// The script surface: the triangles a boolean settled on, so an editor can
/// bake one out to a plain `mesh` asset under `models/`.
pub(crate) fn install_boolean_api(m: &mut dyn Bindings<Engine>) {
    m.describe(&[(
        "built_mesh",
        &["boolean3d"],
        "(node: node) -> table",
        "The triangles the node's boolean settled on, as `#{ positions, indices }` ready to be written out as a `mesh` asset; nil when the node draws no built geometry.",
    )]);
    m.function("built_mesh", |eng: &Engine, node: NodeId| {
        let world = eng.world();
        let Ok(renderable) = world.get::<&Renderable>(entity_of(node)?) else {
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

/// Which booleans need recomputing, and what their operands are.
fn stale(eng: &Engine, flat: bool) -> Vec<(Entity, Op, Vec<Entity>, u64)> {
    let owners: Vec<(Entity, Op)> = {
        let world = eng.world();
        let mut owners = Vec::new();
        if flat {
            for (entity, boolean) in &mut world.query::<(Entity, &Boolean2d)>() {
                owners.push((entity, boolean.op));
            }
        } else {
            for (entity, boolean) in &mut world.query::<(Entity, &Boolean3d)>() {
                owners.push((entity, boolean.op));
            }
        }
        owners
    };
    owners
        .into_iter()
        .filter_map(|(entity, op)| {
            let operands: Vec<Entity> = eng
                .world()
                .get::<&Children>(entity)
                .map(|children| children.0.clone())
                .unwrap_or_default();
            let signature = signature_of(eng, &operands, flat);
            let world = eng.world();
            let seen = if flat {
                world.get::<&Boolean2d>(entity).ok().map(|b| b.signature)
            } else {
                world.get::<&Boolean3d>(entity).ok().map(|b| b.signature)
            };
            (seen != Some(signature)).then_some((entity, op, operands, signature))
        })
        .collect()
}

/// Where a node has settled this tick, or the origin when it has no pose.
fn pose_of(eng: &Engine, entity: Entity) -> GlobalTransform {
    let world = eng.world();
    world
        .get::<&GlobalTransform>(entity)
        .map_or(GlobalTransform::identity(), |at| *at)
}

fn resolve_3d(eng: &Engine) {
    for (entity, op, operands, signature) in stale(eng, false) {
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
            crate::Bounds {
                centre: (min + max) / 2.0,
                half: (max - min) / 2.0,
            }
        });
        let mut world = eng.world_mut();
        if let Ok(mut boolean) = world.get::<&mut Boolean3d>(entity) {
            boolean.signature = signature;
        }
        if let Ok(mut renderable) = world.get::<&mut Renderable>(entity) {
            renderable.shape = Shape::Built;
            renderable.built = Some(std::sync::Arc::new(result));
            renderable.bounds = bounds;
            renderable.version += 1;
        } else {
            let _ = world.insert_one(
                entity,
                Renderable {
                    shape: Shape::Built,
                    bounds,
                    color: [0.8, 0.8, 0.8, 1.0],
                    mesh: None,
                    built: Some(std::sync::Arc::new(result)),
                    skeleton: String::new(),
                    texture: String::new(),
                    material: String::new(),
                    shadows: true,
                    layers: u32::MAX,
                    version: 0,
                },
            );
        }
        drop(world);
        hide(eng, &operands);
    }
}

fn resolve_2d(eng: &Engine) {
    for (entity, op, operands, signature) in stale(eng, true) {
        let parent = pose_of(eng, entity);
        let outlines: Vec<Vec<Vec2>> = operands
            .iter()
            .filter_map(|child| operand_outline(eng, &parent, *child))
            .collect();
        let polygon = std::sync::Arc::new(combine_outlines(&outlines, op));
        let world = eng.world_mut();
        if let Ok(mut boolean) = world.get::<&mut Boolean2d>(entity) {
            boolean.signature = signature;
        }
        drop(world);
        let _ = crate::set_polygon(eng, entity, polygon);
        hide(eng, &operands);
    }
}

/// The outlines folded together and filled. Only the first ring of each
/// shape is kept: `Shape2d::Polygon` fills one loop, and a hole would need a
/// second one it has nowhere to put.
fn combine_outlines(outlines: &[Vec<Vec2>], op: Op) -> PolygonMesh {
    let mut current = outlines.first().cloned().unwrap_or_default();
    for next in outlines.iter().skip(1) {
        let shapes = balaur_core::geometry2d::overlay(&current, next, op);
        current = shapes
            .into_iter()
            .next()
            .and_then(|shape| shape.into_iter().next())
            .unwrap_or_default();
    }
    let ring: Vec<u32> = (0..current.len() as u32).collect();
    let indices = balaur_core::triangulate::triangulate(&current, &ring).unwrap_or_default();
    PolygonMesh {
        mesh: String::new(),
        texture: String::new(),
        skeleton: String::new(),
        pixels_per_unit: crate::DEFAULT_PIXELS_PER_UNIT,
        uvs: current
            .iter()
            .map(|p| PolygonMesh::default_uv(*p, crate::DEFAULT_PIXELS_PER_UNIT, (1, 1)))
            .collect(),
        positions: current,
        indices,
        skin: None,
    }
}

/// Hide the operands: the node draws their result, and drawing them too
/// would put the parts on top of the whole.
fn hide(eng: &Engine, operands: &[Entity]) {
    let world = eng.world_mut();
    for entity in operands {
        if let Ok(mut appearance) = world.get::<&mut Appearance>(*entity) {
            appearance.visible = false;
        }
    }
}
