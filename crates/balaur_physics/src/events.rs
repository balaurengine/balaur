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
//! **Re-entrancy.** A hook runs while the world is borrowed by the step. A
//! `physics3d` call from inside one gets an error saying so (see
//! [`crate::state_mut`]), never a `RefCell` panic.

use balaur_core::hecs::Entity;
use balaur_core::Engine;
use balaur_script::Value;
use rapier3d::prelude::{
    ColliderHandle, ColliderSet, CollisionEvent, ContactForceEvent, ContactModificationContext,
    EventHandler, PairFilterContext, PhysicsHooks, SolverFlags,
};
use std::cell::RefCell;

use crate::vocabulary::map;

/// One thing that happened, in Balaur's terms rather than rapier's handles.
pub(crate) enum Event {
    Started(Entity, Entity),
    Stopped(Entity, Entity),
    Force(Entity, Entity, f32, [f32; 3]),
}

impl Event {
    /// The pair, for sorting. Both sides are told, so the order within a pair
    /// does not matter; the order *between* pairs does.
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

/// Collects a step's events. Not `Sync`, which rapier allows because
/// `unsync-callbacks` is on — the whole reason a handler here can hold a
/// `RefCell` and, in [`Hooks`], an `&Engine`.
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

/// The entity behind a collider handle, from the id stored on it.
fn entity_of(colliders: &ColliderSet, handle: ColliderHandle) -> Option<Entity> {
    Entity::from_bits(colliders.get(handle)?.user_data as u64)
}

impl EventHandler for Collector {
    fn handle_collision_event(
        &self,
        _bodies: &rapier3d::prelude::RigidBodySet,
        colliders: &ColliderSet,
        event: CollisionEvent,
        _pair: Option<&rapier3d::prelude::ContactPair>,
    ) {
        let (h1, h2) = (event.collider1(), event.collider2());
        let (Some(a), Some(b)) = (entity_of(colliders, h1), entity_of(colliders, h2)) else {
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
        _dt: f32,
        _bodies: &rapier3d::prelude::RigidBodySet,
        colliders: &ColliderSet,
        pair: &rapier3d::prelude::ContactPair,
        total_force_magnitude: f32,
    ) {
        let event = ContactForceEvent::from_contact_pair(_dt, pair, total_force_magnitude);
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
            event.total_force_magnitude,
            [d.x, d.y, d.z],
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
pub(crate) fn deliver(eng: &Engine, events: Vec<Event>) {
    for event in &events {
        dispatch(eng, event);
    }
}

/// The three questions rapier asks mid-step, forwarded to the node's script.
///
/// Only colliders that set `hooks` are asked, so a game that sets none pays
/// nothing: rapier checks a flag on the collider before it calls any of this.
pub(crate) struct Hooks<'a> {
    pub(crate) eng: &'a Engine,
}

impl Hooks<'_> {
    /// A hook's answer, or `None` when the node has no such method — which is
    /// the common case and must not be an error.
    fn ask(&self, context: &PairFilterContext<'_>, method: &str) -> Option<Value> {
        let host = self.eng.script_host()?;
        let a = entity_of(context.colliders, context.collider1)?;
        let b = entity_of(context.colliders, context.collider2)?;
        // Both sides may have opted in; the first answer that says no wins,
        // which is the same rule rapier applies to its own filters.
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
        // The built-in one-way platform, which is what `one_way` on a collider
        // means: rapier owns the maths, we own the axis.
        let axis = one_way_axis(context);
        if let Some(axis) = axis {
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
            let normal = *context.normal;
            let answer = host.call_on(
                balaur_core::node_id_of(node),
                "modify_contacts",
                &[
                    Value::Node(other.to_bits().get()),
                    map([
                        ("normal", Value::Vec3([normal.x, normal.y, normal.z])),
                        ("points", Value::Num(context.solver_contacts.len() as f64)),
                    ]),
                ],
            );
            apply_contact_answer(context, answer.as_ref());
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
fn one_way_axis(context: &ContactModificationContext<'_>) -> Option<glamx::Vec3> {
    let collider = context.colliders.get(context.collider1)?;
    crate::collider::decode_one_way(collider.user_data)
}

/// What a `modify_contacts` handler may change: friction, restitution, and
/// whether the contact happens at all.
fn apply_contact_answer(context: &mut ContactModificationContext<'_>, answer: Option<&Value>) {
    let Some(answer) = answer else { return };
    let opts = crate::vocabulary::Opts(Some(answer));
    // Friction and restitution are the pair's, not each contact point's:
    // rapier already combined the two materials before asking.
    if let Some(Value::Num(friction)) = opts.get("friction") {
        *context.friction = *friction as f32;
    }
    if let Some(Value::Num(restitution)) = opts.get("restitution") {
        *context.restitution = *restitution as f32;
    }
    if !opts.boolean("solid", true) {
        context.solver_contacts.clear();
    }
}
