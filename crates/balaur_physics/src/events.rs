//! What the step tells a script: collisions, contact forces, and the three
//! questions rapier asks mid-step.
//!
//! A collider or soft body reports what something hears: a script's hook, a row
//! or a listener, on its node or the body it hangs from. Rapier raises nothing
//! else, so a game that hears nothing pays nothing.
//!
//! **When.** Events are drained inside the fixed step, immediately after
//! `world.step()` and before the next `fixed_update` — so an impulse a handler
//! applies lands on the step it was meant for, on every machine.
//!
//! **Order.** Sorted by the two entities' bits before dispatch. Rapier's own
//! order follows its broad phase, and a replay may not depend on that.
//!
//! **Re-entrancy.** A hook runs while the world is borrowed by the step, so a
//! `physics3d` call from inside one gets an error saying so rather than a
//! `RefCell` panic.

use crate::rapier3d::prelude::{
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
    dimensions = 3,
    towards = Vec3,
    axis = a3,
    normal = crate::rapier3d::math::Vector,
    state = crate::PhysicsState3d,
    tear_record = crate::softbody::tear_record
);
