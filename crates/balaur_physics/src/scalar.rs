//! The one place this crate says what a number is.
//!
//! The crate uses **rapier's** math types rather than glamx's directly, so
//! `Vector`, `Pose` and `Rotation` all come from `rapier3d::math`. Rapier's
//! `Real` and the engine's `f32` are the same width, so these conversions are
//! all no-ops today — they stay because they name the seam, and a rapier that
//! changed width would break here rather than everywhere.

pub(crate) use crate::rapier2d::math::{Pose as Pose2, Rotation as Rotation2, Vector as Vector2};
pub(crate) use crate::rapier3d::math::{Pose, Real, Rotation, Vector};

/// A 3D vector from the three numbers a script or a scene wrote.
pub(crate) fn v3(x: f32, y: f32, z: f32) -> Vector {
    Vector::new(real(x), real(y), real(z))
}

/// The same, from the array a schema property parses to.
pub(crate) fn v3a(v: [f32; 3]) -> Vector {
    v3(v[0], v[1], v[2])
}

pub(crate) fn v2(x: f32, y: f32) -> Vector2 {
    Vector2::new(real(x), real(y))
}

pub(crate) fn v2a(v: [f32; 2]) -> Vector2 {
    v2(v[0], v[1])
}

/// A number on its way into rapier, which is already `f32`.
pub(crate) const fn real(value: f32) -> Real {
    value
}

/// A number on its way back out, to a script or a `Transform`.
pub(crate) const fn f32_of(value: Real) -> f32 {
    value
}

/// A vector on its way back out.
pub(crate) fn a3(v: Vector) -> [f32; 3] {
    [f32_of(v.x), f32_of(v.y), f32_of(v.z)]
}

pub(crate) fn a2(v: Vector2) -> [f32; 2] {
    [f32_of(v.x), f32_of(v.y)]
}

/// A pose from the engine's `f32` transform.
pub(crate) fn pose_of(position: glamx::Vec3, rotation: glamx::Quat) -> Pose {
    Pose::from_parts(
        v3(position.x, position.y, position.z),
        rotation_of(rotation),
    )
}

pub(crate) const fn rotation_of(rotation: glamx::Quat) -> Rotation {
    rotation
}

/// The engine's `f32` rotation, from rapier's.
pub(crate) const fn quat_of(rotation: Rotation) -> glamx::Quat {
    rotation
}

/// The engine's `f32` position, from rapier's.
pub(crate) fn position_of(v: Vector) -> glamx::Vec3 {
    glamx::Vec3::new(f32_of(v.x), f32_of(v.y), f32_of(v.z))
}

/// A voxel coordinate; rapier's cells are 32-bit, as ours are.
pub(crate) const fn cell(x: i32, y: i32, z: i32) -> crate::rapier3d::math::IVector {
    crate::rapier3d::math::IVector::new(x, y, z)
}
