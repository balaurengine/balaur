//! The 2D half of `crate::events`: the same three questions and the same two
//! events, against rapier2d.
//!
//! Handlers are named the same as in 3D (`on_collision_enter`,
//! `on_collision_exit`, `on_contact_force`) because a script author writing a
//! 2D game should not have to learn a second set of names — and no node has
//! both a `collider2d` and a `collider3d` in practice.

use crate::rapier2d::prelude::{
    ColliderHandle, ColliderSet, CollisionEvent, ContactForceEvent, ContactModificationContext,
    ContactPair, EventHandler, PhysicsHooks, RigidBodySet, SoftBodySet, SoftBodyTearEvent,
};
use crate::shared::events::Owner;
use crate::vocabulary::hook;
use crate::vocabulary::keys as k;
use balaur_core::Engine;
use balaur_core::collections::DetHashMap;
use balaur_core::hecs::Entity;
use balaur_script::Value;
use std::sync::Mutex;

crate::shared::events::functions!(
    dimensions = 2,
    towards = Vec2,
    axis = a2,
    normal = crate::rapier2d::math::Vector,
    state = crate::dim2::PhysicsState2d,
    tear_record = crate::dim2::softbody::tear_record
);
