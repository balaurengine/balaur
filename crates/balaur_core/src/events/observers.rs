//! Rust code run the moment an event is announced (PLAN-ecs §2, Bevy's
//! observers): on one node, or on any. An event declared to bubble is carried
//! up the tree after the node's own observers, to each ancestor's in turn,
//! until one stops it. Script listeners keep their delivery at the next pump.

use std::cell::Cell;
use std::rc::Rc;

use balaur_script::Value;
use smol_str::SmolStr;

use crate::collections::{DetHashMap, DetHashSet};
use crate::engine::Engine;
use crate::hecs::Entity;

/// What an observer is handed.
pub struct Observed<'a> {
    /// The event's name.
    pub name: &'a str,
    /// The node it was announced on.
    pub target: Entity,
    /// The node whose observers are running: `target`, or an ancestor of it
    /// while the event bubbles; `target` for an observer of any node.
    pub at: Entity,
    pub payload: &'a Value,
    stopped: Cell<bool>,
}

impl Observed<'_> {
    /// Carry the event no further up the tree: the observers at this node
    /// still run, and the observers of any node still run once.
    pub fn stop(&self) {
        self.stopped.set(true);
    }
}

/// One observer.
pub type ObserverFn = Rc<dyn Fn(&Engine, &Observed<'_>)>;

/// Names one observer, to take it away again.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ObserverId(u64);

/// The observers of one event name on one node, in the order added.
type Held = Vec<(ObserverId, ObserverFn)>;

/// Every observer by event name and node (`None`: any node), in the order
/// added, and the names that bubble.
#[derive(Default)]
pub struct Observers {
    by_name: DetHashMap<(SmolStr, Option<Entity>), Held>,
    next: u64,
    bubbling: DetHashSet<SmolStr>,
}

/// Whether an observer waits for `name` on `on`, or, for `None`, on any node.
pub(super) fn watched(eng: &Engine, on: Option<Entity>, name: &str) -> bool {
    eng.try_resource::<Observers>().is_some_and(|held| {
        held.borrow()
            .by_name
            .get(&(SmolStr::new(name), on))
            .is_some_and(|list| !list.is_empty())
    })
}

/// Run `observer` each time `name` is announced on `on`, or on any node for
/// `None`, before the event's rows and hooks.
pub fn observe(
    eng: &Engine,
    name: &str,
    on: Option<Entity>,
    observer: impl Fn(&Engine, &Observed<'_>) + 'static,
) -> ObserverId {
    let observers = observers(eng);
    let mut observers = observers.borrow_mut();
    let id = ObserverId(observers.next);
    observers.next += 1;
    observers
        .by_name
        .entry((name.into(), on))
        .or_default()
        .push((id, Rc::new(observer)));
    id
}

/// Take an observer away; one already gone is nothing to take.
pub fn unobserve(eng: &Engine, id: ObserverId) {
    let Some(observers) = eng.try_resource::<Observers>() else {
        return;
    };
    let mut observers = observers.borrow_mut();
    observers.by_name.retain(|_, list| {
        list.retain(|(held, _)| *held != id);
        !list.is_empty()
    });
}

/// Declare that `name` bubbles: announced on a node, it is carried up to the
/// observers of each ancestor in turn.
pub fn bubble(eng: &Engine, name: &str) {
    observers(eng).borrow_mut().bubbling.insert(name.into());
}

/// Drop the observers of nodes that are gone, with the pump's sweep of
/// listeners.
pub(crate) fn sweep(eng: &Engine) {
    let Some(observers) = eng.try_resource::<Observers>() else {
        return;
    };
    let world = eng.world();
    observers
        .borrow_mut()
        .by_name
        .retain(|(_, on), _| on.is_none_or(|on| world.contains(on)));
}

fn observers(eng: &Engine) -> Rc<std::cell::RefCell<Observers>> {
    if eng.try_resource::<Observers>().is_none() {
        eng.insert_resource(Observers::default());
    }
    eng.resource::<Observers>()
}

/// Run every observer of `name` announced on `target`: the node's own, each
/// ancestor's while it bubbles and nobody stopped it, then any node's. An
/// observer added while it runs hears the next event, not this one.
pub(crate) fn run(eng: &Engine, target: Entity, name: &str, payload: &Value) {
    let Some(observers) = eng.try_resource::<Observers>() else {
        return;
    };
    // Everything that will hear it, taken now: an observer added while this
    // event runs hears the next one.
    let (chain, any) = {
        let observers = observers.borrow();
        let bubbles = observers.bubbling.contains(name);
        let mut chain = Vec::new();
        let mut at = Some(target);
        let world = eng.world();
        while let Some(node) = at {
            let here = held(&observers, name, Some(node));
            if !here.is_empty() {
                chain.push((node, here));
            }
            at = bubbles
                .then(|| {
                    world
                        .get::<&crate::scene::Parent>(node)
                        .ok()
                        .map(|parent| parent.0)
                })
                .flatten();
        }
        (chain, held(&observers, name, None))
    };
    if chain.is_empty() && any.is_empty() {
        return;
    }
    let event = Observed {
        name,
        target,
        at: target,
        payload,
        stopped: Cell::new(false),
    };
    for (node, found) in chain {
        let here = Observed {
            at: node,
            stopped: Cell::new(false),
            ..event
        };
        for observer in found {
            observer(eng, &here);
        }
        if here.stopped.get() {
            break;
        }
    }
    for observer in any {
        observer(eng, &event);
    }
}

/// The observers of `name` on `on`, copied out so each runs with nothing
/// borrowed: an observer may observe, announce or free.
fn held(observers: &Observers, name: &str, on: Option<Entity>) -> Vec<ObserverFn> {
    observers
        .by_name
        .get(&(SmolStr::new(name), on))
        .map(|list| list.iter().map(|(_, observer)| observer.clone()).collect())
        .unwrap_or_default()
}
