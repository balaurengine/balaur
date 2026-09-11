//! Named events between scripts, in the frame-scoped shape everything else
//! uses.
//!
//! `node.call` reaches a script that the caller already holds a node for,
//! which is the wrong shape for "a player died" — the thing that happened
//! does not know who cares. A subscriber records itself against a name, an
//! emitter names the event and a payload, and a core system delivers.
//!
//! **An emitter may be named on both ends.** `node.emit` says the event came
//! from that node, and `subscribe` takes the node whose events it wants; a
//! subscription with no emitter hears the name from anyone. Naming one is
//! what keeps a hundred emitters of `hit` from waking a hundred handlers
//! apiece: the emitter is part of the key, so delivery is a lookup rather
//! than a scan.
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

use balaur_script::{Bindings, BindingsExt as _, NodeId, Value};
use smol_str::SmolStr;

use crate::collections::DetHashMap;
use crate::engine::Engine;
use crate::hecs::Entity;

/// A subscriber, and where it falls in subscription order.
///
/// The counter is handed out across every bucket rather than within one, so
/// merging the emitter's subscribers with the catch-all's restores the order
/// the subscriptions were actually made in. Handlers run in subscription
/// order is then one rule, not one rule and an exception for naming an
/// emitter.
struct Listener {
    entity: Entity,
    seq: u64,
}

/// Who hears what, what is waiting, and what the last pump handed over.
#[derive(Default)]
pub struct EventState {
    /// Subscribers by event name and the emitter they asked for, `None`
    /// being every emitter. Each bucket is in ascending `seq`, since a
    /// subscription only ever pushes.
    /// Keyed on an inline string: an event name is short, so a pump looks a
    /// bucket up without building a `String` to do it.
    listeners: DetHashMap<(SmolStr, Option<Entity>), Vec<Listener>>,
    /// Handed out in subscription order and never reused.
    next_seq: u64,
    /// Emitted since the last pump, in emission order.
    queued: Vec<(SmolStr, Option<Entity>, Value)>,
    /// What the last pump delivered, with the emitter each came from, until
    /// the next one replaces it. This is what `delivered` reads, so asking
    /// and being called see the same frame.
    delivered: DetHashMap<SmolStr, Vec<(Option<Entity>, Value)>>,
}

/// Hear `name` on this node, through its script's `on_<name>` method.
///
/// `from` is the node whose events to hear, or `None` for every emitter.
/// Subscribing twice is once: a handler called twice for one event would be
/// a bug nobody writing the second call intended. Subscribing to the same
/// name once with an emitter and once without is two subscriptions, and
/// deliberately so — the second is the catch-all.
pub fn subscribe(eng: &Engine, entity: Entity, name: &str, from: Option<Entity>) {
    let state = eng.resource::<EventState>();
    let mut state = state.borrow_mut();
    let seq = state.next_seq;
    let listeners = state.listeners.entry((name.into(), from)).or_default();
    if listeners.iter().any(|l| l.entity == entity) {
        return;
    }
    listeners.push(Listener { entity, seq });
    state.next_seq += 1;
}

/// Stop hearing `name` from `from`, which must be the emitter subscribed with.
pub fn unsubscribe(eng: &Engine, entity: Entity, name: &str, from: Option<Entity>) {
    let state = eng.resource::<EventState>();
    let mut state = state.borrow_mut();
    if let Some(listeners) = state.listeners.get_mut(&(name.into(), from)) {
        listeners.retain(|l| l.entity != entity);
    }
}

/// Queue an event from no particular node, for the next pump.
pub fn emit(eng: &Engine, name: &str, payload: Value) {
    queue(eng, name, None, payload);
}

/// Queue an event from `from`, for the next pump.
///
/// Reaches whoever subscribed to `name` on this node, and whoever subscribed
/// to `name` from anyone.
pub fn emit_from(eng: &Engine, from: Entity, name: &str, payload: Value) {
    queue(eng, name, Some(from), payload);
}

fn queue(eng: &Engine, name: &str, from: Option<Entity>, payload: Value) {
    let state = eng.resource::<EventState>();
    state.borrow_mut().queued.push((name.into(), from, payload));
}

/// What the last pump delivered under `name`, whoever emitted it, in
/// emission order.
#[must_use]
pub fn delivered(eng: &Engine, name: &str) -> Vec<Value> {
    let state = eng.resource::<EventState>();
    let found = state.borrow().delivered.get(name).cloned();
    found
        .unwrap_or_default()
        .into_iter()
        .map(|(_, payload)| payload)
        .collect()
}

/// What the last pump delivered under `name` from `from`, in emission order.
#[must_use]
pub fn delivered_from(eng: &Engine, from: Entity, name: &str) -> Vec<Value> {
    let state = eng.resource::<EventState>();
    let found = state.borrow().delivered.get(name).cloned();
    found
        .unwrap_or_default()
        .into_iter()
        .filter(|(emitter, _)| *emitter == Some(from))
        .map(|(_, payload)| payload)
        .collect()
}

/// Deliver everything queued, then hold it for `delivered` until the next pump.
///
/// Registered before the script tick, so a handler runs at a point in the
/// frame where nothing is mid-iteration.
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
    // The emitter's own `emitted:<name>` rows first, the way a node's
    // bindings run before its script: they need no script to be heard.
    for (name, from, payload) in &queued {
        if let Some(from) = from {
            let event = format!("{}{name}", crate::hooks::EMITTED);
            crate::bindings::fire(eng, *from, &event, std::slice::from_ref(payload));
        }
    }
    let Some(host) = eng.script_host() else {
        return;
    };
    sweep(eng);
    for (name, from, payload) in queued {
        let listeners = {
            let state = eng.resource::<EventState>();
            let state = state.borrow();
            // The emitter's own subscribers and the catch-all's, merged on
            // the sequence they subscribed at. Both buckets ascend already,
            // so this is a walk, not a sort.
            let mut targeted = from
                .and_then(|from| state.listeners.get(&(name.clone(), Some(from))))
                .map_or(&[][..], Vec::as_slice)
                .iter()
                .peekable();
            let mut any = state
                .listeners
                .get(&(name.clone(), None))
                .map_or(&[][..], Vec::as_slice)
                .iter()
                .peekable();
            let mut merged = Vec::with_capacity(targeted.len() + any.len());
            loop {
                let next = match (targeted.peek(), any.peek()) {
                    (Some(t), Some(a)) if t.seq <= a.seq => targeted.next(),
                    (Some(_), None) => targeted.next(),
                    (None, None) => break,
                    _ => any.next(),
                };
                if let Some(listener) = next {
                    merged.push(listener.entity);
                }
            }
            merged
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
            .push((from, payload));
    }
}

/// Drop subscribers that have been freed, and buckets whose emitter has.
///
/// A freed emitter's bucket is inert rather than wrong — an entity id carries
/// a generation, so a recycled slot never matches the dead one — but nothing
/// would ever empty it, and a game that spawns and subscribes all session
/// would grow one bucket per emitter it outlived. Sweeping here costs a walk
/// of a map that is normally tiny, on frames that are delivering anyway.
fn sweep(eng: &Engine) {
    let state = eng.resource::<EventState>();
    let mut state = state.borrow_mut();
    let world = eng.world();
    state.listeners.retain(|(_, from), listeners| {
        if from.is_some_and(|from| !world.contains(from)) {
            return false;
        }
        listeners.retain(|l| world.contains(l.entity));
        !listeners.is_empty()
    });
}

/// Declare `events.*`. Called from `engine_api::install_engine_api`, which is
/// where every other core module is declared.
pub fn install_events_api(m: &mut dyn Bindings<Engine>) {
    m.module_doc(
        "Named events between scripts. A node subscribes to a name — from one \
         emitter, or from anyone — and hears it as its script's `on_<name>` \
         method; `node.emit` emits from a node and `events.emit` from no one \
         in particular. Delivery is at the top of the next frame's update, in \
         emission then subscription order, so a handler never runs inside the \
         call that emitted. `emitted` and `emitted_from` are the asking twins, \
         for a script that would rather look than declare a method.",
    );
    m.describe(&[
        (
            "subscribe",
            &[],
            "(node: node, name: string, from: node?)",
            "Hear an event on this node, as its script's `on_<name>(payload)`. Pass the node whose events to hear, or leave it out for every emitter. Subscribing twice is once.",
        ),
        (
            "unsubscribe",
            &[],
            "(node: node, name: string, from: node?)",
            "Stop hearing an event on this node, `from` being the emitter it was subscribed with. Not an error when it was never subscribed.",
        ),
        (
            "emit",
            &[],
            "(name: string, payload: any?)",
            "Queue an event from no particular node, delivered at the top of the next frame's update to whoever subscribed to the name from anyone.",
        ),
        (
            "emitted",
            &[],
            "(name: string)",
            "The payloads delivered under this name this frame whoever emitted them, in emission order; empty when none were.",
        ),
        (
            "emitted_from",
            &[],
            "(node: node, name: string)",
            "The payloads delivered under this name this frame from that node, in emission order; empty when none were.",
        ),
    ]);
    m.function(
        "subscribe",
        |eng: &Engine, (node, name, from): (NodeId, String, Option<NodeId>)| {
            let from = from.map(crate::entity_of).transpose()?;
            subscribe(eng, crate::entity_of(node)?, &name, from);
            Ok(())
        },
    );
    m.function(
        "unsubscribe",
        |eng: &Engine, (node, name, from): (NodeId, String, Option<NodeId>)| {
            let from = from.map(crate::entity_of).transpose()?;
            unsubscribe(eng, crate::entity_of(node)?, &name, from);
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
    m.function(
        "emitted_from",
        |eng: &Engine, (node, name): (NodeId, String)| {
            Ok(Value::List(delivered_from(
                eng,
                crate::entity_of(node)?,
                &name,
            )))
        },
    );
}
