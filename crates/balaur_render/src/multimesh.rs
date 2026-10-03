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
use balaur_core::hecs::{Entity, World};
use balaur_core::scene::GlobalTransform;
use balaur_plugin::Registry;
use glamx::{EulerRot, Mat3, Mat4, Quat, Vec3};

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
/// four floats a material's shader reads, and the overrides of its node's
/// wireframe, vertex dots and, in 2D, the image rectangle it draws.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Instance {
    pub position: Vec3,
    pub rotation: Quat,
    pub scale: Vec3,
    /// A 3x3 that replaces `rotation` and `scale`, for one that shears.
    pub basis: Option<Mat3>,
    pub color: [f32; 4],
    pub custom: [f32; 4],
    pub wireframe_color: Option<[f32; 4]>,
    pub wireframe_width: Option<f32>,
    pub dot_color: Option<[f32; 4]>,
    pub dot_size: Option<f32>,
    /// `[x, y, w, h]` of the texture in pixels; `None` draws all of it.
    pub region: Option<[f32; 4]>,
}

impl Default for Instance {
    fn default() -> Self {
        Self {
            position: Vec3::ZERO,
            rotation: Quat::IDENTITY,
            scale: Vec3::ONE,
            basis: None,
            color: [1.0; 4],
            custom: [0.0; 4],
            wireframe_color: None,
            wireframe_width: None,
            dot_color: None,
            dot_size: None,
            region: None,
        }
    }
}

/// The keys one instance takes, for the error a typo gets.
const INSTANCE_KEYS: &[&str] = &[
    k::POSITION,
    k::ROTATION_EULER,
    k::SCALE,
    k::BASIS,
    k::COLOR,
    k::CUSTOM,
    k::WIREFRAME_COLOR,
    k::WIREFRAME_WIDTH,
    k::DOT_COLOR,
    k::DOT_SIZE,
    k::REGION_ORIGIN,
    k::REGION_SIZE,
];

impl Instance {
    /// The instance's matrix in its node's space.
    #[must_use]
    pub fn local(&self) -> Mat4 {
        match self.basis {
            Some(basis) => Mat4::from_mat3_translation(basis, self.position),
            None => Mat4::from_scale_rotation_translation(self.scale, self.rotation, self.position),
        }
    }

    /// Take a whole matrix: a position, a rotation and a scale, or a basis
    /// where a shear leaves the matrix none of those.
    pub fn set_local(&mut self, matrix: Mat4) {
        let (scale, rotation, position) = matrix.to_scale_rotation_translation();
        let linear = Mat3::from_mat4(matrix);
        let rebuilt = Mat3::from_mat4(Mat4::from_scale_rotation_translation(
            scale,
            rotation,
            Vec3::ZERO,
        ));
        let sheared = (linear - rebuilt)
            .to_cols_array()
            .iter()
            .any(|d| d.abs() > 1e-4);
        self.position = position;
        self.rotation = rotation;
        self.scale = scale;
        self.basis = sheared.then_some(linear);
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
        let colour = |value: &toml::Value, key: &str| {
            rgba(value).ok_or_else(|| anyhow!("instance {index}'s {key} is not a colour"))
        };
        let number = |value: &toml::Value, key: &str| {
            as_f64(value)
                .map(|n| n as f32)
                .ok_or_else(|| anyhow!("instance {index}'s {key} is not a number"))
        };
        let (mut origin, mut size) = (None, None);
        for (key, value) in fields {
            match key.as_str() {
                k::POSITION => out.position = Vec3::from_array(floats(value, [0.0; 3])),
                k::ROTATION_EULER => out.rotation = rotation_of(floats(value, [0.0; 3])),
                k::SCALE => out.scale = Vec3::from_array(floats(value, [1.0; 3])),
                k::BASIS => {
                    out.basis = Some(Mat3::from_cols_array(&floats(
                        value,
                        Mat3::IDENTITY.to_cols_array(),
                    )));
                }
                k::COLOR => out.color = colour(value, k::COLOR)?,
                k::CUSTOM => out.custom = floats(value, [0.0; 4]),
                k::WIREFRAME_COLOR => out.wireframe_color = Some(colour(value, key)?),
                k::WIREFRAME_WIDTH => out.wireframe_width = Some(number(value, key)?.max(0.0)),
                k::DOT_COLOR => out.dot_color = Some(colour(value, key)?),
                k::DOT_SIZE => out.dot_size = Some(number(value, key)?.max(0.0)),
                k::REGION_ORIGIN => origin = Some(floats(value, [0.0; 2])),
                k::REGION_SIZE => size = Some(floats(value, [0.0; 2])),
                other => bail!(
                    "instance {index} has '{other}'; an instance takes {}",
                    INSTANCE_KEYS.join(", ")
                ),
            }
        }
        if out.basis.is_some()
            && (fields.contains_key(k::ROTATION_EULER) || fields.contains_key(k::SCALE))
        {
            bail!(
                "instance {index} has a {} and a {} or {}; the basis is the rotation and the scale",
                k::BASIS,
                k::ROTATION_EULER,
                k::SCALE
            );
        }
        out.region = size.filter(|[w, h]| *w > 0.0 && *h > 0.0).map(|[w, h]| {
            let [x, y] = origin.unwrap_or([0.0; 2]);
            [x, y, w, h]
        });
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
        // One spelling of the turn: a basis where it shears, else the two.
        if let Some(basis) = self.basis {
            map.insert(k::BASIS.into(), floats(&basis.to_cols_array()));
        } else {
            map.insert(k::ROTATION_EULER.into(), floats(&self.rotation_euler()));
            map.insert(k::SCALE.into(), floats(&self.scale.to_array()));
        }
        map.insert(k::COLOR.into(), floats(&self.color));
        map.insert(k::CUSTOM.into(), floats(&self.custom));
        let mut maybe = |key: &str, value: Option<&[f32]>| {
            if let Some(value) = value {
                map.insert(key.into(), floats(value));
            }
        };
        maybe(
            k::WIREFRAME_COLOR,
            self.wireframe_color.as_ref().map(|c| &c[..]),
        );
        maybe(k::DOT_COLOR, self.dot_color.as_ref().map(|c| &c[..]));
        maybe(k::REGION_ORIGIN, self.region.as_ref().map(|r| &r[..2]));
        maybe(k::REGION_SIZE, self.region.as_ref().map(|r| &r[2..]));
        if let Some(width) = self.wireframe_width {
            map.insert(
                k::WIREFRAME_WIDTH.into(),
                toml::Value::Float(f64::from(width)),
            );
        }
        if let Some(size) = self.dot_size {
            map.insert(k::DOT_SIZE.into(), toml::Value::Float(f64::from(size)));
        }
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
/// two in 2D with a zero where z would be. A shear the rows hold is kept, as
/// the instance's basis.
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
                instance.set_local(matrix);
                &row[8..]
            } else {
                let matrix = Mat4::from_cols(
                    glamx::Vec4::new(row[0], row[4], row[8], 0.0),
                    glamx::Vec4::new(row[1], row[5], row[9], 0.0),
                    glamx::Vec4::new(row[2], row[6], row[10], 0.0),
                    glamx::Vec4::new(row[3], row[7], row[11], 1.0),
                );
                instance.set_local(matrix);
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
  # a 3x3, columns first, in place of rotation_euler and scale: it keeps a shear
  { position = [2.7, 0.0, 0.0], basis = [1.0, 0.0, 0.0, 0.5, 1.0, 0.0, 0.0, 0.0, 1.0] },
  # the node's wireframe and vertex dots, for this instance alone; they draw
  # only while the node's own wireframe_width and dot_size are above zero
  { position = [3.6, 0.0, 0.0], wireframe_color = "#00ff00", wireframe_width = 2.0, dot_color = "#ffff00", dot_size = 4.0 },
]
```

A 2D instance also takes `region_origin` and `region_size`, the rectangle of the texture it draws in pixels. `multimesh2d` draws through Balaur's own pipeline, which reads neither the rectangle nor the four overrides yet."##;

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
                instance: *instance,
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
    /// The instance it came from, for the overrides it carries.
    pub instance: Instance,
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
    crate::set_color(
        eng,
        entity,
        crate::color_from_key(params, k::COLOR, [1.0; 4]),
    )?;
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
        balaur_core::components::prop_f32(params, k::PIXELS_PER_UNIT).max(0.01),
    )?;
    crate::set_polygon(eng, entity, Arc::new(polygon))?;
    crate::set_color(eng, entity, crate::color_from_params(params))?;
    crate::overlay_from_params(eng, entity, params)?;
    crate::material::set_material_2d(eng, entity, &text(params, k::MATERIAL))
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
    map.insert(k::COLOR.into(), crate::color_to_toml(renderable.color));
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
    map.insert(
        k::RENDER_LAYERS.into(),
        toml::Value::Integer(i64::from(renderable.render_layers.cast_signed())),
    );
    crate::overlay::overlay_3d_to_map(&renderable.overlay, &mut map);
    Some(toml::Value::Table(map))
}

fn get_2d(eng: &Engine, entity: Entity) -> Option<toml::Value> {
    let world = eng.world();
    let multimesh = world.get::<&MultiMesh>(entity).ok()?;
    if !multimesh.flat {
        return None;
    }
    let renderable = world.get::<&Renderable2d>(entity).ok()?;
    let (texture, ppu) = renderable
        .polygon
        .as_ref()
        .map_or((String::new(), crate::DEFAULT_PIXELS_PER_UNIT), |polygon| {
            (polygon.texture.clone(), polygon.pixels_per_unit)
        });
    let mut map = toml::map::Map::new();
    map.insert(
        k::SOURCE.into(),
        toml::Value::String(multimesh.source.clone()),
    );
    map.insert(k::TEXTURE.into(), toml::Value::String(texture));
    map.insert(
        k::PIXELS_PER_UNIT.into(),
        toml::Value::Float(f64::from(ppu)),
    );
    map.insert(k::COLOR.into(), crate::color_to_toml(renderable.color));
    map.insert(
        k::MATERIAL.into(),
        toml::Value::String(renderable.material.clone()),
    );
    crate::overlay::overlay_2d_to_map(&renderable.overlay, &mut map);
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
            doc: "Godot's `MultiMeshInstance3D`: the `multimesh` asset in `source`, its mesh drawn once per instance in one call, each instance's `color` tinted over the node's. The node keeps its own copy of the instances, so a script's edits stay on it; children draw once.",
            schema: ComponentDef::parse_schema(
                MULTIMESH_3D,
                &ComponentDef::schema(&crate::overlay::with_rows(&[
                    (k::SOURCE, &source_line()),
                    (k::TEXTURE, &texture_line()),
                    (k::COLOR, r#"{ type = "color", default = [1.0, 1.0, 1.0, 1.0], description = "Tint under every instance's own colour, as channel floats or #rrggbb / #rrggbbaa" }"#),
                    (k::MATERIAL, &material_line),
                    (k::CAST_SHADOW, r#"{ type = "bool", default = true, description = "Whether the instances cast a shadow from the lights that cast" }"#),
                    (k::LIGHT_LAYERS, r#"{ type = "int", default = -1, description = "Light-layer bitmask; a `light3d` lights this when their masks share a bit. -1 is every layer" }"#),
                    (k::RENDER_LAYERS, r#"{ type = "int", default = -1, description = "Layer bitmask; a `camera3d` draws this when their `render_layers` share a bit. -1 is every layer" }"#),
                ], &crate::overlay::schema_3d(crate::overlay::Drawn::Builtin))),
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
                &ComponentDef::schema(&crate::overlay::with_rows(&[
                    (k::SOURCE, &source_line()),
                    (k::TEXTURE, &texture_line()),
                    (k::PIXELS_PER_UNIT, r#"{ type = "float", default = 100.0, min = 0.01, description = "Texture pixels per world unit, for a mesh that carries no UVs of its own" }"#),
                    (k::COLOR, r#"{ type = "color", default = [1.0, 1.0, 1.0, 1.0], description = "Tint under every instance's own colour, as channel floats or #rrggbb / #rrggbbaa" }"#),
                    (k::MATERIAL, &crate::material::material_line_2d()),
                ], &crate::overlay::schema_2d(crate::overlay::Drawn::Pipeline))),
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
                ..Instance::default()
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
            ..Instance::default()
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

    #[test]
    fn a_sheared_basis_survives_the_buffer_and_the_file() {
        let shear = Mat3::from_cols(Vec3::X, Vec3::new(0.5, 1.0, 0.0), Vec3::Z);
        let instance = Instance {
            position: Vec3::new(1.0, 2.0, 3.0),
            basis: Some(shear),
            ..Instance::default()
        };
        let back =
            instances_from_buffer(&buffer_of(&[instance], false), false, true, true).unwrap()[0];
        assert!(
            back.basis
                .is_some_and(|b| (b - shear).to_cols_array().iter().all(|d| d.abs() < 1e-5))
        );
        let read = Instance::from_table(0, &instance.to_table()).unwrap();
        assert_eq!(read.basis, Some(shear));
        assert!(
            instance.to_table().get(k::ROTATION_EULER).is_none(),
            "one spelling of the turn"
        );
        let both = toml::from_str::<toml::Value>(
            "basis = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]\nscale = [2.0, 2.0, 2.0]",
        )
        .unwrap();
        assert!(Instance::from_table(0, &both).is_err());
    }

    #[test]
    fn an_instance_carries_its_overrides_and_region_through_its_file() {
        let row = toml::from_str::<toml::Value>(
            "wireframe_color = \"#00ff00\"\nwireframe_width = 2.0\ndot_size = 3.0\nregion_origin = [8.0, 16.0]\nregion_size = [32.0, 32.0]",
        )
        .unwrap();
        let instance = Instance::from_table(0, &row).unwrap();
        assert_eq!(instance.wireframe_color, Some([0.0, 1.0, 0.0, 1.0]));
        assert_eq!(instance.dot_size, Some(3.0));
        assert_eq!(instance.region, Some([8.0, 16.0, 32.0, 32.0]));
        assert_eq!(
            Instance::from_table(0, &instance.to_table()).unwrap(),
            instance
        );
        let uv = crate::instancing::region_uv(instance.region, Some((64, 64)));
        assert!(
            uv.iter()
                .zip([0.125, 0.25, 0.625, 0.75])
                .all(|(a, b)| (a - b).abs() < 1e-6),
            "{uv:?}"
        );
    }
}
