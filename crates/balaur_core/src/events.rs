//! Named events between scripts, in the frame-scoped shape everything else
//! uses.
//!
//! `node.call` reaches a script that the caller already holds a node for,
//! which is the wrong shape for "a player died" — the thing that happened
//! does not know who cares. A subscriber records itself against a name, an
//! emitter names the event and a payload, and a core system delivers.
//!
//! **Delivery is a frame later, at one point in the frame.** The pump runs at
//! the top of `Stage::Update`, before the script tick, so everything emitted
//! during frame N reaches its subscribers at the start of frame N+1, in
//! emission order and then subscription order. Delivering inside the emitting
//! call would put a handler in the middle of another script's `update`, where
//! it could free the node being ticked.
//!
//! Nothing here is recorded for replay: an emit comes from a script, and a
//! replay re-runs the script, which emits again.

use balaur_script::{Bindings, BindingsExt as _, Value};

use crate::collections::DetHashMap;
use crate::engine::Engine;
use crate::hecs::Entity;

/// Who hears what, what is waiting, and what the last pump handed over.
#[derive(Default)]
pub struct EventState {
    /// Subscribers by event name, in subscription order, which is the order
    /// they are called in.
    listeners: DetHashMap<String, Vec<Entity>>,
    /// Emitted since the last pump, in emission order.
    queued: Vec<(String, Value)>,
    /// What the last pump delivered, until the next one replaces it. This is
    /// what `poll` reads, so asking and being called see the same frame.
    delivered: DetHashMap<String, Vec<Value>>,
}

/// Hear `name` on this node, through its script's `on_<name>` method.
///
/// Subscribing twice is once: a handler called twice for one event would be
/// a bug nobody writing the second call intended.
pub fn subscribe(eng: &Engine, entity: Entity, name: &str) {
    let state = eng.resource::<EventState>();
    let mut state = state.borrow_mut();
    let listeners = state.listeners.entry(name.to_string()).or_default();
    if !listeners.contains(&entity) {
        listeners.push(entity);
    }
}

pub fn unsubscribe(eng: &Engine, entity: Entity, name: &str) {
    let state = eng.resource::<EventState>();
    let mut state = state.borrow_mut();
    if let Some(listeners) = state.listeners.get_mut(name) {
        listeners.retain(|&e| e != entity);
    }
}

/// Queue an event for the next pump.
pub fn emit(eng: &Engine, name: &str, payload: Value) {
    let state = eng.resource::<EventState>();
    state.borrow_mut().queued.push((name.to_string(), payload));
}

/// What the last pump delivered under `name`, in emission order.
#[must_use]
pub fn delivered(eng: &Engine, name: &str) -> Vec<Value> {
    let state = eng.resource::<EventState>();
    let found = state.borrow().delivered.get(name).cloned();
    found.unwrap_or_default()
}

/// Deliver everything queued, then hold it for `poll` until the next pump.
///
/// Registered before the script tick, so a handler runs at a point in the
/// frame where nothing is mid-iteration. A subscriber whose node is gone is
/// dropped here rather than on free: this is the one place the list is
/// already being walked.
pub(crate) fn pump_system(eng: &Engine, _dt: f32) {
    let queued = {
        let state = eng.resource::<EventState>();
        let mut state = state.borrow_mut();
        state.delivered.clear();
        std::mem::take(&mut state.queued)
    };
    if queued.is_empty() {
        return;
    }
    let Some(host) = eng.script_host() else {
        return;
    };
    for (name, payload) in queued {
        let listeners = {
            let state = eng.resource::<EventState>();
            let mut state = state.borrow_mut();
            let world = eng.world();
            let listeners = state.listeners.entry(name.clone()).or_default();
            listeners.retain(|&e| world.contains(e));
            listeners.clone()
        };
        let method = format!("on_{name}");
        for entity in listeners {
            host.call_on(
                crate::node_id_of(entity),
                &method,
                std::slice::from_ref(&payload),
            );
        }
        let state = eng.resource::<EventState>();
        state
            .borrow_mut()
            .delivered
            .entry(name)
            .or_default()
            .push(payload);
    }
}

/// Declare `events.*`. Called from `engine_api::install_engine_api`, which is
/// where every other core module is declared.
pub fn install_events_api(m: &mut dyn Bindings<Engine>) {
    m.module_doc(
        "Named events between scripts. A node subscribes to a name and hears \
         it as its script's `on_<name>` method; anything may emit. Delivery is \
         at the top of the next frame's update, in emission then subscription \
         order, so a handler never runs inside the call that emitted. \
         `emitted` is the asking twin, for a script that would rather look \
         than declare a method.",
    );
    m.describe(&[
        (
            "subscribe",
            &[],
            "(node: node, name: string)",
            "Hear an event on this node, as its script's `on_<name>(payload)`. Subscribing twice is once.",
        ),
        (
            "unsubscribe",
            &[],
            "(node: node, name: string)",
            "Stop hearing an event on this node. Not an error when it was never subscribed.",
        ),
        (
            "emit",
            &[],
            "(name: string, payload: any?)",
            "Queue an event for every subscriber, delivered at the top of the next frame's update.",
        ),
        (
            "emitted",
            &[],
            "(name: string)",
            "The payloads delivered under this name this frame, in emission order; empty when none were.",
        ),
    ]);
    m.function(
        "subscribe",
        |eng: &Engine, (node, name): (balaur_script::NodeId, String)| {
            subscribe(eng, crate::entity_of(node)?, &name);
            Ok(())
        },
    );
    m.function(
        "unsubscribe",
        |eng: &Engine, (node, name): (balaur_script::NodeId, String)| {
            unsubscribe(eng, crate::entity_of(node)?, &name);
            Ok(())
        },
    );
    m.function(
        "emit",
        |eng: &Engine, (name, payload): (String, Option<Value>)| {
            emit(eng, &name, payload.unwrap_or(Value::Nil));
            Ok(())
        },
    );
    m.function("emitted", |eng: &Engine, name: String| {
        Ok(Value::List(delivered(eng, &name)))
    });
}
