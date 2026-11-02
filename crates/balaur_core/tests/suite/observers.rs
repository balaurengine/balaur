//! Observers (PLAN-ecs §2): Rust run the moment an event is announced, on a
//! node or any, carried up the tree for a name declared to bubble.

use std::cell::RefCell;
use std::rc::Rc;

use balaur_core::events::{self, Observed};
use balaur_core::{App, AppConfig, Engine};
use balaur_script::Value;
use hecs::Entity;

type Heard = Rc<RefCell<Vec<(String, Entity, Entity)>>>;

/// A tree root > `World` > `Ship` > `Mast`.
fn tree() -> (App, Entity, Entity, Entity) {
    let app = App::new(AppConfig::bare(".")).unwrap();
    let world = app.engine.spawn_node("World", app.engine.root());
    let ship = app.engine.spawn_node("Ship", world);
    let mast = app.engine.spawn_node("Mast", ship);
    (app, world, ship, mast)
}

fn listen(eng: &Engine, heard: &Heard, tag: &str, name: &str, on: Option<Entity>) {
    let heard = heard.clone();
    let tag = tag.to_owned();
    events::observe(eng, name, on, move |_, event: &Observed<'_>| {
        heard
            .borrow_mut()
            .push((tag.clone(), event.target, event.at));
    });
}

#[test]
fn an_observer_runs_at_once_on_its_node() {
    let (app, _, ship, mast) = tree();
    let heard = Heard::default();
    listen(&app.engine, &heard, "ship", "hit", Some(ship));
    events::announce(&app.engine, mast, "hit", Value::Nil);
    assert!(
        heard.borrow().is_empty(),
        "the mast's hit is not the ship's"
    );
    events::announce(&app.engine, ship, "hit", Value::Int(3));
    assert_eq!(*heard.borrow(), vec![("ship".into(), ship, ship)]);
}

#[test]
fn a_bubbling_event_climbs_until_stopped_and_any_hears_it_once() {
    let (app, world, ship, mast) = tree();
    events::bubble(&app.engine, "hit");
    let heard = Heard::default();
    listen(&app.engine, &heard, "mast", "hit", Some(mast));
    listen(&app.engine, &heard, "world", "hit", Some(world));
    listen(&app.engine, &heard, "any", "hit", None);
    events::announce(&app.engine, mast, "hit", Value::Nil);
    assert_eq!(
        *heard.borrow(),
        vec![
            ("mast".into(), mast, mast),
            ("world".into(), mast, world),
            ("any".into(), mast, mast),
        ]
    );
    heard.borrow_mut().clear();
    events::observe(&app.engine, "hit", Some(ship), |_, event| event.stop());
    events::announce(&app.engine, mast, "hit", Value::Nil);
    assert_eq!(
        *heard.borrow(),
        vec![("mast".into(), mast, mast), ("any".into(), mast, mast)],
        "the ship stopped it short of the world"
    );
}

#[test]
fn an_unobserved_or_freed_node_hears_nothing() {
    let (app, _, ship, mast) = tree();
    let heard = Heard::default();
    let heard_too = heard.clone();
    let id = events::observe(&app.engine, "hit", Some(ship), move |_, _| {
        heard_too
            .borrow_mut()
            .push(("ship".into(), Entity::DANGLING, Entity::DANGLING));
    });
    events::unobserve(&app.engine, id);
    events::announce(&app.engine, ship, "hit", Value::Nil);
    assert!(heard.borrow().is_empty());
    listen(&app.engine, &heard, "mast", "hit", Some(mast));
    balaur_core::scene::free_node(&app.engine, mast);
    let again = app.engine.spawn_node("Mast", ship);
    events::announce(&app.engine, again, "hit", Value::Nil);
    assert!(heard.borrow().is_empty(), "a new node is not the freed one");
}

#[test]
fn an_observer_may_announce_and_observe() {
    let (app, _, ship, _) = tree();
    let heard = Heard::default();
    let inner = heard.clone();
    events::observe(&app.engine, "hit", Some(ship), move |eng, event| {
        events::announce(eng, event.target, "sunk", Value::Nil);
        listen(eng, &inner, "late", "hit", None);
    });
    listen(&app.engine, &heard, "sunk", "sunk", Some(ship));
    events::announce(&app.engine, ship, "hit", Value::Nil);
    assert_eq!(*heard.borrow(), vec![("sunk".into(), ship, ship)]);
}
