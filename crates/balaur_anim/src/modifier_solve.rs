//! The geometry under the rig modifiers: poses composed from locals, turning
//! a bone to aim, the joints a chain reaches with, and the two-bone solve.

use balaur_core::hecs::{Entity, World};
use balaur_core::scene::{self, Children, Parent, Transform};
use balaur_core::skeleton::{Bone, affine_2d, affine_3d, quat_about_z, quat_from_euler};
use glamx::{Mat3, Mat4, Quat, Vec2, Vec3};

/// The shortest bone [`two_bone_ik`] will solve. Below it the reach clamp
/// inverts and there is no elbow angle to find anyway.
pub(crate) const MIN_BONE: f32 = 1e-5;

/// How far down a chain a modifier will walk when `chain` is left at zero.
/// A cycle cannot happen in a scene tree, but a rig deep enough to matter
/// here is already past what a solver converges on.
const MAX_CHAIN: usize = 64;

/// A node's 2D world pose composed from local transforms, so a bone this
/// frame has already moved sees the move.
pub(crate) fn pose_2d(world: &World, entity: Entity) -> Mat3 {
    let mut matrix = Mat3::IDENTITY;
    for e in ancestry(world, entity) {
        if let Ok(t) = world.get::<&Transform>(e) {
            matrix *= affine_2d(t.position, t.rotation, t.scale);
        }
    }
    matrix
}

/// The 3D twin of [`pose_2d`].
pub(crate) fn pose_3d(world: &World, entity: Entity) -> Mat4 {
    let mut matrix = Mat4::IDENTITY;
    for e in ancestry(world, entity) {
        if let Ok(t) = world.get::<&Transform>(e) {
            matrix *= affine_3d(t.position, t.rotation, t.scale);
        }
    }
    matrix
}

/// A node's world rotation alone, composed the same way. Scale would skew an
/// aim direction and translation cannot turn one, so neither is wanted here.
pub(crate) fn rotation_3d(world: &World, entity: Entity) -> Quat {
    let mut q = Quat::IDENTITY;
    for e in ancestry(world, entity) {
        if let Ok(t) = world.get::<&Transform>(e) {
            q *= t.rotation;
        }
    }
    q
}

/// Where a node sits in the scene, as the child index at each step down from
/// the root: the order the scene file reads in, and one that stays put when a
/// node elsewhere is added or removed.
pub(crate) fn scene_order(world: &World, entity: Entity) -> Vec<u32> {
    let line = ancestry(world, entity);
    line.windows(2)
        .map(|step| {
            world
                .get::<&scene::Children>(step[0])
                .ok()
                .and_then(|kids| kids.0.iter().position(|child| *child == step[1]))
                .unwrap_or(0) as u32
        })
        .collect()
}

/// `entity` and every ancestor, root first — the order a pose composes in.
fn ancestry(world: &World, entity: Entity) -> Vec<Entity> {
    let mut chain = vec![entity];
    let mut current = entity;
    while let Ok(parent) = world.get::<&Parent>(current) {
        current = parent.0;
        chain.push(current);
        if chain.len() > MAX_CHAIN * 4 {
            break;
        }
    }
    chain.reverse();
    chain
}

pub(crate) fn angle_of(m: &Mat3) -> f32 {
    libm::atan2f(m.x_axis.y, m.x_axis.x)
}

pub(crate) fn origin_2d(m: &Mat3) -> Vec2 {
    Vec2::new(m.z_axis.x, m.z_axis.y)
}

pub(crate) fn origin_3d(m: &Mat4) -> Vec3 {
    m.w_axis.truncate()
}

fn first_child_bone(world: &World, entity: Entity) -> Option<Entity> {
    let children = world.get::<&Children>(entity).ok()?;
    children
        .0
        .iter()
        .copied()
        .find(|&child| world.get::<&Bone>(child).is_ok())
}

/// The bones a chain solver works on: `root` and its first-child bones, at
/// most `len` of them, or as far as the rig goes when `len` is zero.
pub(crate) fn chain_of(world: &World, root: Entity, len: usize) -> Vec<Entity> {
    let cap = if len == 0 {
        MAX_CHAIN
    } else {
        len.min(MAX_CHAIN)
    };
    let mut out = Vec::new();
    let mut current = Some(root);
    while let Some(bone) = current {
        out.push(bone);
        if out.len() >= cap {
            break;
        }
        current = first_child_bone(world, bone);
    }
    out
}

/// The direction a bone points along in its own frame: toward its first
/// child bone, else its gizmo `angle`.
pub(crate) fn aim_local_2d(world: &World, bone: Entity) -> f32 {
    match first_child_bone(world, bone) {
        Some(child) => {
            let p = world
                .get::<&Transform>(child)
                .map(|t| t.position)
                .unwrap_or_default();
            libm::atan2f(p.y, p.x)
        }
        None => world.get::<&Bone>(bone).map_or(0.0, |b| b.angle),
    }
}

/// Turn `bone` so that its aim points along `wanted` in world space.
pub(crate) fn aim_at_angle(world: &World, bone: Entity, wanted: f32) {
    let parent_angle = world
        .get::<&Parent>(bone)
        .ok()
        .map_or(0.0, |p| angle_of(&pose_2d(world, p.0)));
    let local = wanted - aim_local_2d(world, bone) - parent_angle;
    if let Ok(mut t) = world.get::<&mut Transform>(bone) {
        t.rotation = quat_about_z(local);
    }
}

/// Point `bone`'s aim at a world point, doing nothing when the point is the
/// bone's own origin and names no direction.
pub(crate) fn aim_at_point_2d(world: &World, bone: Entity, point: Vec2) {
    let to = point - origin_2d(&pose_2d(world, bone));
    if to.length_squared() > MIN_BONE * MIN_BONE {
        aim_at_angle(world, bone, libm::atan2f(to.y, to.x));
    }
}

/// Hold a bone within `limit` radians of its rest rotation about z.
pub(crate) fn clamp_angle_2d(world: &World, bone: Entity, limit: f32) {
    if limit <= 0.0 {
        return;
    }
    let rest = world.get::<&Bone>(bone).map_or(0.0, |b| b.rest_rotation.z);
    let Ok(mut t) = world.get::<&mut Transform>(bone) else {
        return;
    };
    let current = balaur_core::skeleton::angle_about_z(t.rotation);
    let delta = wrap_pi(current - rest);
    if delta.abs() > limit {
        t.rotation = quat_about_z(rest + limit.copysign(delta));
    }
}

/// An angle folded onto `-pi..pi`, so "how far from rest" is the short way
/// round rather than a number that grew with the turns.
fn wrap_pi(angle: f32) -> f32 {
    use std::f32::consts::{PI, TAU};
    let mut a = angle;
    while a > PI {
        a -= TAU;
    }
    while a < -PI {
        a += TAU;
    }
    a
}

/// The direction a bone points along in its own frame, as a unit vector:
/// toward its first child bone, else `+X`, which is what a bone with no
/// child and no 2D gizmo angle has to mean.
pub(crate) fn aim_local_3d(world: &World, bone: Entity) -> Vec3 {
    let toward = first_child_bone(world, bone)
        .and_then(|child| world.get::<&Transform>(child).ok().map(|t| t.position))
        .unwrap_or(Vec3::X);
    if toward.length_squared() > MIN_BONE * MIN_BONE {
        toward.normalize()
    } else {
        Vec3::X
    }
}

/// Turn `bone` by the shortest arc so its aim points along `wanted` in world
/// space, leaving its twist about that aim as the clip wrote it.
pub(crate) fn aim_at_dir_3d(world: &World, bone: Entity, wanted: Vec3) {
    if wanted.length_squared() <= MIN_BONE * MIN_BONE {
        return;
    }
    let parent = world
        .get::<&Parent>(bone)
        .ok()
        .map_or(Quat::IDENTITY, |p| rotation_3d(world, p.0));
    // The aim is stated in the bone's own frame, so the wanted direction is
    // carried into the parent's before the arc between them is taken.
    let local_wanted = parent.inverse() * wanted.normalize();
    let rotation = Quat::from_rotation_arc(aim_local_3d(world, bone), local_wanted);
    if let Ok(mut t) = world.get::<&mut Transform>(bone) {
        t.rotation = rotation;
    }
}

pub(crate) fn aim_at_point_3d(world: &World, bone: Entity, point: Vec3) {
    let to = point - origin_3d(&pose_3d(world, bone));
    aim_at_point_dir(world, bone, to);
}

fn aim_at_point_dir(world: &World, bone: Entity, to: Vec3) {
    if to.length_squared() > MIN_BONE * MIN_BONE {
        aim_at_dir_3d(world, bone, to);
    }
}

/// Hold a bone within `limit` radians of its rest rotation.
pub(crate) fn clamp_angle_3d(world: &World, bone: Entity, limit: f32) {
    if limit <= 0.0 {
        return;
    }
    let rest = world
        .get::<&Bone>(bone)
        .map_or(Quat::IDENTITY, |b| quat_from_euler(b.rest_rotation));
    let Ok(mut t) = world.get::<&mut Transform>(bone) else {
        return;
    };
    let delta = rest.inverse() * t.rotation;
    // `w` past one by a rounding step is what makes `acos` return NaN and
    // freeze the bone at a rotation nothing can clamp afterwards.
    let angle = 2.0 * libm::acosf(delta.w.abs().clamp(0.0, 1.0));
    if angle <= limit {
        return;
    }
    let axis = Vec3::new(delta.x, delta.y, delta.z);
    if axis.length_squared() <= MIN_BONE * MIN_BONE {
        return;
    }
    let axis = axis.normalize() * delta.w.signum();
    t.rotation = rest * Quat::from_axis_angle(axis, limit);
}

/// The joints a chain reaches with: every bone's origin, and the last bone's
/// tip when it has one, so there is a segment per bone rather than per gap.
pub(crate) fn chain_points(world: &World, chain: &[Entity], dim3: bool) -> Vec<Vec3> {
    let origin = |bone: Entity| {
        if dim3 {
            origin_3d(&pose_3d(world, bone))
        } else {
            origin_2d(&pose_2d(world, bone)).extend(0.0)
        }
    };
    let mut points: Vec<Vec3> = chain.iter().map(|&bone| origin(bone)).collect();
    if let Some(&last) = chain.last()
        && let Some(tip) = tip_of(world, last, dim3)
    {
        points.push(tip);
    }
    points
}

/// Where a chain's last bone ends: its first child bone if it has one, else
/// its gizmo length along its aim. A bone with neither ends the chain at its
/// own origin and simply does not get a segment.
pub(crate) fn tip_of(world: &World, bone: Entity, dim3: bool) -> Option<Vec3> {
    if let Some(child) = first_child_bone(world, bone) {
        return Some(if dim3 {
            origin_3d(&pose_3d(world, child))
        } else {
            origin_2d(&pose_2d(world, child)).extend(0.0)
        });
    }
    let length = world.get::<&Bone>(bone).map_or(0.0, |b| b.length);
    if length <= MIN_BONE {
        return None;
    }
    if dim3 {
        let pose = pose_3d(world, bone);
        let dir = rotation_3d(world, bone) * aim_local_3d(world, bone);
        Some(origin_3d(&pose) + dir * length)
    } else {
        let pose = pose_2d(world, bone);
        let angle = angle_of(&pose) + aim_local_2d(world, bone);
        let (s, c) = libm::sincosf(angle);
        Some((origin_2d(&pose) + Vec2::new(c, s) * length).extend(0.0))
    }
}

pub(crate) fn segment_lengths(points: &[Vec3]) -> Vec<f32> {
    points.windows(2).map(|w| (w[1] - w[0]).length()).collect()
}

/// The analytic two-bone solve: the root turns to put the middle joint on
/// the circle both segments can reach, then the middle turns to put the tip
/// on the target. Out of reach, the chain straightens toward it.
pub(crate) fn two_bone_ik_2d(world: &World, root: Entity, target: Vec2, flip: bool) {
    let (Some(mid), Some(tip)) = (
        first_child_bone(world, root),
        first_child_bone(world, root).and_then(|mid| first_child_bone(world, mid)),
    ) else {
        tracing::debug!("two_bone_ik needs a root, middle and tip bone");
        return;
    };
    let (r, m, t) = (
        origin_2d(&pose_2d(world, root)),
        origin_2d(&pose_2d(world, mid)),
        origin_2d(&pose_2d(world, tip)),
    );
    let l1 = (m - r).length();
    let l2 = (t - m).length();
    // The clamp below has `min > max` for anything shorter, and `f32::clamp`
    // panics on that: a bone 5e-6 from its parent is what reaches it.
    if !(l1 > MIN_BONE && l2 > MIN_BONE) {
        return;
    }
    let to = target - r;
    let base = libm::atan2f(to.y, to.x);
    let sign = if flip { -1.0 } else { 1.0 };
    let (root_aim, mid_aim) = two_bone_angles(l1, l2, to.length(), base, sign);
    aim_at_angle(world, root, root_aim);
    aim_at_angle(world, mid, mid_aim);
}

/// The two aim angles a two-bone chain takes, shared by both dimensions: 3D
/// solves in the plane the chain and the target span and uses these there.
fn two_bone_angles(l1: f32, l2: f32, distance: f32, base: f32, sign: f32) -> (f32, f32) {
    let d = distance.clamp((l1 - l2).abs() + 1e-5, l1 + l2 - 1e-5);
    let cos_root = ((l1 * l1 + d * d - l2 * l2) / (2.0 * l1 * d)).clamp(-1.0, 1.0);
    let cos_mid = ((l1 * l1 + l2 * l2 - d * d) / (2.0 * l1 * l2)).clamp(-1.0, 1.0);
    let root_aim = base + sign * libm::acosf(cos_root);
    let mid_aim = root_aim - sign * (std::f32::consts::PI - libm::acosf(cos_mid));
    (root_aim, mid_aim)
}

/// The 3D two-bone solve. The chain bends in the plane holding the root, the
/// target and the pole — `flip` picks the other side of it — and the two
/// angles are the same law of cosines the 2D solve uses.
pub(crate) fn two_bone_ik_3d(world: &World, root: Entity, target: Vec3, flip: bool) {
    let (Some(mid), Some(tip)) = (
        first_child_bone(world, root),
        first_child_bone(world, root).and_then(|mid| first_child_bone(world, mid)),
    ) else {
        tracing::debug!("two_bone_ik needs a root, middle and tip bone");
        return;
    };
    let (r, m, t) = (
        origin_3d(&pose_3d(world, root)),
        origin_3d(&pose_3d(world, mid)),
        origin_3d(&pose_3d(world, tip)),
    );
    let l1 = (m - r).length();
    let l2 = (t - m).length();
    if !(l1 > MIN_BONE && l2 > MIN_BONE) {
        return;
    }
    let to = target - r;
    if to.length_squared() <= MIN_BONE * MIN_BONE {
        return;
    }
    let axis = to.normalize();
    // The bend plane: the elbow's current offset from the root-to-target line
    // is what keeps a solved knee pointing where the clip had it. A chain
    // already straight has no such offset, so any perpendicular will do.
    let offset = (m - r) - axis * (m - r).dot(axis);
    let bend = if offset.length_squared() > MIN_BONE * MIN_BONE {
        offset.normalize()
    } else {
        axis.any_orthonormal_vector()
    };
    let bend = if flip { -bend } else { bend };
    let (root_aim, mid_aim) = two_bone_angles(l1, l2, to.length(), 0.0, 1.0);
    let dir = |angle: f32| {
        let (s, c) = libm::sincosf(angle);
        axis * c + bend * s
    };
    aim_at_dir_3d(world, root, dir(root_aim));
    aim_at_dir_3d(world, mid, dir(mid_aim));
}
