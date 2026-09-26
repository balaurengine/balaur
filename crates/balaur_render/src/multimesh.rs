//! Godot's `MultiMesh`: one mesh drawn once per instance, in one call.
//!
//! The `multimesh` asset is the resource: the mesh and a list of instances,
//! each a transform, a colour and four floats of custom data. `multimesh3d`
//! and `multimesh2d` are `MultiMeshInstance3D` and `MultiMeshInstance2D`: a
//! component naming an asset in `source`, as `mesh` names one.
//!
//! An asset is a cached definition every holder reads the same, where a Godot
//! resource is a live object. So the component copies the asset's instances
//! onto its node when it attaches, and what a script changes is that node's.
//! The draw goes through [`crate::instancing`].

use std::sync::Arc;

use anyhow::{Result, anyhow, bail};
use balaur_core::Engine;
use balaur_core::components::{ComponentDef, as_f64, rgba};
use balaur_core::entity_of;
use balaur_core::hecs::{Entity, World};
use balaur_core::scene::GlobalTransform;
use balaur_plugin::Registry;
use balaur_script::{Bindings, BindingsExt as _, NodeId, Value};
use glamx::{Affine2, Affine3A, EulerRot, Mat4, Quat, Vec3};

use crate::vocabulary::{keys as k, words};
use crate::{Renderable2d, Renderable3d};

/// The asset type's name, and what a definition's `type` says.
pub const MULTIMESH_ASSET_TYPE: &str = "multimesh";

/// The 3D component: `MultiMeshInstance3D`.
pub const MULTIMESH_3D: &str = "multimesh3d";

/// The 2D component: `MultiMeshInstance2D`.
pub const MULTIMESH_2D: &str = "multimesh2d";

/// The most instances one node holds. A count a script works out wrong should
/// cost a frame rather than the process.
pub const MAX_INSTANCES: usize = 1 << 20;

/// One instance: where it sits in its node's space, the colour it draws in,
/// and four floats a material's shader reads.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Instance {
    pub position: Vec3,
    pub rotation: Quat,
    pub scale: Vec3,
    pub color: [f32; 4],
    pub custom: [f32; 4],
}

impl Default for Instance {
    fn default() -> Self {
        Self {
            position: Vec3::ZERO,
            rotation: Quat::IDENTITY,
            scale: Vec3::ONE,
            color: [1.0; 4],
            custom: [0.0; 4],
        }
    }
}

impl Instance {
    /// The instance's matrix in its node's space.
    #[must_use]
    pub fn local(&self) -> Mat4 {
        Mat4::from_scale_rotation_translation(self.scale, self.rotation, self.position)
    }

    /// Euler angles in the order a scene file writes them, as the `transform`
    /// component does.
    #[must_use]
    pub fn rotation_euler(&self) -> [f32; 3] {
        let (yaw, pitch, roll) = self.rotation.to_euler(EulerRot::ZYX);
        [roll, pitch, yaw]
    }

    /// An instance as the asset's `instances` list spells it.
    ///
    /// # Errors
    /// A key an instance does not take, or a value of the wrong shape, named
    /// with the index it was found at.
    pub fn from_table(index: usize, table: &toml::Value) -> Result<Self> {
        let Some(fields) = table.as_table() else {
            bail!("instance {index} is a {}, not a table", table.type_str());
        };
        let mut out = Self::default();
        for (key, value) in fields {
            match key.as_str() {
                k::POSITION => out.position = Vec3::from_array(floats(value, [0.0; 3])),
                k::ROTATION_EULER => out.rotation = rotation_of(floats(value, [0.0; 3])),
                k::SCALE => out.scale = Vec3::from_array(floats(value, [1.0; 3])),
                k::COLOR => {
                    out.color = rgba(value)
                        .ok_or_else(|| anyhow!("instance {index}'s color is not a colour"))?;
                }
                k::CUSTOM => out.custom = floats(value, [0.0; 4]),
                other => bail!(
                    "instance {index} has '{other}'; an instance takes {}, {}, {}, {} and {}",
                    k::POSITION,
                    k::ROTATION_EULER,
                    k::SCALE,
                    k::COLOR,
                    k::CUSTOM
                ),
            }
        }
        Ok(out)
    }

    /// The instance as the asset's list spells it, every key written.
    #[must_use]
    pub fn to_table(&self) -> toml::Value {
        let floats = |values: &[f32]| {
            toml::Value::Array(
                values
                    .iter()
                    .map(|v| toml::Value::Float(f64::from(*v)))
                    .collect(),
            )
        };
        let mut map = toml::map::Map::new();
        map.insert(k::POSITION.into(), floats(&self.position.to_array()));
        map.insert(k::ROTATION_EULER.into(), floats(&self.rotation_euler()));
        map.insert(k::SCALE.into(), floats(&self.scale.to_array()));
        map.insert(k::COLOR.into(), floats(&self.color));
        map.insert(k::CUSTOM.into(), floats(&self.custom));
        toml::Value::Table(map)
    }
}

pub(crate) fn rotation_of(euler: [f32; 3]) -> Quat {
    Quat::from_euler(EulerRot::ZYX, euler[2], euler[1], euler[0])
}

/// Up to `N` numbers from an array, each missing one the fallback's. A 2D
/// instance writes `[x, y]` and gets the third from the fallback.
fn floats<const N: usize>(value: &toml::Value, fallback: [f32; N]) -> [f32; N] {
    let mut out = fallback;
    if let Some(row) = value.as_array() {
        for (slot, item) in out.iter_mut().zip(row) {
            if let Some(number) = as_f64(item) {
                *slot = number as f32;
            }
        }
    }
    out
}

/// How many floats one instance takes in Godot's `MultiMesh.buffer`: a 2D
/// transform is 8 and a 3D one 12, then 4 of colour and 4 of custom data
/// where the multimesh carries them.
#[must_use]
pub const fn buffer_stride(flat: bool, colors: bool, custom: bool) -> usize {
    (if flat { 8 } else { 12 }) + if colors { 4 } else { 0 } + if custom { 4 } else { 0 }
}

/// Instances read from Godot's `MultiMesh.buffer` layout.
///
/// A transform is three rows of four in 3D, the origin last in each row, and
/// two in 2D with a zero where z would be. A shear the rows hold is dropped.
///
/// # Errors
/// A length that is not a whole number of instances, named with the stride.
pub fn instances_from_buffer(
    floats: &[f32],
    flat: bool,
    colors: bool,
    custom: bool,
) -> Result<Vec<Instance>> {
    let stride = buffer_stride(flat, colors, custom);
    if !floats.len().is_multiple_of(stride) {
        bail!(
            "a buffer of {} floats is not whole instances of {stride}",
            floats.len()
        );
    }
    if floats.len() / stride > MAX_INSTANCES {
        bail!("a multimesh holds at most {MAX_INSTANCES} instances");
    }
    Ok(floats
        .chunks_exact(stride)
        .map(|row| {
            let mut instance = Instance::default();
            let rest = if flat {
                let matrix = Mat4::from_cols(
                    glamx::Vec4::new(row[0], row[4], 0.0, 0.0),
                    glamx::Vec4::new(row[1], row[5], 0.0, 0.0),
                    glamx::Vec4::Z,
                    glamx::Vec4::new(row[3], row[7], 0.0, 1.0),
                );
                let (scale, rotation, position) = matrix.to_scale_rotation_translation();
                instance.position = position;
                instance.rotation = rotation;
                instance.scale = scale;
                &row[8..]
            } else {
                let matrix = Mat4::from_cols(
                    glamx::Vec4::new(row[0], row[4], row[8], 0.0),
                    glamx::Vec4::new(row[1], row[5], row[9], 0.0),
                    glamx::Vec4::new(row[2], row[6], row[10], 0.0),
                    glamx::Vec4::new(row[3], row[7], row[11], 1.0),
                );
                let (scale, rotation, position) = matrix.to_scale_rotation_translation();
                instance.position = position;
                instance.rotation = rotation;
                instance.scale = scale;
                &row[12..]
            };
            let (fours, _) = rest.as_chunks::<4>();
            let mut rest = fours.iter();
            if colors && let Some(c) = rest.next() {
                instance.color = *c;
            }
            if custom && let Some(c) = rest.next() {
                instance.custom = *c;
            }
            instance
        })
        .collect())
}

/// Instances in Godot's `MultiMesh.buffer` layout, colour and custom data
/// always included.
#[must_use]
pub fn buffer_of(instances: &[Instance], flat: bool) -> Vec<f32> {
    let mut out = Vec::with_capacity(instances.len() * buffer_stride(flat, true, true));
    for instance in instances {
        let m = instance.local();
        let (x, y, z, o) = (m.x_axis, m.y_axis, m.z_axis, m.w_axis);
        if flat {
            out.extend_from_slice(&[x.x, y.x, 0.0, o.x, x.y, y.y, 0.0, o.y]);
        } else {
            out.extend_from_slice(&[x.x, y.x, z.x, o.x, x.y, y.y, z.y, o.y, x.z, y.z, z.z, o.z]);
        }
        out.extend_from_slice(&instance.color);
        out.extend_from_slice(&instance.custom);
    }
    out
}

/// The mesh an asset draws: a reference, or a definition written inline.
#[derive(Clone, Debug, PartialEq)]
pub enum MeshSource {
    Reference(String),
    Inline(toml::Value),
}

/// The parsed `multimesh` asset.
#[derive(Clone, Debug, PartialEq)]
pub struct MultiMeshAsset {
    pub mesh: MeshSource,
    pub instances: Vec<Instance>,
    /// How many instances draw; negative draws every one.
    pub visible_instance_count: i64,
}

impl MultiMeshAsset {
    /// # Errors
    /// No `mesh`, an unknown key, or an instance that will not read.
    pub fn parse(value: &toml::Value) -> Result<Self> {
        let Some(table) = value.as_table() else {
            bail!("a multimesh is a table");
        };
        for key in table.keys() {
            if !["type", k::MESH, k::INSTANCES, k::VISIBLE_INSTANCE_COUNT].contains(&key.as_str()) {
                bail!(
                    "a multimesh has no '{key}'; it takes {}, {} and {}",
                    k::MESH,
                    k::INSTANCES,
                    k::VISIBLE_INSTANCE_COUNT
                );
            }
        }
        let mesh = match table.get(k::MESH) {
            Some(toml::Value::String(reference)) if !reference.trim().is_empty() => {
                MeshSource::Reference(reference.clone())
            }
            Some(inline @ toml::Value::Table(_)) => MeshSource::Inline(inline.clone()),
            _ => bail!("a multimesh names the mesh it draws in '{}'", k::MESH),
        };
        let rows = match table.get(k::INSTANCES) {
            None => &Vec::new(),
            Some(toml::Value::Array(rows)) => rows,
            Some(other) => bail!("'{}' is a {}, not a list", k::INSTANCES, other.type_str()),
        };
        if rows.len() > MAX_INSTANCES {
            bail!("a multimesh holds at most {MAX_INSTANCES} instances");
        }
        let instances = rows
            .iter()
            .enumerate()
            .map(|(index, row)| Instance::from_table(index, row))
            .collect::<Result<Vec<_>>>()?;
        let visible_instance_count = table
            .get(k::VISIBLE_INSTANCE_COUNT)
            .and_then(toml::Value::as_integer)
            .unwrap_or(-1);
        Ok(Self {
            mesh,
            instances,
            visible_instance_count,
        })
    }
}

const MULTIMESH_ASSET_DOC: &str = r##"Godot's `MultiMesh`: a mesh and the instances it is drawn at, for `multimesh3d.source` and `multimesh2d.source`. Each instance is a transform in the node's space, a `color` and four floats of `custom` data a material's shader reads. `visible_instance_count` draws the first so many; -1 draws them all.

```toml
type = "multimesh"
mesh = "models/post.toml"          # a mesh asset, or an inline { type = "mesh", ... }
visible_instance_count = -1
instances = [                      # a 2D instance writes [x, y] and turns about z
  { position = [0.0, 0.0, 0.0] },
  { position = [0.9, 0.0, 0.0], rotation_euler = [0.0, 0.5, 0.0], scale = [1.0, 2.0, 1.0] },
  { position = [1.8, 0.0, 0.0], color = "#ff8080", custom = [1.0, 0.0, 0.0, 0.0] },
]
```"##;

/// A node's multimesh: its own copy of an asset's instances.
#[derive(Clone, Debug, PartialEq)]
pub struct MultiMesh {
    /// The asset reference the component was given.
    pub source: String,
    /// Drawn by `multimesh2d` rather than `multimesh3d`.
    pub flat: bool,
    pub instances: Vec<Instance>,
    /// How many instances draw; negative draws every one.
    pub visible_instance_count: i64,
    /// The asset's definition when it was copied, so a reload that changed
    /// it copies again and one that changed something else does not.
    copied_from: u64,
}

impl MultiMesh {
    /// The instances that draw: the first `visible_instance_count`, or all.
    #[must_use]
    pub fn drawn(&self) -> &[Instance] {
        match usize::try_from(self.visible_instance_count) {
            Ok(count) => &self.instances[..count.min(self.instances.len())],
            Err(_) => &self.instances,
        }
    }

    /// Where each drawn instance sits in the world, with its colour and
    /// custom data: what a backend turns into instance data.
    #[must_use]
    pub fn placed(&self, global: &GlobalTransform) -> Vec<Placed> {
        let here = crate::instancing::model_of(global);
        self.drawn()
            .iter()
            .map(|instance| Placed {
                at: here * instance.local(),
                color: instance.color,
                custom: instance.custom,
            })
            .collect()
    }
}

/// One drawn instance, in the world.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placed {
    pub at: Mat4,
    pub color: [f32; 4],
    pub custom: [f32; 4],
}

/// Whether a node's multimesh has nothing to draw, so the node draws nothing.
#[cfg(feature = "window")]
pub(crate) fn draws_nothing(world: &World, entity: Entity) -> bool {
    world
        .get::<&MultiMesh>(entity)
        .is_ok_and(|multimesh| multimesh.drawn().is_empty())
}

/// A node's instances' custom data, in draw order: what a backend hands a
/// shader material as the object's user data.
#[cfg(feature = "window")]
pub(crate) struct InstanceCustom(pub(crate) Vec<[f32; 4]>);

/// The per-instance buffer of custom data a material binds, grown as the
/// instance count does.
#[cfg(feature = "window")]
#[derive(Default)]
pub(crate) struct CustomBuffer {
    buffer: Option<kiss3d::wgpu::Buffer>,
    capacity: usize,
}

#[cfg(feature = "window")]
impl CustomBuffer {
    /// The buffer holding `copies` instances' custom data from `user_data`,
    /// zero for an object that carries none.
    pub(crate) fn fill(
        &mut self,
        user_data: &dyn std::any::Any,
        copies: usize,
    ) -> &kiss3d::wgpu::Buffer {
        use kiss3d::context::Context;
        use kiss3d::wgpu;
        let copies = copies.max(1);
        let ctxt = Context::get();
        if self.buffer.is_none() || self.capacity < copies {
            self.capacity = copies.next_power_of_two();
            self.buffer = Some(ctxt.create_buffer(&wgpu::BufferDescriptor {
                label: Some("instance_custom"),
                size: (self.capacity * std::mem::size_of::<[f32; 4]>()) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
        }
        let mut floats = vec![[0.0f32; 4]; copies];
        if let Some(InstanceCustom(custom)) = user_data.downcast_ref::<InstanceCustom>() {
            for (slot, value) in floats.iter_mut().zip(custom) {
                *slot = *value;
            }
        }
        let buffer = self
            .buffer
            .as_ref()
            .expect("the buffer is made above whenever it is missing or too small");
        ctxt.write_buffer(buffer, 0, bytemuck::cast_slice(&floats));
        buffer
    }
}

/// Whether the node carries a multimesh, for the components whose renderable
/// it borrows: a `mesh` or a `polygon` read back on it would be a second copy.
pub(crate) fn holds(world: &World, entity: Entity) -> bool {
    world.get::<&MultiMesh>(entity).is_ok()
}

/// A digest of an asset's definition, to tell one reload from another.
fn definition_digest(eng: &Engine, source: &str) -> u64 {
    let text = balaur_core::assets::definition(eng, source)
        .ok()
        .and_then(|definition| toml::to_string(&definition).ok())
        .unwrap_or_default();
    text.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// The asset behind `source`, warned about rather than refused: one bad
/// reference must not take the scene down.
fn asset_of(eng: &Engine, source: &str) -> Option<std::rc::Rc<MultiMeshAsset>> {
    if source.trim().is_empty() {
        return None;
    }
    match balaur_core::assets::load_typed::<MultiMeshAsset>(eng, source) {
        Ok(asset) => Some(asset),
        Err(why) => {
            tracing::warn!("multimesh '{source}': {why:#}");
            None
        }
    }
}

/// The reference a renderable names for the asset's mesh; an inline table is
/// recorded in the cache and named by its digest.
fn mesh_reference(eng: &Engine, asset: &MultiMeshAsset) -> Result<String> {
    match &asset.mesh {
        MeshSource::Reference(reference) => Ok(reference.clone()),
        MeshSource::Inline(table) => {
            let declared = table
                .get("type")
                .and_then(toml::Value::as_str)
                .unwrap_or(balaur_core::mesh::MESH_ASSET_TYPE);
            Ok(balaur_core::assets::define_inline(eng, declared, table.clone())?.to_string())
        }
    }
}

/// Give the node its copy of the asset's instances. A patch that keeps the
/// source keeps the node's instances, so a script's edits outlive it.
fn adopt(eng: &Engine, entity: Entity, source: &str, flat: bool) -> Option<String> {
    let asset = asset_of(eng, source);
    let mesh = asset.as_deref().and_then(|asset| {
        mesh_reference(eng, asset)
            .inspect_err(|why| tracing::warn!("multimesh '{source}': {why:#}"))
            .ok()
    });
    let copied_from = definition_digest(eng, source);
    let mut world = eng.world_mut();
    if let Ok(mut held) = world.get::<&mut MultiMesh>(entity)
        && held.source == source
        && held.copied_from == copied_from
    {
        held.flat = flat;
        return mesh;
    }
    let fresh = MultiMesh {
        source: source.to_string(),
        flat,
        instances: asset
            .as_deref()
            .map(|asset| asset.instances.clone())
            .unwrap_or_default(),
        visible_instance_count: asset
            .as_deref()
            .map_or(-1, |asset| asset.visible_instance_count),
        copied_from,
    };
    if let Ok(mut held) = world.get::<&mut MultiMesh>(entity) {
        *held = fresh;
    } else {
        let _ = world.insert_one(entity, fresh);
    }
    mesh
}

fn text(params: &toml::Value, key: &str) -> String {
    params
        .get(key)
        .and_then(toml::Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn apply_3d(eng: &Engine, entity: Entity, params: &toml::Value) -> Result<()> {
    let source = text(params, k::SOURCE);
    let mesh = adopt(eng, entity, &source, false).unwrap_or_default();
    crate::set_mesh(eng, entity, mesh, String::new(), text(params, k::TEXTURE))?;
    // An instance's colour is the colour it draws in, so the node adds none.
    crate::set_color(eng, entity, [1.0; 4])?;
    crate::lighting_from_params(eng, entity, params);
    crate::material::set_material_3d(eng, entity, &text(params, k::MATERIAL))
}

fn apply_2d(eng: &Engine, entity: Entity, params: &toml::Value) -> Result<()> {
    let source = text(params, k::SOURCE);
    let mesh = adopt(eng, entity, &source, true).unwrap_or_default();
    let polygon = crate::polygon::resolve(
        eng,
        mesh,
        text(params, k::TEXTURE),
        String::new(),
        crate::DEFAULT_PIXELS_PER_UNIT,
    )?;
    crate::set_polygon(eng, entity, Arc::new(polygon))?;
    crate::set_color(eng, entity, crate::color_from_params(params))
}

fn get_3d(eng: &Engine, entity: Entity) -> Option<toml::Value> {
    let world = eng.world();
    let multimesh = world.get::<&MultiMesh>(entity).ok()?;
    if multimesh.flat {
        return None;
    }
    let renderable = world.get::<&Renderable3d>(entity).ok()?;
    let mut map = toml::map::Map::new();
    map.insert(
        k::SOURCE.into(),
        toml::Value::String(multimesh.source.clone()),
    );
    map.insert(
        k::TEXTURE.into(),
        toml::Value::String(renderable.texture.clone()),
    );
    map.insert(
        k::MATERIAL.into(),
        toml::Value::String(renderable.material.clone()),
    );
    map.insert(
        k::CAST_SHADOW.into(),
        toml::Value::Boolean(renderable.shadows),
    );
    map.insert(
        k::LIGHT_LAYERS.into(),
        toml::Value::Integer(i64::from(renderable.layers.cast_signed())),
    );
    Some(toml::Value::Table(map))
}

fn get_2d(eng: &Engine, entity: Entity) -> Option<toml::Value> {
    let world = eng.world();
    let multimesh = world.get::<&MultiMesh>(entity).ok()?;
    if !multimesh.flat {
        return None;
    }
    let renderable = world.get::<&Renderable2d>(entity).ok()?;
    let texture = renderable
        .polygon
        .as_ref()
        .map(|polygon| polygon.texture.clone())
        .unwrap_or_default();
    let mut map = toml::map::Map::new();
    map.insert(
        k::SOURCE.into(),
        toml::Value::String(multimesh.source.clone()),
    );
    map.insert(k::TEXTURE.into(), toml::Value::String(texture));
    map.insert(k::COLOR.into(), crate::color_to_toml(renderable.color));
    Some(toml::Value::Table(map))
}

fn source_line() -> String {
    format!(
        r#"{{ type = "asset", asset = "{MULTIMESH_ASSET_TYPE}", default = "", description = "The multimesh asset: the mesh and the instances it is drawn at" }}"#
    )
}

fn texture_line() -> String {
    format!(
        r#"{{ type = "asset", asset = "{}", default = "", description = "Image file, project-relative, or a `texture` asset; empty draws the colour alone" }}"#,
        balaur_core::texture_asset::TEXTURE_ASSET_TYPE
    )
}

/// The asset type and both components.
pub(crate) fn register_multimesh(reg: &mut Registry<'_>) {
    reg.register_asset_type(
        MULTIMESH_ASSET_TYPE,
        "multimeshes",
        MULTIMESH_ASSET_DOC,
        |value| {
            Ok(std::rc::Rc::new(MultiMeshAsset::parse(value)?) as std::rc::Rc<dyn std::any::Any>)
        },
    );
    reg.add_system(balaur_core::Stage::SceneSync, refresh_multimeshes_system);
    let material_line = format!(
        r#"{{ type = "asset", asset = "{}", default = "", description = "The material every instance draws with; empty draws with the built-in one" }}"#,
        crate::material::MATERIAL_ASSET_TYPE
    );
    reg.register_component(
        MULTIMESH_3D,
        ComponentDef {
            events: &[],
            warnings: None,
            doc: "Godot's `MultiMeshInstance3D`: the `multimesh` asset in `source`, its mesh drawn once per instance in one call. Each instance's `color` is the colour it draws in. The node keeps its own copy of the instances, so a script's edits stay on it; children draw once.",
            schema: ComponentDef::parse_schema(
                MULTIMESH_3D,
                &ComponentDef::schema(&[
                    (k::SOURCE, &source_line()),
                    (k::TEXTURE, &texture_line()),
                    (k::MATERIAL, &material_line),
                    (k::CAST_SHADOW, r#"{ type = "bool", default = true, description = "Whether the instances cast a shadow from the lights that cast" }"#),
                    (k::LIGHT_LAYERS, r#"{ type = "int", default = -1, description = "Light-layer bitmask; a `light3d` lights this when their masks share a bit. -1 is every layer" }"#),
                ]),
            ),
            tags: &[words::PERSPECTIVE, "render"],
            expects: &[],
            apply: Box::new(apply_3d),
            remove: Box::new(|eng, entity| {
                let mut world = eng.world_mut();
                let _ = world.remove_one::<MultiMesh>(entity);
                let _ = world.remove_one::<Renderable3d>(entity);
                Ok(())
            }),
            get: Box::new(get_3d),
        },
    );
    reg.register_component(
        MULTIMESH_2D,
        ComponentDef {
            events: &[],
            warnings: None,
            doc: "Godot's `MultiMeshInstance2D`: the `multimesh` asset in `source`, its mesh drawn flat once per instance in one call, each instance tinted over `color`. The node keeps its own copy of the instances, so a script's edits stay on it; children draw once.",
            schema: ComponentDef::parse_schema(
                MULTIMESH_2D,
                &ComponentDef::schema(&[
                    (k::SOURCE, &source_line()),
                    (k::TEXTURE, &texture_line()),
                    (k::COLOR, r#"{ type = "color", default = [1.0, 1.0, 1.0, 1.0], description = "Tint under every instance's own colour, as channel floats or #rrggbb / #rrggbbaa" }"#),
                ]),
            ),
            tags: &[words::ORTHOGRAPHIC, "render"],
            expects: &[],
            apply: Box::new(apply_2d),
            remove: Box::new(|eng, entity| {
                let mut world = eng.world_mut();
                let _ = world.remove_one::<MultiMesh>(entity);
                let _ = world.remove_one::<Renderable2d>(entity);
                Ok(())
            }),
            get: Box::new(get_2d),
        },
    );
}

/// The node's multimesh, for a handle method to read or write.
fn with_multimesh<R>(
    eng: &Engine,
    node: NodeId,
    f: impl FnOnce(&mut MultiMesh) -> Result<R>,
) -> Result<R> {
    let entity = entity_of(node)?;
    let world = eng.world_mut();
    let mut multimesh = world
        .get::<&mut MultiMesh>(entity)
        .map_err(|_| anyhow!("the node carries no {MULTIMESH_3D} or {MULTIMESH_2D}"))?;
    f(&mut multimesh)
}

/// An index into the node's instances, or an error naming how many it holds.
fn slot(multimesh: &MultiMesh, index: i64) -> Result<usize> {
    usize::try_from(index)
        .ok()
        .filter(|i| *i < multimesh.instances.len())
        .ok_or_else(|| {
            anyhow!(
                "instance {index} is past the end: the multimesh holds {}",
                multimesh.instances.len()
            )
        })
}

/// A count a script asks for, held to what a node may carry.
fn count_of(count: i64) -> Result<usize> {
    usize::try_from(count)
        .ok()
        .filter(|n| *n <= MAX_INSTANCES)
        .ok_or_else(|| anyhow!("a multimesh holds 0 to {MAX_INSTANCES} instances, not {count}"))
}

/// Set an instance's transform from a `Transform3d`, or from a `Transform2d`
/// in the xy plane. A shear either can hold is dropped, as import drops it.
fn place(instance: &mut Instance, transform: &Value) -> Result<()> {
    match transform {
        Value::Transform3d(columns) => {
            let (scale, rotation, position) =
                Mat4::from(Affine3A::from_cols_array(columns)).to_scale_rotation_translation();
            instance.position = position;
            instance.rotation = rotation;
            instance.scale = scale;
        }
        Value::Transform2d(columns) => {
            let (scale, angle, position) =
                Affine2::from_cols_array(columns).to_scale_angle_translation();
            instance.position = position.extend(0.0);
            instance.rotation = Quat::from_rotation_z(angle);
            instance.scale = scale.extend(1.0);
        }
        other => bail!(
            "an instance's transform is a Transform3d or a Transform2d, not a {}",
            other.type_name()
        ),
    }
    Ok(())
}

/// An instance's transform as its node's dimension spells one.
fn transform_of(instance: &Instance, flat: bool) -> Value {
    if flat {
        let angle = instance.rotation_euler()[2];
        Value::Transform2d(
            Affine2::from_scale_angle_translation(
                instance.scale.truncate(),
                angle,
                instance.position.truncate(),
            )
            .to_cols_array(),
        )
    } else {
        Value::Transform3d(
            Affine3A::from_scale_rotation_translation(
                instance.scale,
                instance.rotation,
                instance.position,
            )
            .to_cols_array(),
        )
    }
}

/// Four floats from a colour or a list of numbers.
fn four(value: &Value, what: &str) -> Result<[f32; 4]> {
    match value {
        Value::Color(c) => Ok(*c),
        list @ Value::List(_) => crate::draw_2d::color_of(list),
        other => bail!(
            "{what} is a colour or a list of four numbers, not a {}",
            other.type_name()
        ),
    }
}

/// An instance as `instances()` hands it over: the asset's own keys, each a
/// plain list of numbers as the file spells it, so a list read here saves
/// straight into a `multimesh` file or a scene's `[[assets]]` block.
pub(crate) fn instance_value(instance: &Instance) -> Value {
    let numbers =
        |values: &[f32]| Value::List(values.iter().map(|v| Value::Num(f64::from(*v))).collect());
    Value::Map(vec![
        (k::POSITION.into(), numbers(&instance.position.to_array())),
        (
            k::ROTATION_EULER.into(),
            numbers(&instance.rotation_euler()),
        ),
        (k::SCALE.into(), numbers(&instance.scale.to_array())),
        (k::COLOR.into(), numbers(&instance.color)),
        (k::CUSTOM.into(), numbers(&instance.custom)),
    ])
}

const BOTH: &[&str] = &[MULTIMESH_3D, MULTIMESH_2D];

/// One instance at a time on `node.multimesh3d` and `node.multimesh2d`:
/// Godot's `MultiMesh` calls, readers without their `get_`.
pub(crate) fn install_multimesh_api(m: &mut dyn Bindings<Engine>) {
    m.describe(&[
        ("set_instance_transform", BOTH, "", "Place one instance with a `Transform3d`, or a `Transform2d` on a 2D node. A shear is dropped: an instance is a position, a rotation and a scale."),
        ("instance_transform", BOTH, "", "One instance's transform: a `Transform3d`, or a `Transform2d` on a 2D node."),
        ("set_instance_color", BOTH, "", "The colour one instance draws in."),
        ("instance_color", BOTH, "", "The colour one instance draws in."),
        ("set_instance_custom_data", BOTH, "", "Four floats a material's shader reads for one instance, as a colour or a list."),
        ("instance_custom_data", BOTH, "", "One instance's four floats of custom data, as a colour."),
    ]);
    m.function(
        "set_instance_transform",
        |eng: &Engine, (node, index, transform): (NodeId, i64, Value)| {
            with_multimesh(eng, node, |multimesh| {
                let at = slot(multimesh, index)?;
                place(&mut multimesh.instances[at], &transform)
            })
        },
    );
    m.function(
        "instance_transform",
        |eng: &Engine, (node, index): (NodeId, i64)| {
            with_multimesh(eng, node, |multimesh| {
                let at = slot(multimesh, index)?;
                Ok(transform_of(&multimesh.instances[at], multimesh.flat))
            })
        },
    );
    m.function(
        "set_instance_color",
        |eng: &Engine, (node, index, color): (NodeId, i64, Value)| {
            let color = four(&color, "an instance's colour")?;
            with_multimesh(eng, node, |multimesh| {
                let at = slot(multimesh, index)?;
                multimesh.instances[at].color = color;
                Ok(())
            })
        },
    );
    m.function(
        "instance_color",
        |eng: &Engine, (node, index): (NodeId, i64)| {
            with_multimesh(eng, node, |multimesh| {
                let at = slot(multimesh, index)?;
                Ok(Value::Color(multimesh.instances[at].color))
            })
        },
    );
    m.function(
        "set_instance_custom_data",
        |eng: &Engine, (node, index, data): (NodeId, i64, Value)| {
            let data = four(&data, "an instance's custom data")?;
            with_multimesh(eng, node, |multimesh| {
                let at = slot(multimesh, index)?;
                multimesh.instances[at].custom = data;
                Ok(())
            })
        },
    );
    m.function(
        "instance_custom_data",
        |eng: &Engine, (node, index): (NodeId, i64)| {
            with_multimesh(eng, node, |multimesh| {
                let at = slot(multimesh, index)?;
                Ok(Value::Color(multimesh.instances[at].custom))
            })
        },
    );
}

/// The counts and every instance at once, as a list or Godot's flat buffer.
pub(crate) fn install_multimesh_list_api(m: &mut dyn Bindings<Engine>) {
    m.describe(&[
        ("set_instance_count", BOTH, "", "How many instances the node holds. Those below the count keep where they were; new ones are plain, at the node."),
        ("instance_count", BOTH, "", "How many instances the node holds."),
        ("set_visible_instance_count", BOTH, "", "How many instances draw, from the first; -1 draws them all."),
        ("visible_instance_count", BOTH, "", "How many instances draw; -1 is all of them."),
        ("instances", BOTH, "", "Every instance as the `multimesh` asset spells it, so `assets.save` writes a scripted layout into a file."),
        ("set_instances", BOTH, "", "Replace every instance with a list in the `multimesh` asset's shape."),
        ("buffer", BOTH, "", "Every instance as one flat list of floats in Godot's `MultiMesh.buffer` layout: 12 of transform in 3D or 8 in 2D, then 4 of colour and 4 of custom data."),
        ("set_buffer", BOTH, "", "Replace every instance from one flat list in the layout `buffer` answers in, for a Godot port that sets `buffer`. Building the list in a script costs more than one `set_instance_transform` per instance."),
    ]);
    m.function("buffer", |eng: &Engine, node: NodeId| {
        with_multimesh(eng, node, |multimesh| {
            Ok(Value::List(
                buffer_of(&multimesh.instances, multimesh.flat)
                    .into_iter()
                    .map(|v| Value::Num(f64::from(v)))
                    .collect(),
            ))
        })
    });
    m.function(
        "set_buffer",
        |eng: &Engine, (node, list): (NodeId, Value)| {
            let Value::List(items) = list else {
                bail!("a buffer is a list of numbers, not a {}", list.type_name());
            };
            let floats = items
                .iter()
                .map(|item| match item {
                    Value::Num(n) => Ok(*n as f32),
                    Value::Int(n) => Ok(*n as f32),
                    other => Err(anyhow!(
                        "a buffer holds numbers, not a {}",
                        other.type_name()
                    )),
                })
                .collect::<Result<Vec<f32>>>()?;
            with_multimesh(eng, node, |multimesh| {
                multimesh.instances = instances_from_buffer(&floats, multimesh.flat, true, true)?;
                Ok(())
            })
        },
    );
    m.function(
        "set_instance_count",
        |eng: &Engine, (node, count): (NodeId, i64)| {
            let count = count_of(count)?;
            with_multimesh(eng, node, |multimesh| {
                multimesh.instances.resize(count, Instance::default());
                Ok(())
            })
        },
    );
    m.function("instance_count", |eng: &Engine, node: NodeId| {
        with_multimesh(eng, node, |multimesh| {
            Ok(i64::try_from(multimesh.instances.len()).unwrap_or(i64::MAX))
        })
    });
    m.function(
        "set_visible_instance_count",
        |eng: &Engine, (node, count): (NodeId, i64)| {
            with_multimesh(eng, node, |multimesh| {
                multimesh.visible_instance_count = count.max(-1);
                Ok(())
            })
        },
    );
    m.function("visible_instance_count", |eng: &Engine, node: NodeId| {
        with_multimesh(eng, node, |multimesh| Ok(multimesh.visible_instance_count))
    });
    m.function("instances", |eng: &Engine, node: NodeId| {
        with_multimesh(eng, node, |multimesh| {
            Ok(Value::List(
                multimesh.instances.iter().map(instance_value).collect(),
            ))
        })
    });
    m.function(
        "set_instances",
        |eng: &Engine, (node, list): (NodeId, Value)| {
            let Value::List(rows) = list else {
                bail!("instances are a list, not a {}", list.type_name());
            };
            count_of(i64::try_from(rows.len()).unwrap_or(i64::MAX))?;
            let instances = rows
                .iter()
                .enumerate()
                .map(|(index, row)| {
                    Instance::from_table(index, &balaur_core::node_api::to_toml(row)?)
                })
                .collect::<Result<Vec<_>>>()?;
            with_multimesh(eng, node, |multimesh| {
                multimesh.instances = instances;
                Ok(())
            })
        },
    );
}

/// The asset generation the last refresh saw.
struct Refreshed(u64);

/// After a reload, copy the instances again onto every node whose asset
/// changed: an edit to the file is what the node should show, and a
/// script's edits to it are what the reload replaces.
fn refresh_multimeshes_system(eng: &Engine, _dt: f32) {
    let generation = balaur_core::assets::generation(eng);
    let Some(seen) = eng.try_resource::<Refreshed>() else {
        eng.insert_resource(Refreshed(generation));
        return;
    };
    if seen.borrow().0 == generation {
        return;
    }
    seen.borrow_mut().0 = generation;
    let stale: Vec<(Entity, String, bool)> = {
        let world = eng.world();
        let mut stale = Vec::new();
        for (entity, multimesh) in &mut world.query::<(Entity, &MultiMesh)>() {
            if definition_digest(eng, &multimesh.source) != multimesh.copied_from {
                stale.push((entity, multimesh.source.clone(), multimesh.flat));
            }
        }
        stale
    };
    for (entity, source, flat) in stale {
        let name = if flat { MULTIMESH_2D } else { MULTIMESH_3D };
        if let Some(params) = balaur_core::components::get(eng, entity, name)
            && let Err(why) = balaur_core::components::patch(eng, entity, name, &params)
        {
            tracing::warn!("multimesh '{source}': {why:#}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asset(text: &str) -> Result<MultiMeshAsset> {
        MultiMeshAsset::parse(&toml::from_str(text).unwrap())
    }

    #[test]
    fn an_instance_takes_the_transform_components_keys_and_a_colour() {
        let parsed = asset(
            r##"mesh = "models/post.toml"
instances = [{ position = [1.0, 2.0], rotation_euler = [0.0, 0.0, 0.5], color = "#ff0000", custom = [1.0, 2.0] }]"##,
        )
        .unwrap();
        let instance = parsed.instances[0];
        assert_eq!(instance.position, Vec3::new(1.0, 2.0, 0.0));
        assert!((instance.rotation_euler()[2] - 0.5).abs() < 1e-6);
        assert_eq!(instance.scale, Vec3::ONE);
        assert!(
            instance
                .color
                .iter()
                .zip([1.0, 0.0, 0.0, 1.0])
                .all(|(a, b)| (a - b).abs() < 1e-6)
        );
        assert!(
            instance
                .custom
                .iter()
                .zip([1.0, 2.0, 0.0, 0.0])
                .all(|(a, b)| (a - b).abs() < 1e-6)
        );
        assert_eq!(parsed.visible_instance_count, -1);
    }

    #[test]
    fn a_misspelt_instance_key_is_refused_by_name() {
        let why = asset(
            r#"mesh = "m.toml"
instances = [{}, { rotation = [0.0, 1.0, 0.0] }]"#,
        )
        .unwrap_err()
        .to_string();
        assert!(
            why.contains("instance 1") && why.contains("'rotation'"),
            "{why}"
        );
    }

    #[test]
    fn a_multimesh_with_no_mesh_is_refused() {
        assert!(asset("instances = []").is_err());
    }

    #[test]
    fn a_buffer_round_trips_in_both_dimensions() {
        let instances = [
            Instance {
                position: Vec3::new(1.0, 2.0, 3.0),
                rotation: rotation_of([0.3, -0.2, 0.9]),
                scale: Vec3::new(2.0, 0.5, 1.5),
                color: [0.1, 0.2, 0.3, 0.4],
                custom: [5.0, 6.0, 7.0, 8.0],
            },
            Instance::default(),
        ];
        let back = instances_from_buffer(&buffer_of(&instances, false), false, true, true).unwrap();
        assert!((back[0].position - instances[0].position).length() < 1e-5);
        assert!(back[0].rotation.angle_between(instances[0].rotation) < 1e-4);
        assert!((back[0].scale - instances[0].scale).length() < 1e-4);
        assert!(
            back[0]
                .color
                .iter()
                .zip(instances[0].color)
                .all(|(a, b)| (a - b).abs() < 1e-6)
        );
        assert!(
            back[0]
                .custom
                .iter()
                .zip(instances[0].custom)
                .all(|(a, b)| (a - b).abs() < 1e-6)
        );

        let flat = [Instance {
            position: Vec3::new(4.0, -1.0, 0.0),
            rotation: Quat::from_rotation_z(0.7),
            scale: Vec3::new(2.0, 3.0, 1.0),
            ..Instance::default()
        }];
        let buffer = buffer_of(&flat, true);
        assert_eq!(buffer.len(), buffer_stride(true, true, true));
        let back = instances_from_buffer(&buffer, true, true, true).unwrap();
        assert!((back[0].position - flat[0].position).length() < 1e-5);
        assert!((back[0].rotation_euler()[2] - 0.7).abs() < 1e-5);
        assert!((back[0].scale - flat[0].scale).length() < 1e-4);
    }

    /// Godot writes a 3D transform as three rows, origin last; a translation
    /// alone is where the three origin floats sit.
    #[test]
    fn godots_rows_put_the_origin_fourth_in_each() {
        let row = [1.0, 0.0, 0.0, 7.0, 0.0, 1.0, 0.0, 8.0, 0.0, 0.0, 1.0, 9.0];
        let read = instances_from_buffer(&row, false, false, false).unwrap();
        assert_eq!(read[0].position, Vec3::new(7.0, 8.0, 9.0));
        assert!(instances_from_buffer(&row[..11], false, false, false).is_err());
    }

    #[test]
    fn an_instance_reads_back_as_it_was_written() {
        let instance = Instance {
            position: Vec3::new(1.0, -2.0, 3.0),
            rotation: rotation_of([0.2, -0.4, 0.6]),
            scale: Vec3::new(2.0, 1.0, 0.5),
            color: [0.5, 0.25, 1.0, 1.0],
            custom: [4.0, 3.0, 2.0, 1.0],
        };
        let back = Instance::from_table(0, &instance.to_table()).unwrap();
        assert!((back.position - instance.position).length() < 1e-6);
        assert!(back.rotation.angle_between(instance.rotation) < 1e-5);
        assert!(
            back.color
                .iter()
                .zip(instance.color)
                .all(|(a, b)| (a - b).abs() < 1e-6)
        );
        assert!(
            back.custom
                .iter()
                .zip(instance.custom)
                .all(|(a, b)| (a - b).abs() < 1e-6)
        );
    }
}
