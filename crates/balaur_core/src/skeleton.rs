//! Bones: a node with a rest pose, and the joint palette a skin deforms by.
//!
//! A bone is a node. A clip already animates it by path and the digest
//! already hashes its transform, so this module adds only what a node lacks:
//! the pose a rig returns to (the `bone2d` component), the walk that lists a
//! rig's bones in tree order, and the joint matrices a skinned mesh
//! multiplies its vertices by. There is no skeleton component: a skin names
//! its rig by node path, and the rig's bones are that node's descendants
//! that carry a [`Bone`].
//!
//! Pure scene-tree math with no backend type in it, in core for the reason
//! `mesh` is: rendering skins with it, the editor edits it through the
//! registry, and physics will attach to it. Every transcendental here is
//! `libm`'s, so a palette computed on one platform is the palette computed
//! on another.

use anyhow::{anyhow, Result};
use balaur_script::Value;
use glamx::{Mat3, Mat4, Quat, Vec2, Vec3};
use hecs::{Entity, World};

use crate::components::{as_f64, ComponentDef};
use crate::engine::Engine;
use crate::scene::{Children, GlobalTransform, Parent, Transform};
use crate::App;

/// A bone's rest pose and gizmo hints, in the node's local space.
///
/// One struct for both dimensions: `bone2d` writes the z rotation and leaves
/// the rest at identity. The rotation is kept as the euler radians that were
/// authored, so `get` reads back the number that was written rather than one
/// recovered from a quaternion.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bone {
    pub rest_position: Vec3,
    /// Euler radians, the spelling `rotation_euler` uses.
    pub rest_rotation: Vec3,
    pub rest_scale: Vec3,
    /// Gizmo length of a tip bone; `0` draws to the first child bone.
    pub length: f32,
    /// Gizmo direction of a tip bone, radians about z.
    pub angle: f32,
    /// Written by `bone2d` rather than `bone3d`. One struct backs both keys,
    /// and this is what keeps each key reporting only its own.
    pub planar: bool,
}

impl Default for Bone {
    fn default() -> Self {
        Self {
            rest_position: Vec3::ZERO,
            rest_rotation: Vec3::ZERO,
            rest_scale: Vec3::ONE,
            length: 0.0,
            angle: 0.0,
            planar: false,
        }
    }
}

impl Bone {
    /// The rest pose as the transform `apply_rest` writes.
    #[must_use]
    pub fn rest_transform(&self) -> Transform {
        Transform {
            position: self.rest_position,
            rotation: quat_from_euler(self.rest_rotation),
            scale: self.rest_scale,
        }
    }

    /// The rest pose as a 2D affine matrix, parent-relative.
    #[must_use]
    pub fn rest_matrix_2d(&self) -> Mat3 {
        let (s, c) = libm::sincosf(self.rest_rotation.z);
        Mat3::from_cols(
            Vec3::new(c * self.rest_scale.x, s * self.rest_scale.x, 0.0),
            Vec3::new(-s * self.rest_scale.y, c * self.rest_scale.y, 0.0),
            Vec3::new(self.rest_position.x, self.rest_position.y, 1.0),
        )
    }

    /// The rest pose as a 3D affine matrix, parent-relative.
    #[must_use]
    pub fn rest_matrix_3d(&self) -> Mat4 {
        affine_3d(
            self.rest_position,
            quat_from_euler(self.rest_rotation),
            self.rest_scale,
        )
    }
}

/// A rotation of `angle` radians about z, on `libm` rather than `f32::sin`.
#[must_use]
pub fn quat_about_z(angle: f32) -> Quat {
    quat_from_euler(Vec3::new(0.0, 0.0, angle))
}

/// The angle about z a rotation turns the x axis by, on `libm`.
#[must_use]
pub fn angle_about_z(rotation: Quat) -> f32 {
    let x = rotation * Vec3::X;
    libm::atan2f(x.y, x.x)
}

/// An euler triple as a quaternion, in the engine's own convention.
///
/// `[x, y, z]` are rotations about X, Y and Z, composed Z then Y then X —
/// what a scene file's `rotation_euler` means. Written out on `libm` rather
/// than `Quat::from_euler`, whose sin/cos are the platform's.
#[must_use]
pub fn quat_from_euler(euler: Vec3) -> Quat {
    let (sr, cr) = libm::sincosf(euler.x * 0.5);
    let (sp, cp) = libm::sincosf(euler.y * 0.5);
    let (sy, cy) = libm::sincosf(euler.z * 0.5);
    Quat::from_xyzw(
        cy * cp * sr - sy * sp * cr,
        cy * sp * cr + sy * cp * sr,
        sy * cp * cr - cy * sp * sr,
        cy * cp * cr + sy * sp * sr,
    )
}

/// A quaternion back to the euler triple [`quat_from_euler`] would build it
/// from, in the same convention and on the same `libm`. Straight up or down
/// the X and Z angles trade places, as in every euler convention; the
/// rotation the pair describes is still the right one.
#[must_use]
pub fn euler_from_quat(q: Quat) -> Vec3 {
    let sin_pitch = (2.0 * (q.w * q.y - q.z * q.x)).clamp(-1.0, 1.0);
    Vec3::new(
        libm::atan2f(
            2.0 * (q.w * q.x + q.y * q.z),
            1.0 - 2.0 * (q.x * q.x + q.y * q.y),
        ),
        libm::asinf(sin_pitch),
        libm::atan2f(
            2.0 * (q.w * q.z + q.x * q.y),
            1.0 - 2.0 * (q.y * q.y + q.z * q.z),
        ),
    )
}

/// A 3D affine matrix from a pose. Arithmetic only.
#[must_use]
pub fn affine_3d(position: Vec3, rotation: Quat, scale: Vec3) -> Mat4 {
    Mat4::from_scale_rotation_translation(scale, rotation, position)
}

/// A 2D affine matrix from a pose: the rotation's xy block, scaled, then
/// translated. Arithmetic only, so it is the same on every platform.
#[must_use]
pub fn affine_2d(position: Vec3, rotation: Quat, scale: Vec3) -> Mat3 {
    let r = Mat3::from_quat(rotation);
    Mat3::from_cols(
        Vec3::new(r.x_axis.x * scale.x, r.x_axis.y * scale.x, 0.0),
        Vec3::new(r.y_axis.x * scale.y, r.y_axis.y * scale.y, 0.0),
        Vec3::new(position.x, position.y, 1.0),
    )
}

/// Every bone under `root`, in tree order — `root` itself first when it is
/// one. This is the order a skin's bones are numbered in.
#[must_use]
pub fn bones_under(world: &World, root: Entity) -> Vec<Entity> {
    let mut out = Vec::new();
    let mut stack = vec![root];
    while let Some(entity) = stack.pop() {
        if world.get::<&Bone>(entity).is_ok() {
            out.push(entity);
        }
        if let Ok(children) = world.get::<&Children>(entity) {
            stack.extend(children.0.iter().rev().copied());
        }
    }
    out
}

/// Reset to Rest Pose: every bone under `root` takes its rest transform.
pub fn apply_rest(world: &mut World, root: Entity) {
    for bone in bones_under(world, root) {
        let rest = world.get::<&Bone>(bone).map(|b| b.rest_transform());
        if let (Ok(rest), Ok(mut transform)) = (rest, world.get::<&mut Transform>(bone)) {
            *transform = rest;
        }
    }
}

/// Overwrite Rest Pose: every bone under `root` records its current
/// transform as its rest.
pub fn overwrite_rest(world: &mut World, root: Entity) {
    for bone in bones_under(world, root) {
        let current = world.get::<&Transform>(bone).map(|t| *t);
        if let (Ok(current), Ok(mut b)) = (current, world.get::<&mut Bone>(bone)) {
            b.rest_position = current.position;
            b.rest_rotation = euler_from_quat(current.rotation);
            b.rest_scale = current.scale;
        }
    }
}

/// The 3D twin of `rest_in_rig`: a bone's rest composed down from `rig`.
fn rest_in_rig_3d(world: &World, rig: Entity, bone: Entity) -> Option<Mat4> {
    let mut chain = Vec::new();
    let mut current = bone;
    while current != rig {
        chain.push(current);
        current = world.get::<&Parent>(current).ok()?.0;
    }
    let mut matrix = Mat4::IDENTITY;
    for entity in chain.into_iter().rev() {
        let local = if let Ok(bone) = world.get::<&Bone>(entity) {
            bone.rest_matrix_3d()
        } else {
            let t = world.get::<&Transform>(entity).ok()?;
            affine_3d(t.position, t.rotation, t.scale)
        };
        matrix *= local;
    }
    Some(matrix)
}

/// The 3D twin of [`joint_matrices_2d`]. An imported model carries its own
/// inverse bind matrices (in rig space) and hands them in as `inverse_bind`;
/// a rig authored here derives them from the rest poses.
#[must_use]
pub fn joint_matrices_3d(
    world: &World,
    skin: Entity,
    rig: Entity,
    bones: &[Option<Entity>],
    inverse_bind: Option<&[Mat4]>,
) -> Vec<Mat4> {
    let global = |entity: Entity| {
        world
            .get::<&GlobalTransform>(entity)
            .ok()
            .map(|g| affine_3d(g.position, g.rotation, g.scale))
    };
    let (Some(skin_global), Some(rig_global)) = (global(skin), global(rig)) else {
        return vec![Mat4::IDENTITY; bones.len()];
    };
    let to_skin = skin_global.inverse();
    let rig_inverse = rig_global.inverse();
    bones
        .iter()
        .enumerate()
        .map(|(i, bone)| {
            let Some(bone) = *bone else {
                return Mat4::IDENTITY;
            };
            let bind = match inverse_bind.and_then(|ib| ib.get(i)) {
                Some(matrix) => Some(*matrix),
                None => rest_in_rig_3d(world, rig, bone).map(|rest| rest.inverse()),
            };
            let (Some(bone_global), Some(bind)) = (global(bone), bind) else {
                return Mat4::IDENTITY;
            };
            to_skin * bone_global * bind * rig_inverse * skin_global
        })
        .collect()
}

/// Deform 3D positions by a joint palette, the way [`skin_positions`] does
/// in 2D.
#[must_use]
pub fn skin_positions_3d(
    positions: &[Vec3],
    joints: &[[u32; 4]],
    weights: &[[f32; 4]],
    palette: &[Mat4],
) -> Vec<Vec3> {
    blend_3d(positions, joints, weights, palette, |m, p| {
        m.transform_point3(p)
    })
}

/// Deform normals by a joint palette: rotated by each joint, renormalised.
#[must_use]
pub fn skin_normals_3d(
    normals: &[Vec3],
    joints: &[[u32; 4]],
    weights: &[[f32; 4]],
    palette: &[Mat4],
) -> Vec<Vec3> {
    blend_3d(normals, joints, weights, palette, |m, n| {
        m.transform_vector3(n)
    })
    .into_iter()
    .map(Vec3::normalize_or_zero)
    .collect()
}

fn blend_3d(
    values: &[Vec3],
    joints: &[[u32; 4]],
    weights: &[[f32; 4]],
    palette: &[Mat4],
    apply: impl Fn(&Mat4, Vec3) -> Vec3,
) -> Vec<Vec3> {
    values
        .iter()
        .enumerate()
        .map(|(i, &v)| {
            let (Some(j), Some(w)) = (joints.get(i), weights.get(i)) else {
                return v;
            };
            let total = w[0] + w[1] + w[2] + w[3];
            if total <= 0.0 {
                return v;
            }
            let mut out = Vec3::ZERO;
            for slot in 0..4 {
                if w[slot] == 0.0 {
                    continue;
                }
                let moved = palette.get(j[slot] as usize).map_or(v, |m| apply(m, v));
                out += moved * w[slot];
            }
            out
        })
        .collect()
}

/// A bone's rest pose composed down from `rig` (the rig's own transform
/// excluded), or `None` when `bone` is not under `rig`. A non-bone node on
/// the way down has no rest, so its current transform stands in.
fn rest_in_rig(world: &World, rig: Entity, bone: Entity) -> Option<Mat3> {
    let mut chain = Vec::new();
    let mut current = bone;
    while current != rig {
        chain.push(current);
        current = world.get::<&Parent>(current).ok()?.0;
    }
    let mut matrix = Mat3::IDENTITY;
    for entity in chain.into_iter().rev() {
        let local = if let Ok(bone) = world.get::<&Bone>(entity) {
            bone.rest_matrix_2d()
        } else {
            let t = world.get::<&Transform>(entity).ok()?;
            affine_2d(t.position, t.rotation, t.scale)
        };
        matrix *= local;
    }
    Some(matrix)
}

/// The joint matrices a skin on `skin` node deforms by, one per bone, in
/// the skin node's own space: current pose times inverse rest, carried from
/// rig space into the skin's. A bone that is missing or not under `rig`
/// contributes the identity, so its vertices stay where they were authored.
///
/// Reads `GlobalTransform`, so it wants a propagated tree — a render stage
/// or a test that ran the scene sync.
#[must_use]
pub fn joint_matrices_2d(
    world: &World,
    skin: Entity,
    rig: Entity,
    bones: &[Option<Entity>],
) -> Vec<Mat3> {
    let global = |entity: Entity| {
        world
            .get::<&GlobalTransform>(entity)
            .ok()
            .map(|g| affine_2d(g.position, g.rotation, g.scale))
    };
    let (Some(skin_global), Some(rig_global)) = (global(skin), global(rig)) else {
        return vec![Mat3::IDENTITY; bones.len()];
    };
    let to_skin = skin_global.inverse();
    let rig_inverse = rig_global.inverse();
    bones
        .iter()
        .map(|bone| {
            let Some(bone) = *bone else {
                return Mat3::IDENTITY;
            };
            let (Some(bone_global), Some(rest)) = (global(bone), rest_in_rig(world, rig, bone))
            else {
                return Mat3::IDENTITY;
            };
            to_skin * bone_global * rest.inverse() * rig_inverse * skin_global
        })
        .collect()
}

/// Deform `positions` by a joint palette: each vertex is the weighted blend
/// of its joints' matrices applied to it. A vertex whose weights sum to zero
/// is left alone, and a joint index past the palette is the identity.
///
/// What the GPU does, written out so a headless test can assert it.
#[must_use]
pub fn skin_positions(
    positions: &[Vec2],
    joints: &[[u32; 4]],
    weights: &[[f32; 4]],
    palette: &[Mat3],
) -> Vec<Vec2> {
    positions
        .iter()
        .enumerate()
        .map(|(i, &p)| {
            let (Some(j), Some(w)) = (joints.get(i), weights.get(i)) else {
                return p;
            };
            let total = w[0] + w[1] + w[2] + w[3];
            if total <= 0.0 {
                return p;
            }
            let mut out = Vec2::ZERO;
            for slot in 0..4 {
                if w[slot] == 0.0 {
                    continue;
                }
                let moved = palette
                    .get(j[slot] as usize)
                    .map_or(p, |m| m.transform_point2(p));
                out += moved * w[slot];
            }
            out
        })
        .collect()
}

const BONE2D_SCHEMA: &str = r#"rest_position = { type = "vec2", default = [0.0, 0.0], description = "Local rest translation" }
rest_rotation = { type = "float", default = 0.0, description = "Local rest rotation about z, in radians" }
length = { type = "float", default = 0.0, min = 0.0, description = "Gizmo length of a tip bone; 0 draws to the first child bone" }
angle = { type = "float", default = 0.0, description = "Gizmo direction of a tip bone, in radians; ignored while a child bone exists" }"#;

/// The `bone2d` component: writes one [`Bone`] on the node.
pub(crate) fn register_bone2d_component(app: &mut App) {
    app.register_component(
        "bone2d",
        ComponentDef {
            doc: "",
            schema: ComponentDef::parse_schema("bone2d", BONE2D_SCHEMA),
            tags: &["2d", "animation"],
            expects: &[],
            apply: Box::new(apply_bone2d),
            remove: Box::new(|eng, entity| {
                let _ = eng.world_mut().remove_one::<Bone>(entity);
                Ok(())
            }),
            get: Box::new(bone2d_of),
        },
    );
}

fn apply_bone2d(eng: &Engine, entity: Entity, params: &toml::Value) -> Result<()> {
    let number =
        |key: &str, default: f64| params.get(key).and_then(as_f64).unwrap_or(default) as f32;
    let rest = |i: usize| {
        params
            .get("rest_position")
            .and_then(toml::Value::as_array)
            .and_then(|a| a.get(i))
            .and_then(as_f64)
            .unwrap_or(0.0) as f32
    };
    let bone = Bone {
        rest_position: Vec3::new(rest(0), rest(1), 0.0),
        rest_rotation: Vec3::new(0.0, 0.0, number("rest_rotation", 0.0)),
        rest_scale: Vec3::ONE,
        length: number("length", 0.0).max(0.0),
        angle: number("angle", 0.0),
        planar: true,
    };
    eng.world_mut()
        .insert_one(entity, bone)
        .map_err(|_| anyhow!("node is dead"))
}

const BONE3D_SCHEMA: &str = r#"rest_position = { type = "vec3", default = [0.0, 0.0, 0.0], description = "Local rest translation" }
rest_rotation = { type = "vec3", default = [0.0, 0.0, 0.0], description = "Local rest rotation, euler radians in the order rotation_euler uses" }
rest_scale = { type = "vec3", default = [1.0, 1.0, 1.0], description = "Local rest scale" }
length = { type = "float", default = 0.0, min = 0.0, description = "Gizmo length of a tip bone; 0 draws to the first child bone" }"#;

/// The `bone3d` component: writes one [`Bone`] on the node, every axis.
pub(crate) fn register_bone3d_component(app: &mut App) {
    app.register_component(
        "bone3d",
        ComponentDef {
            doc: "",
            schema: ComponentDef::parse_schema("bone3d", BONE3D_SCHEMA),
            tags: &["3d", "animation"],
            expects: &[],
            apply: Box::new(apply_bone3d),
            remove: Box::new(|eng, entity| {
                let _ = eng.world_mut().remove_one::<Bone>(entity);
                Ok(())
            }),
            get: Box::new(bone3d_of),
        },
    );
}

fn vec3_param(params: &toml::Value, key: &str, default: Vec3) -> Vec3 {
    let axis = |i: usize| {
        params
            .get(key)
            .and_then(toml::Value::as_array)
            .and_then(|a| a.get(i))
            .and_then(as_f64)
            .map(|v| v as f32)
    };
    Vec3::new(
        axis(0).unwrap_or(default.x),
        axis(1).unwrap_or(default.y),
        axis(2).unwrap_or(default.z),
    )
}

fn vec3_value(v: Vec3) -> toml::Value {
    toml::Value::Array(
        v.to_array()
            .into_iter()
            .map(|n| toml::Value::Float(f64::from(n)))
            .collect(),
    )
}

fn apply_bone3d(eng: &Engine, entity: Entity, params: &toml::Value) -> Result<()> {
    let bone = Bone {
        rest_position: vec3_param(params, "rest_position", Vec3::ZERO),
        rest_rotation: vec3_param(params, "rest_rotation", Vec3::ZERO),
        rest_scale: vec3_param(params, "rest_scale", Vec3::ONE),
        length: params
            .get("length")
            .and_then(as_f64)
            .unwrap_or(0.0)
            .max(0.0) as f32,
        angle: 0.0,
        planar: false,
    };
    eng.world_mut()
        .insert_one(entity, bone)
        .map_err(|_| anyhow!("node is dead"))
}

fn bone3d_of(eng: &Engine, entity: Entity) -> Option<toml::Value> {
    let world = eng.world();
    let bone = world.get::<&Bone>(entity).ok().filter(|b| !b.planar)?;
    let mut out = toml::map::Map::new();
    out.insert("rest_position".into(), vec3_value(bone.rest_position));
    out.insert("rest_rotation".into(), vec3_value(bone.rest_rotation));
    out.insert("rest_scale".into(), vec3_value(bone.rest_scale));
    out.insert("length".into(), toml::Value::Float(f64::from(bone.length)));
    Some(toml::Value::Table(out))
}

fn bone2d_of(eng: &Engine, entity: Entity) -> Option<toml::Value> {
    let world = eng.world();
    let bone = world.get::<&Bone>(entity).ok().filter(|b| b.planar)?;
    let mut out = toml::map::Map::new();
    out.insert(
        "rest_position".into(),
        toml::Value::Array(vec![
            toml::Value::Float(f64::from(bone.rest_position.x)),
            toml::Value::Float(f64::from(bone.rest_position.y)),
        ]),
    );
    out.insert(
        "rest_rotation".into(),
        toml::Value::Float(f64::from(bone.rest_rotation.z)),
    );
    out.insert("length".into(), toml::Value::Float(f64::from(bone.length)));
    out.insert("angle".into(), toml::Value::Float(f64::from(bone.angle)));
    Some(toml::Value::Table(out))
}

fn node_arg(args: &[Value]) -> Result<Entity> {
    match args.first() {
        Some(Value::Node(id)) => crate::entity_of(balaur_script::NodeId(*id)),
        other => Err(anyhow!("argument 0 should be a node, got {other:?}")),
    }
}

/// `skeleton.apply_rest(node)`.
pub(crate) fn apply_rest_op(eng: &Engine, args: &[Value]) -> Result<Value> {
    let root = node_arg(args)?;
    apply_rest(&mut eng.world_mut(), root);
    Ok(Value::Nil)
}

/// `skeleton.overwrite_rest(node)`.
pub(crate) fn overwrite_rest_op(eng: &Engine, args: &[Value]) -> Result<Value> {
    let root = node_arg(args)?;
    overwrite_rest(&mut eng.world_mut(), root);
    Ok(Value::Nil)
}

/// `skeleton.bones(node)`: the bones under a node, in the order a skin
/// numbers them.
pub(crate) fn bones_op(eng: &Engine, args: &[Value]) -> Result<Value> {
    let root = node_arg(args)?;
    let world = eng.world();
    Ok(Value::List(
        bones_under(&world, root)
            .into_iter()
            .map(|bone| Value::Node(crate::node_id_of(bone).0))
            .collect(),
    ))
}
