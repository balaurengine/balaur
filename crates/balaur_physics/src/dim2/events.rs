//! The 2D half of `crate::events`: the same three questions and the same two
//! events, against rapier2d.
//!
//! Handlers are named the same as in 3D (`on_collision_start`,
//! `on_collision_stop`, `on_contact_force`) because a script author writing a
//! 2D game should not have to learn a second set of names — and no node has
//! both a `collider2d` and a `collider3d` in practice.

use crate::rapier2d::prelude::{
    ColliderHandle, ColliderSet, CollisionEvent, ContactForceEvent, ContactModificationContext,
    EventHandler, PairFilterContext, PhysicsHooks, SolverFlags,
};
use balaur_core::hecs::Entity;
use balaur_core::Engine;
use balaur_script::Value;
use std::cell::RefCell;

pub(crate) enum Event {
    Started(Entity, Entity),
    Stopped(Entity, Entity),
    Force(Entity, Entity, f32, [f32; 2]),
}

impl Event {
    fn key(&self) -> (u64, u64) {
        let (a, b) = match self {
            Self::Started(a, b) | Self::Stopped(a, b) | Self::Force(a, b, _, _) => (*a, *b),
        };
        (
            a.to_bits().get().min(b.to_bits().get()),
            a.to_bits().get().max(b.to_bits().get()),
        )
    }
}

#[derive(Default)]
pub(crate) struct Collector {
    events: RefCell<Vec<Event>>,
}

impl Collector {
    pub(crate) fn take(self) -> Vec<Event> {
        let mut events = self.events.into_inner();
        events.sort_by_key(Event::key);
        events
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
        self.events.borrow_mut().push(if event.started() {
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
        self.events.borrow_mut().push(Event::Force(
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

pub(crate) struct Hooks<'a> {
    pub(crate) eng: &'a Engine,
}

impl Hooks<'_> {
    fn ask(&self, context: &PairFilterContext<'_>, method: &str) -> Option<Value> {
        let host = self.eng.script_host()?;
        let a = entity_of(context.colliders, context.collider1)?;
        let b = entity_of(context.colliders, context.collider2)?;
        for (node, other) in [(a, b), (b, a)] {
            let answer = host.call_on(
                balaur_core::node_id_of(node),
                method,
                &[Value::Node(other.to_bits().get())],
            );
            if matches!(answer, Some(Value::Bool(false))) {
                return Some(Value::Bool(false));
            }
        }
        None
    }
}

impl PhysicsHooks for Hooks<'_> {
    fn filter_contact_pair(&self, context: &PairFilterContext<'_>) -> Option<SolverFlags> {
        match self.ask(context, "filter_contact") {
            Some(Value::Bool(false)) => None,
            _ => Some(SolverFlags::COMPUTE_IMPULSES),
        }
    }

    fn filter_intersection_pair(&self, context: &PairFilterContext<'_>) -> bool {
        !matches!(
            self.ask(context, "filter_overlap"),
            Some(Value::Bool(false))
        )
    }

    fn modify_solver_contacts(&self, context: &mut ContactModificationContext<'_>) {
        if let Some(axis) = context
            .colliders
            .get(context.collider1)
            .and_then(|c| decode_one_way(c.user_data))
        {
            context.update_as_oneway_platform(axis, 0.1);
        }
        let Some(host) = self.eng.script_host() else {
            return;
        };
        let (Some(a), Some(b)) = (
            entity_of(context.colliders, context.collider1),
            entity_of(context.colliders, context.collider2),
        ) else {
            return;
        };
        for (node, other) in [(a, b), (b, a)] {
            let normal = crate::scalar::a2(*context.normal);
            let answer = host.call_on(
                balaur_core::node_id_of(node),
                "modify_contacts",
                &[
                    Value::Node(other.to_bits().get()),
                    crate::vocabulary::map([
                        ("normal", Value::Vec2(normal)),
                        ("points", Value::Num(context.solver_contacts.len() as f64)),
                    ]),
                ],
            );
            apply_contact_answer(context, answer.as_ref());
        }
    }
}

/// The 2D twin of `crate::events::apply_contact_answer`: what a
/// `modify_contacts` handler may change about a pair the solver is about to
/// see.
fn apply_contact_answer(context: &mut ContactModificationContext<'_>, answer: Option<&Value>) {
    let Some(answer) = answer else { return };
    let opts = crate::vocabulary::Opts(Some(answer));
    if let Some(Value::Num(friction)) = opts.get("friction") {
        *context.friction = *friction as crate::scalar::Real;
    }
    if let Some(Value::Num(restitution)) = opts.get("restitution") {
        *context.restitution = *restitution as crate::scalar::Real;
    }
    if !opts.boolean("solid", true) {
        context.solver_contacts.clear();
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
