//! The 2D half of `crate::events`: the same three questions and the same two
//! events, against rapier2d.
//!
//! Handlers are named the same as in 3D (`on_collision_start`,
//! `on_collision_stop`, `on_contact_force`) because a script author writing a
//! 2D game should not have to learn a second set of names — and no node has
//! both a `collider2d` and a `collider3d` in practice.

use crate::rapier2d::prelude::{
    ColliderHandle, ColliderSet, CollisionEvent, ContactForceEvent, ContactModificationContext,
    EventHandler, PhysicsHooks,
};
use balaur_core::Engine;
use balaur_core::hecs::Entity;
use balaur_script::Value;
use std::sync::Mutex;

pub(crate) enum Event {
    Started(Entity, Entity),
    Stopped(Entity, Entity),
    Force(Entity, Entity, f32, [f32; 2]),
}

impl Event {
    /// The pair and the kind: a threaded step raises these in no order, and
    /// one pair can carry both a `Started` and a `Force`.
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

    fn push(&self, event: Event) {
        self.events
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(event);
    }
}

fn entity_of(colliders: &ColliderSet, handle: ColliderHandle) -> Option<Entity> {
    Entity::from_bits(colliders.get(handle)?.user_data as u64)
}

impl EventHandler for Collector {
    fn handle_collision_event(
        &self,
        _bodies: &crate::rapier2d::prelude::RigidBodySet,
        colliders: &ColliderSet,
        event: CollisionEvent,
        _pair: Option<&crate::rapier2d::prelude::ContactPair>,
    ) {
        let (Some(a), Some(b)) = (
            entity_of(colliders, event.collider1()),
            entity_of(colliders, event.collider2()),
        ) else {
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
        _bodies: &crate::rapier2d::prelude::RigidBodySet,
        colliders: &ColliderSet,
        pair: &crate::rapier2d::prelude::ContactPair,
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
            crate::scalar::a2(d),
        ));
    }
}

pub(crate) fn deliver(eng: &Engine, events: &[Event]) {
    let Some(host) = eng.script_host() else {
        return;
    };
    let node = |e: Entity| Value::Node(e.to_bits().get());
    for event in events {
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
                let towards = Value::Vec2(direction);
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
}

/// The 2D twin of [`crate::events::Hooks`]: collider data, never a script.
pub(crate) struct Hooks;

impl PhysicsHooks for Hooks {
    fn modify_solver_contacts(&self, context: &mut ContactModificationContext<'_>) {
        if let Some(axis) = context
            .colliders
            .get(context.collider1)
            .and_then(|c| decode_one_way(c.user_data))
        {
            context.update_as_oneway_platform(axis, 0.1);
        }
    }
}

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
