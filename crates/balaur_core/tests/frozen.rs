//! A debugger pause holds the fixed step, and names the subtree it holds.

use std::cell::RefCell;
use std::rc::Rc;

use balaur_core::scene::{is_within, spawn_node};
use balaur_core::{App, AppConfig, Stage, FIXED_DT};

fn app() -> App {
    App::new(AppConfig::bare(".")).unwrap()
}

#[test]
fn no_fixed_step_runs_while_frozen_and_the_missed_time_is_not_owed() {
    let mut app = app();
    let steps = Rc::new(RefCell::new(0));
    let counter = Rc::clone(&steps);
    app.add_system(Stage::FixedUpdate, move |_, _| *counter.borrow_mut() += 1);

    app.engine.set_frozen(true);
    app.tick(FIXED_DT * 3.0);
    assert_eq!(*steps.borrow(), 0);

    app.engine.set_frozen(false);
    app.tick(FIXED_DT);
    assert_eq!(
        *steps.borrow(),
        1,
        "no catch-up burst for the frozen frames"
    );
}

#[test]
fn the_frozen_root_is_the_debug_scope_or_the_whole_tree() {
    let app = app();
    let root = app.engine.root();
    let game = spawn_node(&mut app.engine.world_mut(), "Game", root);
    assert_eq!(app.engine.frozen_root(), None);

    app.engine.set_frozen(true);
    assert_eq!(app.engine.frozen_root(), Some(root));
    app.engine.set_debug_scope(Some(game));
    assert_eq!(app.engine.frozen_root(), Some(game));
    app.engine.set_frozen(false);
    assert_eq!(
        app.engine.frozen_root(),
        None,
        "the scope alone holds nothing"
    );
}

#[test]
fn a_node_is_within_its_ancestors_and_itself_only() {
    let app = app();
    let root = app.engine.root();
    let (game, child, other) = {
        let mut world = app.engine.world_mut();
        let game = spawn_node(&mut world, "Game", root);
        let child = spawn_node(&mut world, "Child", game);
        let other = spawn_node(&mut world, "Other", root);
        (game, child, other)
    };
    let world = app.engine.world();
    assert!(is_within(&world, child, game));
    assert!(is_within(&world, child, root));
    assert!(is_within(&world, game, game));
    assert!(!is_within(&world, game, child));
    assert!(!is_within(&world, other, game));
}
