//! The one place this crate says what a number is.
//!
//! Rapier ships twice — `rapier3d` and `rapier3d-f64` — and everything under
//! `math` follows: `Vector` is a `Vec3` in one and a `DVec3` in the other,
//! `Pose` and `Rotation` likewise. So the crate uses **rapier's** math types
//! rather than glamx's directly, and the `f64` feature swaps the dependency
//! under them.
//!
//! What does not swap is the engine around it: `Transform`, the script value
//! model and every scene file are `f32`. Those are the seams, and the
//! conversions below are the only places a number changes width — which is
//! also why they are the only places to look when a `f64` build disagrees
//! with an `f32` one.

pub(crate) use crate::rapier2d::math::{Pose as Pose2, Rotation as Rotation2, Vector as Vector2};
pub(crate) use crate::rapier3d::math::{Pose, Real, Rotation, Vector};

/// A 3D vector from the three numbers a script or a scene wrote.
pub(crate) fn v3(x: f32, y: f32, z: f32) -> Vector {
    Vector::new(Real::from(x), Real::from(y), Real::from(z))
}

/// The same, from the array a schema property parses to.
pub(crate) fn v3a(v: [f32; 3]) -> Vector {
    v3(v[0], v[1], v[2])
}

pub(crate) fn v2(x: f32, y: f32) -> Vector2 {
    Vector2::new(Real::from(x), Real::from(y))
}

pub(crate) fn v2a(v: [f32; 2]) -> Vector2 {
    v2(v[0], v[1])
}

/// A number on its way into rapier.
pub(crate) fn real(value: f32) -> Real {
    Real::from(value)
}

/// A number on its way back out, to a script or a `Transform`.
#[allow(
    clippy::cast_possible_truncation,
    reason = "the f64 build narrows here on purpose"
)]
pub(crate) fn f32_of(value: Real) -> f32 {
    value as f32
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

#[cfg(not(feature = "f64"))]
pub(crate) fn rotation_of(rotation: glamx::Quat) -> Rotation {
    rotation
}

#[cfg(feature = "f64")]
pub(crate) fn rotation_of(rotation: glamx::Quat) -> Rotation {
    Rotation::from_xyzw(
        Real::from(rotation.x),
        Real::from(rotation.y),
        Real::from(rotation.z),
        Real::from(rotation.w),
    )
}

/// The engine's `f32` rotation, from rapier's.
#[cfg(not(feature = "f64"))]
pub(crate) fn quat_of(rotation: Rotation) -> glamx::Quat {
    rotation
}

#[cfg(feature = "f64")]
pub(crate) fn quat_of(rotation: Rotation) -> glamx::Quat {
    glamx::Quat::from_xyzw(
        rotation.x as f32,
        rotation.y as f32,
        rotation.z as f32,
        rotation.w as f32,
    )
}

/// The engine's `f32` position, from rapier's.
pub(crate) fn position_of(v: Vector) -> glamx::Vec3 {
    glamx::Vec3::new(f32_of(v.x), f32_of(v.y), f32_of(v.z))
}

/// A voxel coordinate. The integer width follows the scalar too: a f64 world
/// is big enough to want 64-bit cells.
pub(crate) fn cell(x: i32, y: i32, z: i32) -> crate::rapier3d::math::IVector {
    crate::rapier3d::math::IVector::new(x.into(), y.into(), z.into())
}
