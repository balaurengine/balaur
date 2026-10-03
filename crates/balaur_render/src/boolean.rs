//! `boolean3d` and `boolean2d`: a node whose shape is what its children add
//! up to, cut out of one another or overlap in.
//!
//! The operands stay in the tree, hidden and editable; moving one recomputes
//! the result. Nothing here knows how to combine geometry -- that is
//! `balaur_core::csg` in 3D and `geometry2d` in 2D -- so a script asking for
//! the same operation gets the same answer.

use anyhow::{Result, anyhow};
use balaur_core::components::{ComponentDef, as_f64, prop_bool, prop_f32, prop_str};
use balaur_core::csg::{self, Op};
use balaur_core::geometry2d::{self, BooleanOptions, FillRule, Op2d, Shapes2d};
use balaur_core::hecs::Entity;
use balaur_core::mesh::MeshData;
use balaur_core::scene::{Appearance, Children, GlobalTransform};
use balaur_core::{Engine, entity_of};
use balaur_plugin::Registry;
use balaur_script::{Bindings, BindingsExt, NodeId};
use glamx::{Mat4, Vec2, Vec3};

use crate::vocabulary::{keys as k, options};
use crate::{PolygonMesh, Renderable2d, Renderable3d, Shape2d, Shape3d};

/// What a boolean's result is drawn with, as authored.
#[derive(Clone, PartialEq)]
struct Look {
    color: [f32; 4],
    texture: String,
    material: String,
}

/// The `boolean3d` component: which operation, how the result draws, and
/// what the operands looked like when it last ran.
pub(crate) struct Boolean3d {
    pub(crate) op: Op,
    look: Look,
    shadows: bool,
    layers: u32,
    render_layers: u32,
    overlay: crate::overlay::Overlay3d,
    /// A digest of the children's geometry and poses. Recomputing a boolean
    /// is not cheap, so it happens when this changes and not every tick.
    signature: u64,
}

/// The `boolean2d` component, the same in the plane, with the outlines it
/// settled on so a boolean nested in it hands over its holes too.
pub(crate) struct Boolean2d {
    pub(crate) op: Op2d,
    options: BooleanOptions,
    look: Look,
    overlay: crate::overlay::Overlay2d,
    shapes: Shapes2d,
    signature: u64,
}

/// The keys both booleans draw their result with; `mapped` says how the
/// texture lands on it.
fn look_schema(color: &str, mapped: &str) -> String {
    let texture = balaur_core::texture_asset::TEXTURE_ASSET_TYPE;
    let material = crate::material::MATERIAL_ASSET_TYPE;
    format!(
        r#"color = {{ type = "color", default = {color}, description = "Tint of the result, as channel floats or #rrggbb / #rrggbbaa" }}
texture = {{ type = "asset", asset = "{texture}", default = "", description = "Image file, project-relative, or a `texture` asset the result is drawn with, {mapped}; empty draws the colour alone" }}
material = {{ type = "asset", asset = "{material}", default = "", description = "The material the result draws with; empty takes an inherited `material` component, else the built-in one" }}"#
    )
}

fn look_from_params(params: &toml::Value) -> Look {
    Look {
        color: crate::color_from_key(params, k::COLOR, [1.0; 4]),
        texture: prop_str(params, k::TEXTURE).to_string(),
        material: prop_str(params, k::MATERIAL).to_string(),
    }
}

fn look_to_map(look: &Look, map: &mut toml::map::Map<String, toml::Value>) {
    map.insert(k::COLOR.into(), crate::color_to_toml(look.color));
    map.insert(k::TEXTURE.into(), toml::Value::String(look.texture.clone()));
    map.insert(
        k::MATERIAL.into(),
        toml::Value::String(look.material.clone()),
    );
}

fn boolean3d_schema() -> String {
    format!(
        r#"operation = {{ type = "enum", default = "{union}", options = [{ops}], description = "How the children are combined, in the order they are declared" }}
{look}
cast_shadow = {{ type = "bool", default = true, description = "Whether the result casts a shadow from the lights that cast" }}
light_layers = {{ type = "int", default = -1, description = "Light-layer bitmask; a `light3d` lights the result when their masks share a bit. -1 is every layer" }}
render_layers = {{ type = "int", default = -1, description = "Layer bitmask; a `camera3d` draws the result when their `render_layers` share a bit. -1 is every layer" }}
{overlay}"#,
        overlay =
            crate::overlay::toml_lines(&crate::overlay::schema_3d(crate::overlay::Drawn::Builtin)),
        union = csg::words::UNION,
        ops = options(csg::words::OPS),
        look = look_schema(
            "[0.8, 0.8, 0.8, 1.0]",
            "over the UVs the children carry through the cut"
        ),
    )
}

fn boolean2d_schema() -> String {
    use geometry2d::words as g;
    format!(
        r#"operation = {{ type = "enum", default = "{union}", options = [{ops}], description = "How the children are combined, in the order they are declared" }}
fill_rule = {{ type = "enum", default = "{even_odd}", options = [{rules}], description = "Which parts of crossing or nested outlines count as inside: an odd number of them, any winding, or only counter-clockwise or clockwise winding" }}
min_area = {{ type = "float", default = 0.0, min = 0.0, description = "A result outline or hole enclosing less than this area, in square units of the node's own space, is dropped" }}
keep_collinear = {{ type = "bool", default = false, description = "Keep a point that sits on the straight line between its neighbours, in the children and in the result" }}
clean_result = {{ type = "bool", default = true, description = "Clear the result of the near-duplicate points rounding leaves" }}
{look}
{overlay}"#,
        overlay =
            crate::overlay::toml_lines(&crate::overlay::schema_2d(crate::overlay::Drawn::Pipeline)),
        union = g::UNION,
        ops = options(g::OPS),
        even_odd = g::EVEN_ODD,
        rules = options(g::FILL_RULES),
        look = look_schema(
            "[1.0, 1.0, 1.0, 1.0]",
            "centred on the node at 100 texture pixels per unit, as a `polygon`'s default UVs are"
        ),
    )
}

/// A layer mask as a scene writes it, -1 for every layer.
fn mask(params: &toml::Value, key: &str) -> u32 {
    params
        .get(key)
        .and_then(as_f64)
        .map_or(u32::MAX, |v| v as i64 as u32)
}

fn mask_value(mask: u32) -> toml::Value {
    toml::Value::Integer(i64::from(mask.cast_signed()))
}

fn boolean3d_from_params(params: &toml::Value) -> Result<Boolean3d> {
    let word = prop_str(params, k::OPERATION);
    Ok(Boolean3d {
        op: Op::from_word(word).ok_or_else(|| anyhow!("unknown boolean3d operation '{word}'"))?,
        look: look_from_params(params),
        shadows: prop_bool(params, k::CAST_SHADOW),
        layers: mask(params, k::LIGHT_LAYERS),
        render_layers: mask(params, k::RENDER_LAYERS),
        overlay: crate::overlay::overlay_3d(params),
        signature: 0,
    })
}

/// What the result's renderable is drawn with, onto one that already exists.
fn dress_3d(eng: &Engine, entity: Entity, boolean: &Boolean3d) -> Result<()> {
    {
        let world = eng.world_mut();
        let Ok(mut r) = world.get::<&mut Renderable3d>(entity) else {
            return Ok(());
        };
        r.color = boolean.look.color;
        r.shadows = boolean.shadows;
        r.layers = boolean.layers;
        r.render_layers = boolean.render_layers;
        r.overlay = boolean.overlay;
        if r.texture != boolean.look.texture {
            r.texture.clone_from(&boolean.look.texture);
            r.version += 1;
        }
    }
    crate::material::set_material_3d(eng, entity, &boolean.look.material)
}

/// The `boolean3d` component: writes [`Boolean3d`], and a `Renderable3d` whose
/// shape is built rather than authored.
pub(crate) fn register_boolean3d_component(reg: &mut Registry<'_>) {
    let def = ComponentDef {
        events: &[],
        warnings: None,
        doc: "Draws the node as its children combined by `operation`: `union`, `difference` or `intersection`. The children stay in the tree, hidden and editable; `color`, `texture` and `material` dress the result.",
        schema: ComponentDef::parse_schema("boolean3d", &boolean3d_schema()),
        tags: &[crate::vocabulary::words::PERSPECTIVE, "render"],
        expects: &[],
        apply: Box::new(|eng, entity, params| {
            let mut next = boolean3d_from_params(params)?;
            dress_3d(eng, entity, &next)?;
            let mut world = eng.world_mut();
            if let Ok(mut existing) = world.get::<&mut Boolean3d>(entity) {
                // A changed operation is a changed answer, whatever the operands.
                if existing.op == next.op {
                    next.signature = existing.signature;
                }
                *existing = next;
                return Ok(());
            }
            world
                .insert_one(entity, next)
                .map_err(|_| anyhow!("node is dead"))
        }),
        remove: Box::new(|eng, entity| {
            let mut world = eng.world_mut();
            let _ = world.remove_one::<Boolean3d>(entity);
            let _ = world.remove_one::<Renderable3d>(entity);
            Ok(())
        }),
        get: Box::new(|eng, entity| {
            let world = eng.world();
            let boolean = world.get::<&Boolean3d>(entity).ok()?;
            let mut map = toml::map::Map::new();
            map.insert(
                k::OPERATION.into(),
                toml::Value::String(boolean.op.word().into()),
            );
            look_to_map(&boolean.look, &mut map);
            map.insert(k::CAST_SHADOW.into(), toml::Value::Boolean(boolean.shadows));
            map.insert(k::LIGHT_LAYERS.into(), mask_value(boolean.layers));
            map.insert(k::RENDER_LAYERS.into(), mask_value(boolean.render_layers));
            crate::overlay::overlay_3d_to_map(&boolean.overlay, &mut map);
            Some(toml::Value::Table(map))
        }),
    };
    reg.register_component("boolean3d", def);
}

fn boolean2d_from_params(params: &toml::Value) -> Result<Boolean2d> {
    let word = prop_str(params, k::OPERATION);
    let rule = prop_str(params, k::FILL_RULE);
    Ok(Boolean2d {
        op: Op2d::from_word(word).ok_or_else(|| anyhow!("unknown boolean2d operation '{word}'"))?,
        options: BooleanOptions {
            fill_rule: FillRule::from_word(rule)
                .ok_or_else(|| anyhow!("unknown boolean2d fill_rule '{rule}'"))?,
            min_area: prop_f32(params, k::MIN_AREA).max(0.0),
            keep_collinear: prop_bool(params, k::KEEP_COLLINEAR),
            clean_result: prop_bool(params, k::CLEAN_RESULT),
        },
        look: look_from_params(params),
        overlay: crate::overlay::overlay_2d(params)?,
        shapes: Vec::new(),
        signature: 0,
    })
}

/// The `boolean2d` component: the same over filled outlines.
pub(crate) fn register_boolean2d_component(reg: &mut Registry<'_>) {
    let def = ComponentDef {
        events: &[],
        warnings: None,
        doc: "Draws the node as its 2D children combined by `operation`, holes and all. The children stay in the tree, hidden and editable; `fill_rule` and the cleanup keys tune the combination, and `color`, `texture` and `material` dress the result.",
        schema: ComponentDef::parse_schema("boolean2d", &boolean2d_schema()),
        tags: &[crate::vocabulary::words::ORTHOGRAPHIC, "render"],
        expects: &[],
        apply: Box::new(|eng, entity, params| {
            let mut next = boolean2d_from_params(params)?;
            let mut world = eng.world_mut();
            if let Ok(mut existing) = world.get::<&mut Boolean2d>(entity) {
                // The texture sizes the UVs, so only a new tint or material
                // leaves the result standing.
                let same = existing.op == next.op
                    && existing.options == next.options
                    && existing.look.texture == next.look.texture;
                if same {
                    next.signature = existing.signature;
                    next.shapes = std::mem::take(&mut existing.shapes);
                }
                *existing = next;
            } else {
                world
                    .insert_one(entity, next)
                    .map_err(|_| anyhow!("node is dead"))?;
            }
            drop(world);
            dress_2d(eng, entity)
        }),
        remove: Box::new(|eng, entity| {
            let mut world = eng.world_mut();
            let _ = world.remove_one::<Boolean2d>(entity);
            let _ = world.remove_one::<Renderable2d>(entity);
            Ok(())
        }),
        get: Box::new(|eng, entity| {
            let world = eng.world();
            let boolean = world.get::<&Boolean2d>(entity).ok()?;
            let mut map = toml::map::Map::new();
            let text = |w: &str| toml::Value::String(w.into());
            map.insert(k::OPERATION.into(), text(boolean.op.word()));
            map.insert(k::FILL_RULE.into(), text(boolean.options.fill_rule.word()));
            map.insert(
                k::MIN_AREA.into(),
                toml::Value::Float(f64::from(boolean.options.min_area)),
            );
            map.insert(
                k::KEEP_COLLINEAR.into(),
                toml::Value::Boolean(boolean.options.keep_collinear),
            );
            map.insert(
                k::CLEAN_RESULT.into(),
                toml::Value::Boolean(boolean.options.clean_result),
            );
            look_to_map(&boolean.look, &mut map);
            crate::overlay::overlay_2d_to_map(&boolean.overlay, &mut map);
            Some(toml::Value::Table(map))
        }),
    };
    reg.register_component("boolean2d", def);
}

/// The 2D result's tint and material, onto its renderable if it has one yet.
fn dress_2d(eng: &Engine, entity: Entity) -> Result<()> {
    let look = {
        let world = eng.world();
        let Ok(boolean) = world.get::<&Boolean2d>(entity) else {
            return Ok(());
        };
        (boolean.look.clone(), boolean.overlay)
    };
    let (look, overlay) = look;
    {
        let world = eng.world_mut();
        let Ok(mut r) = world.get::<&mut Renderable2d>(entity) else {
            return Ok(());
        };
        r.color = look.color;
        r.overlay = overlay;
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

/// A child's outlines in the boolean node's space, for the 2D case: one ring
/// for a shape, or a nested boolean's result, holes and all.
fn operand_shapes(eng: &Engine, parent: &GlobalTransform, entity: Entity) -> Option<Shapes2d> {
    let world = eng.world();
    let shapes: Shapes2d = if let Ok(nested) = world.get::<&Boolean2d>(entity) {
        nested.shapes.clone()
    } else {
        let r = world.get::<&Renderable2d>(entity).ok()?;
        let ring = match r.shape {
            Shape2d::Flat(flat) => flat.outline(),
            Shape2d::Sprite { hx, hy } => balaur_core::primitive::Flat::rect(hx, hy).outline(),
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
pub(crate) fn install_boolean_api(m: &mut dyn Bindings<Engine>) {
    m.describe(&[(
        "built_mesh",
        &["boolean3d"],
        "(node: node) -> table",
        "The triangles the node's boolean settled on, as `#{ positions, indices }` ready to be written out as a `mesh` asset; nil when the node draws no built geometry.",
    )]);
    m.function("built_mesh", |eng: &Engine, node: NodeId| {
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
        let world = eng.world();
        if flat {
            world
                .query::<(Entity, &Boolean2d)>()
                .iter()
                .map(|(e, _)| e)
                .collect()
        } else {
            world
                .query::<(Entity, &Boolean3d)>()
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
                world.get::<&Boolean2d>(entity).ok().map(|b| b.signature)
            } else {
                world.get::<&Boolean3d>(entity).ok().map(|b| b.signature)
            };
            (seen != Some(signature)).then_some((entity, operands, signature))
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
    for (entity, operands, signature) in stale(eng, false) {
        let Some(op) = eng.world().get::<&Boolean3d>(entity).ok().map(|b| b.op) else {
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
        let mut world = eng.world_mut();
        let Ok(mut boolean) = world.get::<&mut Boolean3d>(entity) else {
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
            shadows: boolean.shadows,
            layers: boolean.layers,
            render_layers: boolean.render_layers,
            overlay: boolean.overlay,
            version: 0,
        };
        drop(boolean);
        let built = Some(std::sync::Arc::new(result));
        if let Ok(mut renderable) = world.get::<&mut Renderable3d>(entity) {
            renderable.shape = Shape3d::Built;
            renderable.built = built;
            renderable.bounds = bounds;
            renderable.version += 1;
        } else {
            let _ = world.insert_one(entity, Renderable3d { built, ..fresh });
        }
        drop(world);
        hide(eng, &operands);
    }
}

fn resolve_2d(eng: &Engine) {
    for (entity, operands, signature) in stale(eng, true) {
        let Some((op, options, texture)) = eng
            .world()
            .get::<&Boolean2d>(entity)
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
            let world = eng.world_mut();
            if let Ok(mut boolean) = world.get::<&mut Boolean2d>(entity) {
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
        let (points, triangles) = balaur_core::triangulate::triangulate_shape(&contours);
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
    let world = eng.world_mut();
    for entity in operands {
        if let Ok(mut appearance) = world.get::<&mut Appearance>(*entity) {
            appearance.visible = false;
        }
    }
}
