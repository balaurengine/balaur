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

/// One event waiting for the pump.
struct Queued {
    name: SmolStr,
    from: Option<Entity>,
    payload: Value,
    /// Sent by [`announce`], which already ran the emitter's rows and its own
    /// hook, so the pump skips both.
    announced: bool,
    /// Said for the emitter's own subscribers alone: the catch-all hears a
    /// whole one of its own (`changed` under a root).
    scoped: bool,
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
    queued: Vec<Queued>,
    /// What the last pump delivered, with the emitter each came from, until
    /// the next one replaces it. This is what `delivered` reads, so asking
    /// and being called see the same frame.
    delivered: DetHashMap<SmolStr, Vec<(Option<Entity>, Value)>>,
    /// Tokens parked on the next delivery of a name, from one emitter or,
    /// `None`, from any: what a script's `await` on an event is.
    waiters: Vec<(SmolStr, Option<Entity>, u64)>,
    /// The log's running count at the last pump, for [`LOGGED`].
    logs_seen: u64,
    /// The asset cache's generation at the last pump, for [`ASSETS_RELOADED`].
    assets_seen: u64,
    /// The save slots' write count at the last pump, for [`SAVED`].
    saves_seen: u64,
    /// Where a script was stopped at the last pump, for [`DEBUGGER_STOPPED`].
    stop_seen: Option<balaur_script::Pause>,
}

/// What the log says from no node when lines arrived since the last pump,
/// with its running count: what an editor's log view listens for instead of
/// reading the buffer each frame. Queued only while someone listens.
pub const LOGGED: &str = "logged";

/// What the asset cache says from no node when a file it holds was reloaded
/// or declared stale since the last pump, with its generation: what an
/// editor's file views listen for. Queued only while someone listens.
pub const ASSETS_RELOADED: &str = "assets_reloaded";

/// What the save slots say from no node when one was written or removed
/// since the last pump, with their running count: what an editor's view of
/// a game's saves listens for instead of listing the folder each frame.
/// Queued only while someone listens.
pub const SAVED: &str = "saved";

/// What the debugger says from no node when a script stopped, stopped
/// somewhere else or went on since the last pump, with where it is stopped
/// or nil: what an editor's debugger view listens for instead of asking. A
/// stop after a `resume` is new even where it is the same line again.
pub const DEBUGGER_STOPPED: &str = "debugger_stopped";

/// What the scene says from no node when a write changed it since the last
/// pump: a list of `{node, kind, ..}` maps, one for each node and component
/// however often it was written (`crate::changes`). `kind` is `component`
/// (with `component` and `properties`, nil for the whole component),
/// `attached` and `removed` (with `component`), `added`, `freed` and `moved` (with
/// `parent`), or `renamed` (with `was`). Kept only while someone listens,
/// from the frame after the first listener. Heard from a node, it is said
/// from that node with the entries under it alone: a tool editing one scene
/// hears that scene, not its own widgets.
pub const CHANGED: &str = "changed";

/// A token `task.wait` parks on until `name` is next delivered, from `from`
/// or from anyone, and wakes with that event's payload.
pub fn next(eng: &Engine, name: &str, from: Option<Entity>) -> u64 {
    let token = eng.next_token();
    let state = eng.resource::<EventState>();
    state.borrow_mut().waiters.push((name.into(), from, token));
    token
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

/// The nodes someone listens to `name` from, for an engine that says `name`
/// only where it is heard: a widget's `resized` is worth nothing unheard.
pub fn listened_on(eng: &Engine, name: &str) -> Vec<Entity> {
    eng.try_resource::<EventState>()
        .map_or_else(Vec::new, |state| {
            state
                .borrow()
                .listeners
                .keys()
                .filter(|(heard, _)| heard == name)
                .filter_map(|(_, from)| *from)
                .collect()
        })
}

/// Whether anyone listens to `name` from anyone.
pub fn heard(eng: &Engine, name: &str) -> bool {
    eng.try_resource::<EventState>().is_some_and(|state| {
        state
            .borrow()
            .listeners
            .contains_key(&(SmolStr::new(name), None))
    })
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
    state.borrow_mut().queued.push(Queued {
        name: name.into(),
        from,
        payload,
        announced: false,
        scoped: false,
    });
    // Delivered at the next pump, which a loop sleeping under
    // `low_processor` would otherwise not run until some input came.
    crate::wake::wake();
}

/// An engine event on a node, heard every way at once: the node's rows and
/// its own `on_<name>(payload)` now, its subscribers and awaiters at the next
/// pump.
///
/// A name in [`crate::hooks::BINDABLE`] runs the rows spelled with that name;
/// any other runs the `emitted:<name>` rows. Rows run before the hook, as they
/// do for a pointer event, so the script sees the world the rows left.
pub fn announce(eng: &Engine, entity: Entity, name: &str, payload: Value) {
    // Most nodes carry no rows, and spelling the row's event costs an allocation.
    if eng
        .world()
        .get::<&crate::bindings::Bindings>(entity)
        .is_ok()
    {
        crate::bindings::fire(
            eng,
            entity,
            &row_event(name),
            std::slice::from_ref(&payload),
        );
    }
    if let Some(host) = eng.script_host() {
        host.call_on(
            crate::node_id_of(entity),
            &crate::hooks::hook_of(name),
            std::slice::from_ref(&payload),
        );
    }
    // A bare engine, with no event system installed, has nobody to queue for.
    let Some(state) = eng.try_resource::<EventState>() else {
        return;
    };
    state.borrow_mut().queued.push(Queued {
        name: name.into(),
        from: Some(entity),
        payload,
        announced: true,
        scoped: false,
    });
    crate::wake::wake();
}

/// The binding event a row answers `name` with.
fn row_event(name: &str) -> String {
    if crate::hooks::BINDABLE.contains(&name) {
        name.to_string()
    } else {
        format!("{}{name}", crate::hooks::EMITTED)
    }
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
/// Forget the stop the last pump saw, as a `resume` lets it go: the next one
/// is heard even where a loop stops on the same line with the same locals.
pub(crate) fn resumed(eng: &Engine) {
    if let Some(state) = eng.try_resource::<EventState>() {
        state.borrow_mut().stop_seen = None;
    }
}

/// Queue `name` from no node with what `payload` makes, where anyone listens
/// for it from anyone: a change the engine only notices, which nobody need
/// pay for unheard.
fn say_if_heard(state: &mut EventState, name: &'static str, payload: impl FnOnce() -> Value) {
    if state
        .listeners
        .contains_key(&(SmolStr::new_static(name), None))
    {
        state.queued.push(Queued {
            name: name.into(),
            from: None,
            payload: payload(),
            announced: false,
            scoped: false,
        });
    }
}

fn count(n: u64) -> Value {
    Value::Int(i64::try_from(n).unwrap_or(i64::MAX))
}

/// What moved that nobody emitted: lines logged, a file reloaded, a script
/// stopped or let go, the scene written. Each is said once, at the pump
/// after it moved.
fn notice(eng: &Engine, state: &mut EventState, stop: Option<balaur_script::Pause>) {
    let logs = crate::logbuf::total();
    if logs != state.logs_seen {
        state.logs_seen = logs;
        say_if_heard(state, LOGGED, || count(logs));
    }
    let assets = crate::assets::generation(eng);
    if assets != state.assets_seen {
        state.assets_seen = assets;
        say_if_heard(state, ASSETS_RELOADED, || count(assets));
    }
    let saves = crate::save::generation();
    if saves != state.saves_seen {
        state.saves_seen = saves;
        say_if_heard(state, SAVED, || count(saves));
    }
    if stop != state.stop_seen {
        say_if_heard(state, DEBUGGER_STOPPED, || {
            stop.as_ref()
                .map_or(Value::Nil, balaur_script::Pause::to_value)
        });
        state.stop_seen = stop;
    }
    // Once for each scope it is heard from: all of it, or a root's.
    let scopes: Vec<Option<Entity>> = state
        .listeners
        .iter()
        .filter(|((name, _), heard)| name == CHANGED && !heard.is_empty())
        .map(|((_, from), _)| *from)
        .collect();
    for (scope, changed) in crate::changes::take(eng, &scopes) {
        state.queued.push(Queued {
            name: CHANGED.into(),
            from: scope,
            payload: changed,
            announced: false,
            scoped: scope.is_some(),
        });
    }
}

// Who hears `name` from `from`: the emitter's own subscribers and, unless the
// event was said for them alone, the catch-all's, merged on the sequence they
// subscribed at. Both buckets ascend already, so this is a walk, not a sort.
fn hearing(
    eng: &Engine,
    name: &SmolStr,
    from: Option<Entity>,
    announced: bool,
    scoped: bool,
) -> Vec<Entity> {
    let state = eng.resource::<EventState>();
    let state = state.borrow();
    let mut targeted = from
        .and_then(|from| state.listeners.get(&(name.clone(), Some(from))))
        .map_or(&[][..], Vec::as_slice)
        .iter()
        .peekable();
    let mut any = state
        .listeners
        .get(&(name.clone(), None))
        .filter(|_| !scoped)
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
        // An announced event already called the emitter's own hook.
        if let Some(listener) = next
            && !(announced && Some(listener.entity) == from)
        {
            merged.push(listener.entity);
        }
    }
    merged
}

pub(crate) fn pump_system(eng: &Engine, _dt: f32) {
    // Asked before the state is borrowed: the host keeps its own.
    let stop = eng.script_host().and_then(|host| host.paused());
    let queued = {
        let state = eng.resource::<EventState>();
        let mut state = state.borrow_mut();
        state.delivered.clear();
        notice(eng, &mut state, stop);
        std::mem::take(&mut state.queued)
    };
    if queued.is_empty() {
        return;
    }
    // The emitter's own `emitted:<name>` rows first, the way a node's
    // bindings run before its script: they need no script to be heard. A
    // core hook's name keeps its one spelling, so it has no `emitted:` rows.
    for event in &queued {
        if let Some(from) = event.from
            && !event.announced
            && !event.scoped
            && !crate::hooks::BINDABLE.contains(&event.name.as_str())
        {
            let row = format!("{}{}", crate::hooks::EMITTED, event.name);
            crate::bindings::fire(eng, from, &row, std::slice::from_ref(&event.payload));
        }
    }
    // Recorded with no script host too, so `delivered` answers a tool the same.
    let host = eng.script_host();
    sweep(eng);
    for Queued {
        name,
        from,
        payload,
        announced,
        scoped,
    } in queued
    {
        let listeners = hearing(eng, &name, from, announced, scoped);
        let method = format!("on_{name}");
        if let Some(host) = &host {
            for entity in listeners {
                host.call_on(
                    crate::node_id_of(entity),
                    &method,
                    std::slice::from_ref(&payload),
                );
            }
        }
        // Every task parked on this event resumes with it, once.
        let woken: Vec<u64> = if host.is_none() {
            Vec::new()
        } else {
            let state = eng.resource::<EventState>();
            let mut state = state.borrow_mut();
            let mut woken = Vec::new();
            state.waiters.retain(|(waited, emitter, token)| {
                let hit = *waited == name
                    && match emitter {
                        None => !scoped,
                        Some(e) => Some(*e) == from,
                    };
                if hit {
                    woken.push(*token);
                }
                !hit
            });
            woken
        };
        if let Some(host) = &host {
            for token in woken {
                host.wake(token, &payload);
            }
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
        "Named events between scripts. A node listens for a name and hears it as its `on_<name>` method; `node.emit` and `events.emit` send one.",
    );
    m.describe(&[
        (
            "listen",
            &[],
            "(node: node, name: string, from: node?)",
            "Hear an event on this node, as its script's `on_<name>(payload)`. Pass the node whose events to hear, or leave it out for every emitter. Listening twice is once.",
        ),
        (
            "stop_listening",
            &[],
            "(node: node, name: string, from: node?)",
            "Stop hearing an event on this node, `from` being the emitter it listened with. Not an error when it never listened.",
        ),
        (
            "emit",
            &[],
            "(name: string, payload: any?)",
            "Queue an event from no particular node, delivered at the top of the next frame's update to whoever listens for the name from anyone.",
        ),
        (
            "emitted",
            &[],
            "(name: string) -> list",
            "The payloads delivered under this name this frame whoever emitted them, in emission order; empty when none were.",
        ),
        (
            "emitted_from",
            &[],
            "(node: node, name: string) -> list",
            "The payloads delivered under this name this frame from that node, in emission order; empty when none were.",
        ),
        (
            "next",
            &[],
            "(name: string, from: node?) -> int",
            "A token for `task.wait` that wakes, with the payload, the next time the name is delivered from that node or, left out, from anyone: `let payload = task::wait(events::next(\"finished\", door)).await;`, a GDScript `await door.finished`.",
        ),
    ]);
    m.function(
        "listen",
        |eng: &Engine, (node, name, from): (NodeId, String, Option<NodeId>)| {
            let from = from.map(crate::entity_of).transpose()?;
            subscribe(eng, crate::entity_of(node)?, &name, from);
            Ok(())
        },
    );
    m.function(
        "stop_listening",
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
    m.function(
        "next",
        |eng: &Engine, (name, from): (String, Option<NodeId>)| {
            let from = from.map(crate::entity_of).transpose()?;
            Ok(i64::try_from(next(eng, &name, from)).unwrap_or(i64::MAX))
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
