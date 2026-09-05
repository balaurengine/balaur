//! The functions the 3D and 2D physics modules share to the letter, written
//! once and stamped into each dimension's module.
//!
//! rapier2d and rapier3d are separate crates with the same vocabulary, so a
//! function over `Collider`, `RigidBodyHandle` or `QueryFilter` is the same
//! text in both dimensions and differs only in which crate the names resolve
//! to. Each `functions!` macro expands at the invocation site, where that
//! module's own imports decide; its named parameters carry the handful of
//! names that are spelled differently in 2D.

pub(crate) mod body;
pub(crate) mod character;
pub(crate) mod collider;
pub(crate) mod joint;
pub(crate) mod query;
pub(crate) mod world;
