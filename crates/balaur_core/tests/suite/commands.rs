//! Commands (PLAN-ecs §2): writes asked for while the world is being read,
//! made at the end of the stage in the order asked.

use std::cell::RefCell;
use std::rc::Rc;

use balaur_core::{App, AppConfig, Children, Engine, Name, Stage, fixed_dt};

/// A marker a test puts on a node.
#[derive(Debug, PartialEq)]
struct Mark(u32);

fn app() -> App {
    App::new(AppConfig::bare(".")).unwrap()
}

#[test]
fn a_command_lands_at_the_end_of_its_stage() {
    let mut app = app();
    let node = app.engine.spawn_node("N", app.engine.root());
    let seen = Rc::new(RefCell::new(Vec::new()));
    app.add_system(Stage::Update, move |eng: &Engine, _| {
        eng.defer_insert_one(node, Mark(7));
    });
    let in_stage = seen.clone();
    app.add_system(Stage::Update, move |eng: &Engine, _| {
        in_stage
            .borrow_mut()
            .push(("update", eng.world().get::<&Mark>(node).is_ok()));
    });
    let after = seen.clone();
    app.add_system(Stage::PostUpdate, move |eng: &Engine, _| {
        after
            .borrow_mut()
            .push(("post_update", eng.world().get::<&Mark>(node).is_ok()));
    });
    app.tick(0.0);
    assert_eq!(
        *seen.borrow(),
        vec![("update", false), ("post_update", true)],
        "not yet within the stage, there by the next"
    );
}

#[test]
fn commands_run_in_the_order_asked_and_theirs_after() {
    let app = app();
    let order = Rc::new(RefCell::new(Vec::new()));
    for step in 1..=3 {
        let order = order.clone();
        app.engine.defer(move |eng| {
            order.borrow_mut().push(step);
            if step == 1 {
                let order = order.clone();
                eng.defer(move |_| order.borrow_mut().push(10));
            }
        });
    }
    assert_eq!(app.engine.apply_deferred(), 4);
    assert_eq!(*order.borrow(), vec![1, 2, 3, 10]);
    assert_eq!(app.engine.apply_deferred(), 0, "nothing is left");
}

#[test]
fn a_node_spawned_while_the_world_is_read_is_named_at_once() {
    let app = app();
    let root = app.engine.root();
    let spawned = {
        let world = app.engine.world();
        let mut made = Vec::new();
        for (entity, _) in &mut world.query::<(hecs::Entity, &Name)>() {
            if entity == root {
                made.push(app.engine.defer_spawn_node("Child", root));
            }
        }
        made[0]
    };
    app.engine.defer_insert_one(spawned, Mark(3));
    app.engine.apply_deferred();
    let world = app.engine.world();
    assert_eq!(world.get::<&Name>(spawned).unwrap().0, "Child");
    assert_eq!(*world.get::<&Mark>(spawned).unwrap(), Mark(3));
    assert!(world.get::<&Children>(root).unwrap().0.contains(&spawned));
}

#[test]
fn removing_and_despawning_wait_too() {
    let app = app();
    let node = app.engine.spawn_node("N", app.engine.root());
    app.engine.insert_one(node, Mark(1)).unwrap();
    let loose = app.engine.spawn((Mark(2),));
    app.engine.defer_remove_one::<Mark>(node);
    app.engine.defer_despawn(loose);
    assert!(app.engine.world().get::<&Mark>(node).is_ok());
    assert!(app.engine.world().contains(loose));
    app.engine.apply_deferred();
    assert!(app.engine.world().get::<&Mark>(node).is_err());
    assert!(!app.engine.world().contains(loose));
}

#[test]
fn a_fixed_step_lands_its_commands_before_the_next_step() {
    let mut app = app();
    let node = app.engine.spawn_node("N", app.engine.root());
    let marks = Rc::new(RefCell::new(Vec::new()));
    let kept = marks.clone();
    app.add_system(Stage::FixedUpdate, move |eng: &Engine, _| {
        let now = eng.world().get::<&Mark>(node).map_or(0, |mark| mark.0);
        kept.borrow_mut().push(now);
        eng.defer_insert_one(node, Mark(now + 1));
    });
    app.tick(fixed_dt() * 3.0);
    assert_eq!(*marks.borrow(), vec![0, 1, 2]);
}

#[test]
fn a_free_waits_for_the_frame() {
    let mut app = app();
    let node = app.engine.spawn_node("N", app.engine.root());
    let alive_in_post = Rc::new(RefCell::new(None));
    let kept = alive_in_post.clone();
    app.add_system(Stage::Update, move |eng: &Engine, _| eng.defer_free(node));
    app.add_system(Stage::PostUpdate, move |eng: &Engine, _| {
        *kept.borrow_mut() = Some(eng.world().contains(node));
    });
    app.tick(0.0);
    assert_eq!(*alive_in_post.borrow(), Some(true));
    assert!(
        !app.engine.world().contains(node),
        "gone by the frame's end"
    );
}
