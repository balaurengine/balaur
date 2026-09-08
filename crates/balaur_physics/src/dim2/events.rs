//! The 2D half of `crate::events`: the same three questions and the same two
//! events, against rapier2d.
//!
//! Handlers are named the same as in 3D (`on_collision_start`,
//! `on_collision_stop`, `on_contact_force`) because a script author writing a
//! 2D game should not have to learn a second set of names — and no node has
//! both a `collider2d` and a `collider3d` in practice.

use crate::rapier2d::prelude::{
    ColliderHandle, ColliderSet, CollisionEvent, ContactForceEvent, ContactModificationContext,
    ContactPair, EventHandler, PhysicsHooks, RigidBodySet,
};
use crate::vocabulary::hook;
use balaur_core::Engine;
use balaur_core::hecs::Entity;
use balaur_script::Value;
use std::sync::Mutex;

crate::shared::events::functions!(
    dimensions = 2,
    towards = Vec2,
    axis = a2,
    normal = crate::rapier2d::math::Vector,
    decode = decode_one_way
);

/// The 2D reading of the axis `crate::collider::encode_one_way` packed: the
/// same three bits, two of the six directions unused.
fn decode_one_way(user_data: u128) -> Option<crate::rapier2d::math::Vector> {
    let code = ((user_data >> 64) & 0b111) as u8;
    if code == 0 {
        return None;
    }
    let sign: crate::scalar::Real = if (code - 1) % 2 == 1 { -1.0 } else { 1.0 };
    Some(match (code - 1) / 2 {
        0 => crate::rapier2d::math::Vector::new(sign, 0.0),
        _ => crate::rapier2d::math::Vector::new(0.0, sign),
    })
}
