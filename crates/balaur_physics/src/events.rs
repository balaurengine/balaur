//! What the step tells a script: collisions, contact forces, and the three
//! questions rapier asks mid-step.
//!
//! Everything here is opt-in per collider (`events` and `hooks` on
//! `collider3d`), because rapier reports nothing by default and a game that
//! wants nothing should pay nothing.
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
    EventHandler, PhysicsHooks,
};
use balaur_core::Engine;
use balaur_core::hecs::Entity;
use balaur_script::Value;
use std::sync::Mutex;

/// One thing that happened, in Balaur's terms rather than rapier's handles.
pub(crate) enum Event {
    Started(Entity, Entity),
    Stopped(Entity, Entity),
    Force(Entity, Entity, f32, [f32; 3]),
}

impl Event {
    /// The pair and the kind, for sorting. Both sides are told, so the order
    /// within a pair does not matter; the order *between* events does, and a
    /// threaded step collects them in no particular order. The kind is in the
    /// key because one pair can raise a `Started` and a `Force` in one step.
    fn key(&self) -> (u64, u64, u8) {
        let (a, b, kind) = match self {
            Self::Started(a, b) => (*a, *b, 0),
            Self::Stopped(a, b) => (*a, *b, 1),
            Self::Force(a, b, _, _) => (*a, *b, 2),
        };
        (
            a.to_bits().get().min(b.to_bits().get()),
            a.to_bits().get().max(b.to_bits().get()),
            kind,
        )
    }
}

/// Collects a step's events, from whichever thread raised them. The order
/// they arrive in is not the order they are delivered in: [`Event::key`] is a
/// total order, and `take` sorts by it.
#[derive(Default)]
pub(crate) struct Collector {
    events: Mutex<Vec<Event>>,
}

impl Collector {
    pub(crate) fn take(self) -> Vec<Event> {
        let mut events = self
            .events
            .into_inner()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        events.sort_unstable_by_key(Event::key);
        events
    }
}

/// The entity behind a collider handle, from the id stored on it.
fn entity_of(colliders: &ColliderSet, handle: ColliderHandle) -> Option<Entity> {
    Entity::from_bits(colliders.get(handle)?.user_data as u64)
}

impl Collector {
    fn push(&self, event: Event) {
        self.events
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(event);
    }
}

impl EventHandler for Collector {
    fn handle_collision_event(
        &self,
        _bodies: &crate::rapier3d::prelude::RigidBodySet,
        colliders: &ColliderSet,
        event: CollisionEvent,
        _pair: Option<&crate::rapier3d::prelude::ContactPair>,
    ) {
        let (h1, h2) = (event.collider1(), event.collider2());
        let (Some(a), Some(b)) = (entity_of(colliders, h1), entity_of(colliders, h2)) else {
            return;
        };
        self.push(if event.started() {
            Event::Started(a, b)
        } else {
            Event::Stopped(a, b)
        });
    }

    fn handle_contact_force_event(
        &self,
        dt: crate::scalar::Real,
        _bodies: &crate::rapier3d::prelude::RigidBodySet,
        colliders: &ColliderSet,
        pair: &crate::rapier3d::prelude::ContactPair,
        total_force_magnitude: crate::scalar::Real,
    ) {
        let event = ContactForceEvent::from_contact_pair(dt, pair, total_force_magnitude);
        let (Some(a), Some(b)) = (
            entity_of(colliders, event.collider1),
            entity_of(colliders, event.collider2),
        ) else {
            return;
        };
        let d = event.max_force_direction;
        self.push(Event::Force(
            a,
            b,
            crate::scalar::f32_of(event.total_force_magnitude),
            crate::scalar::a3(d),
        ));
    }
}

/// The method a script implements for each event, and the arguments it gets.
fn dispatch(eng: &Engine, event: &Event) {
    let Some(host) = eng.script_host() else {
        return;
    };
    let node = |e: Entity| Value::Node(e.to_bits().get());
    match *event {
        Event::Started(a, b) => {
            host.call_on(balaur_core::node_id_of(a), "on_collision_start", &[node(b)]);
            host.call_on(balaur_core::node_id_of(b), "on_collision_start", &[node(a)]);
        }
        Event::Stopped(a, b) => {
            host.call_on(balaur_core::node_id_of(a), "on_collision_stop", &[node(b)]);
            host.call_on(balaur_core::node_id_of(b), "on_collision_stop", &[node(a)]);
        }
        Event::Force(a, b, magnitude, direction) => {
            let force = Value::Num(f64::from(magnitude));
            let towards = Value::Vec3(direction);
            host.call_on(
                balaur_core::node_id_of(a),
                "on_contact_force",
                &[node(b), force.clone(), towards.clone()],
            );
            host.call_on(
                balaur_core::node_id_of(b),
                "on_contact_force",
                &[node(a), force, towards],
            );
        }
    }
}

/// Hand a step's events to the scripts that asked for them.
///
/// Called with `PhysicsState` **not** borrowed: a handler is ordinary script
/// code and may do anything, including move the body it was told about.
pub(crate) fn deliver(eng: &Engine, events: &[Event]) {
    for event in events {
        dispatch(eng, event);
    }
}

/// The one mid-step rule left, and it reads collider data rather than calling
/// a script.
///
/// A hook runs on rapier's own threads, which is why it may not touch the
/// `Engine`: that is what keeps `unsync-callbacks` off and the solver threaded.
pub(crate) struct Hooks;

impl PhysicsHooks for Hooks {
    fn modify_solver_contacts(&self, context: &mut ContactModificationContext<'_>) {
        // The one-way platform, which is what `one_way` on a collider means:
        // rapier owns the maths, we own the axis.
        if let Some(axis) = one_way_axis(context) {
            context.update_as_oneway_platform(axis, 0.1);
        }
    }
}

/// The direction a one-way platform lets bodies through from, when
/// `collider1` is one.
///
/// The axis rides in the high bits of the collider's `user_data`, beside the
/// entity id: a hook runs while `PhysicsState` is borrowed by the step, so it
/// cannot go and read the component. Six directions rather than a vector,
/// because a platform's axis is a cardinal one in every game that has ever
/// wanted this, and the encoding costs three bits.
fn one_way_axis(context: &ContactModificationContext<'_>) -> Option<crate::rapier3d::math::Vector> {
    let collider = context.colliders.get(context.collider1)?;
    crate::collider::decode_one_way(collider.user_data)
}
