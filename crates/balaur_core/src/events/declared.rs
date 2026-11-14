//! Every event name the engine announces, as a tool reads them: what
//! `balaur check` holds a handler, a listener and a row against, and what the
//! generated hooks page lists. A plugin names what it says where it says it,
//! so a renamed event is renamed here by the same constant.

use crate::collections::DetHashSet;
use crate::engine::Engine;

use super::{ASSETS_RELOADED, CHANGED, DEBUGGER_RESUMED, DEBUGGER_STOPPED, FILES_CHANGED, LOGGED};

/// An event said from no node: its name, what it carries, and when it is
/// said. Only `events::listen` and `events::next` hear one.
pub type Said = (&'static str, &'static str, &'static str);

/// An event any node may announce, whatever its components: its name and
/// what it carries.
pub type Announced = (&'static str, &'static str);

/// The module a script calls `listen`, `next` and `emit` in.
pub const MODULE: &str = "events";

/// The functions of [`MODULE`] that hear an event by its name, and which
/// argument the name is.
pub const HEARING: &[(&str, usize)] = &[(super::LISTEN, 1), (super::NEXT, 0)];

/// What sends an event by its name, the first argument: a function of
/// [`MODULE`] from no node, and a node's method from that node.
pub const SENDING: &str = super::EMIT;

/// What a module's own event is named with, after the module:
/// `multiplayer_event`, which a socket's node hears as `on_multiplayer_event`.
pub const MODULE_EVENT_SUFFIX: &str = "_event";

/// What core itself says from no node.
#[rustfmt::skip]
pub const SAID: &[Said] = &[
    (LOGGED, "the log's running count", "Lines were logged since the last frame."),
    (ASSETS_RELOADED, "the asset cache's generation", "A file the asset cache holds was reloaded or declared stale."),
    (FILES_CHANGED, "the paths, as a list; empty when the watcher lost count", "A burst of file writes under a directory `engine::watch` follows has ended."),
    (DEBUGGER_STOPPED, "where a script is stopped", "A script stopped, or stopped somewhere else."),
    (DEBUGGER_RESUMED, "nil", "The script the debugger last said was stopped went on."),
    (CHANGED, "a list of `{ node, kind, .. }` maps", "A write changed the scene since the last frame; heard from a node, the entries under it alone."),
];

/// What the plugins of this run declared beside core's own.
#[derive(Default)]
pub struct Declared {
    said: Vec<Said>,
    on_nodes: Vec<Announced>,
}

fn held(eng: &Engine) -> std::rc::Rc<std::cell::RefCell<Declared>> {
    if eng.try_resource::<Declared>().is_none() {
        eng.insert_resource(Declared::default());
    }
    eng.resource::<Declared>()
}

/// Name the events a plugin says from no node. Declaring one twice is once.
pub fn declare(eng: &Engine, events: &[Said]) {
    let held = held(eng);
    let mut held = held.borrow_mut();
    for event in events {
        if !held.said.iter().any(|(name, ..)| *name == event.0) {
            held.said.push(*event);
        }
    }
}

/// Name the events a plugin announces on a node that carries no component of
/// its own for them: a tween runs on any node.
pub fn declare_on_nodes(eng: &Engine, events: &[Announced]) {
    let held = held(eng);
    let mut held = held.borrow_mut();
    for event in events {
        if !held.on_nodes.iter().any(|(name, _)| *name == event.0) {
            held.on_nodes.push(*event);
        }
    }
}

/// Every event said from no node, core's first, then in the order declared.
#[must_use]
pub fn said(eng: &Engine) -> Vec<Said> {
    let mut all = SAID.to_vec();
    if let Some(held) = eng.try_resource::<Declared>() {
        all.extend(held.borrow().said.iter().copied());
    }
    all
}

/// Every event any node may announce: [`crate::node_api::NODE_EVENTS`], then
/// what the plugins declared.
#[must_use]
pub fn on_nodes(eng: &Engine) -> Vec<Announced> {
    let mut all = crate::node_api::NODE_EVENTS.to_vec();
    if let Some(held) = eng.try_resource::<Declared>() {
        all.extend(held.borrow().on_nodes.iter().copied());
    }
    all
}

/// Every name the engine announces, and every method it calls by name: what
/// a script may hear with no script of the project having declared it.
pub struct Names {
    /// An event's name: a core hook's, a component's, any node's, or one said
    /// from no node.
    pub events: DetHashSet<String>,
    /// A method the engine calls on a script as it is spelled: `init`,
    /// `on_free`, `on_paused_changed`.
    pub hooks: DetHashSet<String>,
}

impl Names {
    /// Whether the engine announces `event`.
    #[must_use]
    pub fn announces(&self, event: &str) -> bool {
        self.events.contains(event)
    }

    /// Whether the engine answers a method named `method` on a script: a hook
    /// it calls by that name, or `on_<event>` for an event it announces.
    #[must_use]
    pub fn calls(&self, method: &str) -> bool {
        self.hooks.contains(method)
            || method
                .strip_prefix(HANDLER_PREFIX)
                .is_some_and(|event| self.announces(event))
    }
}

/// What a handler's name opens with: `on_<event>`.
pub const HANDLER_PREFIX: &str = "on_";

/// The engine's own names, read from where each is declared: the hooks,
/// every registered component's events, what any node announces and what is
/// said from no node.
#[must_use]
pub fn names(eng: &Engine) -> Names {
    let mut events: DetHashSet<String> = crate::hooks::BINDABLE
        .iter()
        .map(|name| (*name).to_string())
        .collect();
    events.extend(on_nodes(eng).iter().map(|(name, _)| (*name).to_string()));
    events.extend(said(eng).iter().map(|(name, ..)| (*name).to_string()));
    if let Some(registry) = eng.try_resource::<crate::components::ComponentRegistry>() {
        for (_, def) in registry.borrow().iter() {
            events.extend(def.events.iter().map(|(name, _)| (*name).to_string()));
        }
    }
    let hooks = crate::hooks::REFERENCE
        .iter()
        .map(|(name, ..)| (*name).to_string())
        .collect();
    Names { events, hooks }
}
